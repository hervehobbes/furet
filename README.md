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
wires a prompt hook that records every directory change. The same script
also provides Tab completion of `furet`'s subcommands and options, with no
extra profile line.

## Usage

- `f <query>` — jump to the best-ranked directory matching `<query>`.
- `f -l <query>` — same, restricted to the current git project: the
  **project root** is the nearest ancestor of the current directory that
  holds a `.git` entry (directory or file, so worktrees and submodules
  count); known directories outside it are ignored, and the disk fallback
  never climbs above it.
- `f -l` (no argument) — jump to the project root itself.
- `f <partial><Tab>` — cycle through the directories furet ranks for
  `<partial>` (first argument only); accepting one inserts the full path.
  After `f -l <Tab>`, only in-project directories are proposed.
- `f -l <query> --explain` — print the scoring report of the project-scoped
  jump on stderr without jumping or recording.
- `f <path>` — jump straight to `<path>` if it exists on disk.
- `f ..`, `f ...` — go up 1, 2, ... levels.
- `f -` — jump back to the previous directory in this session.
- `f` (no argument) — jump home, or the configured `home` directory when set
  and valid.
- `f <query> --explain` — print the scoring report for `<query>` on stderr
  without jumping or recording.
- `fi [<query>]` — interactively pick from ranked matches (uses `fzf` if
  installed, else a numbered console menu). `fi -l [<query>]` restricts the
  candidates to the current git project, before and after every fzf reload.
- `furet query --local <query>` — the flag behind `f -l`: restrict the
  candidate pool to the current git project; combines with `--list`,
  `--explain`, `--color`, and `--no-ignore`. Outside a git repository it
  fails with `furet: not inside a git repository` (exit 1); an empty query
  without `--list`/`--explain` prints the project root.
- `furet query <query> --explain` — print the scoring report for `<query>`
  on stderr without jumping.
- `furet query <query> --engine <reference|nucleo>` — pick the stage-1
  matcher for this call, overriding the `engine` config key; works with
  `--list`, `--explain`, `--local`, and the disk fallback. `nucleo` is
  Helix's fuzzy matcher, opt-in; the reference engine stays the default.
  The pwsh `f`/`fi` never pass it: set `engine` in `config.toml` instead.
- `furet queries --failures` — list jumps that were probably mistakes
  (SPEC §15).
- `furet list [--all] [--paths]` — print every known directory as one tab-separated
  line: `path`, `visits`, `last_visit`, `first_seen` (local time); `--all`
  also lists directories missing from disk, with a `present`/`missing`
  column. Rows are ordered alphabetically by path, ignoring case.
  `--paths` (`-p`) prints the path alone, also with `--all`.
- `furet remove <pattern> [--confirm]` — forget known directories matching
  `<pattern>`. Without `\`, `/` or `:`, the pattern matches directory
  **names** against every segment of the path: `ombi*` removes `ombi` and
  its known subdirectories, and `*appdata*` removes every known directory
  under an `AppData` folder. Otherwise it is a **path** pattern relative to
  the current directory — except when it starts with `*`: `*\cache\*` is
  matched against the whole path, never anchored at the current directory.
  `*` matches any run of characters, crossing `\`; `?` is exactly one
  character; matching ignores case. Every match is removed at once (missing
  ones included) and reported on stderr — stdout stays empty; a removed
  directory comes back on the next `furet add` of it. `--confirm` lists the
  matches and asks before removing (e.g. `furet remove *appdata* --confirm`).
  Quote the pattern under bash (`'ombi*'`); PowerShell passes it as is.
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
line naming the key. `furet query`, `furet home`, `furet add`, and
`furet import zoxide` read it.

| Key | Type | Default | Valid |
|---|---|---|---|
| `typo_min_length` | integer | 4 | `>= 1` |
| `fallback.depth` | integer | 1 | `1..=5` |
| `fallback.up` | integer | 1 | `0..=5` |
| `fallback.no_ignore` | bool | false | — |
| `fallback.exclude` | array of strings | `node_modules, bin, obj, .git, target` | non-empty strings, replaces the default list |
| `home` | string | unset | absolute path, `/` accepted as separator, no `~`/env-var expansion |
| `retention_days` | integer | 365 | `>= 0`; `furet add` deletes `visits` and `queries` rows older than this many days, `0` keeps everything |
| `engine` | string | `"reference"` | `"reference"` or `"nucleo"`; `furet query --engine` overrides it |
| `exclude_dirs` | array of strings | empty | patterns in the `furet remove` syntax, e.g. `['node_modules', 'C:\Windows\*', '*\target\*']` |

`fallback.exclude = []` disables every exclusion: the list replaces the
defaults, it never adds to them.

`exclude_dirs` names directories that are never recorded — by `furet add`,
by the query's disk fallback, and by `furet import zoxide`. Without `\`,
`/` or `:`, a pattern matches any segment of the path (`node_modules`,
`*appdata*`); a path pattern must be absolute (`C:\Windows\*`) or start
with `*` (`*\target\*`). Prefer TOML literal strings `'C:\...'` (or write
the separators `/`), and a directory already known stays in the database
until `furet remove <pattern>`.

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
