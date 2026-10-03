# Roadmap

| Lot | Content |
|-----|---------|
| 0 | Scaffolding |
| 1 | Normalization + stage 1 (optimal subsequence), unit and property tests |
| 2 | Stage 2 (fault tolerance) |
| 3 | Ranking + complete scenario runner |
| 4 | SQLite schema |
| 5 | Path handling, `furet add`, `furet query` |
| 6 | `init pwsh`, hook, `f`, `f -`, dots, slashes |
| 7 | Ambiguity decision + menu |
| 8 | Soft delete |
| 9 | `--explain` |
| 10 | `fi` with fzf and fallback |
| 11 | `queries` log + failure detection |
| 12 | Resolved database path printed by the top-level `--help` |
| 13 | `DATABASE.md`, the SQLite schema documented from `src/storage.rs` |
| 14 | Daily-rotated file logging under the data dir, `FURET_LOG` override (SPEC §17) |
| 15 | `config.toml` overrides: `typo_min_length`, `fallback.*` (SPEC §16) |
| 16 | Config polish: `fallback.depth`/`up` bounds, `exclude = []` docs, private `///` cleanup |
| 17 | `f <query> --explain` in the pwsh integration (SPEC §4) |
| 18 | Executed pwsh integration tests in `tests/pwsh.rs` + review fixes |
| 19 | `home` config key and the `furet home` subcommand |
| 20 | `furet import zoxide` seeds the database from `zoxide query -ls` |
| 21 | Stage 2 capped at distance 1 for queries shorter than 6 characters |
| 22 | SPEC §13 ranking scenario suite extended to 42 cases |
| 23 | Docs realigned with the code (ROADMAP, README, ARCHITECTURE, DATABASE, CONTRACTS) |
| 24 | `--help` trailer adds config file and log directory; help pages pinned by insta snapshots |
| 25 | pwsh tab completer on the jump function's first argument |
| 26 | `furet list [--all]` |
| 27 | Migration 2: `visits` ranking indexes; `synchronous = NORMAL`, 5 s `busy_timeout` |
| 28 | `furet query`'s `<QUERY>` optional (fixes `fi`'s initial fzf list) |
| 29 | An empty `FURET_DATA_DIR` counts as unset |
| 30 | Single ranking pass in `furet query`; `upsert_dir` via `INSERT ... RETURNING` |
| 31 | A non-empty query reconciles only the directories it matches |
| 32 | Migration 3: `ts` indexes on `visits` and `queries` |
| 33 | `retention_days` config key and the retention purge in `furet add` |
| 34 | `furet list --paths` (`-p`) |
| 35 | `furet list` ordered by path, ignoring case |
| 36 | `furet remove <pattern> [--confirm]` |
| 37 | `furet remove`: name patterns match any path segment; `*`-prefixed path patterns are not anchored |
| 38 | `exclude_dirs` config key honored by `add`, the disk fallback, and `import zoxide` |
| 39 | `furet` subcommand completions (`clap_complete`) embedded in `furet init pwsh` |
| 40 | Project-scoped query: `furet query --local`, `f -l`, `fi -l`, Tab on the second token |
| 40b | pwsh: `-l` as a declared switch on `f`/`fi`; the `-Native` completer fallback removed |
| 41 | Opt-in `nucleo` stage-1 engine: `engine` config key, `furet query --engine`, `engine:` explain line (SPEC-v2 §20) |
| 42 | `furet remove`: per-directory `[y/N/a/q]` confirmation, `--yes`, `--dry-run` |
| 43 | `furet remove --missing`: full reconcile first, confirmation on by default |
| 44 | `furet stats [--top <n>]` (SPEC-v2 §22) |
| 45 | fzf preview in `fi` and the `furet preview` subcommand (SPEC-v2 §23) |
| 46 | Query memory, part A: journal menu and `fi` picks (`outcome = 'pick'`, `furet add --query`) (SPEC-v2 §24) |
| 47 | Query memory, part B: the remembered directory ranks first (`query_memory` config key); version 0.2.0 (SPEC-v2 §24) |
| 47b | Query-memory lookup skipped on the disk-fallback path (explain keeps it) |
| 48 | `query_directories` split into focused helpers (Sonar S3776), no behavior change |
| 49 | `config::parse` and `explain::render` split (Sonar S3776), no behavior change |
| 50 | Build date (UTC, `YYYYMMDD`) as the last line of `furet --help` |
| 51 | Schema: `aliases` table shared by aliases and marks (migration 4) |
| 52 | `furet alias add / list / remove` (no resolution yet) |
| 53 | Alias resolution: `f !name`, `alias_prefix` (`!`/`=`), `did you mean` hint |
| 54 | Tab completion of alias words: strict prefix, path shown, name inserted |
| 55 | Home scope in the binary: `furet query --home` |
| 56 | Home scope in pwsh: `f -h`, `fi -h`, Tab after `-h` |
| 57 | Marks in the binary: `furet mark set/list/delete`, `mark N not set` |
| 58 | Mark cycling in the binary: `furet mark next/prev` |
| 59 | pwsh `fm` (set/list/delete/cycle) and Ctrl+Alt+→/← mark bindings |
| 60 | `fi !`: interactive menu of aliases and marks |
| 62 | Release 0.3.0: aliases, home scope, marks (design cycle, lots 51-61) |
