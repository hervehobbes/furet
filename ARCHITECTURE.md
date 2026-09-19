# Architecture

furet is layered around a pure core: normalization, the matching engine,
and ranking are deterministic functions with no filesystem or clock
access, so they can be tested exhaustively from plain data. Everything
impure — directory listing, timestamps, the SQLite database, shell
integration — lives in the outer layers and reaches the core through
injected traits (filesystem and clock are injected as traits). The CLI,
storage, and shell-hook layers sit around that core and translate between
the real environment and the pure functions.
