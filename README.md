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
- `f -3` — go 3 directories back in this session's raw visit history
  (duplicates kept, `f -1` the same as `f -`); `furet back --steps <N>`
  underneath.
- `f` (no argument) — jump home, or the configured `home` directory when set
  and valid.
- `f <query> --explain` — print the scoring report for `<query>` on stderr
  without jumping or recording.
- `fi [<query>]` — interactively pick from ranked matches (uses `fzf` if
  installed, else a numbered console menu). `fi -l [<query>]` restricts the
  candidates to the current git project, before and after every fzf reload.
  In the fzf branch, both calls (local and non-local) open a preview pane on
  the right (`--preview "furet preview {}"`, `--preview-window
  "right,50%"`) listing the highlighted directory's contents through the
  `furet preview` subcommand; the console menu branch has no preview.
  `fi !` opens an interactive menu of marks and aliases (marks `1`-`9`
  first), fed by `furet alias complete`; `fi !om` gives fzf the initial
  query `om` with the path as preview, or prefix-filters the numbered
  menu without fzf; a pick jumps and, like `f !name`, writes no
  `queries` row.
  Choices made in menus and `fi` are remembered in the query journal
  (`outcome = 'pick'`) — the base of query memory, below.
- **Query memory** — once a query has taken you to a directory, the same
  query goes back there first, even when another directory matches better:
  pick `c:\dev\ombi` for `om` once (menu or `fi`), and the next `f om` goes
  to `ombi` rather than `c:\om`. Case, accents, and extra spaces do not
  matter (`OM`, ` om ` are the same query); token order does. It is the only
  rule above the match score, and it applies only while the directory still
  matches the query and is a candidate (not the current directory, not
  missing, inside the `-l` scope); it never touches the disk fallback. A
  jump followed within 10 seconds by `f -` or by moving elsewhere (a
  probable failure) cancels it. Tab completion and `fi` list the
  remembered directory first; `--explain` shows a `memory:` line. Set
  `query_memory = false` to turn it off.
- `furet query --local <query>` — the flag behind `f -l`: restrict the
  candidate pool to the current git project; combines with `--list`,
  `--explain`, `--color`, and `--no-ignore`. Outside a git repository it
  fails with `furet: not inside a git repository` (exit 1); an empty query
  without `--list`/`--explain` prints the project root.
- `furet query --home <query>` — the same scoping around the home root
  (the `home` key, else the user profile), from anywhere; in pwsh it is
  `f -h <query>` (also `f --home` / `f <query> -h`), `f -h` alone jumps
  to the home root, and `fi -h` / `f -h <Tab>` follow the same scope.
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
- `furet stats [--top <n>]` — print an overview of the database as
  tab-separated `key<TAB>value` lines, in this exact order:
  `known_directories`, `missing_directories`, `visits`,
  `visits_last_30_days`, `queries`, `queries_last_30_days`, `jumps`,
  `probable_failures`, `failure_rate` (`probable_failures / jumps` in
  percent, one decimal, `0.0%` when there are no jumps), `stage_1`,
  `stage_2`, `stage_fallback`, `stage_menu`, `source_hook`, `source_jump`,
  `source_back`, `source_up`, `source_fallback`, `source_import`. Then at
  most `n` lines `top<TAB><visits><TAB><path>`: every present directory,
  0-visit ones included, most visited first, ties broken by path
  (`n` defaults to 10; `0` lists none). Read-only, like `furet list`: the
  stored `missing_since` flags are taken as is, nothing is reconciled or
  written.
- `furet history (--session <id> | --all) [-n <limit>]` — print the visit
  history, newest first. With `--session <id>` every line is
  `<N><TAB><time><TAB><source><TAB><path>`, numbered so that line `N` is
  exactly where `f -N` goes (line 0 is the current directory); with
  `--all`, every session's visits are listed without the numbers.
  `--limit` (`-n`, default 20) caps the number of lines, `0` prints them
  all. Read-only, like `furet list`; `fh` (next clause) is its pwsh
  wrapper.
