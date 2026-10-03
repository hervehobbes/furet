use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::generate;
use clap_complete::shells::PowerShell;
use furet::alias;
use furet::calibration::{self, FailureReason};
use furet::clock::{Clock, SystemClock, Timestamp};
use furet::config::{self, Settings};
use furet::decision::{self, Decision};
use furet::explain::{self, Origin};
use furet::fallback;
use furet::import;
use furet::memory::{self, Recall};
use furet::paths;
use furet::preview;
use furet::project;
use furet::rank::{self, Candidate, Scored, Stage};
use furet::remove;
use furet::soft_delete;
use furet::stats;
use furet::storage;
use rusqlite::Connection;
use tracing::{debug, error, info, warn};

mod logging;
mod pwsh;

#[derive(Parser)]
#[command(
    name = "furet",
    version,
    about = "A zoxide-like fuzzy directory jumper"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Record a visit to a directory.
    Add {
        /// Directory to record, absolute or relative.
        path: String,
        /// Terminal session the visit belongs to.
        #[arg(long)]
        session: String,
        /// What triggered the visit.
        #[arg(long, value_enum, default_value = "hook")]
        source: Source,
        /// Directory the visit started from.
        #[arg(long)]
        from: Option<String>,
        /// Query text that led to this directory; journals it as a pick.
        #[arg(long, requires = "from")]
        query: Option<String>,
    },
    /// Rank recorded directories and print the best match, falling back to a
    /// disk walk (SPEC section 11) when nothing matches.
    Query {
        /// Query text; quote it when it contains spaces.
        #[arg(default_value = "")]
        query: String,
        /// Print every ranked candidate, best first; an empty query lists by
        /// recency instead.
        #[arg(long)]
        list: bool,
        /// Print the scoring report on stderr and jump nowhere.
        #[arg(long)]
        explain: bool,
        /// Wrap every `--list` path in its `LS_COLORS` directory color.
        #[arg(long)]
        color: bool,
        /// Disable gitignore rules in the disk fallback walk (SPEC section 11).
        #[arg(long)]
        no_ignore: bool,
        /// Restrict candidates to the current git project (nearest ancestor with a .git entry).
        #[arg(short, long)]
        local: bool,
        /// Restrict candidates to the home directory (config `home`, else the user profile).
        #[arg(long, conflicts_with = "local")]
        home: bool,
        /// Stage-1 matching engine; overrides the `engine` key of config.toml.
        #[arg(long, value_enum)]
        engine: Option<EngineArg>,
    },
    /// Print the ancestor `n` levels above the current directory.
    Up {
        /// Levels to climb; must be at least 1.
        n: u32,
    },
    /// Print the second-to-last directory visited in this session.
    Back {
        /// Terminal session to look the previous directory up in.
        #[arg(long)]
        session: String,
        /// How many directories back to go; 1 is the previous directory.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        steps: u32,
    },
    /// Print shell integration code for the requested shell.
    Init {
        #[command(subcommand)]
        shell: InitShell,
    },
    /// Inspect the query journal (SPEC section 15).
    Queries {
        /// List queries whose jump was probably a mistake; mandatory today.
        #[arg(long)]
        failures: bool,
    },
    /// List known directories as tab-separated lines.
    List {
        /// Also list directories missing from disk, with a presence column.
        #[arg(long)]
        all: bool,
        /// Print only the path of each directory.
        #[arg(short, long)]
        paths: bool,
    },
    /// Print database statistics as tab-separated lines.
    Stats {
        /// Number of most-visited directories to list; 0 lists none.
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// Forget known directories matching a name or path pattern.
    Remove {
        #[arg(required_unless_present = "missing")]
        pattern: Option<String>,
        /// Target known directories missing from disk; asks before each unless --yes.
        #[arg(long)]
        missing: bool,
        /// Ask before removing each directory ([y/N/a/q]).
        #[arg(long)]
        confirm: bool,
        /// Never ask for confirmation (for scripts).
        #[arg(long, conflicts_with = "confirm")]
        yes: bool,
        /// Print what would be removed and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Manage named directory aliases.
    Alias {
        #[command(subcommand)]
        action: AliasAction,
    },
    /// Manage numbered marks (aliases named 1-9).
    Mark {
        #[command(subcommand)]
        action: MarkAction,
    },
    /// Print the configured home directory, or nothing when unset or invalid.
    Home,
    /// Import directories recorded by another tool, read from stdin.
    Import {
        /// Tool to import from.
        source: ImportSource,
    },
    /// List a directory's contents for the fzf preview pane.
    Preview {
        /// Directory to list; ANSI color codes are ignored.
        path: String,
    },
}

#[derive(Subcommand)]
enum InitShell {
    /// Print the PowerShell integration script.
    Pwsh {
        /// Name of the generated jump function; the interactive `fi` keeps its name.
        #[arg(long, default_value = "f")]
        cmd: String,
    },
}

#[derive(Subcommand)]
enum AliasAction {
    /// Create an alias to a directory, the current one by default.
    Add {
        /// Letters, digits, `_` and `-`; case-insensitive.
        name: String,
        /// Target directory; defaults to the current directory.
        path: Option<String>,
        /// Replace an existing alias with the same name.
        #[arg(long)]
        force: bool,
    },
    /// List aliases as tab-separated lines.
    List,
    /// Delete an alias.
    Remove {
        /// Name of the alias, case-insensitive.
        name: String,
    },
    /// Print alias completions for the pwsh completer.
    #[command(hide = true)]
    Complete {
        /// The word being completed, prefix included.
        #[arg(default_value = "")]
        word: String,
    },
}

#[derive(Subcommand)]
enum MarkAction {
    /// Set a mark on a directory, overwriting it silently.
    Set {
        /// Mark digit, 1 to 9.
        digit: String,
        /// Directory to mark; defaults to the current directory.
        path: Option<String>,
    },
    /// List marks as tab-separated lines, by digit.
    List,
    /// Delete a mark or a range of marks (2 or 2-4); silent on unset marks.
    Delete {
        /// One mark (2) or a range (2-4).
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        spec: Option<String>,
        /// Delete every mark (1-9); named aliases are kept.
        #[arg(long)]
        all: bool,
    },
    /// Print the directory of the next mark, wrapping from 9 to 1.
    Next,
    /// Print the directory of the previous mark, wrapping from 1 to 9.
    Prev,
}

#[derive(Clone, Copy, ValueEnum)]
enum EngineArg {
    Reference,
    Nucleo,
}

impl EngineArg {
    fn engine(self) -> rank::Engine {
        match self {
            EngineArg::Reference => rank::Engine::Reference,
            EngineArg::Nucleo => rank::Engine::Nucleo,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ImportSource {
    /// Import from `zoxide query -ls` on stdin.
    Zoxide,
    /// Import cd-like lines from a PSReadLine history on stdin.
    PwshHistory,
}

#[derive(Clone, Copy, ValueEnum)]
enum Source {
    Hook,
    Jump,
    Back,
    Up,
    Fallback,
    Import,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Hook => "hook",
            Source::Jump => "jump",
            Source::Back => "back",
            Source::Up => "up",
            Source::Fallback => "fallback",
            Source::Import => "import",
        }
    }
}

fn main() {
    let guard = logging::init();
    let matches = Cli::command()
        .after_help(runtime_paths_help())
        .get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    let code = match cli.command {
        Command::Add {
            path,
            session,
            source,
            from,
            query,
        } => report(add(
            &path,
            &session,
            source,
            from.as_deref(),
            query.as_deref(),
        )),
        Command::Query {
            query,
            list,
            explain,
            color,
            no_ignore,
            local,
            home,
            engine,
        } => report(query_directories(
            &query,
            list,
            explain,
            color,
            no_ignore,
            local,
            home,
            engine.map(EngineArg::engine),
        )),
        Command::Up { n } => report(up(n)),
        Command::Back { session, steps } => report(back(&session, steps)),
        Command::Init { shell } => match shell {
            InitShell::Pwsh { cmd } => report(init_pwsh(&cmd)),
        },
        Command::Queries { failures } => report(queries_command(failures)),
        Command::List { all, paths } => report(list_command(all, paths)),
        Command::Stats { top } => report(stats_command(top)),
        Command::Remove {
            pattern,
            missing,
            confirm,
            yes,
            dry_run,
        } => report(remove_command(
            pattern.as_deref(),
            missing,
            confirm,
            yes,
            dry_run,
        )),
        Command::Alias { action } => report(alias_command(action)),
        Command::Mark { action } => report(mark_command(action)),
        Command::Home => report(home_command()),
        Command::Import { source } => report(match source {
            ImportSource::Zoxide => import_zoxide(),
            ImportSource::PwshHistory => import_pwsh_history(),
        }),
        Command::Preview { path } => report(preview_command(&path)),
    };
    // WHY: process::exit skips destructors, so the guard is dropped explicitly to flush buffered log lines.
    drop(guard);
    process::exit(code);
}

// WHY: the runtime file locations depend on the environment, so a static clap attribute cannot hold them.
fn runtime_paths_help() -> String {
    [
        database_help_line(),
        config_help_line(),
        logs_help_line(),
        build_date_help_line(),
    ]
    .join("\n")
}

fn database_help_line() -> String {
    match storage::db_path() {
        Ok(path) => format!("Database file: {}", path.display()),
        Err(error) => format!("Database file: unavailable: {error}"),
    }
}

fn config_help_line() -> String {
    match storage::config_path() {
        Ok(path) => match std::fs::exists(&path) {
            Ok(true) => format!("Config file: {} (found)", path.display()),
            Ok(false) => {
                format!(
                    "Config file: {} (not found, defaults apply)",
                    path.display()
                )
            }
            Err(error) => format!("Config file: unavailable: {error}"),
        },
        Err(error) => format!("Config file: unavailable: {error}"),
    }
}

fn logs_help_line() -> String {
    match storage::logs_dir() {
        Ok(dir) => format!("Log directory: {}", dir.display()),
        Err(error) => format!("Log directory: unavailable: {error}"),
    }
}

fn build_date_help_line() -> String {
    format!("Build date: {}", env!("FURET_BUILD_DATE"))
}

fn report(outcome: Result<(), Box<dyn Error>>) -> i32 {
    match outcome {
        Ok(()) => 0,
        Err(error) => {
            error!("command failed: {error}");
            eprintln!("furet: {error}");
            1
        }
    }
}

fn add(
    path: &str,
    session: &str,
    source: Source,
    from: Option<&str>,
    query: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    debug!(
        path,
        session,
        source = source.as_str(),
        from = ?from,
        query = ?query,
        "add"
    );
    let clock = SystemClock::new();
    let base = env::current_dir()?;
    let dir = paths::resolve(path, &base)?;
    let settings = load_settings();
    if is_excluded(&settings.exclude_dirs, &dir.path) {
        debug!(path = %dir.path, "excluded; not recorded");
        return Ok(());
    }
    let conn = storage::open()?;
    // WHY: the pick row shares the visit's timestamp so calibration sees the landing.
    let now = clock.now();
    let dir_id = storage::upsert_dir(&conn, &dir.path, &dir.key, now)?;
    let (from_dir_id, from_cwd) = match from {
        Some(input) => {
            let origin = paths::resolve(input, &base)?;
            (
                storage::dir_id_by_key(&conn, &origin.key)?,
                Some(origin.path),
            )
        }
        None => (None, None),
    };
    storage::insert_visit(&conn, dir_id, now, source.as_str(), session, from_dir_id)?;
    info!(path = %dir.path, source = source.as_str(), "visit recorded");
    if let Some(text) = query
        && let Some(cwd) = from_cwd.as_deref()
        && !text.trim().is_empty()
    {
        storage::insert_query(&conn, now, cwd, text, Some(dir_id), "menu", "pick")?;
        info!(query = text, outcome = "pick", "query journal insert");
    }
    let retention_days = settings.retention_days;
    if retention_days > 0 {
        let cutoff = clock
            .now()
            .unix_seconds()
            .saturating_sub(i64::from(retention_days) * SECONDS_PER_DAY);
        let (visits, queries) = storage::purge_before(&conn, Timestamp::from_unix_seconds(cutoff))?;
        debug!(retention_days, visits, queries, "retention purge");
    }
    Ok(())
}

const SECONDS_PER_DAY: i64 = 86_400;

// WHY: distinct from any real shell GUID, so `back` never sees this visit.
const FALLBACK_SESSION: &str = "fallback";

// WHY: the lot-pinned one-flag-per-parameter shape reaches 8 arguments; grouping them is a design change.
#[allow(clippy::too_many_arguments)]
fn query_directories(
    query: &str,
    list: bool,
    explain: bool,
    color: bool,
    no_ignore: bool,
    local: bool,
    home: bool,
    engine: Option<rank::Engine>,
) -> Result<(), Box<dyn Error>> {
    debug!(
        query,
        list,
        explain,
        color,
        no_ignore,
        local,
        home,
        ?engine,
        "query"
    );
    let settings = load_settings();
    debug!(?settings, "effective settings");
    if let Some(outcome) = alias_query(query, list, explain, local, home, settings.alias_prefix) {
        return outcome;
    }
    let cwd = env::current_dir()?;
    let current = paths::canonical(&cwd)?;
    let clock = SystemClock::new();
    let scope_root = scope_root(&current.path, local, home, &settings)?;
    // WHY: the root itself is the answer Hervé wants from `f -l`, so no database is even opened.
    if let Some(root) = &scope_root
        && query.trim().is_empty()
        && !list
        && !explain
    {
        print_result(root);
        return Ok(());
    }
    let conn = storage::open()?;
    // WHY: a non-empty query only stats the directories it matches, so its cost no longer grows with every known directory.
    let check_all = explain || query.trim().is_empty();
    let entries = scoped_entries(&conn, scope_root.as_deref(), check_all, &clock)?;
    let candidates: Vec<Candidate> = entries
        .iter()
        .map(|entry| {
            let split = paths::split(&entry.path);
            Candidate {
                name: split.name,
                folder: split.folder,
                last_visit: entry.last_visit,
                missing: check_all && entry.missing,
                path: entry.path.clone(),
            }
        })
        .collect();
    if list && query.trim().is_empty() {
        print_lines(&list_by_recency(&candidates, &current.path, color));
        return Ok(());
    }
    let engine = engine.unwrap_or(settings.engine);
    debug!(engine = engine.name(), "stage-1 engine");
    let mut db_ranked = rank::rank(
        query,
        &current.path,
        &candidates,
        settings.typo_min_length,
        engine,
    );
    if !check_all {
        drop_missing(&conn, entries, &mut db_ranked, &clock)?;
    }
    let is_fallback = !query.trim().is_empty() && db_ranked.is_empty();
    // WHY: a cwd outside the scope is never walked; the fallback starts at the scope root itself.
    let fallback_start = match &scope_root {
        Some(root) if !project::within(&current.path.to_lowercase(), &root.to_lowercase()) => {
            root.clone()
        }
        _ => current.path.clone(),
    };
    // WHY: kept as if/else; the lot's `.then(..).unwrap_or_default()` form trips clippy::obfuscated_if_else.
    let fallback_pool = if is_fallback {
        fallback_candidates(&fallback_start, no_ignore, &settings, scope_root.as_deref())
    } else {
        Vec::new()
    };
    let (pool, ranked) = if is_fallback {
        let ranked = rank::rank(
            query,
            &current.path,
            &fallback_pool,
            settings.typo_min_length,
            engine,
        );
        (&fallback_pool, ranked)
    } else {
        (&candidates, db_ranked)
    };
    debug!(candidates = pool.len(), fallback = is_fallback, "ranking");
    let recall = query_recall(&conn, query, settings.query_memory, is_fallback, explain)?;
    debug!(?recall, "query memory");
    if explain {
        let mut report = explain::explain_recalled(
            query,
            &current.path,
            pool,
            origin(is_fallback),
            settings.typo_min_length,
            engine,
            recall,
        );
        if local {
            report.project_root = scope_root.clone();
        }
        if home {
            report.home_root = scope_root.clone();
        }
        eprint!("{}", explain::render(&report));
        return Ok(());
    }
    // WHY: SPEC-v2 §24 never lets memory reach the disk fallback's candidates.
    let remembered = recall.path().filter(|_| !is_fallback);
    let memory_applied = memory::applies(&ranked, remembered);
    let ranked = memory::promote(ranked, remembered);
    if list {
        let lines: Vec<String> = ranked
            .iter()
            .map(|scored| colorize(&scored.candidate.path, color))
            .collect();
        print_lines(&lines);
        return Ok(());
    }
    debug!(ranked = ranked.len(), "decision");
    // WHY: a remembered directory jumps even from a stage-2 tie; decide itself stays SPEC §9.
    let decision = match ranked.first() {
        Some(top) if memory_applied => Decision::Jump(top.candidate),
        _ => decision::decide(&ranked),
    };
    let stage = query_stage(query, is_fallback, &decision, &ranked);
    let journal = Journal {
        conn: &conn,
        clock: &clock,
        current_path: &current.path,
        query,
        stage: stage.as_deref(),
    };
    conclude(&journal, decision, is_fallback, &settings.exclude_dirs)
}

fn alias_query(
    text: &str,
    _list: bool,
    explain: bool,
    local: bool,
    home: bool,
    prefix: char,
) -> Option<Result<(), Box<dyn Error>>> {
    debug!(text, "alias query");
    let mut tokens = text.split_whitespace();
    let first = tokens.next()?;
    let name = first.strip_prefix(prefix)?;
    if tokens.next().is_some() {
        return Some(Err("an alias takes no other token".into()));
    }
    if local {
        return Some(Err("--local cannot be combined with an alias".into()));
    }
    if home {
        return Some(Err("--home cannot be combined with an alias".into()));
    }
    let Some(key) = alias::key(name) else {
        return Some(Err(format!(
            "invalid alias name '{name}': use letters, digits, '_' and '-'"
        )
        .into()));
    };
    Some((|| {
        let conn = storage::open()?;
        let is_mark = alias::mark_digit(name).is_some();
        let Some(alias) = storage::alias_by_key(&conn, &key)? else {
            if is_mark {
                return Err(format!("mark {name} not set").into());
            }
            let names: Vec<String> = storage::alias_listing(&conn)?
                .into_iter()
                .map(|row| row.name)
                .collect();
            let message = match alias::suggestion(&key, &names) {
                Some(near) => format!("unknown alias '{name}'; did you mean '{near}'?"),
                None => format!("unknown alias '{name}'"),
            };
            return Err(message.into());
        };
        if !Path::new(&alias.path).is_dir() {
            if is_mark {
                return Err(
                    format!("mark {name} points to a missing directory: {}", alias.path).into(),
                );
            }
            return Err(format!(
                "alias '{}' points to a missing directory: {}",
                alias.name, alias.path
            )
            .into());
        }
        if explain {
            eprintln!("alias: {} -> {}", alias.name, alias.path);
        } else {
            print_result(&alias.path);
        }
        Ok(())
    })())
}

fn scope_root(
    current_path: &str,
    local: bool,
    home: bool,
    settings: &Settings,
) -> Result<Option<String>, Box<dyn Error>> {
    if local {
        Ok(Some(
            project::root(current_path, &project::RealGitMarker)
                .ok_or("not inside a git repository")?,
        ))
    } else if home {
        Ok(Some(home_root(settings)?))
    } else {
        Ok(None)
    }
}

fn scoped_entries(
    conn: &Connection,
    project_root: Option<&str>,
    check_all: bool,
    clock: &SystemClock,
) -> Result<Vec<storage::DirEntry>, Box<dyn Error>> {
    let mut entries = storage::dir_entries(conn)?;
    if let Some(root) = project_root {
        let root_key = root.to_lowercase();
        entries.retain(|entry| project::within(&entry.path.to_lowercase(), &root_key));
    }
    if check_all {
        entries = reconcile_on_disk(conn, entries, clock)?;
    }
    Ok(entries)
}

fn drop_missing(
    conn: &Connection,
    entries: Vec<storage::DirEntry>,
    ranked: &mut Vec<Scored<'_>>,
    clock: &SystemClock,
) -> Result<(), Box<dyn Error>> {
    let matched: HashSet<&str> = ranked
        .iter()
        .map(|scored| scored.candidate.path.as_str())
        .collect();
    let to_check: Vec<storage::DirEntry> = entries
        .into_iter()
        .filter(|entry| matched.contains(entry.path.as_str()))
        .collect();
    let missing: HashSet<String> = reconcile_on_disk(conn, to_check, clock)?
        .into_iter()
        .filter(|entry| entry.missing)
        .map(|entry| entry.path)
        .collect();
    ranked.retain(|scored| !missing.contains(&scored.candidate.path));
    Ok(())
}

fn query_recall(
    conn: &Connection,
    query: &str,
    query_memory: bool,
    is_fallback: bool,
    explain: bool,
) -> Result<Recall, Box<dyn Error>> {
    if !query_memory {
        Ok(Recall::Disabled)
    } else if is_fallback && !explain {
        Ok(Recall::Nothing)
    } else {
        Ok(storage::recall(conn, &memory::key(query))?)
    }
}

fn origin(is_fallback: bool) -> Origin {
    if is_fallback {
        Origin::Fallback
    } else {
        Origin::Database
    }
}

fn query_stage(
    query: &str,
    is_fallback: bool,
    decision: &Decision,
    ranked: &[Scored<'_>],
) -> Option<String> {
    // WHY: SPEC §15 never logs the empty-query regression case; `stage` stays unevaluated rather than hit the unreachable `None` arm.
    if query.trim().is_empty() {
        return None;
    }
    if is_fallback {
        Some("fallback".to_owned())
    } else if matches!(decision, Decision::Menu(_)) {
        Some("menu".to_owned())
    } else {
        match ranked.first().map(|scored| scored.stage) {
            Some(Stage::One) => Some("1".to_owned()),
            Some(Stage::Two) => Some("2".to_owned()),
            None => unreachable!("a non-fallback non-empty query always reaches decide"),
        }
    }
}

struct Journal<'a> {
    conn: &'a Connection,
    clock: &'a SystemClock,
    current_path: &'a str,
    query: &'a str,
    stage: Option<&'a str>,
}

impl Journal<'_> {
    fn record(&self, result_dir_id: Option<i64>, outcome: &str) -> Result<(), Box<dyn Error>> {
        if let Some(stage) = self.stage {
            storage::insert_query(
                self.conn,
                self.clock.now(),
                self.current_path,
                self.query,
                result_dir_id,
                stage,
                outcome,
            )?;
        }
        Ok(())
    }
}

fn result_dir_id(
    conn: &Connection,
    path: &str,
    is_fallback: bool,
    clock: &SystemClock,
    exclude_dirs: &[remove::Target],
) -> Result<Option<i64>, Box<dyn Error>> {
    // WHY: a fallback pick books its visit first and reuses the id; excluded keeps NULL.
    if is_fallback {
        record_fallback_visit(conn, path, clock, exclude_dirs)
    } else {
        Ok(storage::dir_id_by_key(conn, &path.to_lowercase())?)
    }
}

fn conclude(
    journal: &Journal<'_>,
    decision: Decision<'_>,
    is_fallback: bool,
    exclude_dirs: &[remove::Target],
) -> Result<(), Box<dyn Error>> {
    match decision {
        Decision::None => {
            journal.record(None, "none")?;
            info!(
                stage = journal.stage.unwrap_or("-"),
                outcome = "none",
                "query outcome"
            );
            Err(format!("no directory matches '{}'", journal.query).into())
        }
        Decision::Jump(candidate) => {
            let result_dir_id = result_dir_id(
                journal.conn,
                &candidate.path,
                is_fallback,
                journal.clock,
                exclude_dirs,
            )?;
            journal.record(result_dir_id, "jump")?;
            info!(
                stage = journal.stage.unwrap_or("-"),
                outcome = "jump",
                target = %candidate.path,
                "query outcome"
            );
            print_result(&candidate.path);
            Ok(())
        }
        Decision::Menu(shown) => {
            eprint!("{}", decision::render_menu(&shown));
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            let chosen =
                decision::selection(&answer, shown.len()).and_then(|index| shown.get(index - 1));
            let Some(chosen) = chosen else {
                journal.record(None, "menu")?;
                return Err("no directory selected".into());
            };
            let result_dir_id = result_dir_id(
                journal.conn,
                &chosen.path,
                is_fallback,
                journal.clock,
                exclude_dirs,
            )?;
            journal.record(result_dir_id, "pick")?;
            info!(
                stage = journal.stage.unwrap_or("-"),
                outcome = "pick",
                target = %chosen.path,
                "query outcome"
            );
            print_result(&chosen.path);
            Ok(())
        }
    }
}

fn reconcile_on_disk(
    conn: &Connection,
    entries: Vec<storage::DirEntry>,
    clock: &SystemClock,
) -> Result<Vec<storage::DirEntry>, Box<dyn Error>> {
    let reconciled = soft_delete::reconcile(entries, &soft_delete::RealFilesystem, clock.now());
    for (dir_id, missing_since) in &reconciled.updates {
        storage::set_missing_since(conn, *dir_id, *missing_since)?;
    }
    Ok(reconciled.entries)
}

fn fallback_candidates(
    current_path: &str,
    no_ignore: bool,
    settings: &Settings,
    stop_at: Option<&str>,
) -> Vec<Candidate> {
    let options = fallback::Options {
        child_depth: settings.fallback.depth,
        ancestor_levels: settings.fallback.up,
        respect_gitignore: !(no_ignore || settings.fallback.no_ignore),
        exclude: settings.fallback.exclude.clone(),
        stop_at: stop_at.map(PathBuf::from),
    };
    fallback::discover(Path::new(current_path), options)
}

fn load_settings() -> Settings {
    let path = match storage::config_path() {
        Ok(path) => path,
        Err(error) => {
            warn!(%error, "cannot resolve the data directory; using default settings");
            return Settings::default();
        }
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let (settings, warnings) = config::parse(&text);
            for warning in &warnings {
                warn!("{warning}");
            }
            settings
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            debug!(path = %path.display(), "no config file; using default settings");
            Settings::default()
        }
        Err(error) => {
            warn!(%error, path = %path.display(), "cannot read config file; using default settings");
            Settings::default()
        }
    }
}

