# Architecture

furet is layered around a pure core: normalization, the matching engine,
and ranking are deterministic functions with no filesystem or clock
access, so they can be tested exhaustively from plain data. Everything
impure — directory listing, timestamps, the SQLite database, shell
integration — lives in the outer layers and reaches the core through
injected traits (filesystem and clock are injected as traits). The CLI,
storage, and shell-hook layers sit around that core and translate between
the real environment and the pure functions.

The modules added since keep the same split. On the pure side:
`config::parse` turns `config.toml` text into `Settings` plus one warning
per invalid key (reading the file is `main::load_settings`'s job);
`import` parses `<score> <path>` lines, deduplicates them by key, and
plans synthetic timestamps as pure functions; `calibration::probable_failures`
flags probably-mistaken jumps from the journaled `queries` and `visits`
rows alone, through the per-row `calibration::failure_of`; `memory` (query
memory, SPEC-v2 §24) computes the memory key of a query and
`memory::promote` moves the remembered directory to the front of `rank`'s
output — the journal lookup behind it is `storage::recall`, on the outer
side; `soft_delete::reconcile` is deterministic given an injected
`Filesystem` trait — `RealFilesystem`, the disk check, is the outer
implementation; `project::root` finds the nearest ancestor holding a
`.git` entry through the injected `GitMarker` trait (`RealGitMarker`, the
`.git` existence check, is the outer implementation); and
`stage1_nucleo::NucleoScorer`, the opt-in nucleo stage-1 scorer (SPEC-v2
§20), is pure too — it owns one `nucleo_matcher::Matcher`, which
`rank::rank` builds once per call and reuses for every candidate, while
`rank::Engine` picks it or the reference `stage1`. On the outer side:
`fallback::discover` walks the real
disk with the `ignore` crate when the database has no candidate, and
`logging::init` — a binary-only module under `main`, not part of the
library — installs the daily-rotated tracing file logger under
`<data dir>/logs`.
