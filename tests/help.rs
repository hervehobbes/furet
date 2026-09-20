// WHY: this is layer-3 test code, where expect() is the norm.
#![allow(clippy::expect_used)]

use std::path::Path;

use assert_cmd::Command;
use assert_fs::TempDir;

fn help(args: &[&str], data: &Path) -> String {
    let mut cmd = Command::cargo_bin("furet").expect("the furet binary is built");
    cmd.env("FURET_DATA_DIR", data);
    cmd.env_remove("FURET_LOG");
    for arg in args {
        cmd.arg(arg);
    }
    let out = cmd.assert().get_output().clone();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// WHY: the sandbox path differs per run, so each snapshot filters it down to a placeholder.
fn snapshot_help(name: &str, args: &[&str]) {
    let data = TempDir::new().expect("a fresh data directory");
    let text = help(args, data.path());
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(&data.path().to_string_lossy()), "<DATA_DIR>");
    settings.add_filter(r"furet\.exe", "furet");
    settings.bind(|| insta::assert_snapshot!(name, text));
}

#[test]
fn top_level_help_is_pinned() {
    snapshot_help("top_level_help", &["--help"]);
}

#[test]
fn add_help_is_pinned() {
    snapshot_help("add_help", &["add", "--help"]);
}

#[test]
fn query_help_is_pinned() {
    snapshot_help("query_help", &["query", "--help"]);
}

#[test]
fn up_help_is_pinned() {
    snapshot_help("up_help", &["up", "--help"]);
}

#[test]
fn back_help_is_pinned() {
    snapshot_help("back_help", &["back", "--help"]);
}

#[test]
fn init_help_is_pinned() {
    snapshot_help("init_help", &["init", "--help"]);
}

#[test]
fn init_pwsh_help_is_pinned() {
    snapshot_help("init_pwsh_help", &["init", "pwsh", "--help"]);
}

#[test]
fn queries_help_is_pinned() {
    snapshot_help("queries_help", &["queries", "--help"]);
}

#[test]
fn list_help_is_pinned() {
    snapshot_help("list_help", &["list", "--help"]);
}

#[test]
fn home_help_is_pinned() {
    snapshot_help("home_help", &["home", "--help"]);
}

#[test]
fn import_help_is_pinned() {
    snapshot_help("import_help", &["import", "--help"]);
}