fn list_by_recency(candidates: &[Candidate], current_path: &str, color: bool) -> Vec<String> {
    let current_key = current_path.to_lowercase();
    let mut listed: Vec<&Candidate> = candidates
        .iter()
        .filter(|candidate| !candidate.missing && candidate.path.to_lowercase() != current_key)
        .collect();
    listed.sort_by(|left, right| {
        right
            .last_visit
            .cmp(&left.last_visit)
            .then_with(|| left.path.cmp(&right.path))
    });
    listed
        .iter()
        .map(|candidate| colorize(&candidate.path, color))
        .collect()
}

fn colorize(path: &str, color: bool) -> String {
    if !color {
        return path.to_owned();
    }
    match ls_colors_directory_code() {
        Some(code) => format!("\x1b[{code}m{path}\x1b[0m"),
        None => path.to_owned(),
    }
}

fn ls_colors_directory_code() -> Option<String> {
    let value = env::var("LS_COLORS").ok()?;
    value
        .split(':')
        .find_map(|entry| entry.strip_prefix("di=").map(str::to_owned))
}

fn is_excluded(exclude: &[remove::Target], path: &str) -> bool {
    exclude.iter().any(|target| remove::matches(target, path))
}

fn record_fallback_visit(
    conn: &Connection,
    path: &str,
    clock: &SystemClock,
    exclude: &[remove::Target],
) -> Result<Option<i64>, Box<dyn Error>> {
    if is_excluded(exclude, path) {
        return Ok(None);
    }
    let key = path.to_lowercase();
    let dir_id = storage::upsert_dir(conn, path, &key, clock.now())?;
    storage::insert_visit(
        conn,
        dir_id,
        clock.now(),
        "fallback",
        FALLBACK_SESSION,
        None,
    )?;
    Ok(Some(dir_id))
}

