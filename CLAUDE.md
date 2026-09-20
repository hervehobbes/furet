# CLAUDE.md — furet

`furet` is a Rust, zoxide-like fuzzy directory jumper (Windows-first). This
file governs every agent session in this repo, including GLM sessions.

## Language
Everything in this repository is English: code, identifiers, comments, docs,
commit messages, and every lot prompt written to `prompts/`. French exists
only in `prompts/SPEC.md`, which is gitignored and never committed.
`example.md` is written in French at Hervé's request (temporary exception).

## Reference engine
Section 7 of the spec reprises a proven fuzzy-matching engine "as is." Look at
how zoxide (MIT) solves shell integration, hooks, `init`, and path resolution
before designing any related mechanism — cite it, don't reinvent it. Any
divergence from the reference engine or from zoxide's approach is Hervé's
decision, not an agent's.

## D1 — recency breaks ties, it never ranks
A more recent directory must never outrank a better-matched one. Changing
this is Hervé's decision.

## stdout discipline
The binary writes only the jump target path to `stdout`. Menus, errors, and
logs go to `stderr`, the console, or the log file — never `stdout`.

## Comments — machine-enforced, not prose
- No `//` line comments except a single-line `// WHY: ...`.
- `///` doc comments only on public items, at most 2 lines.
- A `PostToolUse` hook (`tools/Strip-Comments.ps1`) silently removes
  non-conforming comments after every `Edit`/`Write` on a `.rs` file.
- `tools/hooks/pre-commit` blocks the commit if any survive anyway.
- A separate heuristic in `pre-commit` warns (does not block) on common
  French words in `.rs` files.
- `tools/forbidden-words.txt` (gitignored, not visible to agents by content)
  blocks commits containing professional project names.
Do not treat these as guidelines to interpret — they are binary, checked by
the machine. Write comments correctly the first time; don't rely on the hook.

## Proof
Only raw command output is proof of anything. An agent's prose summary of
what it did is not evidence. Definition of Done for every lot:
`tools/Run-DoD.ps1` green, its raw output pasted into the report.

## Routing — one lot = one mechanism, one executor
State the choice and a one-line reason in the lot prompt:
- **GLM 5.3 Flash** — mechanical, fully specified, no design decision
  (scenarios, fixtures, docs).
- **GLM 5.3** — one well-bounded mechanism with a precise spec and tests.
- **Claude subagent** (`.claude/agents/`) — core algorithm (SPEC §7, §9),
  cross-cutting changes, or a lot GLM failed twice.
  - `implementer`: general-purpose executor.
  - `core-algorithm`: SPEC §7 and §9 only, must prove invariants with
    proptest (stage 1 > stage 2, deterministic total order, D1 tie-break).
  - `reviewer`: read-only, checks diff + raw DoD output after every lot.
Every GLM lot is followed by the reviewer subagent, launched from a Claude
Code session (a GLM session cannot run it and must not imitate it), checking
the diff and the raw DoD output before the next lot starts.

## Journal
`JOURNAL.md`, newest entry first, written by the executor, at most 10 lines,
starting with one short sentence:
```
## YYYY-MM-DD — branch main — vX.Y.Z
Done: … / Decisions: … / Next: …
```

## Database docs
`DATABASE.md` documents every table, field, and index in the SQLite schema
(`src/storage.rs`'s `MIGRATIONS`). Any lot that changes the schema — a new
table, column, index, or migration — updates `DATABASE.md` in the same lot.
Per the Process rule above, a schema change is always its own lot, so this
never spans lots.

## Guardrails (technical, not prose)
Enforced by `.claude/settings.json`: no editing `tools/hooks/**` or
`.claude/**`, no `git commit --no-verify`, no `git push --force`. These are
permission denials, not requests — do not ask to bypass them.

## Ambiguity
If `prompts/SPEC.md` is ambiguous or a lot prompt conflicts with it: stop and
ask Hervé. Never interpret, never guess, never invent a feature not in scope.

## Process
One lot = one mechanism. Tests (or scenarios, SPEC §13) before code. What
compiles and passes is committed; a blocker is reported; the next lot does
not start until Hervé says so. A SQLite schema change is always its own lot.
`BACKLOG.md` is Hervé's: agents propose entries, they don't write them.
