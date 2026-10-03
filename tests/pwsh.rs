// WHY: this whole file is layer-3 test code, where expect() is the norm.
#![allow(clippy::expect_used)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use assert_cmd::cargo::cargo_bin;
use furet::paths;
use furet::project;
use rusqlite::Connection;
use tempfile::TempDir;

struct Sandbox {
    tree: TempDir,
    data: TempDir,
}

fn sandbox(children: &[&str]) -> Sandbox {
    let tree = letter_free_tempdir();
    for child in children {
        std::fs::create_dir_all(tree.path().join(child)).expect("a child directory exists");
    }
    let data = TempDir::new().expect("a fresh data directory");
    Sandbox { tree, data }
}

// WHY: tempfile's random suffix has letters that can complete stage 1's folder bonus on a full parent path, which made same-name fixtures flaky (lot 61).
fn letter_free_tempdir() -> TempDir {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let name = format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        match tempfile::Builder::new()
            .prefix(&name)
            .rand_bytes(0)
            .tempdir()
        {
            Ok(dir) => return dir,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("a fresh tree directory: {error}"),
        }
    }
}

impl Sandbox {
    fn child(&self, name: &str) -> PathBuf {
        self.tree.path().join(name)
    }

    fn furet(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("furet").expect("the furet binary is built");
        cmd.env("FURET_DATA_DIR", self.data.path());
        cmd.env_remove("FURET_LOG");
        cmd
    }
}

// WHY: seeds candidates through the real binary, bypassing pwsh entirely.
fn seed(sandbox: &Sandbox, path: &Path, session: &str) {
    let out = sandbox
        .furet()
        .arg("add")
        .arg(path)
        .arg("--session")
        .arg(session)
        .output()
        .expect("furet add runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn canonical(path: &Path) -> String {
    paths::canonical(path)
        .expect("the fixture directory canonicalizes")
        .path
}

fn seed_alias(sandbox: &Sandbox, name: &str, path: &Path) {
    let out = sandbox
        .furet()
        .arg("alias")
        .arg("add")
        .arg(name)
        .arg(path)
        .output()
        .expect("furet alias add runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn seed_mark(sandbox: &Sandbox, digit: &str, path: &Path) {
    let out = sandbox
        .furet()
        .arg("mark")
        .arg("set")
        .arg(digit)
        .arg(path)
        .output()
        .expect("furet mark set runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn mark_path(sandbox: &Sandbox, digit: &str) -> Option<String> {
    db(sandbox)
        .query_row("SELECT path FROM aliases WHERE name = ?1", [digit], |row| {
            row.get(0)
        })
        .ok()
}

fn db(sandbox: &Sandbox) -> Connection {
    Connection::open(sandbox.data.path().join("furet.db")).expect("the recorded database opens")
}

fn scalar(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0))
        .expect("the scalar query reads")
}

fn last_visit_source(conn: &Connection) -> String {
    conn.query_row(
        "SELECT source FROM visits ORDER BY id DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .expect("the last visit reads back")
}

fn last_visit_session(conn: &Connection) -> String {
    conn.query_row(
        "SELECT session FROM visits ORDER BY id DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .expect("the last visit's session reads back")
}

fn pwsh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn quote(path: &Path) -> String {
    pwsh_quote(&path.to_string_lossy())
}

const CWD_MARKER: &str = "FURET_TEST_CWD=";

struct PwshRun {
    stdout: String,
    stderr: String,
    cwd: String,
}

fn extract(stdout: &str, marker: &str) -> String {
    stdout
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(marker))
        .unwrap_or_else(|| panic!("no line starting with {marker} in stdout: {stdout:?}"))
        .trim()
        .to_owned()
}

// WHY: runs the real furet init pwsh output in a real pwsh, not on script text.
fn run_pwsh(
    sandbox: &Sandbox,
    preamble: &str,
    init_args: &str,
    start_dir: &Path,
    body: &str,
) -> PwshRun {
    let furet_bin = cargo_bin("furet");
    let bin_dir = furet_bin
        .parent()
        .expect("the furet binary has a parent directory");
    let existing_path = env::var_os("PATH").unwrap_or_default();
    let new_path = env::join_paths(
        std::iter::once(bin_dir.to_path_buf()).chain(env::split_paths(&existing_path)),
    )
    .expect("PATH entries join without embedded path separators");

    let script_dir = TempDir::new().expect("a fresh script directory");
    let script_path = script_dir.path().join("run.ps1");
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n\
         {preamble}\n\
         Invoke-Expression (& furet init pwsh {init_args} | Out-String)\n\
         Set-Location -LiteralPath {start}\n\
         {body}\n\
         Write-Output ('{marker}' + (Get-Location).Path)\n",
        start = quote(start_dir),
        marker = CWD_MARKER,
    );
    std::fs::write(&script_path, script).expect("the generated pwsh script is written");

    let output = Command::new("pwsh")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-File")
        .arg(&script_path)
        .env("PATH", &new_path)
        .env("FURET_DATA_DIR", sandbox.data.path())
        .env_remove("FURET_LOG")
        .output()
        .expect("pwsh 7 is required to run tests/pwsh.rs; install PowerShell 7 and put it on PATH");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let cwd = extract(&stdout, CWD_MARKER);
    PwshRun {
        stdout,
        stderr,
        cwd,
    }
}

// WHY: the fzf stub bypasses the real fzf entirely, returning a fixed line.
fn fzf_stub(target: &str) -> String {
    format!("function global:fzf {{ \"`e[01;34m{target}`e[0m\" }}\n")
}

// WHY: fzf prints the final query line before the selection, so the stub emits two objects.
fn fzf_stub_two_lines(query: &str, target: &str) -> String {
    format!(
        "function global:fzf {{ {query}, \"`e[01;34m{target}`e[0m\" }}\n",
        query = pwsh_quote(query),
    )
}

// WHY: the stub echoes the lines fzf receives, then emits the first one.
fn fzf_stub_echoing_input_and_first_line() -> String {
    "function global:fzf {\n\
     $lines = @($input)\n\
     foreach ($line in $lines) {\n\
     Write-Host ('FZF_IN=' + $line)\n\
     }\n\
     $lines[0]\n\
     }\n"
    .to_owned()
}

// WHY: the stub prints fzf's arguments and emits nothing, so no line is ever selected.
fn fzf_stub_echoing_args_silently() -> String {
    "function global:fzf {\n\
     Write-Host ('FZF_ARGS=' + ($args -join ' '))\n\
     }\n"
    .to_owned()
}

// WHY: fi's no-fzf branch only triggers once every fzf/fzf.exe on PATH is hidden.
const HIDE_FZF: &str = "$env:PATH = ((($env:PATH -split ';') | Where-Object { \
    -not (Test-Path (Join-Path $_ 'fzf.exe')) -and -not (Test-Path (Join-Path $_ 'fzf')) \
}) -join ';')\n";

const COMPLETION_MARKER: &str = "FURET_TEST_COMPLETION=";

// WHY: TabExpansion2 is the function a real Tab press runs, so this exercises the completer interactively.
fn completions_in(sandbox: &Sandbox, init_args: &str, start: &Path, line: &str) -> Vec<String> {
    let body = format!(
        "$completed = TabExpansion2 -inputScript {line} -cursorColumn {column}\n\
         foreach ($match in $completed.CompletionMatches) {{\n\
         Write-Output ('{marker}' + $match.CompletionText)\n\
         }}",
        line = pwsh_quote(line),
        column = line.chars().count(),
        marker = COMPLETION_MARKER,
    );
    let run = run_pwsh(sandbox, "", init_args, start, &body);
    run.stdout
        .lines()
        .filter_map(|l| l.strip_prefix(COMPLETION_MARKER))
        .map(str::to_owned)
        .collect()
}

fn completions(sandbox: &Sandbox, init_args: &str, line: &str) -> Vec<String> {
    completions_in(sandbox, init_args, sandbox.tree.path(), line)
}

// WHY: mirrors completions_in but returns all three completion fields per match.
fn completion_items_in(
    sandbox: &Sandbox,
    init_args: &str,
    start: &Path,
    line: &str,
) -> Vec<(String, String, String)> {
    let body = format!(
        "$completed = TabExpansion2 -inputScript {line} -cursorColumn {column}\n\
         foreach ($match in $completed.CompletionMatches) {{\n\
         Write-Output ('{marker}' + $match.CompletionText + \"`t\" + $match.ListItemText + \"`t\" + $match.ToolTip)\n\
         }}",
        line = pwsh_quote(line),
        column = line.chars().count(),
        marker = COMPLETION_MARKER,
    );
    let run = run_pwsh(sandbox, "", init_args, start, &body);
    run.stdout
        .lines()
        .filter_map(|l| l.strip_prefix(COMPLETION_MARKER))
        .map(|l| {
            let mut fields = l.split('\t');
            (
                fields.next().unwrap_or_default().to_owned(),
                fields.next().unwrap_or_default().to_owned(),
                fields.next().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

// WHY: the expectation calls furet itself, so the ordering claim rests on the real ranking.
fn query_list_in(sandbox: &Sandbox, cwd: &Path, args: &[&str]) -> Vec<String> {
    let out = sandbox
        .furet()
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("furet query --list runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn query_list(sandbox: &Sandbox, args: &[&str]) -> Vec<String> {
    query_list_in(sandbox, sandbox.tree.path(), args)
}

#[test]
fn f_query_jumps_to_the_ranked_target_and_records_one_jump_visit() {
    let world = sandbox(&["src/tokio"]);
    let tokio = world.child("src/tokio");
    seed(&world, &tokio, "seed");
    let run = run_pwsh(&world, "", "", world.tree.path(), "f tokio");
    assert_eq!(run.cwd, canonical(&tokio), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 2);
    assert_eq!(last_visit_source(&conn), "jump");
    let session = last_visit_session(&conn);
    assert_ne!(session, "seed");
    assert_ne!(session, "fallback");
    assert!(!session.is_empty());
}

#[test]
fn f_query_with_no_match_stays_put_records_nothing_and_reports_on_stderr() {
    let world = sandbox(&[]);
    let start = world.child("start");
    std::fs::create_dir_all(&start).expect("the isolated start directory exists");
    let run = run_pwsh(&world, "", "", &start, "f zigzagnonexistent");
    assert_eq!(run.cwd, canonical(&start));
    assert!(
        !run.stderr.is_empty(),
        "a failed query must report on stderr"
    );
    assert!(
        !world.data.path().join("furet.db").exists()
            || scalar(&db(&world), "SELECT COUNT(*) FROM visits") == 0
    );
}

#[test]
fn f_existing_path_jumps_directly_with_a_forward_slash_separator() {
    let world = sandbox(&["sub/dir"]);
    let target = world.child("sub/dir");
    let run = run_pwsh(&world, "", "", world.tree.path(), "f sub/dir");
    assert_eq!(run.cwd, canonical(&target), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn f_dotdot_climbs_one_level_and_records_up() {
    let world = sandbox(&["a/b/c"]);
    let start = world.child("a/b/c");
    let expected = world.child("a/b");
    let run = run_pwsh(&world, "", "", &start, "f ..");
    assert_eq!(run.cwd, canonical(&expected), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "up");
}

#[test]
fn f_dotdotdot_climbs_two_levels_and_records_up() {
    let world = sandbox(&["a/b/c"]);
    let start = world.child("a/b/c");
    let expected = world.child("a");
    let run = run_pwsh(&world, "", "", &start, "f ...");
    assert_eq!(run.cwd, canonical(&expected), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "up");
}

#[test]
fn f_dash_returns_to_the_previous_directory_of_the_session_recorded_as_back() {
    let world = sandbox(&["a", "b"]);
    let a = world.child("a");
    let b = world.child("b");
    let body = format!("f {}\nf {}\nf -", quote(&a), quote(&b));
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&a), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 3);
    assert_eq!(last_visit_source(&conn), "back");
}

#[test]
fn f_dash_three_goes_three_directories_back_recorded_as_back() {
    let world = sandbox(&["a", "b", "c", "d"]);
    let a = world.child("a");
    let b = world.child("b");
    let c = world.child("c");
    let d = world.child("d");
    let body = format!(
        "f {}\nf {}\nf {}\nf {}\nf -3",
        quote(&a),
        quote(&b),
        quote(&c),
        quote(&d)
    );
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&a), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 5);
    assert_eq!(last_visit_source(&conn), "back");
}

#[test]
fn f_dash_one_is_f_dash() {
    let world = sandbox(&["a", "b", "c", "d"]);
    let a = world.child("a");
    let b = world.child("b");
    let c = world.child("c");
    let d = world.child("d");
    let body = format!(
        "f {}\nf {}\nf {}\nf {}\nf -1",
        quote(&a),
        quote(&b),
        quote(&c),
        quote(&d)
    );
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&c), "stderr: {}", run.stderr);
}

#[test]
fn f_dash_n_beyond_the_history_stays_put() {
    let world = sandbox(&["a", "b", "c", "d"]);
    let a = world.child("a");
    let b = world.child("b");
    let c = world.child("c");
    let d = world.child("d");
    let body = format!(
        "f {}\nf {}\nf {}\nf {}\nf -9",
        quote(&a),
        quote(&b),
        quote(&c),
        quote(&d)
    );
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&d), "stderr: {}", run.stderr);
    assert!(
        run.stderr
            .contains("no directory 9 steps back in this session"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 4);
}

#[test]
fn f_dash_zero_stays_put() {
    let world = sandbox(&["a", "b", "c", "d"]);
    let a = world.child("a");
    let b = world.child("b");
    let c = world.child("c");
    let d = world.child("d");
    let body = format!(
        "f {}\nf {}\nf {}\nf {}\nf -0",
        quote(&a),
        quote(&b),
        quote(&c),
        quote(&d)
    );
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&d), "stderr: {}", run.stderr);
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 4);
}

