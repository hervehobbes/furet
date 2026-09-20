# DATABASE.md — the furet SQLite database

Source of truth is `src/storage.rs`'s `MIGRATIONS`; update this file
whenever that schema changes.

## File location

Resolved by `storage::db_path()` on every open:

- `FURET_DATA_DIR` set: `<FURET_DATA_DIR>/furet.db` (joined literally, no
  canonicalization — tests rely on this override).
- Otherwise: the platform local data directory joined with `furet/furet.db`
  (on Windows: `%LOCALAPPDATA%\furet\furet.db`).

Opening the database creates the file and its parent directory when missing.

## Connection settings

Applied by `storage::open()` on every connection:

| Pragma | Value | Notes |
|---|---|---|
| `journal_mode` | `WAL` | Set on every open. |
| `foreign_keys` | `ON` | Per-connection in SQLite, never persisted, so re-applied on every open; the foreign keys below are enforced only because of this. |
| `user_version` | `1` | Counts how many `MIGRATIONS` scripts have been applied. `migrate()` reads it, then runs each script whose 1-based version exceeds it inside one transaction that also bumps the pragma. Current value is 1: one script creates the whole schema. |

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

| Column | Type | Constraints | Holds |
|---|---|---|---|
| `id` | INTEGER | `PRIMARY KEY AUTOINCREMENT` | Surrogate row id. |
| `dir_id` | INTEGER | `NOT NULL`, `REFERENCES dirs (id)` | The visited directory. |
| `ts` | INTEGER | `NOT NULL` | Unix seconds of the visit. |
| `source` | TEXT | `NOT NULL`, `CHECK (source IN ('hook', 'jump', 'back', 'up', 'fallback', 'import'))` | What triggered the visit. |
| `session` | TEXT | `NOT NULL` | Terminal-session identifier the visit belongs to. |
| `from_dir_id` | INTEGER | nullable, `REFERENCES dirs (id)` | Origin directory of the jump, when known. |

### `queries` — one row per logged query decision (SPEC §15)

| Column | Type | Constraints | Holds |
|---|---|---|---|
| `id` | INTEGER | `PRIMARY KEY AUTOINCREMENT` | Surrogate row id. |
| `ts` | INTEGER | `NOT NULL` | Unix seconds when the query ran. |
| `cwd` | TEXT | `NOT NULL` | Directory the query was issued from. |
| `query` | TEXT | `NOT NULL` | The query text as typed. |
| `result_dir_id` | INTEGER | nullable, `REFERENCES dirs (id)` | Directory the query jumped straight to; `NULL` when nothing jumped (no match, or a menu was shown). |
| `stage` | TEXT | `NOT NULL`, `CHECK (stage IN ('1', '2', 'fallback', 'menu'))` | Matching stage or path that produced the decision. |
| `outcome` | TEXT | `NOT NULL` (no `CHECK`) | What the query did; the code writes `'jump'`, `'none'`, or `'menu'`. |

## Indexes

| Index | Table (columns) | Unique | Purpose |
|---|---|---|---|
| `idx_dirs_key` | `dirs (key)` | yes | One `dirs` row per comparison key; also the conflict target of `storage::upsert_dir`'s `ON CONFLICT (key)`. |

## Foreign keys

- `visits.dir_id` → `dirs.id` (required)
- `visits.from_dir_id` → `dirs.id` (optional)
- `queries.result_dir_id` → `dirs.id` (optional)

No `ON DELETE` action is declared on any of them; enforcement relies on the
`foreign_keys = ON` pragma above.
