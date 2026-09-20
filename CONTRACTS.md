# Contracts

## Fuzzy engine contract

Pure, no filesystem or clock access. `rank::rank(query, current_dir, candidates, typo_min_length)`
drops the current directory and any `missing` candidate, scores every
survivor with `rank::dispatch` (stage 1 first, stage 2 only when stage 1
returns `None`), and returns a `Vec<Scored>` sorted per SPEC §8: score, then
recency (D1 — never the other way around), then name length, then path, as a
strict deterministic total order.

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
`SCORE_CAP - distance` = `3 - distance` for `distance <= MAX_DISTANCE = 2`
(`a_one_edit_typo_scores_two`, `a_distance_of_two_scores_one_and_a_distance_of_three_scores_nothing`).

**Discrepancy**: SPEC §7.3 calls for full Damerau-Levenshtein distance
(unrestricted transpositions). The implementation uses optimal string
alignment (OSA) instead — each substring may be edited at most once, so
`distance("ca", "abc")` is 3 under OSA but 2 under true Damerau-Levenshtein.
Deliberate lot-2 decision (`JOURNAL.md`, lot 2), pinned by
`an_adjacent_transposition_counts_as_a_single_edit` and the
`distance("ca", "abc") == 3` case; not reconciled with SPEC's wording.

`decision::decide(ranked) -> Decision` (`src/decision.rs`) applies SPEC §9:
a stage-1 best jumps unconditionally; a stage-2 best jumps alone at its
distance, else opens a `Menu` of the leading run of tied stage-2 candidates
(2..=9, `MENU_MAX_ENTRIES`).

## Configuration contract (SPEC §16)

`<data dir>/config.toml` (`storage::data_dir()`, same as the database and the
logs) overrides `config::Settings::default()`; loaded once, only by `furet
query` (`add` stays cheap). `config::parse(text) -> (Settings, Vec<String>)`
is pure — no filesystem, no `tracing` — and never fails the caller.

Fallback is per key, not per file, except malformed TOML:

- an unknown key warns and is ignored;
- a key of the wrong type or out of its documented range warns and keeps
  that key's default;
- malformed TOML warns once and keeps every default.

Every warning `parse` returns is logged with `warn!` by `main::load_settings`;
a missing file logs one `debug!` and changes nothing.

`ambiguity`, `keyboard_layout`, and `engine` are recognized and ignored, each
warning "not supported yet" — SPEC §9 does not define what `ambiguity`
measures, and the other two name features not built yet (Hervé's decision,
lot 15).

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
below. Every invocation also writes structured logs to `<data dir>/logs/`
(SPEC §17) — never to stdout or stderr.

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

### Exit codes

Every subcommand routes through `main::report`
(`src/main.rs:148`): `Ok(())` → **0**, any `Err` → **1**, message printed to
stderr as `furet: {error}`. A malformed invocation (unknown flag, invalid
`--source`/`ValueEnum`, missing required arg) never reaches `report` at all
— clap exits **2** directly, confirmed by running
`furet add /nonexistent --session s --source bogus`, which exits 2.

| Subcommand | Exit 0 | Exit 1 |
|---|---|---|
| `add` | path canonicalizes and the visit is recorded | path does not exist/canonicalize (`add_rejects_a_path_that_does_not_exist`), or a DB error |
| `query` (plain) | a candidate resolves (stage 1, stage 2, or fallback) and prints it | nothing matches at all (`query_with_no_recorded_directory_and_no_fallback_hit_fails_on_stderr`), or a menu is cancelled (`query_menu_cancels_on_an_out_of_range_number`, `..._on_an_empty_answer_and_on_no_answer_at_all`) |
| `query --list` | always, even with zero candidates (`query_list_with_no_candidate_prints_nothing_and_exits_zero`) | — |
| `query --explain` | always, even with zero candidates (`explain_exits_zero_when_nothing_matches`) | — |
| `up <n>` | `n >= 1` and that many ancestors exist | `n == 0` (`up_rejects_zero_levels`) or too few ancestors (`up_fails_when_there_are_fewer_ancestors_than_requested`) |
| `back --session` | the session has at least 2 visits | fewer than 2 visits for that session (`back_fails_when_the_session_has_fewer_than_two_visits`) |
| `init pwsh` | always — pure string rendering, no fallible step | — |
| `queries --failures` | always, even with an empty journal (`queries_failures_with_an_empty_journal_prints_nothing_and_exits_zero`) | — |
| `queries` (no `--failures`) | — | always (`queries_without_failures_fails_on_stderr`) — the flag is mandatory today, SPEC does not define a bare `queries` command |

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