fn up(n: u32) -> Result<(), Box<dyn Error>> {
    debug!(n, "up");
    let cwd = env::current_dir()?;
    let target = up_from(&cwd, n)?;
    print_result(&target);
    Ok(())
}

fn up_from(start: &Path, n: u32) -> Result<String, Box<dyn Error>> {
    if n == 0 {
        return Err("furet up requires n >= 1".into());
    }
    let mut current = start;
    for _ in 0..n {
        current = current.parent().ok_or_else(|| {
            format!(
                "cannot go up {n} level(s) from '{}': not enough ancestors",
                start.display()
            )
        })?;
    }
    let canonical = paths::canonical(current)?;
    Ok(canonical.path)
}

fn back(session: &str, steps: u32) -> Result<(), Box<dyn Error>> {
    debug!(session, steps, "back");
    let conn = storage::open()?;
    let previous = storage::visited_dir_back(&conn, session, steps)?.ok_or_else(|| {
        if steps == 1 {
            "no previous directory for this session".to_owned()
        } else {
            format!("no directory {steps} steps back in this session")
        }
    })?;
    print_result(&previous);
    Ok(())
}

fn home_command() -> Result<(), Box<dyn Error>> {
    debug!("home");
    let settings = load_settings();
    if let Some(home) = configured_home(&settings) {
        print_result(&home);
    }
    Ok(())
}

