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
rows alone; `soft_delete::reconcile` is deterministic given an injected
`Filesystem` trait — `RealFilesystem`, the disk check, is the outer
implementation; and `project::root` finds the nearest ancestor holding a
`.git` entry through the injected `GitMarker` trait (`RealGitMarker`, the
`.git` existence check, is the outer implementation). On the outer side:
`fallback::discover` walks the real
disk with the `ignore` crate when the database has no candidate, and
`logging::init` — a binary-only module under `main`, not part of the
library — installs the daily-rotated tracing file logger under
`<data dir>/logs`.
