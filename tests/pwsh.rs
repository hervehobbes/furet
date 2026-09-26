// WHY: this whole file is layer-3 test code, where expect() is the norm.
#![allow(clippy::expect_used)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use assert_fs::TempDir;
use furet::paths;
use furet::project;
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
