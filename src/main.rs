use std::env;
use std::error::Error;
use std::io::{self, Read};
use std::path::Path;
use std::process;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use furet::calibration::{self, FailureReason};
use furet::clock::{Clock, SystemClock};
use furet::config::{self, Settings};
use furet::decision::{self, Decision};
use furet::explain::{self, Origin};
use furet::fallback;
use furet::import;
use furet::paths;
use furet::rank::{self, Candidate, Stage};
use furet::soft_delete;
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
    },
    /// Rank recorded directories and print the best match.
    Query {
        /// Query text; quote it when it contains spaces.
        query: String,
        /// Print every ranked candidate, best first.
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
    },
    /// Print shell integration code for the requested shell.
    Init {
        #[command(subcommand)]
        shell: InitShell,
    },
    /// Inspect the query journal (SPEC section 15).
    Queries {
        /// List queries whose jump was probably a mistake.
        #[arg(long)]
        failures: bool,
    },
    /// Print the configured home directory, or nothing when unset or invalid.
    Home,
    /// Import directories from another tool's database.
    Import {
        /// Source database to import from.
        source: ImportSource,
    },
}

#[derive(Subcommand)]
enum InitShell {
    /// Print the PowerShell integration script.
    Pwsh {
        /// Name of the generated jump function.
        #[arg(long, default_value = "f")]
        cmd: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ImportSource {
    /// Import from `zoxide query -ls` on stdin.
    Zoxide,
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
        .after_help(database_help_line())
        .get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    let code = match cli.command {
        Command::Add {
            path,
            session,
            source,
            from,
        } => report(add(&path, &session, source, from.as_deref())),
        Command::Query {
            query,
            list,
            explain,
            color,
            no_ignore,
        } => report(query_directories(&query, list, explain, color, no_ignore)),
        Command::Up { n } => report(up(n)),
        Command::Back { session } => report(back(&session)),
        Command::Init { shell } => match shell {
            InitShell::Pwsh { cmd } => report(init_pwsh(&cmd)),
        },
        Command::Queries { failures } => report(queries_command(failures)),
        Command::Home => report(home_command()),
        Command::Import { source } => report(match source {
            ImportSource::Zoxide => import_zoxide(),
        }),
    };
    // WHY: process::exit skips destructors, so the guard is dropped explicitly to flush buffered log lines.
    drop(guard);
    process::exit(code);
}

// WHY: the database path depends on the runtime environment, so a static clap attribute cannot hold it.
fn database_help_line() -> String {
    match storage::db_path() {
        Ok(path) => format!("Database file: {}", path.display()),
        Err(error) => format!("Database file: unavailable: {error}"),
    }
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
) -> Result<(), Box<dyn Error>> {
    debug!(path, session, source = source.as_str(), from = ?from, "add");
    let clock = SystemClock::new();
    let base = env::current_dir()?;
    let dir = paths::resolve(path, &base)?;
    let conn = storage::open()?;
    let dir_id = storage::upsert_dir(&conn, &dir.path, &dir.key, clock.now())?;
    let from_dir_id = match from {
        Some(input) => {
            let origin = paths::resolve(input, &base)?;
            storage::dir_id_by_key(&conn, &origin.key)?
        }
        None => None,
    };
    storage::insert_visit(
        &conn,
        dir_id,
        clock.now(),
        source.as_str(),
        session,
        from_dir_id,
    )?;
    info!(path = %dir.path, source = source.as_str(), "visit recorded");
    Ok(())
}

// WHY: distinct from any real shell GUID, so `back` never sees this visit.
const FALLBACK_SESSION: &str = "fallback";

fn query_directories(
    query: &str,
    list: bool,
    explain: bool,
    color: bool,
    no_ignore: bool,
) -> Result<(), Box<dyn Error>> {
    debug!(query, list, explain, color, no_ignore, "query");
    let settings = load_settings();
    debug!(?settings, "effective settings");
    let cwd = env::current_dir()?;
    let current = paths::canonical(&cwd)?;
    let clock = SystemClock::new();
    let conn = storage::open()?;
    let reconciled = soft_delete::reconcile(
        storage::dir_entries(&conn)?,
        &soft_delete::RealFilesystem,
        clock.now(),
    );
    for (dir_id, missing_since) in &reconciled.updates {
        storage::set_missing_since(&conn, *dir_id, *missing_since)?;
    }
    let candidates: Vec<Candidate> = reconciled
        .entries
        .into_iter()
        .map(|entry| {
            let split = paths::split(&entry.path);
            Candidate {
                name: split.name,
                folder: split.folder,
                last_visit: entry.last_visit,
                missing: entry.missing,
                path: entry.path,
            }
        })
        .collect();
    if list && query.trim().is_empty() {
        print_lines(&list_by_recency(&candidates, &current.path, color));
        return Ok(());
    }
    let (pool, is_fallback) = resolve_pool(query, &current.path, candidates, no_ignore, &settings);
    debug!(candidates = pool.len(), fallback = is_fallback, "ranking");
    if explain {
        let origin = if is_fallback {
            Origin::Fallback
        } else {
            Origin::Database
        };
        let report = explain::explain(
            query,
            &current.path,
            &pool,
            origin,
            settings.typo_min_length,
        );
        eprint!("{}", explain::render(&report));
        return Ok(());
    }
    let ranked = rank::rank(query, &current.path, &pool, settings.typo_min_length);
    if list {
        let lines: Vec<String> = ranked
            .iter()
            .map(|scored| colorize(&scored.candidate.path, color))
            .collect();
        print_lines(&lines);
        return Ok(());
    }
    debug!(ranked = ranked.len(), "decision");
    let decision = decision::decide(&ranked);
    // WHY: SPEC section 15 never logs the empty-query-without-list regression case;
    // its `ranked` is always empty on the non-fallback path, so `stage` must stay
    // unevaluated there rather than hit the otherwise-unreachable `None` arm below.
    let logged_query = !query.trim().is_empty();
    let stage = logged_query.then(|| {
        if is_fallback {
            "fallback".to_owned()
        } else if matches!(decision, Decision::Menu(_)) {
            "menu".to_owned()
        } else {
            match ranked.first() {
                Some(scored) => match scored.stage {
                    Stage::One => "1".to_owned(),
                    Stage::Two => "2".to_owned(),
                },
                None => unreachable!("a non-fallback non-empty query always reaches decide"),
            }
        }
    });
    match decision {
        Decision::None => {
            if let Some(stage) = &stage {
                storage::insert_query(
                    &conn,
                    clock.now(),
                    &current.path,
                    query,
                    None,
                    stage,
                    "none",
                )?;
            }
            info!(
                stage = stage.as_deref().unwrap_or("-"),
                outcome = "none",
                "query outcome"
            );
            Err(format!("no directory matches '{query}'").into())
        }
        Decision::Jump(candidate) => {
            let result_dir_id = if is_fallback {
                Some(record_fallback_visit(&conn, &candidate.path, &clock)?)
            } else {
                storage::dir_id_by_key(&conn, &candidate.path.to_lowercase())?
            };
            if let Some(stage) = &stage {
                storage::insert_query(
                    &conn,
                    clock.now(),
                    &current.path,
                    query,
                    result_dir_id,
                    stage,
                    "jump",
                )?;
            }
            info!(
                stage = stage.as_deref().unwrap_or("-"),
                outcome = "jump",
                target = %candidate.path,
                "query outcome"
            );
            print_result(&candidate.path);
            Ok(())
        }
        Decision::Menu(shown) => {
            if let Some(stage) = &stage {
                storage::insert_query(
                    &conn,
                    clock.now(),
                    &current.path,
                    query,
                    None,
                    stage,
                    "menu",
                )?;
            }
            eprint!("{}", decision::render_menu(&shown));
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            let index = decision::selection(&answer, shown.len()).ok_or("no directory selected")?;
            let chosen = shown.get(index - 1).ok_or("no directory selected")?;
            if is_fallback {
                record_fallback_visit(&conn, &chosen.path, &clock)?;
            }
            info!(
                stage = stage.as_deref().unwrap_or("-"),
                outcome = "menu",
                target = %chosen.path,
                "query outcome"
            );
            print_result(&chosen.path);
            Ok(())
        }
    }
}

fn resolve_pool(
    query: &str,
    current_path: &str,
    db_candidates: Vec<Candidate>,
    no_ignore: bool,
    settings: &Settings,
) -> (Vec<Candidate>, bool) {
    if query.trim().is_empty()
        || !rank::rank(
            query,
            current_path,
            &db_candidates,
            settings.typo_min_length,
        )
        .is_empty()
    {
        return (db_candidates, false);
    }
    let options = fallback::Options {
        child_depth: settings.fallback.depth,
        ancestor_levels: settings.fallback.up,
        respect_gitignore: !(no_ignore || settings.fallback.no_ignore),
        exclude: settings.fallback.exclude.clone(),
    };
    (fallback::discover(Path::new(current_path), options), true)
}

fn load_settings() -> Settings {
    let path = match storage::data_dir() {
        Ok(dir) => dir.join("config.toml"),
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

fn record_fallback_visit(
    conn: &Connection,
    path: &str,
    clock: &SystemClock,
) -> Result<i64, Box<dyn Error>> {
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
    Ok(dir_id)
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

fn back(session: &str) -> Result<(), Box<dyn Error>> {
    debug!(session, "back");
    let conn = storage::open()?;
    let previous = storage::last_visited_dir(&conn, session)?
        .ok_or("no previous directory for this session")?;
    print_result(&previous);
    Ok(())
}

fn home_command() -> Result<(), Box<dyn Error>> {
    debug!("home");
    let settings = load_settings();
    let Some(configured) = settings.home else {
        return Ok(());
    };
    let unified = paths::unify_separators(&configured);
    let candidate = Path::new(&unified);
    if !candidate.is_absolute() {
        warn!(home = %configured, "config home must be an absolute path; ignored");
        return Ok(());
    }
    match paths::canonical(candidate) {
        Ok(canonical) => print_result(&canonical.path),
        Err(error) => {
            warn!(%error, home = %configured, "config home does not resolve to an existing directory; ignored");
        }
    }
    Ok(())
}

const IMPORT_SOURCE: &str = "import";

fn import_zoxide() -> Result<(), Box<dyn Error>> {
    debug!("import zoxide");
    let mut raw = Vec::new();
    io::stdin().read_to_end(&mut raw)?;
    let clock = SystemClock::new();
    let now = clock.now();

    let mut malformed = 0usize;
    let mut not_a_directory = 0usize;
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
            Ok(dir) => candidates.push((entry.score, dir)),
            Err(error) => {
                warn!(%error, path = %entry.path, "import zoxide: not a directory");
                not_a_directory += 1;
            }
        }
    }

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

    let imported = planned.len();
    let skipped = malformed + not_a_directory + known + duplicate;
    info!(
        imported,
        skipped, known, not_a_directory, malformed, duplicate, "import zoxide"
    );
    eprintln!(
        "imported {imported}, skipped {skipped} (known {known}, not a directory {not_a_directory}, malformed {malformed}, duplicate {duplicate})"
    );
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
    print_result(&pwsh::script(cmd));
    Ok(())
}

// WHY: a standalone reporting tool, not the f/fi jump path, so it may use stdout freely.
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

#[allow(clippy::print_stdout)]
fn print_result(path: &str) {
    println!("{path}");
}

#[allow(clippy::print_stdout)]
fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
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
