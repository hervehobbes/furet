# furet

furet is a zoxide-like fuzzy directory jumper with a Windows-first focus.
It learns the directories you actually visit and lets you jump straight
back to them by typing a short fuzzy fragment of a path instead of typing
(or remembering) the whole thing.

## Building

```
cargo build --release
```

Put the resulting `target/release/furet.exe` on your `PATH`.

## Installing the PowerShell integration

Add this to your PowerShell profile (`$PROFILE`):

```powershell
Invoke-Expression (& furet init pwsh | Out-String)
```

`&`'s output splits into a `String[]` when it spans multiple lines, and
`Invoke-Expression -Command` requires a plain `String` — hence the
`Out-String`, exactly as in zoxide's own pwsh hook. This defines a `f`
function (rename it with `furet init pwsh --cmd <name>`) plus `fi`, and
wires a prompt hook that records every directory change.

## Usage

- `f <query>` — jump to the best-ranked directory matching `<query>`.
- `f <path>` — jump straight to `<path>` if it exists on disk.
- `f ..`, `f ...` — go up 1, 2, ... levels.
- `f -` — jump back to the previous directory in this session.
- `f` (no argument) — jump home.
- `fi [<query>]` — interactively pick from ranked matches (uses `fzf` if
  installed, else a numbered console menu).
- `furet query <query> --explain` — print the scoring report for `<query>`
  on stderr without jumping.
- `furet queries --failures` — list jumps that were probably mistakes
  (SPEC §15).

## Status

All V1 roadmap lots (0 through 11) are shipped: normalization, the two-stage
fuzzy engine, SQLite storage, `furet add`/`query`/`up`/`back`, the pwsh
integration (`f`, `fi`), soft delete, `--explain`, disk fallback, and the
query journal. See [CONTRACTS.md](CONTRACTS.md) for the engine, storage, and
CLI contracts.

Licensed under the MIT License — see [LICENSE](LICENSE).
