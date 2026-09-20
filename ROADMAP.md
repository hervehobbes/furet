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
