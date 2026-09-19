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
