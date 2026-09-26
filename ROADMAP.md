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

## Planned — v0.2.0 (`prompts/SPEC-v2.md`)

| Lot | Content | SPEC-v2 |
|-----|---------|---------|
| 39 | `furet` subcommand completions (`clap_complete`) embedded in `furet init pwsh` | §18 |
| 40 | Project-scoped query: `furet query --local`, `f -l`, `fi -l`, Tab on the second token | §19 |
| 41 | Opt-in `nucleo` stage-1 engine (`engine` config key, `--engine`) | §20 |
| 42 | `furet remove`: per-directory `[y/N/a/q]` confirmation, `--yes`, `--dry-run` | §21 |
| 43 | `furet remove --missing` (full reconcile first, confirmation by default) | §21 |
| 44 | `furet stats [--top <n>]` | §22 |
| 45 | fzf preview in `fi` and the `furet preview` subcommand | §23 |
| 46 | Query memory, part A: journal menu and `fi` picks (`outcome = 'pick'`, `furet add --query`) | §24 |
| 47 | Query memory, part B: the remembered directory ranks first (`query_memory` config key); version 0.2.0 | §24 |
