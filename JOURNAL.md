## 2026-09-26 — branch main — v0.1.1
Done: lot 41 — opt-in nucleo stage 1 (`nucleo-matcher` 0.3.1, MPL-2.0): `engine` key, `query --engine`,
`engine:` explain line; `rank` builds one matcher per call (private `Scorer`), tokens §7.1-normalized first.
Decisions: Hervé's 1–7; `nucleo.toml` (8 cases) verified by rendering `explain::explain` + `render` per case
in a deleted scratch test, plus the CLI `--engine nucleo --explain` snapshot. Gaps vs §7.1, not patched:
`Αθήνα`/`й` names miss even their own spelling (4+ chars → stage 2 score 3), NFD name 202 vs 218,
`o` matches `ø`; `ß`/`İ` agree. Extra: lot-40 cli assertion gains the `engine:` line; `decision.rs` passes
`Engine::Reference`. Next: reviewer pass.

## 2026-09-26 — branch main — v0.1.1
Done: lot 40b — `-l` is now a declared switch on `f`/`fi`
(`[Alias('l')] [switch] $Local` before the remaining-arguments parameter;
`--local` still stripped by hand, `f -- -l` passes `-l` as query text), and
the lot 40 `-Native` completer fallback is deleted — the regular `FuretArgs`
completer now handles `-l` lines too. Lot 40's pwsh tests unchanged; 3 new
integration tests (flag after query, long flag, `f --local cl<Tab>`), 1 new
unit test.
Decisions: Hervé's, after the lot 40 review — the native fallback relied on
undocumented pwsh behavior. Next: reviewer pass.

## 2026-09-26 — branch main — v0.1.1
Done: lot 40 — `furet query --local` (`-l`): the pool is scoped to the git
project (nearest ancestor with a `.git` directory **or** file) via pure
`src/project.rs` (`GitMarker` trait); entries outside are dropped before
any reconcile, the disk fallback stops at the root (`Options::stop_at`),
`--explain` gains a `project root:` line, an empty query prints the root
before the DB opens, exit 1 outside a repo. pwsh: `f -l` (root jump with
`--source jump`), `f -l --explain` forwards `--local`, `fi -l` scopes fzf
+ menu, `f -l <Tab>` completes in-project. 6 unit + 6 cli + 6 executed
pwsh tests; query help snapshot; README/CONTRACTS/ARCHITECTURE/example.md.
Decisions: one beyond the prompt — a first `-l` token defeats pwsh's
parameter binding, so the regular FuretArgs completer never fires for
those lines; a second `-Native` registration reconstructs the word from
the raw line + cursor column (no overlap verified on pwsh 7.6.6). zoxide
check: no project/base-directory scoping exists — closest is
`_ZO_EXCLUDE_DIRS`, a global default-`$HOME` filter; `--local` is a furet
extension. Next: reviewer pass.