fn configured_home(settings: &Settings) -> Option<String> {
    let configured = settings.home.as_ref()?;
    let unified = paths::unify_separators(configured);
    let candidate = Path::new(&unified);
    if !candidate.is_absolute() {
        warn!(home = %configured, "config home must be an absolute path; ignored");
        return None;
    }
    match paths::canonical(candidate) {
        Ok(canonical) => Some(canonical.path),
        Err(error) => {
            warn!(%error, home = %configured, "config home does not resolve to an existing directory; ignored");
            None
        }
    }
}

fn home_root(settings: &Settings) -> Result<String, Box<dyn Error>> {
    if let Some(home) = configured_home(settings) {
        return Ok(home);
    }
    let home = dirs::home_dir().ok_or("no home directory")?;
    Ok(paths::canonical(&home)?.path)
}

const IMPORT_SOURCE: &str = "import";

struct ImportCounts {
    imported: usize,
    known: usize,
    duplicate: usize,
}

fn record_import(
    candidates: Vec<(f64, paths::CanonicalDir)>,
    now: Timestamp,
) -> Result<ImportCounts, Box<dyn Error>> {
    let (deduped, duplicate) = import::dedupe_by_key(candidates);

    let mut conn = storage::open()?;
    let known_keys = storage::known_keys(&conn)?;
    let known = deduped
        .iter()
        .filter(|(_, dir)| known_keys.contains(&dir.key))
        .count();
    let planned = import::plan(deduped, &known_keys, now);

    let tx = conn.transaction()?;
    for entry in &planned {
        let dir_id = storage::upsert_dir(&tx, &entry.path, &entry.key, entry.ts)?;
        storage::insert_visit(&tx, dir_id, entry.ts, IMPORT_SOURCE, IMPORT_SOURCE, None)?;
    }
    tx.commit()?;

    Ok(ImportCounts {
        imported: planned.len(),
        known,
        duplicate,
    })
}