#[test]
fn f_dot_does_nothing_and_records_nothing() {
    let world = sandbox(&["x"]);
    let x = world.child("x");
    let run = run_pwsh(&world, "", "", &x, "f .");
    assert_eq!(run.cwd, canonical(&x), "stderr: {}", run.stderr);
    assert!(!world.data.path().join("furet.db").exists());
}

#[test]
fn f_with_no_argument_goes_to_the_process_home_and_records_a_jump() {
    let world = sandbox(&[]);
    let run = run_pwsh(
        &world,
        "",
        "",
        world.tree.path(),
        "Write-Output ('FURET_TEST_HOME=' + $HOME)\nf",
    );
    let home = extract(&run.stdout, "FURET_TEST_HOME=");
    assert_eq!(run.cwd, home, "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "jump");
}

fn write_home_config(sandbox: &Sandbox, home: &Path) {
    let forward = home.to_string_lossy().replace('\\', "/");
    std::fs::write(
        sandbox.data.path().join("config.toml"),
        format!("home = \"{forward}\""),
    )
    .expect("the config file is written");
}

fn write_alias_prefix_config(sandbox: &Sandbox, prefix: &str) {
    std::fs::write(
        sandbox.data.path().join("config.toml"),
        format!("alias_prefix = \"{prefix}\""),
    )
    .expect("the config file is written");
}

#[test]
fn f_with_no_argument_goes_to_the_configured_home_and_records_a_jump() {
    let world = sandbox(&["myhome"]);
    let home = world.child("myhome");
    write_home_config(&world, &home);
    let run = run_pwsh(&world, "", "", world.tree.path(), "f");
    assert_eq!(run.cwd, canonical(&home), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn f_with_a_configured_but_missing_home_falls_back_to_home() {
    let world = sandbox(&[]);
    let missing = world.child("nope");
    write_home_config(&world, &missing);
    let run = run_pwsh(
        &world,
        "",
        "",
        world.tree.path(),
        "Write-Output ('FURET_TEST_HOME=' + $HOME)\nf",
    );
    let home = extract(&run.stdout, "FURET_TEST_HOME=");
    assert_eq!(run.cwd, home, "stderr: {}", run.stderr);
}

#[test]
fn f_explain_reports_on_stderr_without_moving_or_recording_flag_last_and_flag_first() {
    let world = sandbox(&["proj/tokio"]);
    let tokio = world.child("proj/tokio");
    seed(&world, &tokio, "seed");
    let elsewhere = world.child("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("the isolated elsewhere directory exists");

    let flag_last = run_pwsh(&world, "", "", &elsewhere, "f tokio --explain");
    assert_eq!(flag_last.cwd, canonical(&elsewhere));
    assert!(flag_last.stderr.contains("normalized query:"));

    let flag_first = run_pwsh(&world, "", "", &elsewhere, "f --explain tokio");
    assert_eq!(flag_first.cwd, canonical(&elsewhere));
    assert!(flag_first.stderr.contains("normalized query:"));

    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 1);
}

#[test]
fn a_disk_fallback_jump_after_a_real_prompt_then_f_dash_returns_to_projects() {
    let world = sandbox(&["origin", "projects/tokio-rs"]);
    let projects = world.child("projects");
    let target = world.child("projects/tokio-rs");
    let body = format!(
        "f origin\nSet-Location -LiteralPath {projects}\n[void] (prompt)\nf tokio\nf -",
        projects = quote(&projects),
    );
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&projects), "stderr: {}", run.stderr);
    let conn = db(&world);
    let target_key = paths::canonical(&target)
        .expect("the fallback target canonicalizes")
        .key;
    let visited_target: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM visits WHERE dir_id = (SELECT id FROM dirs WHERE key = ?1)",
            [target_key],
            |row| row.get(0),
        )
        .expect("the fallback target's visits count reads back");
    assert!(
        visited_target >= 1,
        "the disk-fallback target must have been visited in the session"
    );
    assert_eq!(last_visit_source(&conn), "back");
    let session = last_visit_session(&conn);
    assert_ne!(session, "seed");
    assert_ne!(session, "fallback");
    assert!(!session.is_empty());
}

