// WHY: this whole file is layer-3 test code, where expect() is the norm.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use assert_fs::TempDir;
use furet::paths;
use rusqlite::{Connection, params};

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

    fn furet(&self) -> Command {
        let mut cmd = Command::cargo_bin("furet").expect("the furet binary is built");
        cmd.env("FURET_DATA_DIR", self.data.path());
        cmd
    }
}

fn run(cmd: &mut Command) -> Output {
    cmd.assert().get_output().clone()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn db(sandbox: &Sandbox) -> Connection {
    Connection::open(sandbox.data.path().join("furet.db")).expect("the recorded database opens")
}

fn scalar(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0))
        .expect("the scalar query reads")
}

fn add(
    sandbox: &Sandbox,
    path: &Path,
    session: &str,
    source: Option<&str>,
    from: Option<&Path>,
) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("add").arg(path).arg("--session").arg(session);
    if let Some(source) = source {
        cmd.arg("--source").arg(source);
    }
    if let Some(from) = from {
        cmd.arg("--from").arg(from);
    }
    run(&mut cmd)
}

fn query(sandbox: &Sandbox, query: &str, cwd: &Path, list: bool) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("query").arg(query).current_dir(cwd);
    if list {
        cmd.arg("--list");
    }
    run(&mut cmd)
}

#[test]
fn add_records_one_dir_and_one_visit_with_the_default_source() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    let out = add(&world, &tokio, "session-1", None, None);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "add writes nothing to stdout");
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 1);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    let canonical = paths::canonical(&tokio).expect("the child canonicalizes");
    let (path, key, missing_since): (String, String, Option<i64>) = conn
        .query_row("SELECT path, key, missing_since FROM dirs", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .expect("the single dirs row reads back");
    assert_eq!(path, canonical.path);
    assert_eq!(key, canonical.key);
    assert_eq!(missing_since, None);
    let (source, session, from_dir_id): (String, String, Option<i64>) = conn
        .query_row(
            "SELECT source, session, from_dir_id FROM visits",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the single visits row reads back");
    assert_eq!(source, "hook");
    assert_eq!(session, "session-1");
    assert_eq!(from_dir_id, None);
}

#[test]
fn add_respects_an_explicit_source_and_rejects_an_unknown_one() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    let out = add(&world, &tokio, "session-1", Some("jump"), None);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let bad = add(&world, &tokio, "session-1", Some("manual"), None);
    assert!(!bad.status.success(), "an unlisted source must be rejected");
    assert!(bad.stdout.is_empty());
    assert!(!bad.stderr.is_empty());
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    let source: String = conn
        .query_row("SELECT source FROM visits", [], |row| row.get(0))
        .expect("the single visits row reads back");
    assert_eq!(source, "jump");
}

#[test]
fn adding_one_directory_twice_upserts_one_row_and_two_visits() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let upper = PathBuf::from(tokio.to_string_lossy().to_uppercase());
    assert!(
        add(&world, &upper, "session-2", None, None)
            .status
            .success()
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 1);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 2);
    assert_eq!(
        scalar(&conn, "SELECT COUNT(DISTINCT session) FROM visits"),
        2
    );
}

