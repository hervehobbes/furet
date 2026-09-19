// WHY: this whole file is layer-3 test code, where expect() is the norm.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_fs::TempDir;
use furet::paths;
use rusqlite::{Connection, params};

const LAB: &str = "tokio-lab";

struct Sandbox {
    tree: TempDir,
    data: TempDir,
}

fn sandbox(children: &[&str]) -> Sandbox {
    let tree = TempDir::new().expect("a fresh tree directory");
    for child in children {
        std::fs::create_dir_all(tree.path().join(LAB).join(child))
            .expect("a child directory exists");
    }
    let data = TempDir::new().expect("a fresh data directory");
    Sandbox { tree, data }
}

impl Sandbox {
    fn child(&self, name: &str) -> PathBuf {
        self.tree.path().join(LAB).join(name)
    }

    fn furet(&self) -> Command {
        let mut cmd = Command::cargo_bin("furet").expect("the furet binary is built");
        cmd.env("FURET_DATA_DIR", self.data.path());
        cmd
    }

    fn db(&self) -> Connection {
        Connection::open(self.data.path().join("furet.db")).expect("the recorded database opens")
    }

    fn add(&self, child: &str) {
        let out = self
            .furet()
            .arg("add")
            .arg(self.child(child))
            .arg("--session")
            .arg("session-1")
            .assert()
            .get_output()
            .clone();
        assert!(out.status.success(), "add {child} failed");
    }

    fn visited_at(&self, child: &str, seconds: i64) {
        let canonical = paths::canonical(&self.child(child))
            .expect("the recorded directory canonicalizes")
            .path;
        let updated = self
            .db()
            .execute(
                "UPDATE visits SET ts = ?2
                 WHERE dir_id = (SELECT id FROM dirs WHERE path = ?1)",
                params![canonical, seconds],
            )
            .expect("the visit timestamp is forced");
        assert_eq!(updated, 1, "exactly one visit belongs to {child}");
    }

    fn explain(&self, query: &str, cwd: &Path) -> String {
        let out = self
            .furet()
            .arg("query")
            .arg(query)
            .arg("--explain")
            .current_dir(cwd)
            .assert()
            .get_output()
            .clone();
        assert!(out.status.success(), "furet query --explain must exit 0");
        assert!(out.stdout.is_empty(), "--explain writes nothing to stdout");
        let root = paths::canonical(self.tree.path())
            .expect("the sandbox root canonicalizes")
            .path;
        String::from_utf8_lossy(&out.stderr).replace(&root, "<tmp>")
    }
}

fn scored_world() -> Sandbox {
    let world = sandbox(&["tokio", "tokei", "helix", "zellij", "gone"]);
    for (child, seconds) in [
        ("gone", 1_700_000_001),
        ("zellij", 1_700_000_002),
        ("helix", 1_700_000_003),
        ("tokei", 1_700_000_004),
        ("tokio", 1_700_000_005),
    ] {
        world.add(child);
        world.visited_at(child, seconds);
    }
    std::fs::remove_dir_all(world.child("gone")).expect("the recorded directory vanishes");
    world
}

#[test]
fn explain_reports_a_stage_one_jump_with_every_elimination_reason() {
    let world = scored_world();
    let cwd = world.child("helix");
    insta::assert_snapshot!("stage_one_jump", world.explain("tokio", &cwd));
}

#[test]
fn explain_reports_a_query_too_short_for_stage_two() {
    let world = scored_world();
    let cwd = world.tree.path().to_path_buf();
    insta::assert_snapshot!("no_subsequence", world.explain("tok", &cwd));
}

#[test]
fn explain_reports_the_menu_a_stage_two_tie_would_open() {
    let world = sandbox(&["aaa/tokio", "zzz/tokio"]);
    for (child, seconds) in [("zzz/tokio", 1_700_000_001), ("aaa/tokio", 1_700_000_002)] {
        world.add(child);
        world.visited_at(child, seconds);
    }
    let cwd = world.tree.path().to_path_buf();
    insta::assert_snapshot!("stage_two_menu", world.explain("tokoi", &cwd));
}

#[test]
fn explain_beats_list_when_both_flags_are_passed() {
    let world = scored_world();
    let out = world
        .furet()
        .arg("query")
        .arg("tokio")
        .arg("--list")
        .arg("--explain")
        .current_dir(world.tree.path())
        .assert()
        .get_output()
        .clone();
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "--explain wins over --list");
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("normalized query: tokio\n"));
}

#[test]
fn explain_exits_zero_when_nothing_matches() {
    let world = scored_world();
    // WHY: an isolated cwd keeps the fallback ancestor walk off the shared OS temp dir.
    let cwd = world.tree.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("the isolated cwd exists");
    let rendered = world.explain("zigzag", &cwd);
    assert!(rendered.contains("decision: none\n"), "{rendered}");
    assert!(
        rendered.contains("deciding criterion: none (no runner-up)"),
        "{rendered}"
    );
}