fn import_zoxide() -> Result<(), Box<dyn Error>> {
    debug!("import zoxide");
    let settings = load_settings();
    let mut raw = Vec::new();
    io::stdin().read_to_end(&mut raw)?;
    let clock = SystemClock::new();
    let now = clock.now();

    let mut malformed = 0usize;
    let mut not_a_directory = 0usize;
    let mut excluded = 0usize;
    let mut candidates = Vec::new();
    for line in stdin_lines(&raw) {
        let text = match std::str::from_utf8(strip_cr(line)) {
            Ok(text) => text,
            Err(_) => {
                malformed += 1;
                continue;
            }
        };
        let entry = match import::parse_line(text) {
            Some(entry) => entry,
            None => {
                malformed += 1;
                continue;
            }
        };
        match paths::canonical(Path::new(&entry.path)) {
            Ok(dir) => {
                if is_excluded(&settings.exclude_dirs, &dir.path) {
                    excluded += 1;
                } else {
                    candidates.push((entry.score, dir));
                }
            }
            Err(error) => {
                warn!(%error, path = %entry.path, "import zoxide: not a directory");
                not_a_directory += 1;
            }
        }
    }

    let ImportCounts {
        imported,
        known,
        duplicate,
    } = record_import(candidates, now)?;
    let skipped = malformed + not_a_directory + known + duplicate + excluded;
    info!(
        imported,
        skipped, known, not_a_directory, malformed, duplicate, excluded, "import zoxide"
    );
    eprintln!(
        "imported {imported}, skipped {skipped} (known {known}, not a directory {not_a_directory}, malformed {malformed}, duplicate {duplicate}, excluded {excluded})"
    );
    Ok(())
}

