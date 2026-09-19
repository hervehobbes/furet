use std::env;
use std::error::Error;
use std::process;

use clap::{Parser, Subcommand, ValueEnum};
use furet::clock::{Clock, SystemClock};
use furet::paths;
use furet::rank::{self, Candidate};
use furet::storage;

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
    },
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
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Add {
            path,
            session,
            source,
            from,
        } => report(add(&path, &session, source, from.as_deref())),
        Command::Query { query, list } => report(query_directories(&query, list)),
    };
    process::exit(code);
}

fn report(outcome: Result<(), Box<dyn Error>>) -> i32 {
    match outcome {
        Ok(()) => 0,
        Err(error) => {
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
    Ok(())
}

fn query_directories(query: &str, list: bool) -> Result<(), Box<dyn Error>> {
    let cwd = env::current_dir()?;
    let current = paths::canonical(&cwd)?;
    let conn = storage::open()?;
    let candidates: Vec<Candidate> = storage::dir_entries(&conn)?
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
    let ranked = rank::rank(query, &current.path, &candidates);
    if list {
        let lines: Vec<&str> = ranked
            .iter()
            .map(|scored| scored.candidate.path.as_str())
            .collect();
        print_lines(&lines);
        return Ok(());
    }
    let best = ranked
        .first()
        .ok_or_else(|| format!("no directory matches '{query}'"))?;
    print_result(&best.candidate.path);
    Ok(())
}

#[allow(clippy::print_stdout)]
fn print_result(path: &str) {
    println!("{path}");
}

#[allow(clippy::print_stdout)]
fn print_lines(lines: &[&str]) {
    for line in lines {
        println!("{line}");
    }
}
