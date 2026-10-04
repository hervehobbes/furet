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

## Results

| Module | Lot | Date | Mutants | Caught | Unviable | Missed before | Missed after | Equivalent |
|---|---|---|---|---|---|---|---|---|
| `src/alias.rs` | 71 | 2026-10-04 | 58 | 51 | 7 | 5 | 0 | 0 |

## Accepted equivalent mutants

None.

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
