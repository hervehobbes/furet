# Contracts

## Fuzzy engine contract

Pure, no filesystem or clock access. `rank::rank(query, current_dir, candidates)`
drops the current directory and any `missing` candidate, scores every
survivor with `rank::dispatch` (stage 1 first, stage 2 only when stage 1
returns `None`), and returns a `Vec<Scored>` sorted per SPEC §8: score, then
recency (D1 — never the other way around), then name length, then path, as a
strict deterministic total order.

- `stage1::score(query, name, folder) -> Option<u32>`, floored at
  `SCORE_FLOOR = 4`, always outscores stage 2.
- `stage2::score(query, name) -> Option<u32>`, capped at `SCORE_CAP = 3`,
  eligible only for a mono-token query of at least `TYPO_MIN_QUERY_LEN = 4`
  characters, within `MAX_DISTANCE = 2` edits (OSA).
- `decision::decide(ranked) -> Decision` applies SPEC §9: a stage-1 best
  jumps unconditionally; a stage-2 best jumps alone at its distance, else
  opens a `Menu` of the leading run of tied stage-2 candidates (2..=9).

## Storage contract

SQLite at `FURET_DATA_DIR/furet.db` (else the platform local data dir), WAL
mode, foreign keys on, ordered `PRAGMA user_version` migrations.

- `dirs(id, path, key, first_seen, missing_since)` — `key` is the
  lowercased on-disk-cased path, unique; `missing_since` is set/cleared by
  soft delete (SPEC §10) and never deletes the row.
- `visits(id, dir_id, ts, source, session, from_dir_id)` — `source` is one
  of `hook | jump | back | up | fallback | import`; one row per recorded
  visit, never updated or deleted.
- `queries(id, ts, cwd, query, result_dir_id, stage, outcome)` — the SPEC §15
  journal; `stage` is one of `'1' | '2' | 'fallback' | 'menu'`.

## CLI contract

Only `furet query`'s resolved jump target reaches stdout; everything else
(menus, `--explain`, errors) goes to stderr, with one documented exception
below.

- `furet add <path> --session <s> [--source <src>] [--from <dir>]` — records
  one visit; `source` defaults to `hook`.
- `furet query <text> [--list] [--explain] [--color] [--no-ignore]` — ranks
  recorded directories, falling back to a disk walk (SPEC §11) when nothing
  matches; prints the jump target to stdout, or the SPEC §9 menu to stderr
  on a stage-2 tie, reading the answer from stdin.
- `furet up <n>` — prints the ancestor `n` levels above the current
  directory.
- `furet back --session <s>` — prints the second-to-last directory visited
  in that session.
- `furet init pwsh [--cmd <name>]` — prints the PowerShell integration
  script (mirrors zoxide's `init` shape).
- `furet queries --failures` — prints tab-separated probable-mistake rows
  **to stdout**, not stderr. Accepted exception to the stdout-discipline
  rule: it is a standalone reporting tool, never invoked by `f`/`fi`, so
  nothing pipes its output into `Set-Location`.

### Accepted spec discrepancies

- **Menu input (SPEC §9)** — accepts a bare digit line only; there is no
  raw-keypress capture, so arrow keys are not read at all, and Esc is not
  literally trapped in the no-fzf branch. Today the menu is cancelled by
  pressing Enter on anything that is not a valid in-range digit (including
  an empty line); in the `fi` fzf branch, Esc/Ctrl-C simply yield an empty
  `fzf` selection, which is already treated as cancel. Arrow-key/Esc support
  is an open BACKLOG proposal, not implemented.
- **Fallback visit session (SPEC §11)** — `record_fallback_visit` books the
  disk-fallback winner under a fixed internal `session = "fallback"`, not
  the caller's real session. This is harmless for `f -`/`back`: the pwsh
  integration always follows a successful `query` with its own `add
  --session $global:__furet_session --source jump`, so the real session
  gets its own proper visit regardless of how the target was found.
  Covered by
  `a_fallback_jump_then_back_returns_to_the_origin_directory` in
  `tests/cli.rs`.
