# Changelog

All notable changes to furet. Versions follow [Semantic Versioning](https://semver.org/);
lot numbers refer to [ROADMAP.md](ROADMAP.md).

## [0.4.0] — 2026-10-04

### Added

- `f -N` goes N directories back in the session's raw visit history — duplicates kept, `f -1` equals `f -`, the jump recorded as a `back` visit; the binary side is `furet back --steps N` (lot 63).
- `furet import pwsh-history` seeds the database from PSReadLine history: `cd`-like lines holding one absolute path (`~` expands to the user profile), later lines more recent, re-running it a no-op (lot 64).
- `furet history` prints the visit history newest first, numbered like `f -N` within a session (`--session <id>`) or unnumbered across every session (`--all`); the pwsh `fh` wrapper — `fh -a` for every session, `fh -n <N>` for a limit — runs it without typing `furet history` (lots 65, 66).
- `furet export` writes the whole database as versioned JSON on stdout, pure ASCII so a redirect keeps every accented path; `furet import json` merges such a file idempotently — directories united by key, visits and queries deduplicated, local aliases kept on conflict (lots 67, 68).

### Internal

- The repeat-free test pins the PSReadLine history line order (lot 64b).
- `backup::validate` checks every `visits[i].dir` reference before any `visits[i].from_dir`, in the order the tests pin (lot 68b).
- `tests/docs.rs` fails when a document cites a test or a `module::item` that no longer exists (lot 69).
- `CHANGELOG.md` replaces the README's lot-by-lot Status narrative; version 0.4.0 (lot 70).

## [0.3.0] — 2026-10-03

### Added

- The binary's UTC build date (`YYYYMMDD`) as the last line of `furet --help` (lot 50).
- `furet alias add`, `furet alias list` and `furet alias remove` manage named aliases (lot 52).
- `f !name` jumps to a named alias; the `alias_prefix` key (`!`/`=`) changes the prefixes; a near-miss offers a `did you mean` hint (lot 53).
- Tab completes alias words: `f !om<Tab>` lists every matching alias with its path and inserts the name (lot 54).
- `furet query --home` scopes the query pool to the home root (the `home` config key, else the user profile), from anywhere; pwsh wires it in as `f -h`, `fi -h` and Tab after `-h` (lots 55, 56).
- Numbered marks: `furet mark set`/`list`/`delete` over the shared `aliases` table (a missing mark answers `mark N not set`), `furet mark next`/`prev` cycle the current directory's marks (lowest mark first, wrap 9 → 1, missing marks skipped), and pwsh's `fm` helper plus the Ctrl+Alt+→/← bindings cycle without typing (lots 57, 58, 59).
- `fi !` opens an interactive menu of aliases and marks, fed by `furet alias complete` — fuzzy or strict-prefix, the pick recorded as a `jump` visit with no `queries` row (lot 60).

### Internal

- `query_directories` split into focused helpers (Sonar S3776), no behavior change (lot 48).
- `config::parse` and `explain::render` split (Sonar S3776), no behavior change (lot 49).
- Migration 4 adds the `aliases` table shared by aliases and marks (lot 51).
- Letter-free sandbox names end the folder-bonus test flake (lot 61).

## [0.2.0] — 2026-09-27

### Added

- `furet init pwsh` embeds completions for `furet`'s subcommands (`clap_complete`) (lot 39).
- `furet query --local` scopes the query to the project, with `f -l`, `fi -l` and Tab on the second token (lot 40).
- The opt-in `nucleo` stage-1 engine: the `engine` config key, `furet query --engine` and an `engine:` explain line (lot 41).
- `furet remove` confirms per directory (`[y/N/a/q]`), with `--yes` and `--dry-run` (lot 42).
- `furet remove --missing` runs the full reconcile first, confirmation on by default (lot 43).
- `furet stats [--top <n>]` (lot 44).
- An fzf preview in `fi` and the `furet preview` subcommand (lot 45).
- Query memory, part A: the journal menu and `fi` picks — picks journaled with `outcome = 'pick'`, `furet add --query` (lot 46).
- Query memory, part B: the remembered directory ranks first, governed by the `query_memory` config key (lot 47).

### Changed

- pwsh: `-l` is a declared switch on `f`/`fi`; the `-Native` completer fallback removed (lot 40b).
- The query-memory lookup is skipped on the disk-fallback path; the explain output keeps showing it (lot 47b).

## [0.1.1] — 2026-09-26

### Added

- Normalization and stage 1, the optimal-subsequence fuzzy match (lot 1).
- Stage 2, fault tolerance for typos (lot 2).
- Ranking, plus the complete scenario runner (lot 3).
- Path handling, with `furet add`, `furet query`, `furet up` and `furet back` (lot 5).
- `init pwsh`, the hook, `f`, `f -`, dots and slashes (lot 6).
- The ambiguity decision and menu (lot 7).
- Soft delete (lot 8).
- `--explain` (lot 9).
- `fi` with fzf and the fallback (lot 10).
- The `queries` log and failure detection (lot 11).
- The resolved database path printed by the top-level `--help` (lot 12).
- Daily-rotated file logging under the data dir, with the `FURET_LOG` override (lot 14).
- `config.toml` overrides: `typo_min_length` and the `fallback.*` keys (lot 15).
- `f <query> --explain` in the pwsh integration (lot 17).
- The `home` config key and the `furet home` subcommand (lot 19).
- `furet import zoxide` seeds the database from `zoxide query -ls` (lot 20).
- A pwsh tab completer on the jump function's first argument (lot 25).
- `furet list [--all]` (lot 26).
- The `retention_days` config key and the retention purge in `furet add` (lot 33).
- `furet list --paths` (`-p`) (lot 34).
- `furet remove <pattern> [--confirm]` (lot 36).
- The `exclude_dirs` config key, honored by `add`, the disk fallback and `import zoxide` (lot 38).

### Changed

- Stage 2 is capped at distance 1 for queries shorter than 6 characters (lot 21).
- `furet query`'s `<QUERY>` is optional, which fixes `fi`'s initial fzf list (lot 28).
- `furet list` is ordered by path, ignoring case (lot 35).
- `furet remove`'s name patterns match any path segment, and `*`-prefixed path patterns are not anchored (lot 37).

### Fixed

- An empty `FURET_DATA_DIR` counts as unset (lot 29).
- A non-empty query reconciles only the directories it matches (lot 31).

### Internal

- Project scaffolding (lot 0).
- The SQLite schema (lot 4).
- `DATABASE.md`, the SQLite schema documented from `src/storage.rs` (lot 13).
- Config polish: `fallback.depth`/`up` bounds, `exclude = []` docs, private `///` cleanup (lot 16).
- Executed pwsh integration tests in `tests/pwsh.rs` (lot 18).
- The ranking scenario suite extended to 42 cases (lot 22).
- Docs realigned with the code (ROADMAP, README, ARCHITECTURE, DATABASE, CONTRACTS) (lot 23).
- The `--help` trailer adds the config file and log directory; help pages pinned by insta snapshots (lot 24).
- Migration 2: `visits` ranking indexes; `synchronous = NORMAL`, 5 s `busy_timeout` (lot 27).
- A single ranking pass in `furet query`; `upsert_dir` via `INSERT ... RETURNING` (lot 30).
- Migration 3: `ts` indexes on `visits` and `queries` (lot 32).

[0.4.0]: https://github.com/hervehobbes/furet/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/hervehobbes/furet/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/hervehobbes/furet/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/hervehobbes/furet/releases/tag/v0.1.1
