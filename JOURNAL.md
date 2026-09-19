## 2026-09-19 — branch main — v0.1.0
Done: lot 6 — `furet up <n>`, `furet back --session`, `furet init pwsh
[--cmd]` (SPEC §4), plus `storage::last_visited_dir` (second-to-last visit
per session). The generated PowerShell script (`src/pwsh.rs`) implements the
`f`/`f -`/`f ..`/`f <path>`/`f <query>` dispatch, a session GUID, an
oldpwd-style dedup variable, and a prompt-wrapping hook that preserves any
prompt already defined, mirroring zoxide's PowerShell init.
Decisions: internal script names use the `__furet_*` prefix; the script is
built from a static template with a `__FURET_CMD__` placeholder instead of
`format!`, to avoid escaping every brace; `up n=0` is an error, not "here".
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
