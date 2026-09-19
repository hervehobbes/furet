use std::env;
use std::error::Error;
use std::io;
use std::path::Path;
use std::process;

use clap::{Parser, Subcommand, ValueEnum};
use furet::clock::{Clock, SystemClock};
use furet::decision::{self, Decision};
use furet::paths;
use furet::rank::{self, Candidate};
use furet::storage;

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
        Command::Up { n } => report(up(n)),
        Command::Back { session } => report(back(&session)),
        Command::Init { shell } => match shell {
            InitShell::Pwsh { cmd } => report(init_pwsh(&cmd)),
        },
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
    match decision::decide(&ranked) {
        Decision::None => Err(format!("no directory matches '{query}'").into()),
        Decision::Jump(candidate) => {
            print_result(&candidate.path);
            Ok(())
        }
        Decision::Menu(shown) => {
            eprint!("{}", decision::render_menu(&shown));
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            let index = decision::selection(&answer, shown.len()).ok_or("no directory selected")?;
            let chosen = shown.get(index - 1).ok_or("no directory selected")?;
            print_result(&chosen.path);
            Ok(())
        }
    }
}

fn up(n: u32) -> Result<(), Box<dyn Error>> {
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
    let conn = storage::open()?;
    let previous = storage::last_visited_dir(&conn, session)?
        .ok_or("no previous directory for this session")?;
    print_result(&previous);
    Ok(())
}

fn init_pwsh(cmd: &str) -> Result<(), Box<dyn Error>> {
    print_result(&pwsh::script(cmd));
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
