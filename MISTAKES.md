# Mistakes

Record here anything an agent got wrong and how it was caught, so it doesn't recur.

- 2026-09-20, lot 24 (GLM): added a second unit test mutating
  `FURET_DATA_DIR` via `set_var`/`remove_var` — unit tests run in parallel
  in one process, so the two tests' set/remove windows interleaved and
  `db_path()` sometimes read the real platform dir. Passed the first DoD
  and pre-push runs by scheduling luck; caught only by re-running the DoD
  after a later comment-only edit. Rule: exactly one env-mutating unit
  test per process — extend `furet_data_dir_overrides_the_database_location`
  instead of adding a sibling.
- 2026-09-27, lots 43–46 (GLM): the pasted DoD "raw output" was retyped
  four lots running — relabelled binaries, a malformed `test result` line,
  an appended "correction", and in lot 46 an invented binary hash
  (`explain-631fc2e8c71dfa4a1c46.exe` instead of the real
  `explain-631bc2e8c71dfa4d.exe`), even when the lot demanded a
  `Select-String` over a log file. The code was fine each time; caught by
  the reviewer re-running the DoD. Rule: paste tool output by copying it,
  never by retyping it; if a line cannot be pasted verbatim, say so. The
  reviewer's own DoD run remains the only accepted proof.
- 2026-09-27, lot 49 (GLM): same rule broken again the same day — the
  report's "before" block repeated the after log's timings (0.60s / 3.90s
  / 13.02s) instead of the real before log's (0.46s / 3.55s / 12.71s).
  Code and logs on disk were fine; caught by the architect reading both
  logs and confirmed by the reviewer's own DoD run. Rule unchanged: the
  reviewer's run is the proof, the executor's paste is not.
- 2026-10-03, lot 66 (GLM 5.3 Flash): the report's mutation-B block
  contained an edited line, `ok→FAILED (shown above as FAILED)`, instead
  of the raw test output. The code was fine; the reviewer's own rerun of
  the mutations was the proof. Rule unchanged: paste raw output by
  copying it, never annotate inside it — comments go outside the block.
