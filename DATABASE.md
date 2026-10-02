# DATABASE.md — the furet SQLite database

Source of truth is `src/storage.rs`'s `MIGRATIONS`; update this file
whenever that schema changes.

## File location

Resolved by `storage::db_path()` on every open:

- `FURET_DATA_DIR` set and non-empty: `<FURET_DATA_DIR>/furet.db` (joined literally, no
  canonicalization — tests rely on this override).
- Otherwise (unset or empty): the platform local data directory joined with `furet/furet.db`
  (on Windows: `%LOCALAPPDATA%\furet\furet.db`).

Opening the database creates the file and its parent directory when missing.

## Connection settings

Applied by `storage::open()` on every connection:

| Pragma | Value | Notes |
|---|---|---|
| `journal_mode` | `WAL` | Set on every open. |
| `foreign_keys` | `ON` | Per-connection in SQLite, never persisted, so re-applied on every open; the foreign keys below are enforced only because of this. |
| `synchronous` | `NORMAL` | Per-connection. WAL's pairing value: durable across crashes, skips the per-commit `fsync` that `FULL` makes every `furet add` write pay. |
| `busy_timeout` | `5000` ms | Per-connection, set via rusqlite's `busy_timeout`. Concurrent prompt-hook writers wait instead of failing instantly with `SQLITE_BUSY`. |
| `user_version` | `4` | Counts how many `MIGRATIONS` scripts have been applied. `migrate()` reads it, then runs each script whose 1-based version exceeds it inside one transaction that also bumps the pragma. Current value is 4: script 1 creates the whole schema, script 2 adds the `visits` ranking indexes, script 3 the `ts` indexes of the retention purge, script 4 the `aliases` table. |

## Tables

### `dirs` — one row per known directory

| Column | Type | Constraints | Holds |
|---|---|---|---|
| `id` | INTEGER | `PRIMARY KEY AUTOINCREMENT` | Surrogate row id referenced by the other tables. |
| `path` | TEXT | `NOT NULL` | Canonical, displayable path of the directory. |
| `key` | TEXT | `NOT NULL`; unique via `idx_dirs_key` | Lowercased comparison key; case variants of the same path share one row. |
| `first_seen` | INTEGER | `NOT NULL` | Unix seconds of the first recording of the directory. |
| `missing_since` | INTEGER | nullable | Unix seconds when the directory was found absent from disk (soft delete, SPEC §10); `NULL` while present. |

### `visits` — one row per recorded directory visit

Rows older than `retention_days` (config, default 365, `0` = never) are
deleted by `storage::purge_before` on every `furet add`, as are `queries`
rows; `dirs` rows are deleted only by `furet remove`.

| Column | Type | Constraints | Holds |
|---|---|---|---|
| `id` | INTEGER | `PRIMARY KEY AUTOINCREMENT` | Surrogate row id. |
| `dir_id` | INTEGER | `NOT NULL`, `REFERENCES dirs (id)` | The visited directory. |
| `ts` | INTEGER | `NOT NULL` | Unix seconds of the visit. |
| `source` | TEXT | `NOT NULL`, `CHECK (source IN ('hook', 'jump', 'back', 'up', 'fallback', 'import'))` | What triggered the visit (see the source values below). |
| `session` | TEXT | `NOT NULL` | Terminal-session identifier the visit belongs to (see the session values below). |
| `from_dir_id` | INTEGER | nullable, `REFERENCES dirs (id)` | Origin directory of the jump, when known. |

#### `session` values

Most rows carry the caller's terminal-session id (`furet add --session`;
the pwsh integration mints one GUID per shell). The binary itself writes
two fixed values, both chosen distinct from any real session id so
`storage::last_visited_dir` (the `f -` lookup) never sees them:

- `fallback` — `main::record_fallback_visit` (`FALLBACK_SESSION`,
  `src/main.rs`) books a disk-fallback winner under it (SPEC §11).
- `import` — `furet import zoxide` (`IMPORT_SOURCE`, `src/main.rs`) stamps
  every imported row with it, as both the session and the `source`.

#### `source` values