fn import_pwsh_history() -> Result<(), Box<dyn Error>> {
    debug!("import pwsh-history");
    let settings = load_settings();
    let mut raw = Vec::new();
    io::stdin().read_to_end(&mut raw)?;
    let clock = SystemClock::new();
    let now = clock.now();
    let home = dirs::home_dir().unwrap_or_default();

    let mut relative = 0usize;
    let mut not_a_directory = 0usize;
    let mut excluded = 0usize;
    let mut candidates = Vec::new();
    for (index, line) in stdin_lines(&raw).into_iter().enumerate() {
        let text = match std::str::from_utf8(strip_cr(line)) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let arg = match import::parse_history_line(text) {
            Some(arg) => arg,
            None => continue,
        };
        let path = import::expand_home(&arg, &home);
        if !path.is_absolute() {
            relative += 1;
            continue;
        }
        match paths::canonical(&path) {
            Ok(dir) => {
                if is_excluded(&settings.exclude_dirs, &dir.path) {
                    excluded += 1;
                } else {
                    candidates.push((index as f64, dir));
                }
            }
            Err(error) => {
                warn!(%error, path = %path.display(), "import pwsh-history: not a directory");
                not_a_directory += 1;
            }
        }
    }

    let ImportCounts {
        imported,
        known,
        duplicate,
    } = record_import(candidates, now)?;
    let skipped = known + not_a_directory + relative + duplicate + excluded;
    info!(
        imported,
        skipped, known, not_a_directory, relative, duplicate, excluded, "import pwsh-history"
    );
    eprintln!(
        "imported {imported}, skipped {skipped} (known {known}, not a directory {not_a_directory}, relative {relative}, duplicate {duplicate}, excluded {excluded})"
    );
    Ok(())
}

// WHY: fzf pipes this output into its preview pane, not into Set-Location.
fn preview_command(path: &str) -> Result<(), Box<dyn Error>> {
    debug!(path, "preview");
    let cleaned = preview::strip_sgr(path);
    let target = Path::new(&cleaned);
    if !target.is_dir() {
        print_lines(&["(not a directory)".to_owned()]);
        return Ok(());
    }
    let entries: Vec<(String, bool)> = match std::fs::read_dir(target) {
        Ok(iterator) => iterator
            .filter_map(|entry| entry.ok())
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                (name, entry.path().is_dir())
            })
            .collect(),
        Err(error) => {
            print_lines(&[format!("(unreadable: {error})")]);
            return Ok(());
        }
    };
    print_lines(&preview::render(entries));
    Ok(())
}

fn stdin_lines(raw: &[u8]) -> Vec<&[u8]> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&[u8]> = raw.split(|&byte| byte == b'\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn strip_cr(line: &[u8]) -> &[u8] {
    match line.split_last() {
        Some((b'\r', rest)) => rest,
        _ => line,
    }
}

fn init_pwsh(cmd: &str) -> Result<(), Box<dyn Error>> {
    debug!(cmd, "init pwsh");
    let mut completions = Vec::new();
    generate(PowerShell, &mut Cli::command(), "furet", &mut completions);
    // WHY: pwsh only accepts using statements before any other statement, so clap's block leads the script.
    print_result(&format!(
        "{}\n{}",
        String::from_utf8_lossy(&completions),
        pwsh::script(cmd)
    ));
    Ok(())
}

// WHY: stdout output goes through `stdout_line`, the binary's only stdout writer.
fn queries_command(failures: bool) -> Result<(), Box<dyn Error>> {
    debug!(failures, "queries");
    if !failures {
        return Err("furet queries requires --failures".into());
    }
    let conn = storage::open()?;
    let queries = storage::query_log(&conn)?;
    let visits = storage::visit_log(&conn)?;
    let mut lines = Vec::new();
    for failure in calibration::probable_failures(&queries, &visits) {
        let result_path = match failure.query.result_dir_id {
            Some(dir_id) => {
                storage::dir_path_by_id(&conn, dir_id)?.unwrap_or_else(|| "(unknown)".to_owned())
            }
            None => "(unknown)".to_owned(),
        };
        let reason = match failure.reason {
            FailureReason::Backtrack => "backtrack",
            FailureReason::MovedElsewhere => "moved",
        };
        lines.push(format!(
            "{}\t{}\t{}\t{}",
            failure.query.cwd, failure.query.query, result_path, reason
        ));
    }
    print_lines(&lines);
    Ok(())
}

// WHY: the only stdout write in the binary (CLAUDE.md stdout discipline); every command funnels here.
#[allow(clippy::print_stdout, clippy::disallowed_methods)]
fn stdout_line(line: &str) {
    println!("{line}");
}

fn print_result(path: &str) {
    stdout_line(path);
}

// WHY: stdout output goes through `stdout_line`, the binary's only stdout writer.
fn list_command(all: bool, paths: bool) -> Result<(), Box<dyn Error>> {
    debug!(all, paths, "list");
    let conn = storage::open()?;
    let lines: Vec<String> = storage::dir_listing(&conn, all)?
        .iter()
        .map(|row| {
            if paths {
                return row.path.clone();
            }
            let mut line = format!(
                "{}\t{}\t{}\t{}",
                row.path,
                row.visits,
                row.last_visit.as_deref().unwrap_or(""),
                row.first_seen
            );
            if all {
                line.push('\t');
                line.push_str(if row.missing { "missing" } else { "present" });
            }
            line
        })
        .collect();
    print_lines(&lines);
    Ok(())
}

// WHY: stdout output goes through `stdout_line`, the binary's only stdout writer.
fn stats_command(top: usize) -> Result<(), Box<dyn Error>> {
    debug!(top, "stats");
    let clock = SystemClock::new();
    let since = Timestamp::from_unix_seconds(
        clock
            .now()
            .unix_seconds()
            .saturating_sub(30 * SECONDS_PER_DAY),
    );
    let conn = storage::open()?;
    let counts = storage::stats_counts(&conn, since)?;
    let queries = storage::query_log(&conn)?;
    let visits = storage::visit_log(&conn)?;
    let probable = calibration::probable_failures(&queries, &visits).len();
    let top_rows = storage::top_dirs(&conn, top)?;
    print_lines(&stats::render(&counts, probable, &top_rows));
    Ok(())
}

