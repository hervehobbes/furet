# Contracts

## Fuzzy engine contract

Pure, no filesystem or clock access. `rank::rank(query, current_dir, candidates, typo_min_length, engine)`
drops the current directory and any `missing` candidate, scores every
survivor like `rank::dispatch` (the `engine`'s stage 1 first, stage 2 only
when stage 1 returns `None`), and returns a `Vec<Scored>` sorted per SPEC §8: score, then
recency (D1 — never the other way around), then name length, then path, as a
strict deterministic total order.

### Query memory (SPEC-v2 §24)

Hervé's decision of 2026-09-26 (`CLAUDE.md`, D1) adds one criterion above
the score, and only this one: the sort order of `furet query` is **query
memory, score, recency, name length, path**. It is not recency, and
recency still never outranks a better match.

- `rank::rank` and `decision::decide` are unchanged. `memory::promote(ranked,
  remembered) -> Vec<Scored>` (pure) moves the remembered directory, matched
  by path ignoring case like `rank`'s current-directory check, to the front
  when it is among `ranked`; every other candidate keeps `rank`'s relative
  order, so D1 still holds among them. A remembered directory absent from
  `ranked` changes nothing. `memory::applies(ranked, remembered)` says
  whether it is among them. Proven by proptest in `src/memory.rs`: no
  memory or an unranked memory gives exactly `rank`'s order; a ranked
  memory comes first with the others' relative order kept; the result is
  deterministic (input order included) and keeps every `Scored` exactly
  once; D1 holds among every directory but the remembered one.
- The **memory key** (`memory::key`) is the query split on whitespace, each
  token normalized with SPEC §7.1, empty tokens dropped, joined by one
  space: case, §7.1 accents, and extra spaces never change it, token order
  does (`tok io` ≠ `io tok`). `memory::matches(text, key)` answers `key(text)
  == key` without allocating for ASCII text (proptest
  `matches_agrees_with_comparing_keys`).
- The **remembered directory** of a key is the `result_dir_id` of the most
  recent `queries` row with that key, `outcome IN ('jump', 'pick')`, and a
  non-null `result_dir_id` (`ts` descending, then `id` descending). If that
  row is a probable failure (SPEC §15, decided by
  `calibration::failure_of`, the per-row function `probable_failures` is
  built on), there is no memory: the latest word wins, an older row is never
  used. Because every jump journals a `jump` row, a query that jumped once
  keeps going to the same directory until a probable failure or a different
  choice (menu, `fi`) replaces it.
- The §9 decision: when the memory applied, the call site decides
  `Jump(first)` directly, even on a stage-2 tie that would open a menu;
  otherwise `decision::decide` runs as before.

### Stage 1 — optimal subsequence (SPEC §7.2)

`stage1::score(query, name, folder) -> Option<u32>` (`src/stage1.rs`). The
query is split on whitespace; every token must be a subsequence of `name`
(logical AND) or the whole match is `None`
(`rejects_a_token_that_is_not_a_subsequence`). Each token's placement is the
maximum-scoring one found by a two-row O(|query|×|name|) DP, not the first
greedy one (`the_optimal_placement_beats_the_first_greedy_one`).

Per-token score = `TOKEN_BASE + length + placement + prefix + density`,
where:

| Bonus | Value | Constant | Pinned by |
|---|---|---|---|
| Token base | `+10` | `TOKEN_BASE` | `explain_reports_the_same_total_as_score_on_an_exact_match` |
| Consecutive placement | `+8` per char placed right after the previous one | `CONSECUTIVE_BONUS` | `the_consecutive_bonus_adds_eight_per_adjacent_character` |
| Word start | `+10` — index 0, after a separator (`. - _ / \` space `:` `(`), or a camelCase hump read on the *original* string | `WORD_START_BONUS` | `a_word_start_after_a_separator_adds_ten`, `a_camel_hump_read_on_the_original_string_adds_ten` |
| Name start | `+8` if the token's first character lands at index 0 | `NAME_START_BONUS` | `the_name_start_bonus_needs_the_first_character_at_index_zero` |
| Prefix | `+10` if the normalized candidate starts with the whole token | `PREFIX_BONUS` | `the_prefix_bonus_needs_a_whole_string_prefix` |
| Density | `(10 × token_len) / name_len`, truncating integer division | `DENSITY_FACTOR = 10` | `the_density_bonus_uses_truncating_division` |
| Gap | `−1` per skipped character, uncapped | `GAP_PENALTY = 1` | `the_gap_penalty_removes_one_point_per_skipped_character` |

Order bonus **+5** (`ORDER_BONUS`) once, only if every token's match start
is strictly increasing in typed order
(`the_order_bonus_needs_strictly_increasing_token_starts`). Folder bonus
**+2** (`FOLDER_BONUS`) once, only if every token is also a subsequence of
the joined parent segments
(`the_folder_bonus_adds_two_when_the_query_also_matches_the_folder`). Total
= sum of token scores + order bonus, floored at `SCORE_FLOOR = 4`, then
folder bonus added on top of the floor
(`the_folder_bonus_is_added_on_top_of_the_floor`,
`the_score_is_floored_at_four`). The floor is the structural guarantee that
stage 1 always outscores stage 2 (`stage2::SCORE_CAP = 3`); every table
value above matches SPEC §7.2 as written.

### Stage 1 — opt-in nucleo engine (SPEC-v2 §20)

`rank::Engine { Reference, Nucleo }` picks the stage-1 scorer;
`Reference` (the table above) is the default. `Engine::Nucleo` swaps in
`stage1_nucleo::NucleoScorer` (`src/stage1_nucleo.rs`, crate
`nucleo-matcher` 0.3.1, MPL-2.0, Helix's matcher) and changes nothing
else: stage 2, the SPEC §8 order, D1, and the §9 decision are shared.
`rank` builds one `NucleoScorer` (one `nucleo_matcher::Matcher`) per call
and scores every candidate through it; only the standalone
`rank::dispatch` builds its own per call
(`nucleo_rank_scores_every_candidate_like_a_standalone_dispatch`).

- The query is split on whitespace, as in §7.2. Each token is normalized
  with §7.1 (`normalize::Normalized`) and matched against the **original**
  name (last segment) by a fuzzy `Atom` built directly — never parsed, so
  `^ $ ! '` are ordinary characters
  (`pattern_syntax_characters_are_matched_literally`) — with
  `CaseMatching::Ignore`, `Normalization::Smart`, and `Config::DEFAULT`
  (not the path config). The name keeps nucleo's word-boundary and
  camelCase bonuses.
- Every token must match (logical AND), else `None`
  (`a_token_missing_from_the_name_fails_the_whole_query`); an empty token
  after §7.1 is `None`, as in §7.2.
- Total = sum of the token scores, floored at `SCORE_FLOOR = 4`, then the
  reference `+2` folder bonus (`stage1::folder_bonus`, the exact code and
  §7.1 normalization `stage1::score` uses) on top of the floor
  (`the_folder_bonus_is_added_after_the_floor`). **No order bonus.** A
  matched nucleo token scored at least 16 in every probe, so the floor is
  a guarantee rather than a frequent case. The invariants are unchanged:
  every nucleo stage-1 score is `>= SCORE_FLOOR > SCORE_CAP`
  (`a_nucleo_score_never_falls_below_the_floor_nor_into_stage_two_range`,
  `nucleo_every_stage_one_match_ranks_before_every_stage_two_match`), the
  order is total and input-independent
  (`nucleo_ranked_keys_are_strictly_ordered_whatever_the_input_order`), and
  D1 holds (`nucleo_d1_making_every_weaker_match_more_recent_never_lifts_it`).

nucleo matches with **its own normalization, not §7.1**: only the query
tokens go through §7.1 first (Hervé, 2026-09-26: `Normalization::Smart`
folds one way only, so an accented query such as `réunions` would
otherwise miss `Reunions`). Gaps observed against the reference engine,
documented and not patched (`src/stage1_nucleo.rs` tests):

| Input | Reference | nucleo | Pinned by |
|---|---|---|---|
| Non-Latin letter with a diacritic in the name: `Αθήνα` | matches `αθηνα` and `Αθήνα` | stage 1 misses both, even the name's own spelling; a query of 4+ characters still lands in stage 2 at score 3 | `a_greek_name_with_a_tonos_is_not_matched_even_by_its_own_spelling` |
| Cyrillic `й` in the name | matches `и` and `й` | misses both (a 1-character query never reaches stage 2); `й` → `и` in the query, so the query `й` does match a name `и` | `a_cyrillic_short_i_name_is_not_matched_even_by_its_own_spelling` |
| Name already NFD-decomposed: `Re\u{301}unions` | same score as `Réunions` | matches, lower score (202 vs 218 for `reunions`) | `an_nfd_decomposed_name_matches_with_a_lower_score_than_its_composed_form` |
| `ø` in the name | `o` does not match | `o` matches `ø` | `a_plain_o_query_matches_o_with_stroke_unlike_the_reference` |
| `ß` / `ẞ` | fold to each other, never to `ss` | same | `sharp_s_folds_to_its_capital_but_never_to_ss` |
| `İ` | matches `i` | same | `a_dotted_capital_i_name_matches_a_plain_i_query` |

### Stage 2 — typo tolerance (SPEC §7.3)

`stage2::score(query, name, min_length) -> Option<u32>` (`src/stage2.rs`),
reached only when stage 1 returns `None`. Eligible only for a mono-token
query (exactly one whitespace-separated token,
`a_multi_token_query_never_matches`) of at least `min_length` characters —
`TYPO_MIN_QUERY_LEN = 4` by default, overridable by `typo_min_length`
(SPEC §16) — (`a_query_shorter_than_the_threshold_never_matches`), scored
against every
sliding window of `name` of length `|query| ± 2`, clamped to `name`'s length
(`a_sliding_window_matches_a_fragment_of_a_longer_name`). Score =
`SCORE_CAP - distance` = `3 - distance`
(`a_one_edit_typo_scores_two`, `a_distance_of_two_scores_one_and_a_distance_of_three_scores_nothing`).

The accepted distance depends on the query's length, in normalized
characters (`stage2::max_distance`): below `LONG_QUERY_MIN_LEN = 6`,
`SHORT_QUERY_MAX_DISTANCE = 1`; from 6 on, `MAX_DISTANCE = 2`
(`the_maximum_distance_depends_on_the_query_length`,
`five_characters_refuse_the_distance_six_characters_accept`,
`a_query_shorter_than_six_characters_never_accepts_two_edits`).
`stage2::explain` still returns the raw distance, so `--explain` reports
both the applied ceiling (`distance 1 (max 1)`) and the reason behind a
rejection (`distance 2 > 1`), keyed on `Report::stage2_max_distance`
(`an_elimination_names_the_threshold_the_query_length_applies`).

**Discrepancy**: SPEC §7.3 calls for full Damerau-Levenshtein distance
(unrestricted transpositions). The implementation uses optimal string
alignment (OSA) instead — each substring may be edited at most once, so
`distance("ca", "abc")` is 3 under OSA but 2 under true Damerau-Levenshtein.
Deliberate lot-2 decision (`JOURNAL.md`, lot 2), pinned by
`an_adjacent_transposition_counts_as_a_single_edit` and the
`distance("ca", "abc") == 3` case; not reconciled with SPEC's wording.

**Discrepancy**: SPEC §7.3 allows `distance <= 2` for every eligible query.
The implementation caps a query shorter than 6 normalized characters at
distance 1, because two edits on a 4-character query rewrite half of it and
matched unrelated names. Deliberate lot-21 decision by Hervé, pinned by
`a_four_character_query_refuses_every_two_edit_name` and
`tests/scenarios/stage2_length.toml`.

`decision::decide(ranked) -> Decision` (`src/decision.rs`) applies SPEC §9:
a stage-1 best jumps unconditionally; a stage-2 best jumps alone at its
distance, else opens a `Menu` of the leading run of tied stage-2 candidates
(2..=9, `MENU_MAX_ENTRIES`).

## Configuration contract (SPEC §16)

`<data dir>/config.toml` (`storage::data_dir()`, same as the database and the
logs) overrides `config::Settings::default()`; loaded by `furet query`,
`furet home`, `furet add` (`retention_days`, `exclude_dirs`), and
`furet import zoxide` (`exclude_dirs`).
`config::parse(text) -> (Settings, Vec<String>)` is pure — no filesystem, no `tracing` — and never fails the caller.

Fallback is per key, not per file, except malformed TOML:

- an unknown key warns and is ignored;
- a key of the wrong type or out of its documented range warns and keeps
  that key's default;
- malformed TOML warns once and keeps every default.

Every warning `parse` returns is logged with `warn!` by `main::load_settings`;
a missing file logs one `debug!` and changes nothing. `fallback.exclude = []`
disables every exclusion — the list replaces the defaults, it never adds to
them.

`ambiguity` and `keyboard_layout` are recognized and ignored, each warning
"not supported yet" — SPEC §9 does not define what `ambiguity` measures, and
`keyboard_layout` names a feature not built yet (Hervé's decision, lot 15).

`engine` (SPEC-v2 §20) is `"reference"` (default) or `"nucleo"`; any other
string or a non-string warns `engine must be "reference" or "nucleo"; using
"reference"` and keeps `Reference` (`an_unknown_engine_name_warns_and_keeps_the_reference_engine`,
`a_wrong_type_engine_warns_and_keeps_the_reference_engine`). `furet query
--engine` overrides it.

`query_memory` (SPEC-v2 §24) is a boolean, `true` by default. `false`
disables query memory entirely — `furet query` does no lookup at all and
`--explain` prints `memory: disabled` — while menu and `fi` picks keep
being journaled. A non-boolean warns `query_memory must be a boolean; using
true` and keeps `true` (`a_wrong_type_query_memory_warns_and_keeps_true`).

`alias_prefix` (not in SPEC — scope extension decided by Hervé on
2026-10-02, see the **Alias queries** paragraph of the `furet query` entry)
is a one-character string naming the first character of an alias query:
`"!"` (default) or `"="` — a whitelist, decided because `@` needs AltGr on
an AZERTY keyboard and, worse, is **splatting** in PowerShell argument
mode: `f @ombi` with no `$ombi` variable would send `f` no argument at all
and silently jump home. Anything else — wrong type, the empty string,
`"!!"`, `"@"` — warns `alias_prefix must be '!' or '='; using '!'` and
keeps `!` (`an_invalid_alias_prefix_warns_and_keeps_bang`). Each allowed
prefix is proven by a real pwsh test (`f_bang_alias_jumps_and_records_one_jump_visit`,
`f_equals_alias_jumps_when_configured`).

`exclude_dirs` (not in SPEC — scope extension decided by Hervé 2026-09-26,
modelled on zoxide's `_ZO_EXCLUDE_DIRS`) is an array of non-empty strings,
each resolved by `remove::exclusion_target` with the same pattern syntax as
`furet remove`: a pattern without `\`, `/` or `:` matches any normal segment
of the path (`node_modules`, `*appdata*`); a path pattern must be absolute
(`C:\Windows\*`) or start with `*` (`*\target\*`); `*` crosses `\`, `?` is
one character, case is ignored. The default is the empty list — no behavior
change until the key is set (divergence from zoxide, whose default excludes
`$HOME`); unlike zoxide, which filters only `add`, every visit write honors
it: `furet add` (silent exit 0, nothing on stdout or stderr, the database is
not opened, `--from` unaffected), the query's disk-fallback recording (the
jump itself stands, nothing is written, and the `queries` row gets
`result_dir_id = NULL`), and `furet import zoxide` (counted in the new
`excluded` bucket before dedupe, so never as `known` or `duplicate`).
An absolute pattern without a wildcard excludes exactly that directory,
never its descendants. Not an array, or an item that is not a non-empty
string, warns `exclude_dirs must be an array of non-empty strings; using no
exclusion` and keeps the empty default (same rule as `fallback.exclude`);
an entry `exclusion_target` rejects (a relative path pattern like `a\b`)
warns `exclude_dirs entry '<item>' is a relative path; ignored` and is
dropped, the other entries kept. A directory already known that matches is
never deleted nor hidden — it gets no new visit but stays in the database
and in the results, until `furet remove <pattern>` purges it.

## Storage contract

SQLite at `FURET_DATA_DIR/furet.db` (else the platform local data dir), WAL
mode, foreign keys on, ordered `PRAGMA user_version` migrations.

- `dirs(id, path, key, first_seen, missing_since)` — `key` is the
  lowercased on-disk-cased path, unique; `missing_since` is set/cleared by
  soft delete (SPEC §10) and never deletes the row. A non-empty `furet
  query` checks on disk only the rows its query matches (flagging or
  reactivating them before the decision); an empty query and `--explain`
  check every row. **Discrepancy** with SPEC §10's "absent at query time":
  an unmatched vanished directory keeps its flag until a query matches it
  (Hervé's decision, lot 31 — the per-query cost no longer grows with the
  number of known directories).
- `visits(id, dir_id, ts, source, session, from_dir_id)` — `source` is one
  of `hook | jump | back | up | fallback | import`; one row per recorded
  visit, never updated; deleted only by the retention purge below and by
  `furet remove` (`storage::remove_dirs` unlinks surviving visits'
  `from_dir_id` instead of cascading).
- `queries(id, ts, cwd, query, result_dir_id, stage, outcome)` — the SPEC §15
  journal; `stage` is one of `'1' | '2' | 'fallback' | 'menu'`; `outcome` is
  one of `'jump' | 'none' | 'menu' | 'pick'` (SPEC-v2 §24). A `pick`
  journals a menu choice, written after the answer: `result_dir_id` is the
  chosen directory — database menus look it up by key, a fallback menu
  choice gets its `dirs` row through `record_fallback_visit` first and
  reuses that id, and only an excluded (`exclude_dirs`) fallback choice
  keeps `NULL` — with the stage the query had computed (`'menu'` or
  `'fallback'`). A cancelled menu (empty, invalid, or EOF answer) writes
  exactly the pre-lot-46 row: same stage, `outcome = 'menu'`,
  `result_dir_id = NULL`, same error and exit code.
  `storage::recall(conn, key)` (read-only, no schema change, no index)
  finds the remembered directory of a memory key: one sequential scan of the
  `jump`/`pick` rows with a result, comparing each text with
  `memory::matches` and keeping the greatest `(ts, id)`; then only the
  `visits` with `ts >=` that row's `ts` are read for
  `calibration::failure_of`. It returns `memory::Recall`: `Nothing`,
  `ProbableFailure`, or `Remembered { path, chosen }`, `chosen` being the
  row's `ts` in the `furet list` local-time format. Measured on 20,000
  `queries` rows (lot 47): under 3 ms of median overhead per query.
  `retention_days` purges old rows, so the memory forgets them, and `furet
  remove` deletes a directory's `queries` rows, so its memory goes with it.
- Retention: every `furet add` runs `storage::purge_before(now -
  retention_days)`, deleting older `visits` and `queries` rows (indexed by
  `idx_visits_ts` / `idx_queries_ts`); `dirs` rows are deleted only by
  `furet remove`, and a directory with no visit left ranks on `first_seen`.
  `retention_days = 0` disables it. **Discrepancy** with SPEC §5 (append-only event journal) and
  §8 (frequency recorded for later use): history is bounded to one year by
  default (Hervé's decision, lot 33).

## CLI contract

Only jump targets reach stdout: `furet query`'s resolved match and its
`--list` lines, `up`'s ancestor, `back`'s previous directory, and `home`'s
configured directory — plus `init pwsh`'s generated script. Everything
else (menus, `--explain`, errors) goes to stderr, with four documented
exceptions below. Every invocation also writes structured logs to
`<data dir>/logs/` (SPEC §17) — never to stdout or stderr. The rule is
compile-time enforced: `clippy::print_stdout` + `disallowed-methods` on
`std::io::stdout` (`[lints]` in `Cargo.toml`, `clippy.toml`), with every
write funnelled through `stdout_line` in `src/main.rs`, the only
`#[allow]` holder. `unsafe_code` is `deny`, not `forbid`, because
`storage.rs`'s `furet_data_dir_overrides_the_database_location` test
needs edition-2024 `unsafe` env mutation (one fn-scoped allow). Outside
the binary, `build.rs` holds the one other `print_stdout` allow —
fn-scoped on its `main`, for the `cargo:rustc-env` directive a build
script must print on stdout; it lives outside `src/`, and its output
goes to cargo, never to furet's stdout.

`furet --help`/`-h` ends with four trailer lines — three runtime-resolved,
`Database file:`, `Config file: … (found | not found, defaults apply)`, and
`Log directory:`, all built from `storage::data_dir()` (honors
`FURET_DATA_DIR`); an unresolvable location prints `unavailable: <error>`
instead of failing — plus `Build date: YYYYMMDD`, the UTC date of the
binary's last compilation, set at compile time by `build.rs` through the
`FURET_BUILD_DATE` rustc-env variable. The build script emits no rerun
directive, so cargo reruns it whenever a package file changes and the date
tracks the last real compilation; a no-op rebuild keeps the old date.
`storage::config_path`/`storage::logs_dir` are the single
source of truth for those locations (`load_settings` and `logging::init` call
them too). Pinned by `help_prints_the_database_file_path_resolved_at_runtime`,
`help_ends_with_the_utc_build_date`, and the `tests/help.rs` insta snapshots
(data dir redacted to `<DATA_DIR>`, build date to `<BUILD_DATE>`).

- `furet add <path> --session <s> [--source <src>] [--from <dir>] [--query <text>]` — records
  one visit; `source` defaults to `hook`. A path matching `exclude_dirs` is
  not recorded (exit 0, nothing on stdout or stderr, the database is not
  opened; `--from` is unaffected, and `--query` writes nothing either).
  `--query` (SPEC-v2 §24) requires `--from` (clap exits 2 without it); a
  non-blank `<text>` is journaled after the visit as one `queries` row —
  `outcome = 'pick'`, `stage = 'menu'`, `result_dir_id` = the added
  directory, `ts` = the visit's timestamp (one `clock.now()` covers both, so
  §15 calibration sees the landing visit), `cwd` = the canonical `--from`
  path (the same format `furet query` writes), `query` = the text as typed.
  A blank or whitespace-only text writes no row; the retention purge still
  runs after these writes.
- `furet query [<text>] [--list] [--explain] [--color] [--no-ignore] [--local] [--engine <reference|nucleo>]` — ranks
  recorded directories, falling back to a disk walk (SPEC §11) when nothing
  matches; prints the jump target to stdout, or the SPEC §9 menu to stderr
  on a stage-2 tie, reading the answer from stdin. The menu's journal row is
  written **after** the answer (SPEC-v2 §24): a valid choice writes
  `outcome = 'pick'` with `result_dir_id` = the chosen directory (see the
  storage contract above for the fallback and excluded cases), while a
  cancelled menu (empty, invalid, or EOF answer) writes the pre-lot-46 row —
  `outcome = 'menu'`, `result_dir_id = NULL` — with the same error and exit
  code. An omitted `<text>` is
  the empty query (`fi` relies on it for its initial fzf list).
  `--local` (`-l`, SPEC §19, Hervé 2026-09-26) restricts the candidate pool
  to the current **git project**: the project root is the nearest of the
  canonical current directory and its ancestors that holds a `.git` entry —
  a directory or a file, so worktrees and submodules count
  (`src/project.rs`: the pure `root` over the injectable `GitMarker` trait,
  `RealGitMarker` checking `Path::join(".git").exists()`). The root is
  computed before `storage::open`; when it does not exist the command fails
  with `furet: not inside a git repository`, exit 1, in every mode (plain,
  `--list`, `--explain`) and nothing is written. Entries outside the root
  (`project::within` on the lowercased keys: the root itself, everything
  below it, never a sibling sharing a text prefix) are dropped right after
  `storage::dir_entries`, before any reconcile, so reconcile, `--list`,
  ranking, decision, and journal all see only the scoped pool; the usual
  current-directory and `missing` exclusions are unchanged. The disk
  fallback is capped at the root (`fallback::Options::stop_at`: the climb
  breaks before moving to a parent once it reaches `stop_at`, so with the
  current directory equal to the root no ancestor is walked; `fallback.up`
  still applies below it). An empty query with `--local` and without
  `--list`/`--explain` prints the project root on stdout and exits 0 before
  the database is even opened — no reconcile, no `queries` row (`f -l`
  records the jump itself as `--source jump`). `--explain` gains a
  `project root: <path>` line right after the `memory:` line (absent
  without `--local`); the `queries` row is written exactly as for a global
  query, the scope is not stored.
  `--engine <reference|nucleo>` (SPEC-v2 §20, clap value enum, no clap
  default) picks the stage-1 engine for this call; precedence is
  `--engine`, then the `engine` config key, then `reference`
  (`query_engine_flag_overrides_the_config`). It applies to every mode
  (plain, `--list`, `--explain`, `--local`) and to the disk-fallback
  ranking. The pwsh script never passes it. `--explain` always prints an
  `engine: <reference|nucleo>` line right after `normalized query:`. Under
  nucleo, a stage-1 match's detail is one `token '<token>': nucleo <score>`
  line per token (the §7.1-normalized token), then `sum <s>, floored
  <max(s, 4)>`, then `folder bonus +2` only when awarded; nucleo's internal
  bonuses are not shown (`explain__nucleo_tokens` snapshot).
  Query memory (SPEC-v2 §24, see the engine contract) applies after `rank`
  on the database pool only — once the current directory, `missing`
  entries, and everything outside the `--local` scope are excluded — and
  never to the disk fallback's candidates
  (`query_memory_never_applies_to_the_disk_fallback`,
  `query_memory_respects_local_scope`). The remembered directory jumps
  directly, without a menu even on a stage-2 tie; its `queries` row keeps
  its real stage (`'1'` or `'2'`) with `outcome = 'jump'`
  (`query_memory_jump_journals_the_real_stage`). `--list`, and so Tab
  completion and `fi`, lists it first (`query_list_puts_the_remembered_directory_first`).
  `--explain` prints a `memory:` line right after `engine:` (before any
  `project root:` line): `memory: <path> (chosen <local date>)` when
  applied — the deciding criterion is then `query memory`, the decision is
  the jump, and the remembered directory is listed first — else `memory:
  none`, `memory: not applied (probable failure)`, `memory: not applied (no
  longer matches)` (remembered, but not among the ranked candidates, which
  includes every fallback report), or `memory: disabled`; nothing else in
  the report changes (`explain__memory_applied` snapshot).
  **Alias queries** (scope extension beyond SPEC, decided by Hervé on
  2026-10-02; lot 53): right after the settings load and before anything
  else — current directory, project root, database — `main::alias_query`
  inspects the query text. When its first whitespace token starts with the
  configured `alias_prefix` (`!` by default, see the configuration
  contract), the query is an alias query and the ranking path never runs:
  exact, case-insensitive lookup in `aliases` (`alias::key` +
  `storage::alias_by_key`), no fuzzy matching, no disk fallback, no
  reconcile. A second token (`f !ombi src`) fails with `furet: an alias
  takes no other token`, exit 1; `--local` combined with an alias fails
  with `furet: --local cannot be combined with an alias`, exit 1, before
  the git-repository check; a name outside `[A-Za-z0-9_-]` — including a
  bare `!` or `!ombi\src` — fails with the `furet alias add` message
  `furet: invalid alias name '<name>': use letters, digits, '_' and '-'`,
  exit 1. An unknown name fails with `furet: unknown alias '<name>'`,
  enriched to `furet: unknown alias '<name>'; did you mean '<near>'?`
  when exactly one edit (optimal string alignment, `alias::suggestion`
  over the stored names) separates it from a known one; a tie picks the
  smallest lowercased name. An alias whose target is gone from disk fails
  with `furet: alias '<name>' points to a missing directory: <path>`,
  exit 1 — no fallback, no ranking. On success the resolved path is the
  answer: plain and `--list !ombi` print it alone on stdout, and
  `--explain !ombi` prints `alias: <name> -> <path>` on stderr and leaves
  stdout empty; **no `queries` row and no visit are written** (D1 is
  untouched — an alias jump bypasses ranking entirely; pwsh still records
  the `jump` visit as for any jump,
  `f_bang_alias_jumps_and_records_one_jump_visit`). A directory literally
  named like the alias wins in `f`: the pwsh `Test-Path` branch runs
  before the query, so `f !ombi` with a `!ombi` directory in the current
  directory goes there
  (`f_existing_directory_named_like_the_alias_wins`); `src/pwsh.rs` is
  unchanged. Known interim behavior until lot 54: Tab on an exact alias
  word (`f !ombi<Tab>`) replaces it with the target path, and Tab on a
  partial one (`f !om<Tab>`) proposes nothing.
- `furet up <n>` — prints the ancestor `n` levels above the current
  directory.
- `furet back --session <s>` — prints the second-to-last directory visited
  in that session.
- `furet init pwsh [--cmd <name>]` — prints the PowerShell integration
  script (mirrors zoxide's `init` shape), preceded by the `clap_complete`
  PowerShell completion block: a native completer registered for `furet`
  (`Register-ArgumentCompleter -Native -CommandName 'furet'`) that completes
  furet's subcommands and options. The block comes first because pwsh
  accepts `using` statements only before every other statement, and clap's
  block opens with two of them. `--cmd` does not affect the block, which
  always completes `furet`. This diverges from zoxide, whose `build.rs`
  generates its completions at build time
  (`clap_complete::generate_to` → `contrib/completions/_zoxide.ps1`) and
  ships them as separate files, while furet embeds them in the `init`
  output. The script itself registers a tab
  completer (`Register-ArgumentCompleter` on the jump function's
  `FuretArgs`): it completes only the first, single argument token, returning
  the ranked `furet query --list` lines in order (an empty word completes the
  recency list; `-`/`--explain` and `.`/`..`/`...` words get nothing; paths
  with PowerShell metacharacters are emitted single-quoted, `'` doubled).
  With `-l`/`--local` (SPEC §19), the jump function and `fi` jump only
  inside the project. `-l` is a declared switch — `param([Alias('l')]
  [switch] $Local, [Parameter(ValueFromRemainingArguments…)] $FuretArgs)` —
  so pwsh's own binder takes it wherever it appears (`f -l cl` and
  `f cl -l` alike); `--local` does not bind that parameter and is still
  stripped by hand from `$FuretArgs` (`$scoped = $Local.IsPresent -or
  ($FuretArgs -contains '--local')`); a literal `-l` can only reach
  `$FuretArgs` through `f -- -l`, where it is query text and is not
  stripped. pwsh's parameter-name abbreviation applies (accepted by Hervé,
  2026-09-26, verified on pwsh 7.6.6): any case-insensitive prefix of
  `-Local` (`-L`, `-lo`, `-local`) turns the switch on instead of being
  query text, and since both functions are advanced functions, `-F…` binds
  `-FuretArgs` and prefixes of the common parameters are consumed too
  (`-v`, `-d`, `-ev`, `-ov`, `-pv` bind silently; `-e`, `-o`, `-w`, `-i`,
  `-p` fail as ambiguous) — the common-parameter and `-F…` cases already
  held before lot 40b. `f -- <token>` passes any such token as query text.
  Tokens matching no parameter (`-x`, `-dev`) stay query text. With a query
  the scoped branch calls `furet query --local --
  $query`, without one it calls `furet query --local` and jumps to the
  project root, recording the landing
  as `--source jump` (Hervé, 2026-09-26) — the `.`, `..`, `-`, direct-path,
  and home branches are never reached. `f -l <query> --explain` forwards the
  scope (`furet query --explain --local -- $query`, decision 2, Hervé
  2026-09-26). `fi -l` scopes the whole interactive flow: the fzf initial
  list and every reload (`furet query --list --color --local [ {q}]`) and
  the console menu (`furet query --list --local $query`). The fzf branch
  of `fi` — both calls, local and non-local — passes `--preview "furet
  preview {}"` and `--preview-window "right,50%"` (SPEC-v2 §23), so a
  right-hand pane lists the highlighted directory through the `furet
  preview` subcommand; fzf quotes `{}` correctly under cmd.exe (a path
  with a space and a `&` verified against fzf 0.74.2, lot 45). Lot 46
  (SPEC-v2 §24) adds `--print-query` to both calls and captures the output
  as an array: `lines[0]` is fzf's final query (an empty query line comes
  back as an empty element, so `lines[1]` is the selection) and fewer than
  two lines — abort or no selection — returns without moving; the selection
  is ANSI-stripped as before. The console menu branch has no preview. On a
  jump the fzf branch calls `__furet_record $target $from 'jump'
  $finalQuery` and the console-menu branch `__furet_record $target $from
  'jump' $query` (its joined arguments); `__furet_record($target, $from,
  $source, $query)` gained that optional 4th parameter — when it is
  non-blank (`-not [string]::IsNullOrWhiteSpace`) the add call gains
  `--query $query` before `-- $target`, so a choice journals exactly one
  `pick` row (none for an empty query), and every 3-argument call (all of
  `f`'s) behaves exactly as before. One completer —
  the `FuretArgs` registration above — handles every line, `-l` included:
  the declared switch keeps pwsh's completion binder on the normal path
  (lot 40b, Hervé 2026-09-26, replacing a `-Native` fallback that relied on
  undocumented pwsh behavior), so after `-l`/`--local` as the first
  argument it completes the second argument token — `f -l <Tab>` and
  `f -l cl<Tab>` run `furet query --list --local -- $word` — while
  `f -l cl <Tab>` returns nothing. The generated script is covered by
  executed integration tests in `tests/pwsh.rs`, which run it in a real
  `pwsh` process; these tests require pwsh 7.
- `furet queries --failures` — prints tab-separated probable-mistake rows
  **to stdout**, not stderr. Calibration (SPEC-v2 §24) considers
  `outcome IN ('jump', 'pick')` — a menu choice followed by a quick back is
  flagged like a jump — and the output format is unchanged. Accepted exception to the stdout-discipline
  rule: it is a standalone reporting tool, never invoked by `f`/`fi`, so
  nothing pipes its output into `Set-Location`. `furet list` below is the
  second accepted stdout exception, `furet stats` below the third, for
  the same reason, and `furet preview` below the fourth — its output is
  read by fzf's preview pane, never by `Set-Location` — and `furet alias
  list` below the fifth, same justification as `queries`: a reporting
  tool whose output is never piped into `Set-Location`.
- `furet list [--all] [--paths]` — prints one tab-separated line per known directory —
  `path`, `visits` (count of `visits` rows), `last_visit`, `first_seen` —
  ordered by path, ignoring case (the lowercased `key`);
  timestamps are local time formatted by SQLite itself. Without
  `--all`, rows with a `missing_since` are excluded; with `--all`, every
  row appears plus a fifth `present`/`missing` column. `--paths` (`-p`)
  prints the path alone, also with `--all`. Read-only: the
  stored `missing_since` is shown as is, with no soft-delete
  reconciliation. An empty database prints nothing, exit 0
  (`src/storage.rs`, `dir_listing`; `main::list_command`).
- `furet stats [--top <n>]` — prints the database overview as
  tab-separated `key<TAB>value` lines **to stdout** (SPEC-v2 §22), in
  this exact order: `known_directories` (`dirs` rows without
  `missing_since`), `missing_directories` (rows with), `visits`,
  `visits_last_30_days` (`ts >= since`), `queries`,
  `queries_last_30_days` (same window), `jumps` (`outcome IN ('jump',
  'pick')`),
  `probable_failures` (`calibration::probable_failures(&query_log,
  &visit_log).len()`), `failure_rate` (`{:.1}%` of `probable_failures ×
  100 / jumps`, `0.0%` when `jumps = 0`), `stage_1`, `stage_2`,
  `stage_fallback`, `stage_menu` (`queries` grouped by `stage`),
  `source_hook`, `source_jump`, `source_back`, `source_up`,
  `source_fallback`, `source_import` (`visits` grouped by `source`); a
  group with no row counts 0. The 30-day window is `since = now -
  30 × 86 400` seconds, `now` read from `SystemClock` in the CLI and
  injected as a parameter of `storage::stats_counts`, so the window is
  testable against a fixed instant. Then at most `n` lines
  `top<TAB><visits><TAB><path>` (`--top`, default 10, `0` prints none):
  **every present directory** (`missing_since IS NULL`), 0-visit ones
  included, visit count descending then `key` ascending
  (`storage::top_dirs`). Read-only: no reconcile, no write of any kind —
  `missing_since` is reported exactly as stored (`stats_writes_nothing`).
  Third accepted stdout exception, same justification as `list`: a
  reporting tool never called by `f`/`fi`. Empty database: every count 0,
  `failure_rate` `0.0%`, no `top` line, exit 0. Pure renderer:
  `stats::render` (`src/stats.rs`), fed by `storage::stats_counts` /
  `storage::top_dirs` (no schema change).
- `furet remove [<pattern>] [--missing] [--confirm | --yes] [--dry-run]` — forgets known directories matching
  `<pattern>`, **hard-deleting** their `dirs` row (no `missing_since` reuse,
  no `removed_at` column, no schema change; not in SPEC — scope extension
  decided by Hervé 2026-09-26). zoxide's `remove` takes exact paths only;
  the wildcard is a furet extension. Without `\`, `/` or `:`, and not equal
  to `.`/`..`, the pattern is a **name** pattern matched against every normal
  segment (`std::path::Component`) of each known path — the drive prefix is
  never a segment, so `c*` does not select everything on `C:` — and `ombi*`
  selects `ombi` itself and every known directory below it; otherwise it is
  a **path** pattern. Matching
  is case-insensitive (`str::to_lowercase` both sides); `*` matches any run
  of characters including `\`, `?` exactly one, every other character is
  literal. A path pattern without a wildcard resolves against the disk first
  (`paths::resolve`), falling back to the lexical `paths::absolute_key` when
  the directory no longer exists — the main use case; with a wildcard it is
  matched lexically against every lowercased path — a pattern starting with
  `*` (after `paths::unify_separators`) is taken as is, never joined to the
  current directory, so `*\cache\*` works from any cwd, while other relative
  patterns stay anchored at the cwd — so `apps\*` removes the
  descendants of `apps`, never `apps` itself. Candidates come straight from
  `storage::dir_entries` (missing rows included, no reconciliation), sorted
  by lowercased path like `furet list`; nothing matches → `furet: no known
  directory matches '<pattern>'`, exit 1; an empty/whitespace pattern →
  `furet: empty pattern`, exit 1. Without `--confirm` every candidate is
  removed at once. With `--confirm` one question per candidate is printed
  on stderr (`Remove <path>? [y/N/a/q] `, trailing space, no newline,
  flushed) and one stdin line is read per question (trimmed,
  case-insensitive): `y`/`yes` removes that one, `n`/`no`/an empty line
  keeps it, `a`/`all` removes it and every later candidate without asking,
  `q`/`quit` or EOF keeps it and every later candidate and stops asking,
  anything else re-asks the same question. Removals are applied after all
  questions, in one `storage::remove_dirs` call — a `y` given before a `q`
  is applied. The protocol lives in `src/remove.rs`
  (`Answer`/`parse_answer`/`confirm_each`); `main::ask` wires `ask` to
  `eprint!` + flush + `stdin().read_line`, a 0-byte read meaning EOF.
  `--yes` never asks — for scripts; with `--missing` it skips the
  default questions. `conflicts_with = "confirm"`, so
  `--confirm --yes` is refused by clap, exit 2. `--dry-run` computes the
  candidates exactly as a real removal and prints `would remove <path>` per
  candidate on stderr; it asks no question, even with `--confirm`, writes
  nothing to the database, and exits 0 with at least one candidate
  (otherwise the same `no known directory matches` error as a real
  removal, exit 1). Nothing removed — every answer kept, or `q`/EOF before
  any `y` — leaves the database untouched
  (`furet: nothing removed`, exit 1, the SPEC §9 menu-cancel convention).
  Each removal is reported on stderr as `removed <path>`; nothing is ever
  written to stdout. `storage::remove_dirs` deletes, in one transaction per
  call, the `dirs` row, its `visits` and `queries` rows, and unlinks other
  visits' `from_dir_id` (set to `NULL`) so foreign keys stay enforced without
  `ON DELETE`. A removed directory comes back on the next `furet add` of it,
  exactly like zoxide (BACKLOG item 3: not prevented).
  `--missing` (lot 43, SPEC-v2 §21) targets the known directories that are
  really gone. `main::remove_missing` first reads every stored marker
  (`storage::missing_since_by_id`), then runs `soft_delete::reconcile` over
  **every** `dirs` row with `SystemClock`'s now, overlays the updates in
  memory (`Some` sets the date, `None` removes it), and — unless
  `--dry-run` — persists them with `storage::set_missing_since`, exactly as
  `reconcile_on_disk` does; under `--dry-run` nothing is persisted, so the
  date still works. A directory that came back despite a stale
  `missing_since` is never a candidate and its marker is cleared (except
  under `--dry-run`). Candidates are the reconciled entries with
  `missing == true`, narrowed by the pattern (`remove::matches` on
  `remove_target`, unchanged syntax) when one is given, sorted by
  lowercased path. No candidate → `furet: no missing known directory`
  (no pattern) or `furet: no missing known directory matches '<pattern>'`,
  exit 1. Confirmation is **on by default**: `remove::confirm_each` runs
  unless `--yes` is given; `--confirm` is accepted and changes nothing.
  Each question carries the absence date — `Remove <path> (missing since
  <date>)? [y/N/a/q] ` — `<date>` being the overlaid timestamp (the stored
  `missing_since`, or the reconcile's now for a directory just marked)
  formatted by `storage::format_local_time` with the same SQLite
  `strftime('%Y-%m-%dT%H:%M:%S', <ts>, 'unixepoch', 'localtime')`
  expression as `dir_listing`. `--dry-run` prints `would remove <path>`
  per candidate — no date — asks nothing, and persists nothing; removal
  itself, `removed <path>` lines, `nothing removed` (exit 1), and the
  empty-pattern error (`furet: empty pattern`, exit 1, with or without
  `--missing`) are lot 42's. The pattern is optional only with `--missing`
  (`required_unless_present = "missing"`): a bare `furet remove` is
  refused by clap, exit 2. stdout stays empty throughout.
- `furet alias add <name> [<path>] [--force] | list | remove <name>` —
  manages the `aliases` table (scope extension beyond SPEC, decided by
  Hervé on 2026-10-02; lot 52, storage from lot 51). `add` validates the
  name first (`alias::key`, `src/alias.rs`): non-empty, ASCII letters,
  digits, `_` and `-` only, no length limit; anything else fails with
  `furet: invalid alias name '<name>': use letters, digits, '_' and '-'`,
  exit 1, nothing written. Names are case-insensitive — `Ombi` and
  `ombi` are one entry, the stored lowercased `key`. `<path>` defaults to
  the current directory and is canonicalized through `paths::canonical`
  (directories only; a relative path resolves against the process's
  current directory); its error propagates unchanged, exit 1, nothing
  written. An existing name is refused — `furet: alias '<stored name>'
  already exists (<stored path>); use --force to replace it`, exit 1 —
  unless `--force` replaces the name, path and `created` in place
  (`storage::upsert_alias`). Success prints `alias <name> -> <path>` on
  stderr, exit 0, and records **no visit** — `dirs` and `visits` stay
  untouched (`alias_add_does_not_record_a_visit`). `list` prints one
  `name<TAB>path<TAB>created` line per row **to stdout** — the fifth
  accepted stdout exception, same justification as `list`/`stats`: a
  reporting tool whose output is never piped into `Set-Location` —
  ordered by the lowercased `key` (case-insensitive alphabetical), with
  `created` in the same SQLite local-time format as `furet list`; an
  empty table prints nothing, exit 0 (`storage::alias_listing`). `remove
  <name>` lowercases the name without validation (an invalid name cannot
  exist) and deletes the row (`storage::remove_alias`); success prints
  `removed alias <name>` on stderr, exit 0; no such key → `furet:
  unknown alias '<name>'`, exit 1. A single-digit name is allowed: it is
  the future mark of the same digit — aliases and marks share one
  namespace, so `furet alias add 1` creates what the marks lot will treat
  as mark 1. `furet remove`, the retention purge and `exclude_dirs`
  never touch these rows (standalone `path`, no foreign key;
  `furet_remove_leaves_aliases_untouched`). Resolution is lot 53's: see the
  **Alias queries** paragraph in the `furet query` entry above (`f !<name>`,
  exact case-insensitive match, `did you mean` hint).
- `furet home` — prints the configured `home` (SPEC §16), canonicalized, or
  nothing when it is unset, a relative path, or does not resolve to an
  existing directory (missing, or a file — SPEC §1, `paths::PathError`);
  never fails because of the config. The pwsh `f` with no argument calls it
  and falls back to `$HOME` on empty output.
- `furet import zoxide` — reads `<score> <path>` lines from stdin (as
  produced by `zoxide query -ls`), canonicalizes each path through
  `paths::canonical` (directories only), skips already-known directories,
  and records the rest as `visits(source = 'import', session = 'import')`
  with synthetic timestamps ordered by score descending then path ascending
  (`src/import.rs`, `main::import_zoxide`). zoxide's score is discarded once
  it has ordered the import — D1 stays the only recency rule. One
  transaction for the whole import; stdout stays empty; a summary line
  (`imported N, skipped M (known K, not a directory D, malformed X,
  duplicate Y, excluded E)`) goes to stderr; `duplicate` counts candidates
  that share a key with another candidate in the same batch, collapsed onto
  the one with the highest score before the known-keys filter; an excluded
  directory (`exclude_dirs`) is counted in `excluded` before dedupe, so it
  is never counted as `known` or `duplicate`. No `queries` journal entry.
- `furet preview <path>` — lists a directory's contents for fzf's preview
  pane (SPEC-v2 §23) **to stdout** — the fourth accepted stdout exception:
  unlike the three reporting exceptions above it is called by `fi`, but
  only through fzf, which reads the output into its preview pane; nothing
  ever pipes it into `Set-Location`. ANSI SGR sequences (`ESC[` + digits
  or `;` + `m`, and nothing else) are stripped from `<path>` first
  (`preview::strip_sgr`), so the colored lines `fi` feeds fzf work as is;
  a lone `ESC` or a non-SGR sequence is kept. Output: subdirectories
  first, each name followed by `\`, then files — each group sorted by the
  lowercased name with the original name as tiebreak (deterministic),
  hidden entries included; at most 50 lines, then `… +<K> more` when
  `K > 0` entries remain (pure `preview::render`, `src/preview.rs`).
  `main::preview_command` does the disk access: a missing path **or a
  file** prints the single line `(not a directory)`; a directory whose
  `read_dir` fails prints `(unreadable: <error>)`; each entry is
  classified by `entry.path().is_dir()` (follows links, so a symlink or
  junction pointing at a directory lists as a directory) and an entry
  that cannot be read during iteration is skipped. Exit 0 in every case.
  Never opens the database and never reads `config.toml`, so the pane
  stays fast (`preview_never_opens_the_database`).

### Exit codes

Every subcommand routes through `main::report`
(`src/main.rs:183`): `Ok(())` → **0**, any `Err` → **1**, message printed to
stderr as `furet: {error}`. A malformed invocation (unknown flag, invalid
`--source`/`ValueEnum`, missing required arg) never reaches `report` at all
— clap exits **2** directly, confirmed by running
`furet add /nonexistent --session s --source bogus`, which exits 2; so does
a conflicting pair of flags, e.g. `furet remove x --confirm --yes`
(`remove_confirm_and_yes_conflict`).

| Subcommand | Exit 0 | Exit 1 |
|---|---|---|
| `add` | path canonicalizes to a directory and the visit is recorded | path does not exist (`add_rejects_a_path_that_does_not_exist`), path is a file, not a directory (`add_rejects_a_file_path`, `paths::PathError::NotADirectory`), or a DB error |
| `query` (plain) | a candidate resolves (stage 1, stage 2, or fallback) and prints it | nothing matches at all (`query_with_no_recorded_directory_and_no_fallback_hit_fails_on_stderr`), or a menu is cancelled (`query_menu_cancels_on_an_out_of_range_number`, `..._on_an_empty_answer_and_on_no_answer_at_all`) |
| `query --list` | always, even with zero candidates (`query_list_with_no_candidate_prints_nothing_and_exits_zero`) | with `--local`, outside a git repository (`query_local_outside_a_repository_fails_and_records_nothing`) |
| `query --explain` | always, even with zero candidates (`explain_exits_zero_when_nothing_matches`) | with `--local`, outside a git repository (`query_local_outside_a_repository_fails_and_records_nothing`) |
| `query --local` | exit 0 with the project root on stdout for an empty query without `--list`/`--explain` (`query_local_with_an_empty_query_prints_the_project_root`) | outside a git repository, in every mode (`query_local_outside_a_repository_fails_and_records_nothing`) |
| `up <n>` | `n >= 1` and that many ancestors exist | `n == 0` (`up_rejects_zero_levels`) or too few ancestors (`up_fails_when_there_are_fewer_ancestors_than_requested`) |
| `back --session` | the session has at least 2 visits | fewer than 2 visits for that session (`back_fails_when_the_session_has_fewer_than_two_visits`) |
| `init pwsh` | always — pure string rendering, no fallible step | — |
| `queries --failures` | always, even with an empty journal (`queries_failures_with_an_empty_journal_prints_nothing_and_exits_zero`) | — |
| `queries` (no `--failures`) | — | always (`queries_without_failures_fails_on_stderr`) — the flag is mandatory today, SPEC does not define a bare `queries` command |
| `list [--all] [--paths]` | always, even with an empty database (`list_on_an_empty_database_prints_nothing_and_exits_zero`) | a DB error |
| `stats [--top <n>]` | always, even with an empty database (`stats_on_an_empty_database_prints_zeros_and_no_top_line`) | a DB error |
| `remove [<pattern>] [--missing] [--confirm \| --yes] [--dry-run]` | every match removed and reported on stderr (`remove_by_name_glob_removes_every_match_and_reports_on_stderr_only`, `remove_confirm_yes_to_each_removes_both`), or `--dry-run` with at least one candidate, printing `would remove <path>` and changing nothing (`remove_dry_run_changes_nothing`, `remove_missing_dry_run_changes_nothing`), or `--missing` removing at least one vanished directory (`remove_missing_yes_removes_a_vanished_directory_and_keeps_a_returned_one`) | empty pattern (`remove_empty_pattern_fails_with_exit_one`, `remove_missing_with_an_empty_pattern_fails_like_a_plain_remove`), no match (`remove_with_no_match_fails_with_exit_one_and_removes_nothing`, `remove_dry_run_with_no_match_fails_like_a_real_remove`), no missing known directory, with or without a pattern (`remove_missing_without_missing_directories_fails`), nothing removed — every answer kept, `q`, or EOF (`remove_confirm_empty_and_no_answers_keep_everything`, `remove_confirm_eof_removes_nothing`, `remove_confirm_declined_or_eof_removes_nothing_and_exits_one`), or a DB error; `--confirm --yes` is refused by clap, exit 2 (`remove_confirm_and_yes_conflict`), and so is a bare `furet remove` — no pattern, no `--missing` (`remove_without_a_pattern_or_missing_is_refused`) |
| `home` | always, whether or not it prints a path (`home_prints_the_configured_directory_canonicalized`, `home_prints_nothing_and_exits_zero_when_unset`, `home_prints_nothing_and_warns_when_the_directory_is_missing`, `home_prints_nothing_and_warns_when_home_is_a_file`, `home_prints_nothing_and_warns_on_a_relative_path`) | — |
| `alias add <name> [<path>] [--force]` | the name is valid, the target canonicalizes to a directory, and the name is new — or `--force` was given (`alias_add_defaults_to_the_current_directory`, `alias_add_force_replaces_name_and_path`, `alias_add_accepts_a_digit_name`) | invalid name (`alias_add_rejects_invalid_names`), a target that is missing or a file (`alias_add_rejects_a_missing_path_and_a_file`), an existing name without `--force` (`alias_add_refuses_an_existing_name_ignoring_case`), or a DB error |
| `alias list` | always, even with an empty table (`alias_list_is_empty_then_sorted_by_name_ignoring_case`) | a DB error |
| `alias remove <name>` | the name matches a stored key, ignoring case (`alias_remove_ignores_case`) | no alias matches (`alias_remove_of_an_unknown_name_fails`), or a DB error |
| `import zoxide` | always once stdin is read and the transaction commits, including empty input or everything skipped (`importing_empty_stdin_exits_zero_and_imports_nothing`, `a_missing_path_a_file_and_a_malformed_line_are_each_skipped_and_counted`) | a stdin read error or a DB error |
| `preview <path>` | always — a directory lists up to 50 lines plus `… +K more`; a missing path or a file prints `(not a directory)`, an unreadable directory `(unreadable: …)` (`preview_of_a_missing_path_or_a_file_says_not_a_directory`) | — |

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
