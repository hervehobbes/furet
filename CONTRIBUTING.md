# Contributing

Solo project, single branch (`main`), sequential commits.

## Required one-time setup

After a fresh clone, activate the git hooks:

```
git config core.hooksPath tools/hooks
```

and install cargo-deny, which the DoD's dependency gate runs, and put [gitleaks](https://github.com/gitleaks/gitleaks) (secret scans) and [Semgrep](https://semgrep.dev/docs/getting-started/) (code scan) on `PATH`:

```
cargo install --locked cargo-deny
python -m pip install semgrep
```

For the security scan (below), also put
[Trivy](https://trivy.dev/) and [Grype](https://github.com/anchore/grype)
on `PATH`.

`pre-commit` checks the staged content (comment policy in `.rs` files,
forbidden words from `tools/forbidden-words.txt`, secrets with
`gitleaks git --pre-commit --staged`); `pre-push` runs the full DoD,
`tools/Run-DoD.ps1`, so nothing is pushed unless it is green.

## Before every commit

`tools/Run-DoD.ps1` (PowerShell 7) must run green: format, clippy,
`cargo deny check` (advisories, licenses, bans, sources — configured by
`deny.toml` at the repo root), a Semgrep scan of `src`, `build.rs` and
`tests` (`p/rust`, `p/security-audit`, `p/secrets`; any finding fails
it), tests, release build, a `gitleaks` scan
of the full history, and a smoke run of the release binary. It stops at
the first failing step and names it. It writes its full output to
`target/dod/dod-<timestamp>.log` and prints that path first and last;
that path is the one a lot report cites.
Its test step runs `cargo test` with `TMP`/`TEMP` pointing at a
throwaway directory holding a decoy that matches almost any query, so
a test whose sandbox lets the disk fallback reach the temp dir fails on
every machine; such a test runs its queries from an isolated
`world.tree/cwd`.
The cargo-deny step needs network access: it fetches the RustSec
advisory database.

The Semgrep step needs network access too: it downloads its rulesets
from the Semgrep registry. It excludes
`rust.lang.security.unsafe-usage.unsafe-usage`, because
`unsafe_code = "deny"` in `Cargo.toml` already gates `unsafe` at
compile time and each exception is an audited `#[allow(unsafe_code)]`.

`tests/docs.rs` fails when a doc cites a test or a `module::item` that
no longer exists. A rename therefore updates the docs in the same
commit.

## Security scan

`tools/Run-Security.ps1` (PowerShell 7) is run by hand, not by the DoD
or a hook: it needs network access and refreshes the Grype
vulnerability database first. It runs Trivy (`trivy fs`: dependencies,
secrets, misconfigurations) and Grype (`grype dir`) on the repo,
skipping `target`, `prompts`, `mutants.out*` and `.sonar`, each once as
a full report and once as a gate. A gate fails on any HIGH or CRITICAL
finding that has a fix (Trivy also fails on HIGH or CRITICAL secrets
and misconfigurations). Every step runs even when an earlier one
fails; the summary names the failing blocking steps, and a missing
tool fails its blocking steps. The full output goes to
`target/security/security-<timestamp>.log`, whose path is printed
first and last.

Its last step, report only, runs `cargo update --dry-run --verbose`:
it lists the semver-compatible updates `cargo update` would apply and,
as `Unchanged <crate> (available: <version>)`, the newer versions that
the `Cargo.toml` requirements exclude. It never writes `Cargo.lock`.

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
