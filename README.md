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
- `f <partial><Tab>` — cycle through the directories furet ranks for
  `<partial>` (first argument only); accepting one inserts the full path.
- `f <path>` — jump straight to `<path>` if it exists on disk.
- `f ..`, `f ...` — go up 1, 2, ... levels.
- `f -` — jump back to the previous directory in this session.
- `f` (no argument) — jump home, or the configured `home` directory when set
  and valid.
- `f <query> --explain` — print the scoring report for `<query>` on stderr
  without jumping or recording.
- `fi [<query>]` — interactively pick from ranked matches (uses `fzf` if
  installed, else a numbered console menu).
- `furet query <query> --explain` — print the scoring report for `<query>`
  on stderr without jumping.
- `furet queries --failures` — list jumps that were probably mistakes
  (SPEC §15).
- `furet list [--all] [--paths]` — print every known directory as one tab-separated
  line: `path`, `visits`, `last_visit`, `first_seen` (local time); `--all`
  also lists directories missing from disk, with a `present`/`missing`
  column. `--paths` (`-p`) prints the path alone, also with `--all`.
- `furet query <query> --list --color` — wrap each printed path in the
  `LS_COLORS` directory color (the `di=` entry). Does nothing unless the
  `LS_COLORS` environment variable is set — PowerShell doesn't set it by
  default, so set it yourself, e.g. `$env:LS_COLORS = "di=1;36"`.

## Importing from zoxide

Start with a filled database instead of an empty one:

```powershell
zoxide query -ls | furet import zoxide
```

Reads `<score> <path>` lines from stdin (zoxide's own score is discarded —
only the list of directories matters, SPEC §3). Already-known directories
are skipped, so running it again is a no-op. furet never runs zoxide or
reads its binary database directly.

## Configuration

`<data dir>/config.toml` overrides these built-in defaults; a missing file
is normal, and any invalid key falls back to its default with a `WARN` log
line naming the key. `furet query`, `furet home`, and `furet add` read it.

| Key | Type | Default | Valid |
|---|---|---|---|
| `typo_min_length` | integer | 4 | `>= 1` |
| `fallback.depth` | integer | 1 | `1..=5` |
| `fallback.up` | integer | 1 | `0..=5` |
| `fallback.no_ignore` | bool | false | — |
| `fallback.exclude` | array of strings | `node_modules, bin, obj, .git, target` | non-empty strings, replaces the default list |
| `home` | string | unset | absolute path, `/` accepted as separator, no `~`/env-var expansion |
| `retention_days` | integer | 365 | `>= 0`; `furet add` deletes `visits` and `queries` rows older than this many days, `0` keeps everything |

`fallback.exclude = []` disables every exclusion: the list replaces the
defaults, it never adds to them.

## Logs

Every invocation writes plain-text logs to `<data dir>/logs/` — a daily
rotated `furet.<date>.log`, 7 files kept — and never to stdout or stderr.
The data dir is `%LOCALAPPDATA%\furet` unless `FURET_DATA_DIR` is set (an empty value counts as unset).
The default level is `info`; `FURET_LOG` overrides it with `EnvFilter`
syntax (e.g. `FURET_LOG=debug`). A failure to open the log file never
breaks a command.

## Status

Roadmap lots 0 through 21 are shipped: normalization, the two-stage fuzzy
engine, SQLite storage, `furet add`/`query`/`up`/`back`/`home`, the pwsh
integration (`f`, `fi`) covered by executed pwsh tests, soft delete,
`--explain`, disk fallback, and the query journal — plus the database path
in `--help`, daily-rotated file logging, `config.toml` overrides, the
configurable `home`, stage-2's query-length rule, and importing zoxide's
database (`furet import zoxide`). Lot 22 added a 42-case ranking scenario
suite. See [CONTRACTS.md](CONTRACTS.md) for the engine, storage, and CLI
contracts.

Licensed under the MIT License — see [LICENSE](LICENSE).
