# Contributing

Solo project, single branch (`main`), sequential commits.

## Required one-time setup

After a fresh clone, activate the git hooks:

```
git config core.hooksPath tools/hooks
```

and install cargo-deny, which the DoD's dependency gate runs:

```
cargo install --locked cargo-deny
```

`pre-commit` checks the staged content (comment policy in `.rs` files,
forbidden words from `tools/forbidden-words.txt`); `pre-push` runs the
quality gates (`cargo fmt --check`, `cargo clippy --all-targets --
-D warnings`, `cargo test`).

## Before every commit

`tools/Run-DoD.ps1` (PowerShell 7) must run green: format, clippy,
`cargo deny check` (advisories, licenses, bans, sources — configured by
`deny.toml` at the repo root), tests, release build, and a smoke run of
the release binary. It stops at the first failing step and names it.
The cargo-deny step needs network access: it fetches the RustSec
advisory database.

`tests/docs.rs` fails when a doc cites a test or a `module::item` that
no longer exists. A rename therefore updates the docs in the same
commit.

## Comment policy

Standalone `//` comments in Rust code must be `// WHY: ...` lines, and
`///` doc-comment blocks are limited to 2 lines. `tools/Strip-Comments.ps1`
repairs violations in place when wired as a Claude Code `PostToolUse`
hook on `Edit`/`Write`.

## Mutation testing

The mutation-testing campaign runs [cargo-mutants](https://mutants.rs) one module at a
time: install it with `cargo install --locked cargo-mutants`, then kill every missed
mutant of `cargo mutants -f src/<module>.rs --cargo-test-arg=--lib` with a new or stronger
test; the procedure and the results table live in `MUTANTS.md`. The DoD does not run
mutation testing, because it is too slow.
