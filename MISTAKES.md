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