#[test]
fn hook_records_one_visit_on_move_and_nothing_on_a_repeated_prompt() {
    let world = sandbox(&["a"]);
    let run = run_pwsh(
        &world,
        "",
        "",
        world.tree.path(),
        "Set-Location a\n[void] (prompt)\n[void] (prompt)",
    );
    assert!(run.stderr.is_empty(), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "hook");
}

#[test]
fn a_prompt_defined_before_loading_the_script_still_produces_its_own_output() {
    let world = sandbox(&[]);
    let preamble = "function global:prompt { 'CUSTOM_PROMPT_OUTPUT' }";
    let run = run_pwsh(
        &world,
        preamble,
        "",
        world.tree.path(),
        "Write-Output ('PROMPT_RESULT=' + (prompt))",
    );
    assert!(run.stderr.is_empty(), "stderr: {}", run.stderr);
    assert_eq!(
        extract(&run.stdout, "PROMPT_RESULT="),
        "CUSTOM_PROMPT_OUTPUT"
    );
}

fn ambiguous_world() -> (Sandbox, String, String) {
    let world = sandbox(&["aaa/tokio", "zzz/tokio"]);
    let far = world.child("zzz/tokio");
    let near = world.child("aaa/tokio");
    seed(&world, &far, "seed");
    seed(&world, &near, "seed");
    (world, canonical(&near), canonical(&far))
}

#[test]
fn fi_no_fzf_answering_2_jumps_to_the_second_candidate_and_records_a_jump() {
    let (world, _first, second) = ambiguous_world();
    let body = format!("{HIDE_FZF}function global:Read-Host {{ '2' }}\nfi tokoi");
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, second, "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 3);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn fi_no_fzf_an_out_of_range_or_empty_answer_cancels_without_moving() {
    let (world, _first, _second) = ambiguous_world();
    let start = world.tree.path();

    let out_of_range = format!("{HIDE_FZF}function global:Read-Host {{ '9' }}\nfi tokoi");
    let run = run_pwsh(&world, "", "", start, &out_of_range);
    assert_eq!(run.cwd, canonical(start));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 2);

    let empty = format!("{HIDE_FZF}function global:Read-Host {{ '' }}\nfi tokoi");
    let run = run_pwsh(&world, "", "", start, &empty);
    assert_eq!(run.cwd, canonical(start));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 2);
}

#[test]
fn fi_fzf_branch_uses_the_stub_line_strips_ansi_and_jumps() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    seed(&world, &tokio, "seed");
    let body = format!("{}fi tok", fzf_stub_two_lines("tok", &canonical(&tokio)));
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&tokio), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 2);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn fi_fzf_branch_journals_exactly_one_pick_with_the_final_query() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    seed(&world, &tokio, "seed");
    let body = format!("{}fi tok", fzf_stub_two_lines("toki", &canonical(&tokio)));
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&tokio), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
    let (query_text, stage, outcome, result_path): (String, String, String, String) = conn
        .query_row(
            "SELECT query, stage, outcome, (SELECT path FROM dirs WHERE id = result_dir_id) FROM queries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the single queries row reads back");
    assert_eq!(query_text, "toki");
    assert_eq!(stage, "menu");
    assert_eq!(outcome, "pick");
    assert_eq!(result_path, canonical(&tokio));
}

#[test]
fn fi_fzf_aborted_with_only_a_query_line_does_not_move() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    seed(&world, &tokio, "seed");
    let start = world.tree.path();
    let body = format!("{}fi tok", fzf_stub("toki"));
    let run = run_pwsh(&world, "", "", start, &body);
    assert_eq!(run.cwd, canonical(start), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn fi_menu_branch_journals_exactly_one_pick() {
    let (world, _first, second) = ambiguous_world();
    let body = format!("{HIDE_FZF}function global:Read-Host {{ '2' }}\nfi tokoi");
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, second, "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
    let (query_text, stage, outcome, result_path): (String, String, String, String) = conn
        .query_row(
            "SELECT query, stage, outcome, (SELECT path FROM dirs WHERE id = result_dir_id) FROM queries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the single queries row reads back");
    assert_eq!(query_text, "tokoi");
    assert_eq!(stage, "menu");
    assert_eq!(outcome, "pick");
    assert_eq!(result_path, second);
}

#[test]
fn fi_without_a_query_writes_no_pick_row() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    seed(&world, &tokio, "seed");
    let via_fzf = format!("{}fi", fzf_stub_two_lines("", &canonical(&tokio)));
    let run = run_pwsh(&world, "", "", world.tree.path(), &via_fzf);
    assert_eq!(run.cwd, canonical(&tokio), "stderr: {}", run.stderr);
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM queries"), 0);

    let (world, _first, second) = ambiguous_world();
    let via_menu = format!("{HIDE_FZF}function global:Read-Host {{ '2' }}\nfi");
    let run = run_pwsh(&world, "", "", world.tree.path(), &via_menu);
    assert_eq!(run.cwd, second, "stderr: {}", run.stderr);
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn fi_fzf_branch_passes_print_query() {
    let world = sandbox(&[]);
    let out = world
        .furet()
        .arg("init")
        .arg("pwsh")
        .output()
        .expect("furet init pwsh runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let script = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        script.matches("--print-query").count(),
        1,
        "the fzf call must pass --print-query: {script}"
    );
}

#[test]
fn fi_fzf_branch_passes_the_preview_options() {
    let world = sandbox(&[]);
    let out = world
        .furet()
        .arg("init")
        .arg("pwsh")
        .output()
        .expect("furet init pwsh runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let script = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        script.matches("--preview \"furet preview {}\"").count(),
        1,
        "the fzf call must pass the preview command: {script}"
    );
    assert_eq!(
        script.matches("--preview-window \"right,50%\"").count(),
        2,
        "both fzf calls (the query list and the alias menu) must pass the preview window: {script}"
    );
}

#[test]
fn import_zoxide_through_a_real_pwsh_pipe_imports_an_accented_path() {
    let world = sandbox(&["r\u{e9}f\u{e9}rence"]);
    let target = world.child("r\u{e9}f\u{e9}rence");
    let body = format!("'  12.5 {}' | furet import zoxide", canonical(&target));
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert!(
        run.stderr.contains("imported 1, skipped 0"),
        "stderr: {}",
        run.stderr
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 1);
    let path: String = conn
        .query_row("SELECT path FROM dirs", [], |row| row.get(0))
        .expect("the imported dir row reads back");
    assert_eq!(path, canonical(&target));
}