fn remove_command(
    pattern: Option<&str>,
    missing: bool,
    confirm: bool,
    yes: bool,
    dry_run: bool,
) -> Result<(), Box<dyn Error>> {
    debug!(?pattern, missing, confirm, yes, dry_run, "remove");
    let pattern = match pattern {
        Some(pattern) if pattern.trim().is_empty() => {
            return Err("empty pattern".into());
        }
        Some(pattern) => pattern,
        None => return remove_missing(None, confirm, yes, dry_run),
    };
    if missing {
        return remove_missing(Some(pattern), confirm, yes, dry_run);
    }
    let base = env::current_dir()?;
    let target = remove_target(pattern, &base);
    let conn = storage::open()?;
    let mut entries = storage::dir_entries(&conn)?;
    entries.sort_by_key(|entry| entry.path.to_lowercase());
    let candidates: Vec<storage::DirEntry> = entries
        .into_iter()
        .filter(|entry| remove::matches(&target, &entry.path))
        .collect();
    if candidates.is_empty() {
        return Err(format!("no known directory matches '{pattern}'").into());
    }
    if dry_run {
        for candidate in &candidates {
            eprintln!("would remove {}", candidate.path);
        }
        return Ok(());
    }
    let questions: Vec<String> = candidates
        .iter()
        .map(|candidate| format!("Remove {}? [y/N/a/q] ", candidate.path))
        .collect();
    let decisions = if confirm {
        remove::confirm_each(&questions, ask)
    } else {
        vec![true; candidates.len()]
    };
    let selected: Vec<&storage::DirEntry> = candidates
        .iter()
        .zip(&decisions)
        .filter_map(|(candidate, &remove)| remove.then_some(candidate))
        .collect();
    if selected.is_empty() {
        return Err("nothing removed".into());
    }
    let ids: Vec<i64> = selected.iter().map(|entry| entry.id).collect();
    let count = storage::remove_dirs(&conn, &ids)?;
    info!(count, "removed directories");
    for candidate in &selected {
        eprintln!("removed {}", candidate.path);
    }
    Ok(())
}