- `fh` — the session history in one word: print the current session's
  visit history, numbered like `f -N` (line 0 is the current directory,
  line `N` is where `f -N` goes). `fh -a` lists every session without
  numbers, `fh -n 50` caps the output at 50 lines (`0` prints it all).
  Fixed name, like `fi` and `fm` — `furet init pwsh --cmd j` still
  defines `fh`, never `jh`.
- `furet remove [<pattern>] [--missing] [--confirm | --yes] [--dry-run]` —
  forget known directories matching
  `<pattern>`. Without `\`, `/` or `:`, the pattern matches directory
  **names** against every segment of the path: `ombi*` removes `ombi` and
  its known subdirectories, and `*appdata*` removes every known directory
  under an `AppData` folder. Otherwise it is a **path** pattern relative to
  the current directory — except when it starts with `*`: `*\cache\*` is
  matched against the whole path, never anchored at the current directory.
  `*` matches any run of characters, crossing `\`; `?` is exactly one
  character; matching ignores case. Every match is removed at once (missing
  ones included) and reported on stderr — stdout stays empty; a removed
  directory comes back on the next `furet add` of it. `--confirm` asks one
  question per directory on stderr (`Remove <path>? [y/N/a/q]`): `y`/`yes`
  removes it, `n`/`no`/Enter keeps it, `a`/`all` removes it and every
  later match without asking, `q`/`quit` or EOF keeps it and every later
  match, and any other answer re-asks the same question (e.g.
  `furet remove *appdata* --confirm`). `--yes` never asks — for scripts;
  `--confirm --yes` is refused. `--dry-run` prints
  `would remove <path>` per match on stderr, asks nothing even with
  `--confirm`, and never touches the database.
  Without a pattern, `--missing` reconciles every known directory with the
  disk first, then targets the ones that are really gone: a directory that
  came back — a reconnected USB key, a network drive back online — is never
  removed and its stale marker is cleared. Questions are asked **by
  default** (`Remove <path> (missing since <date>)? [y/N/a/q]`, the date in
  local time), because a disconnected USB or network drive looks missing
  too; `--yes` skips them. A pattern narrows the selection
  (`furet remove ombi* --missing`); under `--dry-run` the reconciliation
  stays in memory — no marker is written or cleared. Without a pattern and
  without `--missing`, clap refuses the command (exit 2).
  Quote the pattern under bash (`'ombi*'`); PowerShell passes it as is.
- `furet alias add <name> [<path>] [--force]` — create a named shortcut
  to a directory (`furet alias add ombi C:\apps\ombi`; without a path,
  the current directory). The name uses letters, digits, `_` and `-` and
  is case-insensitive; the path must be an existing directory. An
  existing name is refused unless `--force` replaces it. `furet alias
  list` prints `name`, path and creation date per line; `furet alias
  remove ombi` deletes one. Aliases survive `furet remove` and the
  retention purge. Jump with `f !ombi` (exact, case-insensitive, no
  fuzzy); a close-but-unknown name gets a `did you mean 'ombi'?` hint,
  and an alias to a deleted directory is an error. The prefix is the
  `alias_prefix` config key — `!` (default) or `=`; a directory literally
  named `!ombi` in the current directory wins over the alias. In the jump
  function, `f !<Tab>` lists every alias with its target path and inserts
  only the name (`f !om<Tab>` proposes `!ombi`, `!omnitool`, …).
- `furet mark set <digit> [<path>]` — set a numbered mark (`1` to `9`) on a
  directory (`furet mark set 1 C:\dev\ombi`; without a path, the current
  directory). Setting a mark again overwrites it silently, like Vim's `m1`
  — unlike `furet alias add 1`, which refuses an existing name unless
  `--force`. `furet mark list` prints one `digit<TAB>path` line per mark,
  in digit order; `furet mark delete 2` removes one mark,
  `furet mark delete 2-4` a range, `furet mark delete --all` every mark
  (`1`-`9` only — named aliases survive), all silent on unset digits and
  without confirmation. Marks share the aliases' namespace and table.
  Jump with `f !1`; an unset mark fails with `mark 3 not set` (no
  `did you mean` hint), and a mark to a deleted directory is an error.
  `furet mark next` / `furet mark prev` print the next / previous mark's
  path on stdout — wrapping from 9 to 1, skipping holes, marks on the
  current directory and missing directories (`furet: skipped mark 3:
  missing directory`), `furet: no marks set` / `furet: no other mark`
  otherwise — and record nothing.
- `fm` (pwsh) — the marks helper over lots 57-58, fixed like `fi` (never
  `jm` under `--cmd j`): `fm` lists the marks, `fm 3` marks the current
  directory (silent overwrite, Vim's `m3`; a path argument goes through),
  `fm -d 2` / `fm -d 2-4` / `fm -d!` delete one mark, a range, every mark,
  and `fm +` / `fm -` jump to the next / previous mark (wrap 9 → 1),
  recording the landing as a `jump` visit like any jump. Ctrl+Alt+→ and
  Ctrl+Alt+← run `fm +` / `fm -` from an **empty** command line — a
  non-empty line only dings — and exist only when PSReadLine is loaded.
- `furet query <query> --list --color` — wrap each printed path in the
  `LS_COLORS` directory color (the `di=` entry). Does nothing unless the
  `LS_COLORS` environment variable is set — PowerShell doesn't set it by
  default, so set it yourself, e.g. `$env:LS_COLORS = "di=1;36"`.
- `furet preview <path>` — list a directory's contents for the fzf preview
  pane: directories first (each name followed by `\`), then files, each
  group sorted ignoring case, hidden entries included, at most 50 lines
  then `… +<K> more`. Any ANSI SGR sequences in `<path>` are stripped
  first, so the colored lines `fi` feeds fzf work as is. A missing path or
  a file prints `(not a directory)`, an unreadable directory prints
  `(unreadable: <error>)`; the exit code is 0 in every case. Never opens
  the database and never reads `config.toml`, so it stays fast.

## Importing from zoxide

Start with a filled database instead of an empty one:

```powershell
zoxide query -ls | furet import zoxide
```

Reads `<score> <path>` lines from stdin (zoxide's own score is discarded —
only the list of directories matters, SPEC §3). Already-known directories
are skipped, so running it again is a no-op. furet never runs zoxide or
reads its binary database directly.

## Importing from PowerShell history

Seed the database from PSReadLine's own history file:

```powershell
Get-Content (Get-PSReadLineOption).HistorySavePath | furet import pwsh-history
```

Counts every line that is exactly one `cd`-like command (`cd`, `chdir`,
`sl`, `Set-Location`, `pushd`, `Push-Location`) followed by one literal
path, quoted or not. Everything else — other commands, pipes, `$`
variables — is ignored without being counted. Paths must be absolute: the
history does not record the working directory each command ran in, so a
relative path cannot be resolved reliably; a leading `~` means the user
profile. Already-known directories are skipped, so re-running it is a
no-op.

## Backup and moving to another machine

Save the whole database — directories, visits, the query journal and the
aliases/marks — as versioned JSON:

```powershell
furet export > furet-backup.json
```

The output is pure ASCII (every non-ASCII character is escaped), so the
redirection cannot corrupt an accented path. Restore it on another
machine (or after a reinstall) — the import is an idempotent merge, never
a replacement:

```powershell
Get-Content furet-backup.json -Raw | furet import json
```

Directories are united by key; visits and queries are added without
duplicates, so importing the same file twice adds nothing; an alias that
already exists keeps its local target, and the conflict is reported on
stderr.

## Configuration

`<data dir>/config.toml` overrides these built-in defaults; a missing file
is normal, and any invalid key falls back to its default with a `WARN` log
line naming the key. `furet query`, `furet home`, `furet add`,
`furet import zoxide`, and `furet import pwsh-history` read it.

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
| `query_memory` | bool | true | `false` stops sending a query back to the directory last chosen for it; choices are still journaled |
| `alias_prefix` | string | `"!"` | `"!"` or `"="` only; the first character that marks a query as an alias jump (`f !ombi`) |
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
suite. Lots 23 through 39 added a tab completer on the jump function's first
argument, `furet list` (`--all`, `--paths`, ordered by path), `ts` indexes on
`visits` and `queries` with the `retention_days` purge in `furet add`,
`furet remove <pattern>`, the `exclude_dirs` config key, and a compile-time
lint that keeps stdout reserved for the jump target. See [CONTRACTS.md](CONTRACTS.md) for the engine, storage, and CLI
contracts.

Version 0.2.0 completes `prompts/SPEC-v2.md`: completions for `furet`'s
subcommands, `--local`, the opt-in `nucleo` engine, `furet remove`'s
confirmation and `--missing`, `furet stats`, the `fi` preview, and query
memory; lot 50 shows the binary's UTC build date as the last line of
`furet --help`. Lot 51 adds the `aliases` table shared by aliases and
marks (migration 4); lot 52 adds `furet alias add`, `list` and `remove`
over it; lot 53 resolves them — `f !ombi`, the `alias_prefix` key
(`!`/`=`) and the `did you mean` hint; lot 54 completes alias words on
Tab — `f !om<Tab>` lists every matching alias with its path and inserts
the name. Lot 55 scopes `furet query --home` to the home root (the
`home` key, else the user profile), from anywhere; lot 56 wires it into
pwsh — `f -h`, `fi -h`, and Tab after `-h` all scope to the home. Lot 57
adds numbered marks in the binary — `furet mark set`, `list` and
`delete` over the shared `aliases` table, with the mark messages in
alias jumps (`mark 3 not set`). Lot 58 adds mark cycling in the binary —
`furet mark next` / `furet mark prev` print the next / previous mark's
path through the pure `alias::cycle` (lowest mark of the current
directory, wrap 9 → 1, missing marks skipped on stderr), recording
nothing. Lot 59 wires the pwsh side: the `fm` helper (fixed name, plain
`$args` so `-d` is never bound — set/list/delete/cycle) and the
Ctrl+Alt+→/← PSReadLine bindings, which run `fm +` / `fm -` on an empty
command line and only ding otherwise. Lot 60 adds `fi !`, the interactive
menu of aliases and marks (fed by `furet alias complete`, fzf-fuzzy or
strict-prefix, a `jump` visit and no `queries` row) — **closing design
section 3**: aliases, home scope and marks are complete.

Version 0.3.0 adds the aliases, home-scope and marks cycle (lots 51-61):
`furet alias` add/list/remove with `f !name` resolution, the `alias_prefix`
(`!`/`=`) key, Tab completion and the `fi !` menu; the home scope (`f -h`,
`furet query --home`); and marks — `furet mark`, the `fm` helper, `fm +`/`fm -`
cycling, and Ctrl+Alt+→/←. French usage examples live in [example.md](example.md).
Lot 63 generalizes `f -` into `f -N`: go `N` directories back in the
session's raw visit history (duplicates kept, `f -1` equal to `f -`),
recorded as a `back` visit — the binary side is `furet back --steps <N>`.
Lot 64 adds `furet import pwsh-history`: seed the database from PSReadLine
history — `cd`-like lines with one absolute path (`~` expands to the user
profile), later lines more recent, re-running it a no-op. Lot 65 adds
`furet history`: the visit history, newest first, numbered like `f -N`
per session (`--session <id>`) or listed across every session without
numbers (`--all`); the `fh` pwsh wrapper is the next lot. Lot 66 wires
that wrapper: the fixed-name `fh` prints the session history (`fh -a`
for every session, `fh -n <N>` for a limit) without typing
`furet history`. Lot 67 adds `furet export`: the whole database as
versioned JSON on stdout, pure ASCII so a redirect keeps every accented
path; the import of that file is the next lot. Lot 68 completes the
backup pair: `furet import json` merges a `furet export` file
idempotently — directories united by key, visits and queries
deduplicated, local aliases kept on conflict.

Licensed under the MIT License — see [LICENSE](LICENSE).