#[test]
fn add_from_links_a_known_origin_and_skips_an_unknown_one() {
    let world = sandbox(&["helix", "tokio", "stranger"]);
    let helix = world.child("helix");
    let tokio = world.child("tokio");
    let stranger = world.child("stranger");
    assert!(
        add(&world, &helix, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(&world, &tokio, "session-1", None, Some(&helix))
            .status
            .success()
    );
    assert!(
        add(&world, &tokio, "session-1", None, Some(&stranger))
            .status
            .success()
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 2);
    let helix_key = paths::canonical(&helix)
        .expect("the origin canonicalizes")
        .key;
    let helix_id: i64 = conn
        .query_row(
            "SELECT id FROM dirs WHERE key = ?1",
            params![helix_key],
            |row| row.get(0),
        )
        .expect("the known origin row reads back");
    let tokio_key = paths::canonical(&tokio)
        .expect("the target canonicalizes")
        .key;
    let mut stmt = conn
        .prepare("SELECT from_dir_id FROM visits WHERE dir_id = (SELECT id FROM dirs WHERE key = ?1) ORDER BY id")
        .expect("the visits statement prepares");
    let origins: Vec<Option<i64>> = stmt
        .query_map(params![tokio_key], |row| row.get(0))
        .expect("the visits read")
        .collect::<Result<Vec<_>, _>>()
        .expect("the visits collect");
    assert_eq!(origins, [Some(helix_id), None]);
}

#[test]
fn add_rejects_a_path_that_does_not_exist() {
    let world = sandbox(&[]);
    let missing = world.tree.path().join("nope");
    let out = add(&world, &missing, "session-1", None, None);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
    assert!(!world.data.path().join("furet.db").exists());
}

#[test]
fn query_with_no_recorded_directory_fails_on_stderr() {
    let world = sandbox(&["tokio"]);
    let out = query(&world, "tokio", world.tree.path(), false);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn query_prints_the_best_match_and_nothing_else() {
    let world = sandbox(&["tokei", "tokio"]);
    let tokei = world.child("tokei");
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokei, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = query(&world, "tokio", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&tokio)
        .expect("the best match canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
}

#[test]
fn query_list_prints_every_ranked_candidate_best_first() {
    let world = sandbox(&["stock", "tokio"]);
    let stock = world.child("stock");
    let tokio = world.child("tokio");
    assert!(
        add(&world, &stock, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = query(&world, "tok", world.tree.path(), true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let first = paths::canonical(&tokio)
        .expect("the best match canonicalizes")
        .path;
    let second = paths::canonical(&stock)
        .expect("the second match canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{first}\n{second}\n"));
}

#[test]
fn query_list_with_no_candidate_prints_nothing_and_exits_zero() {
    let world = sandbox(&[]);
    let out = query(&world, "anything", world.tree.path(), true);
    assert!(out.status.success());
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn query_never_returns_the_current_directory() {
    let world = sandbox(&["tokei", "tokio"]);
    let tokei = world.child("tokei");
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokei, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let from_tree = query(&world, "tokio", world.tree.path(), false);
    assert!(
        from_tree.status.success(),
        "stderr: {}",
        text(&from_tree.stderr)
    );
    let tokio_line = paths::canonical(&tokio)
        .expect("the current directory canonicalizes")
        .path;
    assert_eq!(text(&from_tree.stdout), format!("{tokio_line}\n"));
    let from_inside = query(&world, "tokio", &tokio, false);
    assert!(
        from_inside.status.success(),
        "stderr: {}",
        text(&from_inside.stderr)
    );
    let tokei_line = paths::canonical(&tokei)
        .expect("the runner-up canonicalizes")
        .path;
    assert_eq!(text(&from_inside.stdout), format!("{tokei_line}\n"));
}

#[test]
fn up_prints_the_canonical_ancestor_n_levels_above() {
    let world = sandbox(&["a/b/c"]);
    let deep = world.child("a").join("b").join("c");
    let out = run(world.furet().arg("up").arg("2").current_dir(&deep));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&world.child("a"))
        .expect("the ancestor canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
}

#[test]
fn up_fails_when_there_are_fewer_ancestors_than_requested() {
    let world = sandbox(&["a"]);
    let out = run(world
        .furet()
        .arg("up")
        .arg("10000")
        .current_dir(world.child("a")));
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn up_rejects_zero_levels() {
    let world = sandbox(&["a"]);
    let out = run(world
        .furet()
        .arg("up")
        .arg("0")
        .current_dir(world.child("a")));
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn back_prints_the_second_to_last_visited_directory_for_the_session() {
    let world = sandbox(&["tokei", "tokio", "helix"]);
    let tokei = world.child("tokei");
    let tokio = world.child("tokio");
    let helix = world.child("helix");
    assert!(
        add(&world, &tokei, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(&world, &helix, "session-1", None, None)
            .status
            .success()
    );
    let out = run(world.furet().arg("back").arg("--session").arg("session-1"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&tokio)
        .expect("the expected previous directory canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
}

#[test]
fn back_fails_when_the_session_has_fewer_than_two_visits() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = run(world.furet().arg("back").arg("--session").arg("session-1"));
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn init_pwsh_prints_a_nonempty_script_naming_the_default_command() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stderr.is_empty());
    let script = text(&out.stdout);
    assert!(!script.trim().is_empty());
    assert!(script.contains("function global:f "));
}

#[test]
fn init_pwsh_respects_a_custom_cmd_name() {
    let world = sandbox(&[]);
    let out = run(world
        .furet()
        .arg("init")
        .arg("pwsh")
        .arg("--cmd")
        .arg("jump"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let script = text(&out.stdout);
    assert!(script.contains("function global:jump "));
}

#[test]
fn init_pwsh_never_bakes_in_a_session_id() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh"));
    let script = text(&out.stdout);
    assert!(script.contains("[guid]::NewGuid()"));
}

#[test]
fn version_prints_a_nonempty_string() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("--version"));
    assert!(out.status.success());
    assert!(!text(&out.stdout).trim().is_empty());
}