fn remove_missing(
    pattern: Option<&str>,
    confirm: bool,
    yes: bool,
    dry_run: bool,
) -> Result<(), Box<dyn Error>> {
    debug!(?pattern, confirm, yes, dry_run, "remove --missing");
    let clock = SystemClock::new();
    let conn = storage::open()?;
    let mut absence = storage::missing_since_by_id(&conn)?;
    let reconciled = soft_delete::reconcile(
        storage::dir_entries(&conn)?,
        &soft_delete::RealFilesystem,
        clock.now(),
    );
    for (dir_id, update) in &reconciled.updates {
        match update {
            Some(ts) => {
                absence.insert(*dir_id, *ts);
            }
            None => {
                absence.remove(dir_id);
            }
        }
    }
    if !dry_run {
        for (dir_id, update) in &reconciled.updates {
            storage::set_missing_since(&conn, *dir_id, *update)?;
        }
    }
    let target = match pattern {
        Some(pattern) => Some(remove_target(pattern, &env::current_dir()?)),
        None => None,
    };
    let mut candidates: Vec<(storage::DirEntry, Timestamp)> = reconciled
        .entries
        .into_iter()
        .filter(|entry| entry.missing)
        .filter(|entry| match &target {
            Some(target) => remove::matches(target, &entry.path),
            None => true,
        })
        .map(|entry| {
            let since = absence.remove(&entry.id).unwrap_or_else(|| clock.now());
            (entry, since)
        })
        .collect();
    candidates.sort_by_key(|(entry, _)| entry.path.to_lowercase());
    if candidates.is_empty() {
        return Err(match pattern {
            Some(pattern) => format!("no missing known directory matches '{pattern}'").into(),
            None => "no missing known directory".into(),
        });
    }
    if dry_run {
        for (candidate, _) in &candidates {
            eprintln!("would remove {}", candidate.path);
        }
        return Ok(());
    }
    let questions: Vec<String> = candidates
        .iter()
        .map(|(candidate, since)| {
            Ok::<String, Box<dyn Error>>(format!(
                "Remove {} (missing since {})? [y/N/a/q] ",
                candidate.path,
                storage::format_local_time(&conn, *since)?
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let decisions = if yes {
        vec![true; candidates.len()]
    } else {
        remove::confirm_each(&questions, ask)
    };
    let selected: Vec<(i64, String)> = candidates
        .iter()
        .zip(&decisions)
        .filter_map(|((entry, _), &remove)| remove.then_some((entry.id, entry.path.clone())))
        .collect();
    if selected.is_empty() {
        return Err("nothing removed".into());
    }
    let ids: Vec<i64> = selected.iter().map(|(id, _)| *id).collect();
    let count = storage::remove_dirs(&conn, &ids)?;
    info!(count, "removed missing directories");
    for (_, path) in &selected {
        eprintln!("removed {path}");
    }
    Ok(())
}

fn remove_target(pattern: &str, base: &Path) -> remove::Target {
    if !remove::is_path_pattern(pattern) {
        return remove::Target::Name(pattern.to_owned());
    }
    if pattern.contains('*') || pattern.contains('?') {
        let unified = paths::unify_separators(pattern);
        if unified.starts_with('*') {
            return remove::Target::KeyPattern(unified.to_lowercase());
        }
        return remove::Target::KeyPattern(paths::absolute_key(pattern, base));
    }
    match paths::resolve(pattern, base) {
        Ok(dir) => remove::Target::Key(dir.key),
        Err(_) => remove::Target::Key(paths::absolute_key(pattern, base)),
    }
}

fn alias_command(action: AliasAction) -> Result<(), Box<dyn Error>> {
    match action {
        AliasAction::Add { name, path, force } => alias_add(&name, path.as_deref(), force),
        AliasAction::List => alias_list(),
        AliasAction::Remove { name } => alias_remove(&name),
        AliasAction::Complete { word } => alias_complete(&word),
    }
}

fn alias_add(name: &str, path: Option<&str>, force: bool) -> Result<(), Box<dyn Error>> {
    debug!(name, ?path, force, "alias add");
    let Some(key) = alias::key(name) else {
        return Err(
            format!("invalid alias name '{name}': use letters, digits, '_' and '-'").into(),
        );
    };
    let dir = match path {
        Some(input) => paths::canonical(Path::new(input))?,
        None => paths::canonical(&env::current_dir()?)?,
    };
    let conn = storage::open()?;
    if let Some(existing) = storage::alias_by_key(&conn, &key)?
        && !force
    {
        return Err(format!(
            "alias '{}' already exists ({}); use --force to replace it",
            existing.name, existing.path
        )
        .into());
    }
    storage::upsert_alias(&conn, name, &key, &dir.path, SystemClock::new().now())?;
    eprintln!("alias {name} -> {}", dir.path);
    Ok(())
}

// WHY: stdout output goes through `stdout_line`, the binary's only stdout writer.
fn alias_list() -> Result<(), Box<dyn Error>> {
    debug!("alias list");
    let conn = storage::open()?;
    let lines: Vec<String> = storage::alias_listing(&conn)?
        .iter()
        .map(|row| format!("{}\t{}\t{}", row.name, row.path, row.created))
        .collect();
    print_lines(&lines);
    Ok(())
}

fn alias_remove(name: &str) -> Result<(), Box<dyn Error>> {
    debug!(name, "alias remove");
    let conn = storage::open()?;
    if !storage::remove_alias(&conn, &name.to_ascii_lowercase())? {
        return Err(format!("unknown alias '{name}'").into());
    }
    eprintln!("removed alias {name}");
    Ok(())
}

// WHY: stdout output goes through `stdout_line`, the binary's only stdout writer.
fn alias_complete(word: &str) -> Result<(), Box<dyn Error>> {
    debug!(word, "alias complete");
    let prefix = load_settings().alias_prefix;
    let Some(stripped) = word.strip_prefix(prefix) else {
        return Ok(());
    };
    let wanted = stripped.to_ascii_lowercase();
    let conn = storage::open()?;
    let lines: Vec<String> = storage::alias_listing(&conn)?
        .iter()
        .filter(|row| row.name.to_ascii_lowercase().starts_with(&wanted))
        .map(|row| format!("{}{}\t{}", prefix, row.name, row.path))
        .collect();
    print_lines(&lines);
    Ok(())
}

fn mark_command(action: MarkAction) -> Result<(), Box<dyn Error>> {
    match action {
        MarkAction::Set { digit, path } => mark_set(&digit, path.as_deref()),
        MarkAction::List => mark_list(),
        MarkAction::Delete { spec, all } => mark_delete(spec.as_deref(), all),
        MarkAction::Next => mark_cycle(true),
        MarkAction::Prev => mark_cycle(false),
    }
}

fn mark_set(digit: &str, path: Option<&str>) -> Result<(), Box<dyn Error>> {
    debug!(digit, ?path, "mark set");
    if alias::mark_digit(digit).is_none() {
        return Err(format!("invalid mark '{digit}': use a digit from 1 to 9").into());
    }
    let dir = match path {
        Some(input) => paths::canonical(Path::new(input))?,
        None => paths::canonical(&env::current_dir()?)?,
    };
    let conn = storage::open()?;
    // WHY: no existence check, so overwriting a mark is silent (Vim's m1).
    storage::upsert_alias(&conn, digit, digit, &dir.path, SystemClock::new().now())?;
    eprintln!("mark {digit} -> {}", dir.path);
    Ok(())
}

// WHY: stdout output goes through `stdout_line`, the binary's only stdout writer.
fn mark_list() -> Result<(), Box<dyn Error>> {
    debug!("mark list");
    let conn = storage::open()?;
    let lines: Vec<String> = storage::alias_listing(&conn)?
        .iter()
        .filter(|row| alias::mark_digit(&row.name).is_some())
        .map(|row| format!("{}\t{}", row.name, row.path))
        .collect();
    print_lines(&lines);
    Ok(())
}

fn mark_delete(spec: Option<&str>, all: bool) -> Result<(), Box<dyn Error>> {
    debug!(?spec, all, "mark delete");
    let digits = if all {
        1..=9
    } else {
        let spec = spec.unwrap_or_default();
        let Some(range) = alias::mark_range(spec) else {
            return Err(
                format!("invalid mark range '{spec}': use a digit or a range like 2-4").into(),
            );
        };
        range
    };
    let conn = storage::open()?;
    for digit in digits {
        if storage::remove_alias(&conn, &digit.to_string())? {
            eprintln!("removed mark {digit}");
        }
    }
    Ok(())
}

fn mark_cycle(forward: bool) -> Result<(), Box<dyn Error>> {
    debug!(forward, "mark cycle");
    let current = paths::canonical(&env::current_dir()?)?;
    let conn = storage::open()?;
    let marks: Vec<(u8, String)> = storage::alias_listing(&conn)?
        .into_iter()
        .filter_map(|row| alias::mark_digit(&row.name).map(|digit| (digit, row.path)))
        .collect();
    if marks.is_empty() {
        return Err("no marks set".into());
    }
    let slots: Vec<alias::MarkSlot> = marks
        .iter()
        .map(|(digit, path)| alias::MarkSlot {
            digit: *digit,
            here: path.to_lowercase() == current.path.to_lowercase(),
            present: Path::new(path).is_dir(),
        })
        .collect();
    let (target, skipped) = alias::cycle(&slots, forward);
    for digit in &skipped {
        eprintln!("furet: skipped mark {digit}: missing directory");
    }
    match target {
        Some(digit) => {
            let path = marks
                .iter()
                .find(|(mark, _)| *mark == digit)
                .map(|(_, path)| path.clone())
                .ok_or("no other mark")?;
            print_result(&path);
            Ok(())
        }
        None => Err("no other mark".into()),
    }
}

fn ask(question: &str) -> Option<String> {
    eprint!("{question}");
    if io::stderr().flush().is_err() {
        return None;
    }
    let mut line = String::new();
    if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
        return None;
    }
    Some(line)
}

fn print_lines(lines: &[String]) {
    for line in lines {
        stdout_line(line);
    }
}

#[cfg(test)]
mod tests {
    // WHY: this is layer-3-ish test code exercising real directories.
    #![allow(clippy::expect_used)]

    use super::up_from;
    use assert_fs::TempDir;

    #[test]
    fn up_from_walks_n_parents_and_canonicalizes_the_result() {
        let root = TempDir::new().expect("a fresh scratch root");
        let deep = root.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).expect("the nested scratch tree exists");
        let expected = dunce::canonicalize(root.path().join("a"))
            .expect("the ancestor canonicalizes")
            .to_string_lossy()
            .into_owned();
        let target = up_from(&deep, 2).expect("two levels up resolves");
        assert_eq!(target, expected);
    }

    #[test]
    fn up_from_rejects_zero_levels() {
        let root = TempDir::new().expect("a fresh scratch root");
        assert!(up_from(root.path(), 0).is_err());
    }

    #[test]
    fn up_from_fails_when_there_are_fewer_parents_than_requested() {
        let root = TempDir::new().expect("a fresh scratch root");
        assert!(up_from(root.path(), 10_000).is_err());
    }
}
