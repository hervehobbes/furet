// WHY: this whole file is layer-3 test code, where expect() is the norm.
#![allow(clippy::expect_used)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use assert_fs::TempDir;
use furet::paths;
use rusqlite::Connection;

struct Sandbox {
    tree: TempDir,
    data: TempDir,
}

fn sandbox(children: &[&str]) -> Sandbox {
    let tree = TempDir::new().expect("a fresh tree directory");
    for child in children {
        std::fs::create_dir_all(tree.path().join(child)).expect("a child directory exists");
    }
    let data = TempDir::new().expect("a fresh data directory");
    Sandbox { tree, data }
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

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
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

// WHY: fi's no-fzf branch only triggers once every fzf/fzf.exe on PATH is hidden.
const HIDE_FZF: &str = "$env:PATH = ((($env:PATH -split ';') | Where-Object { \
    -not (Test-Path (Join-Path $_ 'fzf.exe')) -and -not (Test-Path (Join-Path $_ 'fzf')) \
}) -join ';')\n";

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
    let body = format!("{}fi tok", fzf_stub(&canonical(&tokio)));
    let run = run_pwsh(&world, "", "", world.tree.path(), &body);
    assert_eq!(run.cwd, canonical(&tokio), "stderr: {}", run.stderr);
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 2);
    assert_eq!(last_visit_source(&conn), "jump");
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