#[test]
fn import_pwsh_history_through_a_real_pwsh_pipe_imports_an_accented_path() {
    let world = sandbox(&["r\u{e9}f\u{e9}rence"]);
    let target = world.child("r\u{e9}f\u{e9}rence");
    let body = format!(
        "@('git status', \"Set-Location '{}'\") | furet import pwsh-history",
        canonical(&target)
    );
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert!(
        run.stderr.contains("imported 1, skipped 0"),
        "stderr: {}",
        run.stderr
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 1);
    let path: String = conn
        .query_row("SELECT path FROM dirs", [], |row| row.get(0))
        .expect("the imported dir row reads back");
    assert_eq!(path, canonical(&target));
}

#[test]
fn init_pwsh_cmd_j_defines_j_not_f_and_j_query_jumps() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    seed(&world, &tokio, "seed");
    let body = "Write-Output ('FURET_TEST_F=' + [bool](Get-Command f -ErrorAction SilentlyContinue))\nj tokio";
    let run = run_pwsh(&world, "", "--cmd j", world.tree.path(), body);
    assert_eq!(extract(&run.stdout, "FURET_TEST_F="), "False");
    assert_eq!(run.cwd, canonical(&tokio), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 2);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn tab_completing_a_single_token_lists_the_ranked_candidates_in_order() {
    let world = sandbox(&["aaa/tokio", "zzz/tokio-tests"]);
    seed(&world, &world.child("aaa/tokio"), "seed");
    seed(&world, &world.child("zzz/tokio-tests"), "seed");
    let expected = query_list(&world, &["query", "--list", "tok"]);
    assert_eq!(expected.len(), 2, "both seeded directories rank for tok");
    assert_eq!(
        completions(&world, "", "f tok"),
        expected,
        "completions must equal furet query --list tok in order"
    );
}

#[test]
fn tab_completing_an_empty_word_lists_by_recency_like_an_empty_query() {
    let world = sandbox(&["alpha", "beta"]);
    seed(&world, &world.child("alpha"), "seed");
    seed(&world, &world.child("beta"), "seed");
    let expected = query_list(&world, &["query", "--list", ""]);
    assert_eq!(expected.len(), 2, "both seeded directories are listed");
    assert_eq!(
        completions(&world, "", "f "),
        expected,
        "an empty word must complete like an empty query"
    );
}

#[test]
fn tab_completing_a_second_token_proposes_no_furet_candidate() {
    let world = sandbox(&["reddit/ui"]);
    seed(&world, &world.child("reddit/ui"), "seed");
    let candidate = canonical(&world.child("reddit/ui"));
    let got = completions(&world, "", "f reddit u");
    assert!(
        !got.iter().any(|text| text.contains(&candidate)),
        "no completion may propose the furet candidate {candidate}: {got:?}"
    );
}

#[test]
fn tab_completing_special_forms_proposes_no_furet_candidate() {
    let world = sandbox(&["tokio"]);
    seed(&world, &world.child("tokio"), "seed");
    let candidate = canonical(&world.child("tokio"));
    for line in ["f -", "f --explain", "f .."] {
        let got = completions(&world, "", line);
        assert!(
            !got.iter().any(|text| text.contains(&candidate)),
            "{line:?} must not complete the furet candidate {candidate}: {got:?}"
        );
    }
}