## 2026-09-26 — branch main — v0.1.1
Done: lot 39 — `furet init pwsh` output now leads with a clap_complete
4.6.11 (MIT OR Apache-2.0) PowerShell block (native completer for
`furet`'s subcommands and options), then the integration script; `--cmd`
doesn't touch it. Unlike zoxide, whose `build.rs` generates
`contrib/completions/_zoxide.ps1` via `clap_complete::generate_to`,
completions shipped as separate files. 2 cli + 3 executed pwsh tests.
Decisions: one beyond the prompt (Hervé 2026-09-26): clap's block opens
with a blank line; kept byte-identical, the test pins the first non-empty
stdout line. Next: reviewer pass.

## 2026-09-26 — branch main — v0.1.1
Done: lot 38 — `exclude_dirs` config key (array, default empty): patterns in
the `furet remove` syntax (name segment, absolute path, `*`-prefixed) of
directories never recorded — honored by `furet add` (silent exit 0, DB not
opened), the query's disk fallback (jump stands, `result_dir_id` NULL) and
`furet import zoxide` (new `excluded` bucket in the summary). Pure
`remove::exclusion_target`; 6 unit + 8 cli tests (3 import summaries
extended); README/CONTRACTS/example.md.
Decisions: none beyond the prompt (Hervé 2026-09-26: empty default, every
write site honors it, known directories untouched). Next: reviewer pass.

## 2026-09-26 — branch main — v0.1.1
Done: lot 37 — remove matching: a name pattern matches any normal path
segment (`*appdata*` selects everything known under `AppData`, `ombi*` its
known subdirectories; the drive prefix is never a segment), and a path
pattern starting with `*` is matched as is against the full lowercased
path, never joined to the cwd. 1 replaced + 1 new unit test, 2 new cli
tests; README/CONTRACTS/example.md.
Decisions: none beyond the prompt (Hervé 2026-09-26). Next: reviewer pass.

## 2026-09-26 — branch main — v0.1.1
Done: lot 36 — `furet remove <pattern> [--confirm]`: hard-deletes known dirs
matching a name pattern (no separator) or a path pattern (`\`/`/`/`:`,
`.`/`..`), `*` crossing `\`, `?` one char, case-insensitive;
`storage::remove_dirs` drops the dir's visits/queries and unlinks other
visits' `from_dir_id` in one transaction; report on stderr only, stdout
untouched. Pure `src/remove.rs` (8 unit tests), `paths::absolute_key`, 13 cli
tests, 2 help snapshots; README/CONTRACTS/DATABASE/example.md.
Decisions: none beyond the prompt (Hervé 2026-09-26: hard delete, name vs
path mode, no per-match confirmation; not in SPEC — zoxide takes exact paths only). Next: reviewer pass.

## 2026-09-26 — branch main — v0.1.1
Done: lot 35 — `furet list` (plain, `--all`, `--paths`) is now ordered by
`dirs.key ASC` (lowercased path, binary collation), not by last visit;
zero-visit rows are no longer pushed last. `dir_listing` only; one
replaced + one touched storage test, one replaced + one deleted + one
updated cli test; CONTRACTS/README/example.md.
Decisions: none beyond the prompt (Hervé: no flag restores the recency
order, `furet query --list` covers it). Next: reviewer pass from a Claude
Code session.

## 2026-09-26 — branch main — v0.1.1
Done: lot 34 — `furet list --paths` (`-p`) prints only the path of each
directory, one per line, same rows and order as `furet list`;
`--all --paths` keeps missing rows without the presence column; the default
output is unchanged. 4 new cli tests, `help__list_help.snap` updated;
README/CONTRACTS/example.md.
Decisions: none beyond the prompt (Hervé: opt-in flag, no presence column
with `--all`). Next: reviewer pass from a Claude Code session.

## 2026-09-23 — branch main — v0.1.1
Done: lot 33 — `retention_days` config key (default 365, 0 = keep all);
every `furet add` deletes `visits`/`queries` rows older than that via
`storage::purge_before` (indexed by lot 32); `dirs` never deleted. `add`
now reads config.toml. 4 config + 1 storage + 2 cli tests; explain.rs
fixtures opt out (2023-dated visits). README/CONTRACTS/DATABASE updated.
Decisions: Hervé's — purge both tables, in `add`, 0 disables; CONTRACTS
records the SPEC §5/§8 discrepancy. Next: reviewer pass over lots 28–33.

## 2026-09-23 — branch main — v0.1.1
Done: lot 32 — migration 3 adds `idx_visits_ts` and `idx_queries_ts`, so
lot 33's age-based purge on every `furet add` never scans either journal.
1 new + 5 updated storage tests; DATABASE.md pragma/index tables updated.
Decisions: none. Next: lot 33 (retention purge, `retention_days`).

## 2026-09-23 — branch main — v0.1.1
Done: lot 31 — a non-empty `furet query` stats only the directories its
query matches (rank with stored flags ignored, reconcile the matches, drop
the missing ones); empty query and `--explain` still check every row.
2 new cli tests; CONTRACTS.md notes the SPEC §10 discrepancy.
Decisions: strategy "only the matching directories" chosen by Hervé over
top-K and a grace-period column. Next: lot 32 (visits/queries ts indexes).

## 2026-09-23 — branch main — v0.1.1
Done: lot 30 — `furet query` ranks the database pool once (the old
`resolve_pool` ranked it fully just to test emptiness, then it was ranked
again); the fallback pool is built only when that ranking is empty.
`upsert_dir` is one cached `INSERT ... RETURNING id` instead of INSERT +
SELECT. Refactor only; existing suites green.
Decisions: none. Next: lot 31 (P5, reconcile only matching candidates).

## 2026-09-23 — branch main — v0.1.1
Done: lot 29 — an empty `FURET_DATA_DIR` counts as unset (security audit
F3): it used to resolve to a database relative to the current directory.
Pure `resolve_data_dir` helper + 1 unit test without env mutation;
DATABASE.md and README.md wording updated.
Decisions: none. Next: lot 30 (single ranking pass, upsert RETURNING).

## 2026-09-23 — branch main — v0.1.1
Done: lot 28 — `furet query`'s `<QUERY>` is optional (default empty), fixing
`fi`'s fzf branch whose initial `furet query --list --color` was rejected by
clap (exit 2), so fzf always started empty. 1 new cli test, query help
snapshot and CONTRACTS.md updated.
Decisions: security-audit finding F2 (fzf `{q}` under cmd.exe) checked
against fzf 0.74.2 and dropped: fzf double-quotes and `^`-escapes `{q}`.
Next: lot 29 (empty FURET_DATA_DIR).

## 2026-09-22 — branch main — v0.1.1
Done: lot 27 — migration 2 adds `idx_visits_dir_ts` and `idx_visits_session_ts`
(covers `dir_entries`'s GROUP BY join and `last_visited_dir`'s session filter);
`configure()` now sets `synchronous = NORMAL` and a 5 s `busy_timeout` on every
connection. Measured on a 200k-visit copy: dir_entries 61→12 ms, back 12→0.01 ms,
list 274→22 ms, one-row commit 1.16→0.03 ms. 2 new + 3 updated storage tests;
DATABASE.md pragma/index tables updated.
Decisions: none beyond the prompt. Next: reviewer pass.

## 2026-09-20 — branch main — v0.1.1
Done: lot 26 — new `furet list [--all]`: one tab-separated line per known
directory (`path`, `visits`, `last_visit`, `first_seen`, local time formatted
by SQLite; `--all` keeps missing rows and adds a presence column), ordered by
last visit desc then path asc, zero-visit rows last; read-only, no reconcile.
New `storage::dir_listing`; 7 cli.rs tests, 3 storage unit tests, 1 new + 1
updated help snapshot; README/CONTRACTS/example.md updated.
Decisions: none beyond the prompt. Next: reviewer pass.

## 2026-09-20 — branch main — v0.1.1
Done: lot 25 — the pwsh jump function registers an argument completer:
plain `<Tab>` on the first, single argument token cycles through `furet
query --list`'s ranked lines (empty word = recency; `-`/`..`+ excluded;
spaces/`'` single-quoted, `'` doubled); stderr discarded, LASTEXITCODE
snapshotted+restored. 7 executed pwsh tests, 1 unit test, docs updated.
Decisions: zoxide has no pwsh completer at all — its `z foo<Space><Tab>`
(zsh/fish/bash templates) triggers on a trailing space and rewrites the
whole line to `z <result>`; here (Hervé's lot prompt) plain `<Tab>` on the
current word, one CompletionResult per line. Next: reviewer pass.

## 2026-09-20 — branch main — v0.1.1
Done: Hervé's review fixup on lot 22 — the flat twin in the camelCase hump
case moved from `/dev/toolwindows` to `/archive/toolwindows` so the two paths
no longer share a case-insensitive comparison key; expectation unchanged and
scenarios green.
Decisions: none — `/archive` cannot match query `tw`, so no folder bonus on
either side and the 43 vs 33 outcome stands.
Next: none assigned.

## 2026-09-20 — branch main — v0.1.1
Done: lot 24 — the `--help`/`-h` trailer now also prints `Config file: …
(found | not found, defaults apply)` and `Log directory:` beside the database
line; `storage::config_path`/`logs_dir` are the single source of truth
(`load_settings` and `logging::init` now call them); six stale help strings
fixed (query fallback, `--list` empty-query recency, `--cmd`/`fi`,
`--failures` mandatory, import stdin/tool); every help page pinned by ten
insta snapshots in `tests/help.rs` (data dir redacted to `<DATA_DIR>`).
Decisions: added the `regex` dev-dependency and insta's `filters` feature
for the redaction. Next: reviewer pass from a Claude Code session.

## 2026-09-20 — branch main — v0.1.1
Done: lot 23 — docs-only, brought the docs back in line with the code:
ROADMAP rows 12–21, README Status (lots 0–21 + the lot-22 scenario suite),
ARCHITECTURE's new modules with their layer, DATABASE.md's `session`
(`fallback`, `import`) and `source` values, CONTRACTS fixes (stdout opener
now names `--list`/`up`/`back`/`home`/`init pwsh`; `main::report` line
148 → 183), `stage2::TYPO_MIN_QUERY_LEN` doc no longer says "future".
Decisions: none — every sentence checked against the code; example.md
verified against the CLI unchanged. Next: none assigned.

## 2026-09-20 — branch main — v0.1.1
Done: lot 22 — 20 new SPEC §13 scenario cases: 5 new theme files
(`normalization`, `multi_token`, `folder_bonus`, `d1`, `menu`) and 4
`exclusions.toml` extensions; 42 cases total, green on first run.
Decisions: every expectation was hand-derived from SPEC §7–§9, then
cross-checked against the engine's `--explain` on a throwaway database before
the run; the order-bonus case pairs real `neovim` with synthetic `vimneo` (no
real OSS name carries both tokens twice); `ToolWindows` vs `toolwindows`
cannot coexist on a case-insensitive disk, but the scenario runner reads TOML
directly. Next: none assigned.

## 2026-09-20 — branch main — v0.1.1
Done: lot 21 — stage 2's accepted distance now depends on the query length:
`stage2::max_distance` returns 1 below `LONG_QUERY_MIN_LEN = 6` normalized
characters, `MAX_DISTANCE = 2` from there on. `--explain` states the applied
ceiling (`Report::stage2_max_distance`). New `tests/scenarios/stage2_length.toml`
(5 cases), 5 unit tests, 1 proptest.
Decisions: Hervé's call, a deliberate departure from SPEC §7.3 (documented in
CONTRACTS.md); the matched line now reads `(max 1)` too, not just the
elimination reason. Fixtures that relied on `tokio`/`tokei` at distance 2 use
`tokyo` (distance 1). Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: fixed a bug in `furet import zoxide` where same-key candidates (e.g.
two case variants) were both planned and both inserted; `import::dedupe_by_key`
now collapses them (highest score wins, path asc breaks a tie) before the
known-keys filter. Stderr summary gained a `duplicate` bucket.
Decisions: `known` (already in `dirs`) and `duplicate` (collided within this
batch) are separate counters, not merged — Hervé's call. `known` is now
computed on the deduped set so a duplicate that is also known counts once.
CONTRACTS.md updated. Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 20 — `furet import zoxide` reads `<score> <path>` lines from
stdin, canonicalizes each path (directories only), skips already-known
directories, and records the rest with synthetic timestamps (score desc,
path asc) under `source = session = 'import'`, one transaction. New pure
`src/import.rs` (11 unit tests); `storage::known_keys`; 6 cli.rs e2e tests,
1 executed pwsh test (accented path).
Decisions: score only orders the import, never stored or reused (D1 stays
the only recency rule); malformed includes non-UTF-8 lines, not a stdin
read error. README/CONTRACTS/example.md updated. Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: `paths::canonical` now rejects a resolved path that is not a directory
(SPEC §1), returning a new `PathError::NotADirectory` variant alongside the
renamed `PathError::Canonicalize`; `home_command` reuses `paths::unify_separators`
(now `pub`) instead of its own `.replace`. 2 paths unit tests, 2 cli.rs e2e
tests (`add_rejects_a_file_path`, `home_prints_nothing_and_warns_when_home_is_a_file`).
Decisions: audited every `canonical`/`resolve` caller (`add`, `query`'s cwd,
`up`, `fallback`, `home`) — all only ever see real directories or user input
meant to be one, so directories-only is correct everywhere; CONTRACTS.md
updated. Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 19 — new `home` config key (`config::Settings.home`, absolute path,
`/` accepted) and `furet home` subcommand: prints the canonicalized value or
nothing (warns on a relative path or a missing directory, silent when
unset); pwsh's no-arg `f` now calls `furet home` and falls back to `$HOME`
on empty output. 4 config unit tests, 4 cli.rs tests, 2 executed pwsh tests.
Decisions: `main::home_command` never fails, mirroring the lot's "never
fails because of config" rule; README/CONTRACTS/example.md updated.
Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 18 review fixes — the fallback test now calls a real `prompt` after
`Set-Location`, so `f -` returns to `projects` (session-second-to-last), not
`origin`; added `last_visit_session` assertion (not seed/fallback, non-empty)
on that test and on the plain jump test; `run_pwsh` builds PATH via
`env::split_paths`/`join_paths` instead of a hard-coded `;`.
Decisions: none beyond the fixes as specified; test-only, no contract change.
Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 18 — new `tests/pwsh.rs`: 16 tests run the real `furet init pwsh`
script in a real `pwsh -NoProfile -NonInteractive -File` process, asserting
on the printed cwd, stdout/stderr, and the sqlite visits, covering all 15
behaviors from the lot prompt (`f`'s jump/no-match/direct-path/up/back/dot/
home/--explain/fallback-then-back, hook dedup + preservation, `fi`'s
no-fzf menu + cancel and fzf branch, `--cmd` rename).
Decisions: `Sandbox`/`db`/`scalar` copied from `tests/cli.rs`, not shared;
direct-path `f` calls use absolute quoted literals to avoid an incidental
disk-fallback visit for non-child sibling names.
Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 17 — SPEC §4 `f <query> --explain` in pwsh: the `f` function
detects a whole-token `--explain` anywhere in the arguments, strips every
occurrence, rebuilds the query (same / → \), runs `furet query --explain --
$query` without moving or recording; `fi` untouched; 3 script-text tests;
README Usage line; example.md shows `f mcp --explain` (direct form noted),
menu sample matched to `render_menu`, bare `furet queries` removed.
Decisions: branch placed first so bare `f --explain` reports instead of
jumping home; smoke-tested the real pwsh run end to end.
Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 15 follow-ups — `fallback.depth` now bounded to `1..=5`,
`fallback.up` to `0..=5` (`FALLBACK_MAX_DEPTH`/`FALLBACK_MAX_UP`), out of
range warns and keeps the default; `exclude = []` documented in README and
CONTRACTS; `///` removed from the five private `main.rs` functions.
Decisions: `InitShell`'s clap docs kept — absent from the exception list, but
they are `--help` text like `Command`'s, so removing them would change the CLI.
Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 15 — SPEC §16 `config.toml` overrides: new pure `src/config.rs`
(`parse(text) -> (Settings, Vec<String>)`); wired `typo_min_length` and the
`fallback.*` keys through `stage2`, `rank`, and a new `fallback::Options`
struct; loaded once in `main`'s `query` path only, per-key fallback with a
`warn!` naming the bad key; `ambiguity`/`keyboard_layout`/`engine` recognized
and ignored with a "not supported yet" warning.
Decisions: `toml::Table` (not `toml::Value`) parses a document; unsupported
keys stay unwired per the lot prompt.
Next: none assigned.

## 2026-09-20 — branch main — v0.1.0
Done: lot 14 — SPEC §17 file logging: every invocation writes plain-text
logs (tracing + tracing-appender, daily rotation, `furet.<date>.log`, 7 kept)
to `<data dir>/logs/`; default level `info`, overridable via `FURET_LOG`
(EnvFilter syntax); WorkerGuard dropped before `process::exit`; init failure
silently disables logging; 5 end-to-end tests; outer layers instrumented
(main, storage, fallback, soft_delete, calibration), pure core untouched.
Decisions: `storage::data_dir()` factored out so logs follow FURET_DATA_DIR;
a failed query logs an info outcome plus the error at `report`.
Next: none assigned.

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