| Value | Written by | Meaning |
|---|---|---|
| `hook` | the pwsh prompt hook (also `furet add`'s default `--source`) | A directory change made outside `f`, i.e. any plain `cd`. |
| `jump` | the pwsh `f` and `fi` functions | A jump the function performed: query match, direct path, menu/fzf pick, or no-argument home. |
| `up` | the pwsh `f ..`/`f ...` branch | An ancestor climb through `furet up`. |
| `back` | the pwsh `f -` branch | A return to the session's previous directory through `furet back`. |
| `fallback` | `main::record_fallback_visit` | The disk-fallback winner recorded when no known directory matched (SPEC §11). |
| `import` | `furet import zoxide` | A directory seeded from `zoxide query -ls` (SPEC §3). |

### `queries` — one row per logged query decision (SPEC §15)

| Column | Type | Constraints | Holds |
|---|---|---|---|
| `id` | INTEGER | `PRIMARY KEY AUTOINCREMENT` | Surrogate row id. |
| `ts` | INTEGER | `NOT NULL` | Unix seconds when the query ran. |
| `cwd` | TEXT | `NOT NULL` | Directory the query was issued from. |
| `query` | TEXT | `NOT NULL` | The query text as typed. |
| `result_dir_id` | INTEGER | nullable, `REFERENCES dirs (id)` | Directory the query jumped straight to; `NULL` when nothing jumped (no match, or a menu was shown). |
| `stage` | TEXT | `NOT NULL`, `CHECK (stage IN ('1', '2', 'fallback', 'menu'))` | Matching stage or path that produced the decision. |
| `outcome` | TEXT | `NOT NULL` (no `CHECK`) | What the query did; the code writes `'jump'`, `'none'`, `'menu'`, `'pick'`. |

`pick` (SPEC-v2 §24) journals the directory a menu chose: it appears with
`stage = 'menu'` (a §9 menu choice or `furet add --query`) or `stage =
'fallback'` (a fallback menu choice); `result_dir_id` is `NULL` for a pick
only when the chosen directory is excluded (`exclude_dirs`, lot 38's rule).
`furet stats` counts it in `jumps`, and calibration treats it like `jump`.

### `aliases` — one row per alias or mark

Named aliases (`f !ombi`) and marks (`fm 1`, `f !1`) share this table and
one namespace: a mark is a row whose `name` is a single digit `1`–`9`.
The target is a plain path with no foreign key to `dirs`, so
`furet remove`, the retention purge and `exclude_dirs` never touch these
rows. No command writes the table yet (lot 51 adds only the schema).

| Column | Type | Constraints | Holds |
|---|---|---|---|
| `id` | INTEGER | `PRIMARY KEY AUTOINCREMENT` | Surrogate row id. |
| `name` | TEXT | `NOT NULL` | The name as the user typed it, shown in listings. |
| `key` | TEXT | `NOT NULL`; unique via `idx_aliases_key` | The lowercased `name`; names are case-insensitive, so `Ombi` and `ombi` are one entry. |
| `path` | TEXT | `NOT NULL` | Canonical, displayable path of the target directory. |
| `created` | INTEGER | `NOT NULL` | Unix seconds when the entry was created or last overwritten. |

## Indexes

| Index | Table (columns) | Unique | Purpose |
|---|---|---|---|
| `idx_dirs_key` | `dirs (key)` | yes | One `dirs` row per comparison key; also the conflict target of `storage::upsert_dir`'s `ON CONFLICT (key)`. |
| `idx_visits_dir_ts` | `visits (dir_id, ts)` | no | Migration 2. Covers `dir_entries`'s `GROUP BY dir_id` / `MAX(ts)` join and `dir_listing`'s visit join, so neither scans all of `visits` as it grows. |
| `idx_visits_session_ts` | `visits (session, ts)` | no | Migration 2. Serves `last_visited_dir`'s `WHERE session = ?` with `ORDER BY ts DESC` instead of a full scan plus sort. |
| `idx_visits_ts` | `visits (ts)` | no | Migration 3. Lets the retention purge delete `visits` rows older than the cutoff without scanning the table. |
| `idx_queries_ts` | `queries (ts)` | no | Migration 3. Same, for `queries` rows. |
| `idx_aliases_key` | `aliases (key)` | yes | Migration 4. One entry per case-insensitive name; the lookup key of alias and mark resolution. |

## Foreign keys

- `visits.dir_id` → `dirs.id` (required)
- `visits.from_dir_id` → `dirs.id` (optional)
- `queries.result_dir_id` → `dirs.id` (optional)

`aliases` has no foreign key: its `path` is standalone text.

No `ON DELETE` action is declared on any of them; enforcement relies on the
`foreign_keys = ON` pragma above. `storage::remove_dirs` (`furet remove`)
clears the three references itself before deleting a `dirs` row — other
visits' `from_dir_id` is set to `NULL`, and the removed directory's own
`visits` and `queries` rows are deleted — so no cascade is needed.