#[test]
fn tab_completing_a_bang_word_shows_paths_but_inserts_names() {
    let world = sandbox(&["ombi", "omnitool"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    seed_alias(&world, "omnitool", &world.child("omnitool"));
    let items = completion_items_in(&world, "", world.tree.path(), "f !om");
    assert_eq!(
        items,
        vec![
            (
                "!ombi".to_owned(),
                format!("!ombi  {}", canonical(&world.child("ombi"))),
                canonical(&world.child("ombi")),
            ),
            (
                "!omnitool".to_owned(),
                format!("!omnitool  {}", canonical(&world.child("omnitool"))),
                canonical(&world.child("omnitool")),
            ),
        ]
    );
}

#[test]
fn tab_completing_the_bare_bang_lists_every_alias() {
    let world = sandbox(&["ombi", "apps", "1"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    seed_alias(&world, "apps", &world.child("apps"));
    seed_alias(&world, "1", &world.child("1"));
    let items = completion_items_in(&world, "", world.tree.path(), "f !");
    let names: Vec<&str> = items.iter().map(|(text, _, _)| text.as_str()).collect();
    assert_eq!(names, ["!1", "!apps", "!ombi"]);
}

#[test]
fn tab_completing_an_exact_alias_keeps_the_alias_word() {
    let world = sandbox(&["ombi"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    let items = completion_items_in(&world, "", world.tree.path(), "f !ombi");
    let texts: Vec<&str> = items.iter().map(|(text, _, _)| text.as_str()).collect();
    assert_eq!(texts, ["!ombi"]);
}

#[test]
fn tab_completing_an_equals_word_under_the_equals_prefix() {
    let world = sandbox(&["ombi"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    write_alias_prefix_config(&world, "=");
    let equals = completion_items_in(&world, "", world.tree.path(), "f =om");
    let texts: Vec<&str> = equals.iter().map(|(text, _, _)| text.as_str()).collect();
    assert_eq!(texts, ["=ombi"]);
    let bang = completion_items_in(&world, "", world.tree.path(), "f !om");
    assert!(
        bang.is_empty(),
        "bang is not the configured prefix: {bang:?}"
    );
}

#[test]
fn tab_completing_an_alias_word_after_local_proposes_nothing() {
    let world = sandbox(&["ombi"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    let start = world.child("start");
    std::fs::create_dir_all(&start).expect("the isolated start directory exists");
    let items = completion_items_in(&world, "", &start, "f -l !om");
    assert!(items.is_empty(), "no completion after -l: {items:?}");
}

#[test]
fn tab_completing_an_alias_word_never_falls_through_to_the_query_list() {
    let world = sandbox(&["deep/!omega"]);
    seed(&world, &world.child("deep/!omega"), "seed");
    write_alias_prefix_config(&world, "=");
    let global_list = query_list_in(&world, world.tree.path(), &["query", "--list", "--", "!om"]);
    assert!(
        global_list.contains(&canonical(&world.child("deep/!omega"))),
        "a fall-through to query --list would return the bang directory: {global_list:?}"
    );
    let items = completion_items_in(&world, "", world.tree.path(), "f !om");
    assert!(
        items.is_empty(),
        "no completion may fall through: {items:?}"
    );
}

#[test]
fn tab_completing_an_alias_word_leaves_lastexitcode_untouched() {
    let world = sandbox(&["ombi"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    let body = "$global:LASTEXITCODE = 42\n\
                [void] (TabExpansion2 -inputScript 'f !om' -cursorColumn 5)\n\
                Write-Output ('FURET_TEST_EXIT=' + $global:LASTEXITCODE)";
    let run = run_pwsh(&world, "", "", world.tree.path(), body);
    assert_eq!(
        extract(&run.stdout, "FURET_TEST_EXIT="),
        "42",
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn tab_completing_quotes_a_path_with_space_and_apostrophe_and_jumps_with_it() {
    let world = sandbox(&["it's here"]);
    let target = world.child("it's here");
    seed(&world, &target, "seed");
    let got = completions(&world, "", "f it");
    assert_eq!(got, vec![quote(&target)]);
    let body = format!("f {}", quote(&target));
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&target), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM visits WHERE source = 'jump'"),
        1
    );
}

#[test]
fn init_pwsh_cmd_j_completes_j_and_leaves_f_without_a_furet_candidate() {
    let world = sandbox(&["aaa/tokio"]);
    seed(&world, &world.child("aaa/tokio"), "seed");
    let candidate = canonical(&world.child("aaa/tokio"));
    assert_eq!(
        completions(&world, "--cmd j", "j tok"),
        vec![candidate.clone()]
    );
    let f = completions(&world, "--cmd j", "f tok");
    assert!(
        !f.iter().any(|text| text.contains(&candidate)),
        "f must have no furet candidate {candidate}: {f:?}"
    );
}

#[test]
fn tab_completes_furet_subcommands() {
    let world = sandbox(&[]);
    let got = completions(&world, "", "furet re");
    assert!(
        got.iter().any(|text| text == "remove"),
        "`furet re` must complete remove: {got:?}"
    );
}

#[test]
fn tab_completes_furet_list_options() {
    let world = sandbox(&[]);
    let got = completions(&world, "", "furet list --");
    assert!(
        got.iter().any(|text| text == "--all"),
        "`furet list --` must complete --all: {got:?}"
    );
    assert!(
        got.iter().any(|text| text == "--paths"),
        "`furet list --` must complete --paths: {got:?}"
    );
}

#[test]
fn tab_completes_furet_subcommands_with_a_custom_cmd() {
    let world = sandbox(&[]);
    let got = completions(&world, "--cmd j", "furet re");
    assert!(
        got.iter().any(|text| text == "remove"),
        "`--cmd j` must leave `furet re` completing remove: {got:?}"
    );
}

#[test]
fn tab_completion_leaves_lastexitcode_untouched() {
    let world = sandbox(&["tokio"]);
    seed(&world, &world.child("tokio"), "seed");
    let body = "$global:LASTEXITCODE = 42\n\
                [void] (TabExpansion2 -inputScript 'f tok' -cursorColumn 5)\n\
                Write-Output ('FURET_TEST_EXIT=' + $global:LASTEXITCODE)";
    let run = run_pwsh(&world, "", "", world.tree.path(), body);
    assert_eq!(
        extract(&run.stdout, "FURET_TEST_EXIT="),
        "42",
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn f_local_jumps_to_the_in_project_match_and_records_a_jump() {
    let world = sandbox(&["proj/.git", "proj/tokio", "elsewhere/tokio"]);
    seed(&world, &world.child("proj/tokio"), "seed");
    seed(&world, &world.child("elsewhere/tokio"), "seed");
    let start = world.child("proj");
    let run = run_pwsh(&world, "", "", &start, "f -l tokio");
    assert_eq!(
        run.cwd,
        canonical(&world.child("proj/tokio")),
        "stderr: {}",
        run.stderr
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 3);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn f_local_without_a_query_jumps_to_the_project_root() {
    let world = sandbox(&["proj/.git", "proj/sub"]);
    let start = world.child("proj/sub");
    let run = run_pwsh(&world, "", "", &start, "f -l");
    assert_eq!(
        run.cwd,
        canonical(&world.child("proj")),
        "stderr: {}",
        run.stderr
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn f_local_outside_a_repository_stays_put_and_records_nothing() {
    let world = sandbox(&["start"]);
    let start = world.child("start");
    assert!(
        project::root(
            world.tree.path().to_string_lossy().as_ref(),
            &project::RealGitMarker
        )
        .is_none(),
        "the sandbox tree must not sit inside a git repository"
    );
    let run = run_pwsh(&world, "", "", &start, "f -l tokio");
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        !world.data.path().join("furet.db").exists()
            || scalar(&db(&world), "SELECT COUNT(*) FROM visits") == 0
    );
}

#[test]
fn f_local_explain_reports_the_project_root_without_moving() {
    let world = sandbox(&["proj/.git", "proj/tokio", "proj/sub"]);
    seed(&world, &world.child("proj/tokio"), "seed");
    let start = world.child("proj/sub");
    let run = run_pwsh(&world, "", "", &start, "f -l tokio --explain");
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        run.stderr.contains("normalized query: tokio"),
        "stderr: {}",
        run.stderr
    );
    let root = canonical(&world.child("proj"));
    assert!(
        run.stderr.contains(&format!("project root: {root}\n")),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 1);
}

#[test]
fn fi_local_menu_branch_lists_only_the_project() {
    let world = sandbox(&["proj/.git", "proj/alpha", "proj/beta", "outside/gamma"]);
    for child in ["proj/alpha", "proj/beta", "outside/gamma"] {
        seed(&world, &world.child(child), "seed");
    }
    let start = world.child("proj");
    let body = format!("{HIDE_FZF}function global:Read-Host {{ '' }}\nfi -l");
    let run = run_pwsh(&world, "", "", &start, &body);
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        run.stdout.contains(&canonical(&world.child("proj/alpha"))),
        "stdout: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains(&canonical(&world.child("proj/beta"))),
        "stdout: {}",
        run.stdout
    );
    assert!(
        !run.stdout
            .contains(&canonical(&world.child("outside/gamma"))),
        "stdout: {}",
        run.stdout
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 3);
}

#[test]
fn tab_completing_after_local_proposes_project_candidates() {
    let world = sandbox(&["proj/.git", "proj/clio", "outside/clio"]);
    seed(&world, &world.child("proj/clio"), "seed");
    seed(&world, &world.child("outside/clio"), "seed");
    let proj = world.child("proj");
    let expected_empty = query_list_in(&world, &proj, &["query", "--list", "--local", "--", ""]);
    assert_eq!(
        expected_empty,
        vec![canonical(&world.child("proj/clio"))],
        "the project's empty-query list holds exactly the in-project candidate"
    );
    assert_eq!(
        completions_in(&world, "", &proj, "f -l "),
        expected_empty,
        "`f -l <Tab>` must complete the project's recency list"
    );
    let expected_cl = query_list_in(&world, &proj, &["query", "--list", "--local", "--", "cl"]);
    assert_eq!(
        expected_cl,
        vec![canonical(&world.child("proj/clio"))],
        "the project's ranked list holds exactly the in-project candidate"
    );
    assert_eq!(
        completions_in(&world, "", &proj, "f -l cl"),
        expected_cl,
        "`f -l cl<Tab>` must complete the project's ranked candidates"
    );
    let candidate = canonical(&world.child("proj/clio"));
    let too_long = completions_in(&world, "", &proj, "f -l cl ");
    assert!(
        !too_long.iter().any(|text| text.contains(&candidate)),
        "`f -l cl <Tab>` must propose no furet candidate {candidate}: {too_long:?}"
    );
}

#[test]
fn f_local_flag_after_the_query_still_scopes() {
    let world = sandbox(&["proj/.git", "proj/tokio", "elsewhere/tokio"]);
    seed(&world, &world.child("proj/tokio"), "seed");
    seed(&world, &world.child("elsewhere/tokio"), "seed");
    let start = world.child("proj");
    let run = run_pwsh(&world, "", "", &start, "f tokio -l");
    assert_eq!(
        run.cwd,
        canonical(&world.child("proj/tokio")),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn f_long_local_flag_still_scopes() {
    let world = sandbox(&["proj/.git", "proj/tokio", "elsewhere/tokio"]);
    seed(&world, &world.child("proj/tokio"), "seed");
    seed(&world, &world.child("elsewhere/tokio"), "seed");
    let start = world.child("proj");
    let run = run_pwsh(&world, "", "", &start, "f --local tokio");
    assert_eq!(
        run.cwd,
        canonical(&world.child("proj/tokio")),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn tab_completing_after_long_local_proposes_project_candidates() {
    let world = sandbox(&["proj/.git", "proj/clio", "outside/clio"]);
    seed(&world, &world.child("proj/clio"), "seed");
    seed(&world, &world.child("outside/clio"), "seed");
    let proj = world.child("proj");
    let got = completions_in(&world, "", &proj, "f --local cl");
    assert_eq!(
        got,
        vec![canonical(&world.child("proj/clio"))],
        "`f --local cl<Tab>` must propose only the in-project candidate: {got:?}"
    );
}

// WHY: seeds home/tokio before elsewhere/tokio, so the global winner is always elsewhere/tokio.
fn home_world(children: &[&str]) -> Sandbox {
    let mut all: Vec<&str> = vec!["home/tokio", "elsewhere/tokio", "start"];
    all.extend_from_slice(children);
    let world = sandbox(&all);
    seed(&world, &world.child("home/tokio"), "seed");
    seed(&world, &world.child("elsewhere/tokio"), "seed");
    write_home_config(&world, &world.child("home"));
    world
}

// WHY: the stub echoes fzf's arguments, then emits the empty query line and the first pipeline line.
fn fzf_stub_echoing_args() -> String {
    "function global:fzf {\n\
     Write-Host ('FZF_ARGS=' + ($args -join ' '))\n\
     ''\n\
     $input\n\
     }\n"
    .to_owned()
}

#[test]
fn f_home_jumps_to_the_in_home_match_and_records_a_jump() {
    let world = home_world(&[]);
    let start = world.child("start");
    let run = run_pwsh(&world, "", "", &start, "f -h tokio");
    assert_eq!(
        run.cwd,
        canonical(&world.child("home/tokio")),
        "stderr: {}",
        run.stderr
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 3);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn f_home_flag_forms_all_scope() {
    let world = home_world(&[]);
    let start = world.child("start");
    let run = run_pwsh(&world, "", "", &start, "f --home tokio");
    assert_eq!(
        run.cwd,
        canonical(&world.child("home/tokio")),
        "stderr: {}",
        run.stderr
    );
    let run = run_pwsh(&world, "", "", &start, "f tokio -h");
    assert_eq!(
        run.cwd,
        canonical(&world.child("home/tokio")),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn f_home_without_a_query_jumps_to_the_home_root() {
    let world = home_world(&[]);
    let start = world.child("start");
    let run = run_pwsh(&world, "", "", &start, "f -h");
    assert_eq!(
        run.cwd,
        canonical(&world.child("home")),
        "stderr: {}",
        run.stderr
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 3);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn f_local_and_home_together_stay_put_and_report_on_stderr() {
    let world = home_world(&["start/.git"]);
    let start = world.child("start");
    let run = run_pwsh(&world, "", "", &start, "f -l -h tokio");
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        run.stderr.contains("cannot be used with"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 2);
}

#[test]
fn f_home_explain_reports_the_home_root_without_moving() {
    let world = home_world(&[]);
    let start = world.child("start");
    let run = run_pwsh(&world, "", "", &start, "f -h tokio --explain");
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    let root = canonical(&world.child("home"));
    assert!(
        run.stderr.contains(&format!("home root: {root}\n")),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 2);
}

#[test]
fn f_home_with_an_alias_stays_put() {
    let world = home_world(&["zebra"]);
    seed_alias(&world, "ombi", &world.child("zebra"));
    let start = world.child("start");
    let run = run_pwsh(&world, "", "", &start, "f -h !ombi");
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        run.stderr
            .contains("--home cannot be combined with an alias"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 2);
}

#[test]
fn fi_home_menu_branch_lists_only_the_home() {
    let world = sandbox(&["home/alpha", "home/beta", "outside/gamma", "start"]);
    for child in ["home/alpha", "home/beta", "outside/gamma"] {
        seed(&world, &world.child(child), "seed");
    }
    write_home_config(&world, &world.child("home"));
    let start = world.child("start");
    let body = format!("{HIDE_FZF}function global:Read-Host {{ '' }}\nfi -h");
    let run = run_pwsh(&world, "", "", &start, &body);
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        run.stdout.contains(&canonical(&world.child("home/alpha"))),
        "stdout: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains(&canonical(&world.child("home/beta"))),
        "stdout: {}",
        run.stdout
    );
    assert!(
        !run.stdout
            .contains(&canonical(&world.child("outside/gamma"))),
        "stdout: {}",
        run.stdout
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 3);
}

#[test]
fn fi_home_fzf_branch_scopes_the_list_and_the_reload() {
    let world = sandbox(&["home/alpha", "outside/gamma", "start"]);
    seed(&world, &world.child("home/alpha"), "seed");
    seed(&world, &world.child("outside/gamma"), "seed");
    write_home_config(&world, &world.child("home"));
    let start = world.child("start");
    let body = format!("{}fi -h", fzf_stub_echoing_args());
    let run = run_pwsh(&world, "", "", &start, &body);
    assert_eq!(
        run.cwd,
        canonical(&world.child("home/alpha")),
        "stderr: {}",
        run.stderr
    );
    assert!(
        run.stdout
            .contains("reload:furet query --list --color --home {q}"),
        "stdout: {}",
        run.stdout
    );
}

#[test]
fn tab_completing_after_home_proposes_home_candidates() {
    let world = home_world(&[]);
    let start = world.child("start");
    let home_tokio = canonical(&world.child("home/tokio"));
    for line in ["f -h tok", "f --home tok"] {
        let got = completions_in(&world, "", &start, line);
        assert_eq!(got, vec![home_tokio.clone()], "{line}: {got:?}");
    }
}

#[test]
fn tab_completing_an_alias_word_after_home_proposes_nothing() {
    let world = home_world(&["zebra"]);
    seed_alias(&world, "ombi", &world.child("zebra"));
    let start = world.child("start");
    let items = completion_items_in(&world, "", &start, "f -h !om");
    assert!(items.is_empty(), "no completion after -h: {items:?}");
}

#[test]
fn f_bang_alias_jumps_and_records_one_jump_visit() {
    let world = sandbox(&["ombi"]);
    let target = world.child("ombi");
    seed_alias(&world, "ombi", &target);
    let start = world.child("start");
    std::fs::create_dir_all(&start).expect("the isolated start directory exists");
    let run = run_pwsh(&world, "", "", &start, "f !ombi");
    assert_eq!(run.cwd, canonical(&target), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "jump");
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn f_equals_alias_jumps_when_configured() {
    let world = sandbox(&["ombi"]);
    let target = world.child("ombi");
    seed_alias(&world, "ombi", &target);
    write_alias_prefix_config(&world, "=");
    let start = world.child("start");
    std::fs::create_dir_all(&start).expect("the isolated start directory exists");
    let run = run_pwsh(&world, "", "", &start, "f =ombi");
    assert_eq!(run.cwd, canonical(&target), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "jump");
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn f_unknown_alias_stays_put_and_reports_on_stderr() {
    let world = sandbox(&["ombi"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    let start = world.child("start");
    std::fs::create_dir_all(&start).expect("the isolated start directory exists");
    let run = run_pwsh(&world, "", "", &start, "f !nope");
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        run.stderr.contains("unknown alias"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 0);
}

#[test]
fn f_local_with_an_alias_stays_put() {
    let world = sandbox(&["ombi", "zebra"]);
    seed_alias(&world, "ombi", &world.child("zebra"));
    let start = world.child("start");
    std::fs::create_dir_all(&start).expect("the isolated start directory exists");
    let run = run_pwsh(&world, "", "", &start, "f -l !ombi");
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        run.stderr
            .contains("--local cannot be combined with an alias"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 0);
}

#[test]
fn f_existing_directory_named_like_the_alias_wins() {
    let world = sandbox(&["ombi", "elsewhere"]);
    seed_alias(&world, "ombi", &world.child("elsewhere"));
    let start = world.child("start");
    let literal = start.join("!ombi");
    std::fs::create_dir_all(&literal).expect("the literal !ombi child exists");
    let run = run_pwsh(&world, "", "", &start, "f !ombi");
    assert_eq!(run.cwd, canonical(&literal), "stderr: {}", run.stderr);
}

#[test]
fn fm_digit_marks_the_current_directory() {
    let world = sandbox(&["a", "b"]);
    let first = run_pwsh(&world, "", "", &world.child("a"), "fm 1");
    let a = canonical(&world.child("a"));
    assert!(
        first.stderr.contains(&format!("mark 1 -> {a}")),
        "stderr: {}",
        first.stderr
    );
    assert_eq!(mark_path(&world, "1"), Some(a));
    let second = run_pwsh(&world, "", "", &world.child("b"), "fm 1");
    let b = canonical(&world.child("b"));
    assert!(
        second.stderr.contains(&format!("mark 1 -> {b}")),
        "stderr: {}",
        second.stderr
    );
    assert!(!second.stderr.contains("already exists"));
    assert_eq!(mark_path(&world, "1"), Some(b));
}

#[test]
fn fm_without_arguments_lists_the_marks() {
    let world = sandbox(&["a", "b"]);
    seed_mark(&world, "1", &world.child("a"));
    seed_mark(&world, "3", &world.child("b"));
    let run = run_pwsh(&world, "", "", world.tree.path(), "fm");
    assert!(run.stderr.is_empty(), "stderr: {}", run.stderr);
    assert!(
        run.stdout
            .contains(&format!("1\t{}", canonical(&world.child("a")))),
        "stdout: {}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains(&format!("3\t{}", canonical(&world.child("b")))),
        "stdout: {}",
        run.stdout
    );
}

#[test]
fn fm_delete_forms() {
    let world = sandbox(&["a", "b", "c", "d"]);
    for (digit, child) in [("1", "a"), ("2", "b"), ("3", "c"), ("4", "d")] {
        seed_mark(&world, digit, &world.child(child));
    }
    seed_alias(&world, "ombi", &world.child("a"));
    let one = run_pwsh(&world, "", "", world.tree.path(), "fm -d 2");
    assert!(
        one.stderr.contains("removed mark 2"),
        "stderr: {}",
        one.stderr
    );
    let range = run_pwsh(&world, "", "", world.tree.path(), "fm -d 3-4");
    assert!(
        range.stderr.contains("removed mark 3") && range.stderr.contains("removed mark 4"),
        "stderr: {}",
        range.stderr
    );
    let all = run_pwsh(&world, "", "", world.tree.path(), "fm -d!");
    assert!(
        all.stderr.contains("removed mark 1"),
        "stderr: {}",
        all.stderr
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM aliases"), 1);
    let listed = world
        .furet()
        .arg("alias")
        .arg("list")
        .output()
        .expect("furet alias list runs");
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("ombi"),
        "stdout: {}",
        String::from_utf8_lossy(&listed.stdout)
    );
}

#[test]
fn fm_plus_jumps_to_the_next_mark_and_records_a_jump() {
    let world = sandbox(&["a", "b"]);
    seed_mark(&world, "1", &world.child("a"));
    seed_mark(&world, "3", &world.child("b"));
    let run = run_pwsh(&world, "", "", &world.child("a"), "fm +");
    assert_eq!(
        run.cwd,
        canonical(&world.child("b")),
        "stderr: {}",
        run.stderr
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "jump");
}

#[test]
fn fm_minus_wraps_to_the_previous_mark() {
    let world = sandbox(&["a", "b"]);
    seed_mark(&world, "1", &world.child("a"));
    seed_mark(&world, "3", &world.child("b"));
    let run = run_pwsh(&world, "", "", &world.child("a"), "fm -");
    assert_eq!(
        run.cwd,
        canonical(&world.child("b")),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn fm_cycling_without_marks_stays_put() {
    let world = sandbox(&["x"]);
    let run = run_pwsh(&world, "", "", &world.child("x"), "fm +");
    assert_eq!(
        run.cwd,
        canonical(&world.child("x")),
        "stderr: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains("no marks set"),
        "stderr: {}",
        run.stderr
    );
    assert!(
        !world.data.path().join("furet.db").exists()
            || scalar(&db(&world), "SELECT COUNT(*) FROM visits") == 0
    );
}

#[test]
fn fm_invalid_digit_reports_and_changes_nothing() {
    let world = sandbox(&["a"]);
    let run = run_pwsh(&world, "", "", &world.child("a"), "fm 0");
    assert!(
        run.stderr.contains("invalid mark '0'"),
        "stderr: {}",
        run.stderr
    );
    assert!(
        !world.data.path().join("furet.db").exists()
            || scalar(&db(&world), "SELECT COUNT(*) FROM aliases") == 0
    );
}

#[test]
fn f_bang_digit_jumps_to_a_mark_set_with_fm() {
    let world = sandbox(&["a", "x"]);
    let body = format!(
        "fm 1\nSet-Location -LiteralPath {x}\nf !1",
        x = quote(&world.child("x"))
    );
    let run = run_pwsh(&world, "", "", &world.child("a"), &body);
    assert_eq!(
        run.cwd,
        canonical(&world.child("a")),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn fm_is_fixed_whatever_the_cmd() {
    let world = sandbox(&["a"]);
    let body = "Write-Output ('FURET_TEST_JM=' + [bool](Get-Command jm -CommandType Function -ErrorAction SilentlyContinue))\nfm 1";
    let run = run_pwsh(&world, "", "--cmd j", &world.child("a"), body);
    assert_eq!(
        extract(&run.stdout, "FURET_TEST_JM="),
        "False",
        "stderr: {}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains(&format!("mark 1 -> {}", canonical(&world.child("a")))),
        "stderr: {}",
        run.stderr
    );
}

// WHY: the fh lines are picked by their tab-field shape, the only tab-separated stdout the script prints.
fn history_lines(stdout: &str, field_count: usize) -> Vec<Vec<&str>> {
    stdout
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|fields| fields.len() == field_count)
        .collect()
}

#[test]
fn fh_lists_the_session_numbered_like_f_dash_n() {
    let world = sandbox(&["a", "b", "c"]);
    let a = world.child("a");
    let b = world.child("b");
    let c = world.child("c");
    let body = format!(
        "f {}\nf {}\nf {}\nfh\nf -2",
        quote(&a),
        quote(&b),
        quote(&c)
    );
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    let lines = history_lines(&run.stdout, 4);
    assert_eq!(lines.len(), 3, "stdout: {}", run.stdout);
    for (index, dir) in [("0", &c), ("1", &b), ("2", &a)] {
        let fields = lines
            .iter()
            .find(|fields| fields[0] == index)
            .unwrap_or_else(|| panic!("no history line {index} in: {}", run.stdout));
        assert_eq!(fields[2], "jump", "line {index}: {}", run.stdout);
        assert_eq!(fields[3], canonical(dir), "line {index}: {}", run.stdout);
    }
    assert_eq!(run.cwd, canonical(&a), "stderr: {}", run.stderr);
}

#[test]
fn fh_dash_a_lists_every_session_without_numbers() {
    let world = sandbox(&["a", "x"]);
    let a = world.child("a");
    seed(&world, &world.child("x"), "other-session");
    let body = format!("f {}\nfh -a", quote(&a));
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    let lines = history_lines(&run.stdout, 3);
    assert_eq!(lines.len(), 2, "stdout: {}", run.stdout);
    let seeded = canonical(&world.child("x"));
    assert!(
        lines.iter().any(|fields| fields[2] == seeded),
        "stdout: {}",
        run.stdout
    );
    assert!(
        lines.iter().any(|fields| fields[2] == canonical(&a)),
        "stdout: {}",
        run.stdout
    );
}

#[test]
fn fh_dash_n_limits_the_lines() {
    let world = sandbox(&["a", "b", "c"]);
    let a = world.child("a");
    let b = world.child("b");
    let c = world.child("c");
    let body = format!("f {}\nf {}\nf {}\nfh -n 1", quote(&a), quote(&b), quote(&c));
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    let lines = history_lines(&run.stdout, 4);
    assert_eq!(lines.len(), 1, "stdout: {}", run.stdout);
    assert_eq!(lines[0][0], "0", "stdout: {}", run.stdout);
    assert_eq!(lines[0][3], canonical(&c), "stdout: {}", run.stdout);
}

#[test]
fn fh_is_fixed_whatever_the_cmd() {
    let world = sandbox(&["a"]);
    let body = "Write-Output ('FURET_TEST_FH=' + [bool](Get-Command fh -CommandType Function -ErrorAction SilentlyContinue))\n\
                Write-Output ('FURET_TEST_JH=' + [bool](Get-Command jh -CommandType Function -ErrorAction SilentlyContinue))";
    let run = run_pwsh(&world, "", "--cmd j", world.tree.path(), body);
    assert_eq!(
        extract(&run.stdout, "FURET_TEST_FH="),
        "True",
        "stderr: {}",
        run.stderr
    );
    assert_eq!(
        extract(&run.stdout, "FURET_TEST_JH="),
        "False",
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn init_binds_ctrl_alt_arrows_when_psreadline_is_loaded() {
    let world = sandbox(&[]);
    let body = "Write-Output ('FURET_TEST_NEXT=' + (Get-PSReadLineKeyHandler -Chord 'Ctrl+Alt+RightArrow').Function)\n\
                Write-Output ('FURET_TEST_PREV=' + (Get-PSReadLineKeyHandler -Chord 'Ctrl+Alt+LeftArrow').Function)";
    let run = run_pwsh(
        &world,
        "Import-Module PSReadLine",
        "",
        world.tree.path(),
        body,
    );
    assert_eq!(
        extract(&run.stdout, "FURET_TEST_NEXT="),
        "FuretNextMark",
        "stderr: {}",
        run.stderr
    );
    assert_eq!(
        extract(&run.stdout, "FURET_TEST_PREV="),
        "FuretPreviousMark",
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn init_without_psreadline_binds_nothing() {
    let world = sandbox(&[]);
    let body = "Write-Output ('FURET_TEST_MODULE=' + [bool](Get-Module PSReadLine))";
    let run = run_pwsh(&world, "", "", world.tree.path(), body);
    assert!(run.stderr.is_empty(), "stderr: {}", run.stderr);
    assert_eq!(
        extract(&run.stdout, "FURET_TEST_MODULE="),
        "False",
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn fi_bang_fzf_lists_marks_first_and_jumps_without_a_query_row() {
    let world = sandbox(&["ombi", "omnitool", "apps", "one", "x"]);
    seed_mark(&world, "1", &world.child("one"));
    seed_alias(&world, "ombi", &world.child("ombi"));
    seed_alias(&world, "apps", &world.child("apps"));
    let start = world.child("x");
    let body = format!("{}fi !", fzf_stub_echoing_input_and_first_line());
    let run = run_pwsh(&world, "", "", &start, &body);
    assert_eq!(
        run.cwd,
        canonical(&world.child("one")),
        "stderr: {}",
        run.stderr
    );
    let listed: Vec<&str> = run
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("FZF_IN="))
        .collect();
    assert_eq!(
        listed,
        vec![
            format!("!1\t{}", canonical(&world.child("one"))),
            format!("!apps\t{}", canonical(&world.child("apps"))),
            format!("!ombi\t{}", canonical(&world.child("ombi"))),
        ],
        "stdout: {}",
        run.stdout
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(last_visit_source(&conn), "jump");
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn fi_bang_fzf_passes_the_initial_query_and_the_preview() {
    let world = sandbox(&["ombi", "omnitool", "apps", "x"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    let start = world.child("x");
    let body = format!("{}fi !om", fzf_stub_echoing_args_silently());
    let run = run_pwsh(&world, "", "", &start, &body);
    let args = extract(&run.stdout, "FZF_ARGS=");
    assert!(args.contains("--query om"), "args: {args}");
    assert!(args.contains("--delimiter"), "args: {args}");
    assert!(args.contains("furet preview {2}"), "args: {args}");
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
}

#[test]
fn fi_bang_menu_without_fzf_filters_by_prefix_and_jumps() {
    let world = sandbox(&["ombi", "omnitool", "apps", "x"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    seed_alias(&world, "omnitool", &world.child("omnitool"));
    seed_alias(&world, "apps", &world.child("apps"));
    let start = world.child("x");
    let body = format!("{HIDE_FZF}function global:Read-Host {{ '2' }}\nfi !om");
    let run = run_pwsh(&world, "", "", &start, &body);
    assert_eq!(
        run.cwd,
        canonical(&world.child("omnitool")),
        "stderr: {}",
        run.stderr
    );
    let ombi = canonical(&world.child("ombi"));
    let omnitool = canonical(&world.child("omnitool"));
    assert!(
        run.stdout.contains(&format!("1) !ombi  {ombi}")),
        "stdout: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains(&format!("2) !omnitool  {omnitool}")),
        "stdout: {}",
        run.stdout
    );
    assert!(!run.stdout.contains("!apps"), "stdout: {}", run.stdout);
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn fi_bang_menu_cancel_stays_put() {
    let world = sandbox(&["ombi", "x"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    let start = world.child("x");
    let body = format!("{HIDE_FZF}function global:Read-Host {{ '' }}\nfi !om");
    let run = run_pwsh(&world, "", "", &start, &body);
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        !world.data.path().join("furet.db").exists()
            || scalar(&db(&world), "SELECT COUNT(*) FROM visits") == 0
    );
}

#[test]
fn fi_bang_with_no_alias_does_nothing() {
    let world = sandbox(&["x"]);
    let start = world.child("x");
    let body = format!("{HIDE_FZF}fi !");
    let run = run_pwsh(&world, "", "", &start, &body);
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        !run.stdout.contains("Choose a directory:"),
        "stdout: {}",
        run.stdout
    );
    assert!(
        !world.data.path().join("furet.db").exists()
            || scalar(&db(&world), "SELECT COUNT(*) FROM visits") == 0
    );
}

#[test]
fn fi_local_with_an_alias_word_keeps_the_scoped_behavior() {
    let world = sandbox(&["x/.git", "ombi"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    let start = world.child("x");
    let body = format!("{HIDE_FZF}fi -l !om");
    let run = run_pwsh(&world, "", "", &start, &body);
    assert_eq!(run.cwd, canonical(&start), "stderr: {}", run.stderr);
    assert!(
        run.stderr
            .contains("--local cannot be combined with an alias"),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn fi_bang_under_the_equals_prefix_uses_equals() {
    let world = sandbox(&["ombi", "x"]);
    seed_alias(&world, "ombi", &world.child("ombi"));
    write_alias_prefix_config(&world, "=");
    let start = world.child("x");
    let pick = format!("{HIDE_FZF}function global:Read-Host {{ '1' }}\nfi =om");
    let run = run_pwsh(&world, "", "", &start, &pick);
    assert_eq!(
        run.cwd,
        canonical(&world.child("ombi")),
        "stderr: {}",
        run.stderr
    );
    let bang = format!("{HIDE_FZF}fi !om");
    let fresh = run_pwsh(&world, "", "", &start, &bang);
    assert_eq!(fresh.cwd, canonical(&start), "stderr: {}", fresh.stderr);
    assert!(
        !fresh.stdout.contains("Choose a directory:"),
        "stdout: {}",
        fresh.stdout
    );
}

#[test]
fn export_redirected_by_pwsh_keeps_an_accented_path() {
    let world = sandbox(&["réf"]);
    seed(&world, &world.child("réf"), "session-1");
    let backup = world.tree.path().join("backup.json");
    let body = format!("furet export > {}", quote(&backup));
    run_pwsh(&world, "", "", world.tree.path(), &body);
    let bytes = std::fs::read(&backup).expect("pwsh wrote the backup file");
    let contents = std::str::from_utf8(&bytes).expect("the backup file is valid UTF-8");
    assert!(contents.is_ascii(), "the backup file must be pure ASCII");
    let value: serde_json::Value =
        serde_json::from_str(contents).expect("the backup file parses as JSON");
    assert_eq!(
        value["dirs"][0]["path"],
        canonical(&world.child("réf")),
        "the accented path round-trips through the pwsh redirection"
    );
}

#[test]
fn export_then_import_json_through_pwsh_round_trips_an_accented_path() {
    let world = sandbox(&["réf"]);
    seed(&world, &world.child("réf"), "session-1");
    let other = sandbox(&[]);
    let backup = world.tree.path().join("backup.json");
    let body = format!(
        "furet export > {file}\n$env:FURET_DATA_DIR = {data}\nGet-Content {file} -Raw | furet import json",
        file = quote(&backup),
        data = quote(other.data.path()),
    );
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert!(
        run.stderr
            .contains("added 1 dirs, 1 visits, 0 queries, 0 aliases"),
        "stderr: {}",
        run.stderr
    );
    let stored: String = {
        let conn = db(&other);
        conn.query_row("SELECT path FROM dirs", [], |row| row.get(0))
            .expect("the imported directory reads back")
    };
    assert_eq!(
        stored,
        canonical(&world.child("réf")),
        "the accented path is stored byte for byte"
    );
}
