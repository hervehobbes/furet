# MUTANTS.md — the mutation-testing campaign

## Purpose

[cargo-mutants](https://mutants.rs) edits the code one small change at a
time: a replaced comparison, a deleted `!`, a body swapped for a default
value. A **missed** mutant is such an edit that no test notices — the test
suite still passes with the broken code. The campaign kills every missed
mutant with a new or stronger test, one module per lot; a mutant may be
classified as **equivalent** only when it cannot change any observable
behavior, with a one-line justification.

## Procedure

For `src/<module>.rs`, in order:

1. **Baseline.** Run
   `cargo mutants -f src/<module>.rs --cargo-test-arg=--lib`
   (`--lib` runs the library's unit tests only; the goal is to kill each
   mutant at the unit level). Paste the final summary line and the full
   `mutants.out/missed.txt` (and `mutants.out/timeout.txt`, when not empty)
   into the lot report.
2. **Kill every missed mutant.** Add a test, or strengthen an existing
   assertion, inside the module's `#[cfg(test)] mod tests` so that the
   mutant fails. Tests only: no production change. If a missed mutant
   reveals a real bug — the current behavior is wrong, not just untested —
   stop and report it with the mutant and a failing test idea. Prefer one
   focused test per behavior over one test per mutant, and name tests after
   the behavior (`cycle_wraps_from_nine_to_one`), never after the mutant.
   Strengthening never means weakening or deleting an existing assertion.
3. **Equivalent mutants.** Classify a mutant as equivalent only when no
   input can tell it apart from the original code, and give a one-sentence
   justification. Expect few of these; "hard to test" does not make a
   mutant equivalent.
4. **Confirmation.** Re-run the Step 1 command. `missed.txt` must then be
   empty, or list only the mutants classified as equivalent in Step 3.

- A `pub` function's contract covers every input its signature accepts,
  including inputs the CLI cannot produce today (lot 71: `alias::cycle`
  with a duplicated digit, `alias::suggestion` with two names sharing a
  key). A mutant such an input exposes is killed by a test, never
  classified as equivalent.
- A helper program hunting a distinguishing input runs in the foreground —
  never in the background, never in parallel (one thread, one process) —
  is bounded (at most 100 000 candidate inputs, stops by itself), is
  wrapped in a 60-second limit (`timeout 60 <program>`), lives outside the
  repo and is deleted afterwards; reason first, use a search only to check
  a hypothesis, and report the mutant as unresolved when 60 seconds are
  not enough (lot 76: two background searches saturated the CPU and Hervé
  had to reboot twice).

## Results

| Module | Lot | Date | Mutants | Caught | Unviable | Missed before | Missed after | Equivalent | Timeouts |
|---|---|---|---|---|---|---|---|---|---|
| `src/alias.rs` | 71 | 2026-10-04 | 58 | 51 | 7 | 5 | 0 | 0 | 0 |
| `src/import.rs` | 72 | 2026-10-04 | 54 | 44 | 8 | 8 | 2 | 2 | 0 |
| `src/backup.rs` | 72 | 2026-10-04 | 16 | 14 | 2 | 0 | 0 | 0 | 0 |
| `src/remove.rs` | 73 | 2026-10-04 | 59 | 54 | 2 | 1 | 0 | 0 | 3 |
| `src/calibration.rs` | 73 | 2026-10-04 | 25 | 23 | 2 | 0 | 0 | 0 | 0 |
| `src/memory.rs` | 73 | 2026-10-04 | 20 | 19 | 1 | 0 | 0 | 0 | 0 |
| `src/config.rs` | 74 | 2026-10-04 | 17 | 16 | 1 | 0 | 0 | 0 | 0 |
| `src/paths.rs` | 74 | 2026-10-04 | 14 | 11 | 3 | 0 | 0 | 0 | 0 |
| `src/stats.rs` | 75 | 2026-10-04 | 6 | 6 | 0 | 0 | 0 | 0 | 0 |
| `src/preview.rs` | 75 | 2026-10-04 | 35 | 26 | 0 | 5 | 4 | 4 | 5 |
| `src/soft_delete.rs` | 75 | 2026-10-04 | 3 | 2 | 1 | 2 | 0 | 0 | 0 |
| `src/project.rs` | 75 | 2026-10-04 | 8 | 8 | 0 | 0 | 0 | 0 | 0 |
| `src/normalize.rs` | 76 | 2026-10-04 | 19 | 19 | 0 | 0 | 0 | 0 | 0 |
| `src/stage2.rs` | 76 | 2026-10-04 | 74 | 73 | 1 | 1 | 0 | 0 | 0 |
| `src/stage1.rs` | 77 | 2026-10-04 | 81 | 76 | 3 | 10 | 2 | 2 | 0 |
| `src/stage1_nucleo.rs` | 77 | 2026-10-04 | 7 | 6 | 1 | 0 | 0 | 0 | 0 |

A timeout is a mutant whose test run hangs (cargo-mutants' 20 s cap): it is
detected without a test failing, and it stays a timeout in the confirmation
run.

## Accepted equivalent mutants

- `src/import.rs:67:23: replace < with <= in parse_history_line` — the only input the
  widened guard adds to the early return is `''`, an empty single-quoted path, which the
  empty-path check after the branch also rejects; every input returns the same `Option`.
- `src/import.rs:72:23: replace < with <= in parse_history_line` — the same reasoning for
  the double-quoted branch: the added input `""` reaches the same `None` through the
  empty-path check.
- `src/preview.rs:32:5: replace char_width -> usize with 1` and the three deleted arms
  (`:33:9` `0xc0..=0xdf`, `:34:9` `0xe0..=0xef`, `:35:9` `0xf0..=0xf7`) — `char_width` only
  advances `strip_sgr`'s scan, and `sgr_end` strips only at a `0x1b` byte, which valid UTF-8
  holds solely as a one-byte char at a char boundary (every byte of a multibyte char is
  `0x80..=0xbf` or `0xc2..=0xf4`, never `0x1b`); visiting or skipping those continuation bytes
  cannot change which strips are found nor make a slice land mid-char, so every input yields
  the same output with no new panic (brute-forced for each mutant over all 22 625 inputs of
  length ≤ 4 over `ESC [ m 0 ; 1 é € 😀 a \x7f Â`: zero differences, zero panics).
- `src/stage1.rs:122:58: replace || with && in match_token` — the conjunction only skips the early
  return in two new cases, both ending at the same `None`: a non-subsequence non-empty token
  reaches `best_placement`, which returns `Some` exactly when the token is a subsequence (every
  placed character requires a placement of the previous one at a strictly smaller index, and a
  subsequence embedding drives the DP to place every character), so a non-subsequence yields no
  placement of the last character and the fold returns `None`; and an empty token reaches
  `best_placement`'s `split_first()?`, which is `None`. Every input returns the same `Option`,
  with no new panic.
- `src/stage1.rs:186:22: replace > with >= in best_placement` — `index` is a `usize` from the scan,
  so `index >= 0` is always true and the mutant only deletes the `index > 0` gate; at index 0
  `checked_sub(1)` makes `earlier` `None`, so `running` (only ever assigned from `earlier`) and the
  consecutive branch (which requires `earlier`) are both `None` and `placed` stays `None` with or
  without the gate; for every index ≥ 1 the two guards coincide. No input can tell them apart.

## Campaign order

The architect may reorder. `src/storage.rs` and `src/main.rs` come last:
their behavior is mostly covered by integration tests, so they need a
different command, without `--lib`.

1. The pure, non-core modules: `src/import.rs`, `src/backup.rs`,
   `src/remove.rs`, `src/calibration.rs`, `src/memory.rs`, `src/config.rs`,
   `src/paths.rs`, `src/stats.rs`, `src/preview.rs`, `src/soft_delete.rs`,
   `src/project.rs`.
2. The core engine (SPEC §7, §9): `src/normalize.rs`, `src/stage1.rs`,
   `src/stage2.rs`, `src/stage1_nucleo.rs`, `src/rank.rs`,
   `src/decision.rs`.
3. Last: `src/storage.rs`, `src/main.rs`.
