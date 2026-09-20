## 2026-09-20 — branch main — v0.1.0
Done: lot 13 — docs-only: new root `DATABASE.md` describing the SQLite
database straight from `src/storage.rs` (`db_path` resolution, WAL +
foreign_keys = ON + user_version = 1, `dirs`/`visits`/`queries` with every
column, constraint, and CHECK value, `idx_dirs_key`, the three foreign
keys); no code change, no tests, nothing to test in Markdown.
Decisions: none beyond the prompt; the uncommitted CLAUDE.md/BACKLOG.md
working-tree edits were left unstaged (Hervé's sibling change).
Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 12 — top-level `--help`/`-h` ends with `Database file: <path>`,
computed at startup from `storage::db_path()` (honors `FURET_DATA_DIR`);
resolution errors print `Database file: unavailable: <error>`, no panic.
Parsing goes through `Cli::command().after_help(...).get_matches()` +
`from_arg_matches`; subcommand help untouched. New test
`help_prints_the_database_file_path_resolved_at_runtime` in `tests/cli.rs`.
Decisions: after_help so both -h and --help show the line; the matches
round-trip exits via clap::Error::exit instead of unwrap.
Next: none assigned.

## 2026-09-19 — branch main — v0.1.0
Done: lot 11 — SPEC §15 query journal + probable-failure detection: `query`
now logs every real decision to `queries` (skipped for `--list`, `--explain`,
and the pre-existing empty-query regression); new `src/calibration.rs`
(`probable_failures`, `FAILURE_THRESHOLD_SECS = 10`); new `furet queries
--failures` prints tab-separated lines to **stdout** — an explicit exception
to the jump-target-only discipline, since this subcommand is a standalone
reporting tool never invoked by `f`/`fi`.
Decisions: none beyond the prompt; no ambiguity hit.
Next: none — this was the last lot of the V1 roadmap.

## 2026-09-19 — branch main — v0.1.0
Done: lot 10 — SPEC §11 disk fallback + §12 `fi`: new `src/fallback.rs`
(`ignore::WalkBuilder`), `furet query` gains `--no-ignore`, `--color`,
empty-query `--list` (recency then path), and records the fallback winner
(`source = 'fallback'`); `explain::Report` gains `origin`; `fi` added to the
pwsh script (fzf `--disabled` reload, or a §9 console menu).
Decisions: fallback visits use a fixed `session = "fallback"` (no `--session`
on `query`) — flagged for review; `.gitignore` needs a real `.git` dir to
take effect (`require_git` default), matching git itself, left untouched.
Next: lot 11, on Hervé's go.

## 2026-09-19 — branch main — v0.1.0
Done: lot 9 — SPEC §14 `--explain`: `stage1::explain` (per-token bonuses),
`stage2::explain` (raw distance past `MAX_DISTANCE`), `rank::TieBreak` +
`deciding_criterion`, new `src/explain.rs` (`Report`, `Elimination`, pure
`render`), `furet query --explain` on stderr only and exit 0, 3 `insta` snapshots.
Decisions: layout is normalized query / evaluated (rank order) / eliminated
(path order) / deciding criterion / decision; `placement` stays one bundled DP
number (word-start, name-start, consecutive run and gap penalty not split out);
`decide`'s lifetime relaxed to `&[Scored<'a>]` so a local ranking can be reported.
Next: lot 10, on Hervé's go.

## 2026-09-19 — branch main — v0.1.0
Done: lot 8 — SPEC §10 soft delete: new `src/soft_delete.rs` (injectable
`Filesystem`, `RealFilesystem`, `reconcile` flipping `missing` against disk
and emitting `(id, missing_since)` updates), `storage::set_missing_since`,
`DirEntry.id`, `upsert_dir` reactivating via `ON CONFLICT DO UPDATE SET
missing_since = NULL`, `furet query` reconciling and persisting before
ranking; 6 fake-fs unit tests, 2 storage tests, 2 end-to-end CLI tests.
Decisions: each update is its own auto-committed UPDATE, no wrapping
transaction (lot 5's per-statement style); nothing left open.
Next: lot 9, on Hervé's go.

## 2026-09-19 — branch main — v0.1.0
Done: lot 7 — SPEC §9 in a new `src/decision.rs`: `decide` over `rank`'s output
(stage-1 best jumps unconditionally, stage-2 best jumps when alone at its
distance, else a menu of at most 9 equals), `render_menu`, `selection`, the
`furet query` wiring, `menu = [...]` scenarios, and proptests (a menu is always
2..=9 tied stage-2 candidates, stage 1 never lists, decide is deterministic).
Decisions: own module rather than `rank.rs`, since it consumes rank's output and
lot 10 (`fi`) reuses the menu; cancel reuses `report`'s exit code 1; digits only
on line-buffered stdin — arrow keys and Esc deferred, no new terminal dependency.
Next: lot 8, on Hervé's go.

## 2026-09-19 — branch main — v0.1.0
Done: lot 6 — `furet up <n>`, `furet back --session`, `furet init pwsh
[--cmd]` (SPEC §4), plus `storage::last_visited_dir` (second-to-last visit
per session). The generated PowerShell script (`src/pwsh.rs`) implements the
`f`/`f -`/`f ..`/`f <path>`/`f <query>` dispatch, a session GUID, an
oldpwd-style dedup variable, and a prompt-wrapping hook that preserves any
prompt already defined, mirroring zoxide's PowerShell init.
Decisions: `__furet_*` script names; static template with a `__FURET_CMD__`
placeholder instead of `format!`, to avoid escaping every brace; `up n=0` is
an error, not "here".
Next: lot 7, the ambiguity/menu decision (SPEC §9).

## 2026-09-19 — branch main — v0.1.0
Done: lot 5 — SPEC §6 paths in `src/paths.rs` (separator unification, `dunce`
canonicalization, lowercased `key`, `name`/`folder` split), storage CRUD
(`upsert_dir`, `dir_id_by_key`, `insert_visit`, `dir_entries`), `SystemClock`,
and the real CLI (`furet add`, `furet query [--list]`, working `--version`)
covered by 11 assert_cmd tests under `FURET_DATA_DIR`.
Decisions: storage helpers take `clock::Timestamp` (lot 4 deferred typed rows
to this lot); a drive root splits to an empty `name` and no `folder`;
`upsert_dir` keeps the first `path`/`first_seen` via `ON CONFLICT DO NOTHING`;
`dunce` returns on-disk casing, so case-variant inputs share one row.
Next: lot 6, `init pwsh`, the hook and the shell functions.

## 2026-09-19 — branch main — v0.1.0
Done: lot 4 — SQLite storage (SPEC §5) in `src/storage.rs`: `db_path` (honors
`FURET_DATA_DIR`, else the platform local data dir), `open` (WAL, foreign
keys, ordered `user_version` migrations), the `dirs`/`visits`/`queries`
schema with CHECKs on `source`/`stage` and a unique index on `dirs.key`.
Decisions: timestamps are plain INTEGER Unix seconds matching
`clock::Timestamp` (no typed rows yet, those are lot 5's); `stage` stored as
TEXT `'1'`/`'2'`/`'fallback'`/`'menu'` to keep one column type.
Next: lot 5, paths (§6), `furet add`, `furet query`.

## 2026-09-19 — branch main — v0.1.0
Done: lot 3 — ranking (SPEC §8) and stage dispatch in `src/rank.rs`, an
injectable clock in `src/clock.rs`, a scenario runner (SPEC §13) executing 13
cases over 4 TOML files; proptest covers D1, the strict total order, the
stage-1 > stage-2 floor and both exclusions.
Decisions: `Timestamp` is a newtype over i64 Unix seconds (no new crate);
`rank` returns a sorted `Vec<Scored>` so lot 7 can read ties; name length in
characters; `current_dir` compared lowercased, no canonicalization until lots
5/6; `dirs` entries accept `{ path, visited = "-2h", missing }`.
Next: lot 4, the SQLite schema and storage.

## 2026-09-19 — branch main — v0.1.0
Done: lot 2 — stage-2 typo tolerance (SPEC §7.3) in `src/stage2.rs`, 13 tests
including the cross-module invariant "stage 1 always outranks stage 2".
Decisions: signature `score(query: &str, name: &str) -> Option<u32>`, no
`folder` part (§7.3 mentions none); OSA (optimal string alignment) chosen over
full Damerau-Levenshtein, `distance("ca", "abc") == 3` pins that choice;
`TYPO_MIN_QUERY_LEN = 4` and `SCORE_CAP = 3` are public constants; mono-token
means exactly one whitespace-separated token, as in stage 1; window lengths
`|q|-2 ..= |q|+2` are clamped to the name length at both ends.
Next: lot 3, ranking and tie-breaking (SPEC §8).

## 2026-09-19 — branch main — v0.1.0
Done: lot 1 — normalization (SPEC §7.1) and stage-1 matching (SPEC §7.2) as a
pure `furet` library (`src/normalize.rs`, `src/stage1.rs`), 29 tests including
proptest invariants for the floor of 4, determinism, and greedy/DP agreement.
Decisions: folder part is an `Option<&str>` the caller joins from the parent
segments; the +2 folder bonus is added after the floor of 4; an empty or
whitespace-only query returns `None`; equal-scoring DP placements keep the
earliest start index; a one-token match counts as strictly increasing and so
takes the +5 order bonus.
Next: lot 2, stage-2 typo tolerance (SPEC §7.3).

## 2026-09-19 — branch main — v0.1.0
Done: project scaffolding, hooks, DoD script, scenario runner skeleton.
Decisions: none beyond what the lot prompt specified.
Next: lot 1, normalization and stage-1 matching engine.
