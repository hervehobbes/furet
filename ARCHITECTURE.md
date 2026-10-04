# Architecture

furet is layered around a pure core: normalization, the matching engine,
and ranking are deterministic functions with no filesystem or clock
access, so they can be tested exhaustively from plain data. Everything
impure — directory listing, timestamps, the SQLite database, shell
integration — lives in the outer layers and reaches the core through
injected traits (filesystem and clock are injected as traits). The CLI,
storage, and shell-hook layers sit around that core and translate between
the real environment and the pure functions.

## Module map

One row per file under `src/`, `lib.rs` excepted. Modules owning both
sides of a seam sit where their real implementation lives; the injected
trait is the boundary.

| Module | Side | Role |
|---|---|---|
| `alias` | pure | alias-name validation (`alias::key`), marks (`alias::mark_digit`), the `did you mean` hint, mark cycling (`alias::cycle`) |
| `backup` | pure | the export snapshot model, `backup::validate`, ASCII escaping (`backup::escape_non_ascii`) |
| `calibration` | pure | probable-failure detection over the journal (`calibration::probable_failures`) |
| `clock` | outer | the `Clock` trait, `SystemClock`'s real clock, and the pure `FixedClock` test double |
| `config` | pure | `config::parse` turns `config.toml` text into `Settings` plus one warning per invalid key |
| `decision` | pure | the SPEC §9 decision — jump or menu — over ranked candidates (`decision::decide`) |
| `explain` | pure | renders the `--explain` scoring report (`explain::render`) |
| `fallback` | outer | `fallback::discover` walks the real disk, gitignore-aware, when nothing known matches |
| `import` | pure | parses zoxide lines and PSReadLine history, deduplicates, plans timestamps (`import::plan`) |
| `logging` | outer | binary-only; the daily-rotated tracing file logger (`logging::init`) |
| `main` | outer | the clap CLI and command implementations, `main::report`'s exit codes, the single stdout funnel `stdout_line` |
| `memory` | pure | query-memory keys and matching; `memory::promote` moves the remembered directory first |
| `normalize` | pure | the SPEC §7.1 normalization (`Normalized`) |
| `paths` | outer | canonicalizes real directories (`paths::canonical`); lexical keys, splits, separator unification |
| `preview` | pure | renders the fzf preview listing (`preview::render`) |
| `project` | pure | `project::root` finds the nearest `.git` ancestor through the injected `GitMarker` (`RealGitMarker` checks the disk) |
| `pwsh` | outer | the PowerShell integration template (`pwsh::script`) |
| `rank` | pure | ranking: `rank::rank` applies the SPEC §8 order, `rank::Engine` picks the stage-1 scorer |
| `remove` | pure | wildcard matching (`remove::matches`) and the `[y/N/a/q]` confirm protocol (`remove::confirm_each`) |
| `soft_delete` | pure | `soft_delete::reconcile` over the injected `Filesystem` trait (`RealFilesystem` checks the disk) |
| `stage1` | pure | the reference stage-1 scorer (`stage1::score`) |
| `stage1_nucleo` | pure | the opt-in nucleo stage-1 scorer (`stage1_nucleo::NucleoScorer`) |
| `stage2` | pure | the typo-tolerant stage 2 (`stage2::score`) |
| `stats` | pure | renders the `furet stats` lines (`stats::render`) |
| `storage` | outer | the SQLite schema, its `MIGRATIONS`, and every query |

## Path of a query

Typing `f mcp` in pwsh runs `furet query -- mcp`. `main::query_directories`
loads the settings (`main::load_settings`), resolves alias queries first
(`main::alias_query`), opens and scopes the candidate pool
(`storage::dir_entries`), and ranks it with `rank::rank` — `stage1::score`
(or the nucleo engine), then `stage2::score` — in the SPEC §8 order: score,
recency (D1), name length, path. Query memory (`storage::recall` +
`memory::promote`) puts the directory last chosen for the same query first;
`decision::decide` then jumps or opens the SPEC §9 menu, and the target
reaches stdout through `stdout_line`. pwsh `Set-Location`s to it and
`__furet_record` books the landing with `furet add --source jump`; the
prompt hook records every other directory change the same way.
