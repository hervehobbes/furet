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
        cmd.env_remove("FURET_LOG");
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

fn log_contents(sandbox: &Sandbox) -> String {
    let logs = sandbox.data.path().join("logs");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&logs)
        .expect("the logs directory exists")
        .map(|entry| entry.expect("a log directory entry").path())
        .filter(|path| {
            let name = path
                .file_name()
                .expect("the entry has a name")
                .to_string_lossy();
            name.starts_with("furet.") && name.ends_with(".log")
        })
        .collect();
    names.sort();
    names
        .iter()
        .map(|path| std::fs::read_to_string(path).expect("the log file reads"))
        .collect::<Vec<_>>()
        .join("\n")
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

fn query_answering(sandbox: &Sandbox, query: &str, cwd: &Path, answer: &str) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("query")
        .arg(query)
        .current_dir(cwd)
        .write_stdin(answer);
    run(&mut cmd)
}

fn import_zoxide(sandbox: &Sandbox, stdin: &str) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("import").arg("zoxide").write_stdin(stdin);
    run(&mut cmd)
}

fn write_config(sandbox: &Sandbox, contents: &str) {
    std::fs::write(sandbox.data.path().join("config.toml"), contents)
        .expect("the config file is written");
}

fn visited_at(sandbox: &Sandbox, path: &Path, seconds: i64) {
    let canonical = paths::canonical(path)
        .expect("the recorded directory canonicalizes")
        .path;
    let updated = db(sandbox)
        .execute(
            "UPDATE visits SET ts = ?2
             WHERE dir_id = (SELECT id FROM dirs WHERE path = ?1)",
            params![canonical, seconds],
        )
        .expect("the visit timestamp is forced");
    assert_eq!(
        updated,
        1,
        "exactly one visit belongs to {}",
        path.display()
    );
}

fn first_seen_at(sandbox: &Sandbox, path: &Path, seconds: i64) {
    let canonical = paths::canonical(path)
        .expect("the recorded directory canonicalizes")
        .path;
    let updated = db(sandbox)
        .execute(
            "UPDATE dirs SET first_seen = ?2 WHERE path = ?1",
            params![canonical, seconds],
        )
        .expect("the first_seen timestamp is forced");
    assert_eq!(updated, 1, "exactly one dir row is {}", path.display());
}

fn missing_since_at(sandbox: &Sandbox, path: &Path, seconds: i64) {
    let canonical = paths::canonical(path)
        .expect("the recorded directory canonicalizes")
        .path;
    let updated = db(sandbox)
        .execute(
            "UPDATE dirs SET missing_since = ?2 WHERE path = ?1",
            params![canonical, seconds],
        )
        .expect("the missing_since timestamp is forced");
    assert_eq!(updated, 1, "exactly one dir row is {}", path.display());
}

fn list(sandbox: &Sandbox, all: bool) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("list");
    if all {
        cmd.arg("--all");
    }
    run(&mut cmd)
}

fn local_time(conn: &Connection, seconds: i64) -> String {
    conn.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S', ?1, 'unixepoch', 'localtime')",
        params![seconds],
        |row| row.get(0),
    )
    .expect("the timestamp formats in local time")
}

fn ambiguous_world() -> (Sandbox, String, String) {
    let world = sandbox(&["aaa/tokio", "zzz/tokio"]);
    let far = world.child("zzz").join("tokio");
    let near = world.child("aaa").join("tokio");
    assert!(add(&world, &far, "session-1", None, None).status.success());
    assert!(add(&world, &near, "session-1", None, None).status.success());
    let first = paths::canonical(&near)
        .expect("the first menu entry canonicalizes")
        .path;
    let second = paths::canonical(&far)
        .expect("the second menu entry canonicalizes")
        .path;
    (world, first, second)
}

fn expected_menu(first: &str, second: &str) -> String {
    format!("Choose a directory:\n  1) {first}\n  2) {second}\nEnter to confirm, Esc to cancel\n")
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
fn add_rejects_a_file_path() {
    let world = sandbox(&[]);
    let file = world.tree.path().join("readme.txt");
    std::fs::write(&file, b"content").expect("the scratch file is written");
    let out = add(&world, &file, "session-1", None, None);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
    assert!(!world.data.path().join("furet.db").exists());
}

#[test]
fn query_with_no_recorded_directory_and_no_fallback_hit_fails_on_stderr() {
    let world = sandbox(&["tokio"]);
    // WHY: an isolated cwd keeps the fallback ancestor walk off the shared OS temp dir.
    let cwd = world.tree.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("the isolated cwd exists");
    let out = query(&world, "zigzagnonexistent", &cwd, false);
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
    // WHY: an isolated cwd keeps the fallback ancestor walk off the shared OS temp dir.
    let cwd = world.tree.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("the isolated cwd exists");
    let out = query(&world, "zigzagnonexistent", &cwd, true);
    assert!(out.status.success());
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn query_with_a_stage_two_tie_prints_the_menu_on_stderr_only() {
    let (world, first, second) = ambiguous_world();
    let out = query_answering(&world, "tokoi", world.tree.path(), "nope\n");
    assert!(!out.status.success());
    assert_eq!(text(&out.stdout), "");
    assert_eq!(
        text(&out.stderr),
        format!(
            "{}furet: no directory selected\n",
            expected_menu(&first, &second)
        )
    );
}

#[test]
fn query_menu_prints_the_selected_path_and_exits_zero() {
    let (world, first, second) = ambiguous_world();
    let picked_first = query_answering(&world, "tokoi", world.tree.path(), "1\n");
    assert!(
        picked_first.status.success(),
        "stderr: {}",
        text(&picked_first.stderr)
    );
    assert_eq!(text(&picked_first.stdout), format!("{first}\n"));
    let picked_second = query_answering(&world, "tokoi", world.tree.path(), "2\n");
    assert!(picked_second.status.success());
    assert_eq!(text(&picked_second.stdout), format!("{second}\n"));
    assert!(text(&picked_second.stderr).starts_with("Choose a directory:\n"));
}

#[test]
fn query_menu_cancels_on_an_out_of_range_number() {
    let (world, _, _) = ambiguous_world();
    let out = query_answering(&world, "tokoi", world.tree.path(), "3\n");
    assert!(!out.status.success());
    assert_eq!(text(&out.stdout), "");
    assert!(text(&out.stderr).contains("no directory selected"));
}

#[test]
fn query_menu_cancels_on_an_empty_answer_and_on_no_answer_at_all() {
    let (world, _, _) = ambiguous_world();
    let empty_line = query_answering(&world, "tokoi", world.tree.path(), "\n");
    assert!(!empty_line.status.success());
    assert_eq!(text(&empty_line.stdout), "");
    let eof = query_answering(&world, "tokoi", world.tree.path(), "");
    assert!(!eof.status.success());
    assert_eq!(text(&eof.stdout), "");
    assert!(text(&eof.stderr).contains("no directory selected"));
}

#[test]
fn query_list_dumps_a_tie_without_asking_anything() {
    let (world, first, second) = ambiguous_world();
    let out = query(&world, "tokoi", world.tree.path(), true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("{first}\n{second}\n"));
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn query_never_returns_the_current_directory() {
    let world = sandbox(&["tokyo", "tokio"]);
    let tokyo = world.child("tokyo");
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokyo, "session-1", None, None)
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
    let tokyo_line = paths::canonical(&tokyo)
        .expect("the runner-up canonicalizes")
        .path;
    assert_eq!(text(&from_inside.stdout), format!("{tokyo_line}\n"));
}

#[test]
fn a_directory_deleted_from_disk_is_soft_deleted_then_reactivated_by_readding() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    // WHY: an isolated cwd keeps the fallback ancestor walk off the shared OS temp dir.
    let cwd = world.tree.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("the isolated cwd exists");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let conn = db(&world);
    let original_ts: i64 = conn
        .query_row("SELECT ts FROM visits ORDER BY id LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("the original visit reads back");
    std::fs::remove_dir_all(&tokio).expect("the recorded directory vanishes from disk");
    let out = query(&world, "tokio", &cwd, true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "");
    let missing_since: Option<i64> = conn
        .query_row("SELECT missing_since FROM dirs", [], |row| row.get(0))
        .expect("the dirs row reads back after the query");
    assert!(
        missing_since.is_some(),
        "a vanished directory must be soft-deleted by the query"
    );
    std::fs::create_dir_all(&tokio).expect("the directory reappears on disk");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let reactivated: Option<i64> = conn
        .query_row("SELECT missing_since FROM dirs", [], |row| row.get(0))
        .expect("the dirs row reads back after the re-add");
    assert_eq!(
        reactivated, None,
        "a successful add must reactivate the row"
    );
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 2);
    let first_ts: i64 = conn
        .query_row("SELECT ts FROM visits ORDER BY id LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("the original visit still reads back");
    assert_eq!(
        first_ts, original_ts,
        "the original visit must survive the vanish-and-return cycle"
    );
    let out = query(&world, "tokio", &cwd, true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&tokio)
        .expect("the reactivated directory canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
}

#[test]
fn a_reappeared_directory_is_reactivated_by_the_next_query_alone() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    // WHY: an isolated cwd keeps the fallback ancestor walk off the shared OS temp dir.
    let cwd = world.tree.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("the isolated cwd exists");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    std::fs::remove_dir_all(&tokio).expect("the recorded directory vanishes from disk");
    let vanished = query(&world, "tokio", &cwd, true);
    assert!(
        vanished.status.success(),
        "stderr: {}",
        text(&vanished.stderr)
    );
    assert_eq!(text(&vanished.stdout), "");
    std::fs::create_dir_all(&tokio).expect("the directory reappears on disk");
    let out = query(&world, "tokio", &cwd, true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&tokio)
        .expect("the reactivated directory canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
    let conn = db(&world);
    let missing_since: Option<i64> = conn
        .query_row("SELECT missing_since FROM dirs", [], |row| row.get(0))
        .expect("the dirs row reads back");
    assert_eq!(
        missing_since, None,
        "the query itself must reactivate a reappeared directory"
    );
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM visits"),
        1,
        "reactivation must keep the original visit without adding one"
    );
}

#[test]
fn a_query_only_checks_the_directories_it_matches_on_disk() {
    let world = sandbox(&["tokio", "gone"]);
    let tokio = world.child("tokio");
    let gone = world.child("gone");
    let cwd = world.tree.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("the isolated cwd exists");
    for child in [&tokio, &gone] {
        assert!(add(&world, child, "session-1", None, None).status.success());
    }
    std::fs::remove_dir_all(&gone).expect("the unmatched directory vanishes");
    let out = query(&world, "tokio", &cwd, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let missing: i64 = scalar(
        &db(&world),
        "SELECT COUNT(*) FROM dirs WHERE missing_since IS NOT NULL",
    );
    assert_eq!(
        missing, 0,
        "a directory the query never matched stays unchecked"
    );
}

#[test]
fn a_vanished_best_match_is_soft_deleted_and_the_next_match_wins() {
    let world = sandbox(&["tokio", "tokio-old"]);
    let tokio = world.child("tokio");
    let tokio_old = world.child("tokio-old");
    let cwd = world.tree.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("the isolated cwd exists");
    for child in [&tokio, &tokio_old] {
        assert!(add(&world, child, "session-1", None, None).status.success());
    }
    let before = query(&world, "tokio", &cwd, true);
    let tokio_path = paths::canonical(&tokio)
        .expect("the best match canonicalizes")
        .path;
    let old_path = paths::canonical(&tokio_old)
        .expect("the runner-up canonicalizes")
        .path;
    assert_eq!(text(&before.stdout), format!("{tokio_path}\n{old_path}\n"));
    std::fs::remove_dir_all(&tokio).expect("the best match vanishes");
    let out = query(&world, "tokio", &cwd, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("{old_path}\n"));
    let flagged: i64 = scalar(
        &db(&world),
        "SELECT COUNT(*) FROM dirs WHERE missing_since IS NOT NULL",
    );
    assert_eq!(
        flagged, 1,
        "the vanished match is soft-deleted by the query"
    );
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
fn init_pwsh_records_a_real_session_visit_after_every_successful_query_including_fallback() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh"));
    let script = text(&out.stdout);

    let query_call = "$target = furet query -- $query";
    let query_pos = script
        .find(query_call)
        .expect("the general query dispatch line is present");
    assert_eq!(
        script.matches(query_call).count(),
        1,
        "there is exactly one general query dispatch site"
    );
    let after_query = &script[query_pos + query_call.len()..];
    let expected_tail = "\n    if ($LASTEXITCODE -ne 0) {\n        return\n    }\n    \
        Set-Location -LiteralPath $target\n    __furet_record $target $from 'jump'\n";
    assert!(
        after_query.starts_with(expected_tail),
        "the query dispatch must be immediately followed by an exit-code \
         check and then __furet_record $target $from 'jump', got: {}",
        &after_query[..expected_tail.len().min(after_query.len())]
    );

    let record_def = script
        .find("function global:__furet_record($target, $from, $source) {")
        .expect("__furet_record is defined");
    let record_body = &script[record_def..];
    assert!(
        record_body.contains(
            "furet add --session $global:__furet_session --source $source --from $from -- $target"
        ),
        "__furet_record must add the visit under the real session, source, and target"
    );
}

#[test]
fn init_pwsh_explain_prints_the_report_without_jumping_or_recording() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh"));
    let script = text(&out.stdout);

    let branch_open = "if ($FuretArgs -contains '--explain') {";
    let open_pos = script
        .find(branch_open)
        .expect("the explain branch opens on a whole-token test of --explain");
    let explain_call = "furet query --explain -- $query";
    let call_pos = script[open_pos..]
        .find(explain_call)
        .expect("the explain branch dispatches furet query --explain")
        + open_pos;
    assert_eq!(
        script.matches(explain_call).count(),
        1,
        "there is exactly one explain dispatch site"
    );
    let branch = &script[open_pos..call_pos];
    assert!(
        branch.contains("Where-Object { $_ -ne '--explain' }"),
        "the branch strips every --explain occurrence before rebuilding $query"
    );
    assert!(
        branch.contains("-replace '/', '\\'"),
        "the rebuilt query applies the same / to \\ replacement"
    );
    assert!(
        !branch.contains("Set-Location"),
        "the explain branch must never move the caller"
    );
    assert!(
        !branch.contains("__furet_record"),
        "the explain branch must never record a visit"
    );
    assert!(
        !branch.contains("$LASTEXITCODE"),
        "the explain branch returns whatever the exit code is"
    );
    let after_call = &script[call_pos + explain_call.len()..];
    let expected_tail = "\n        return\n    }\n";
    assert!(
        after_call.starts_with(expected_tail),
        "the explain dispatch must be immediately followed by a bare return, \
         got: {}",
        &after_call[..expected_tail.len().min(after_call.len())]
    );
}

#[test]
fn init_pwsh_explain_works_with_the_flag_first_in_the_argument_list() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh"));
    let script = text(&out.stdout);

    let branch_open = "if ($FuretArgs -contains '--explain') {";
    let open_pos = script.find(branch_open).expect(
        "the trigger is a membership test over every argument, so the \
                 flag is detected wherever it appears",
    );
    let explain_call = "furet query --explain -- $query";
    let call_pos = script[open_pos..]
        .find(explain_call)
        .expect("the explain dispatch is present")
        + open_pos;
    let branch = &script[open_pos..call_pos];
    assert!(
        !branch.contains("$FuretArgs["),
        "the branch must not slice the argument list by position"
    );
    assert!(
        !branch.contains("-Skip"),
        "the branch must not drop a fixed number of leading arguments"
    );
    assert!(
        branch.contains("Where-Object { $_ -ne '--explain' }"),
        "every occurrence is removed token by token, so flag-first reaches the \
         same dispatch as flag-last"
    );
}

#[test]
fn init_pwsh_treats_a_token_merely_containing_explain_as_a_plain_query() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh"));
    let script = text(&out.stdout);

    assert!(
        script.contains("if ($FuretArgs -contains '--explain') {"),
        "the trigger compares whole tokens, so foo--explain is not the flag"
    );
    assert!(
        script.contains("Where-Object { $_ -ne '--explain' }"),
        "the removal keeps tokens that merely contain --explain"
    );
    assert!(
        !script.contains("-replace '--explain'"),
        "the flag is never stripped by substring replacement"
    );
    let plain_join = "$query = ($FuretArgs -join ' ') -replace '/', '\\'";
    let join_pos = script.find(plain_join).expect(
        "without the flag the query is still the raw join of every \
                 argument",
    );
    assert!(
        script
            .find("if ($FuretArgs -contains '--explain') {")
            .expect("the explain branch opens")
            < join_pos,
        "the explain branch sits before any other dispatch"
    );
    let query_call = "$target = furet query -- $query";
    assert_eq!(
        script.matches(query_call).count(),
        1,
        "the general query dispatch is unchanged and still the only one"
    );
}

#[test]
fn init_pwsh_defines_fi_with_an_fzf_branch_and_a_console_menu_branch() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh"));
    let script = text(&out.stdout);
    assert!(script.contains("function global:fi "));
    assert!(script.contains("fzf"));
    assert!(script.contains("Choose a directory:"));
    assert!(script.contains("Enter to confirm, Esc to cancel"));
}

#[test]
fn version_prints_a_nonempty_string() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("--version"));
    assert!(out.status.success());
    assert!(!text(&out.stdout).trim().is_empty());
}

#[test]
fn query_finds_an_unindexed_directory_through_disk_fallback_and_records_it() {
    let world = sandbox(&["projects/tokio"]);
    let cwd = world.child("projects");
    let out = query(&world, "tokio", &cwd, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&cwd.join("tokio"))
        .expect("the fallback hit canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
    let conn = db(&world);
    let source: String = conn
        .query_row(
            "SELECT source FROM visits WHERE source = 'fallback'",
            [],
            |row| row.get(0),
        )
        .expect("a fallback visit row was recorded for the winning path");
    assert_eq!(source, "fallback");
}

#[test]
fn a_fallback_jump_then_back_returns_to_the_origin_directory() {
    let world = sandbox(&["origin", "projects/tokio"]);
    let origin = world.child("origin");
    assert!(
        add(&world, &origin, "session-1", None, None)
            .status
            .success()
    );
    let cwd = world.child("projects");
    let query_out = query(&world, "tokio", &cwd, false);
    assert!(
        query_out.status.success(),
        "stderr: {}",
        text(&query_out.stderr)
    );
    let target = text(&query_out.stdout).trim().to_owned();
    assert!(
        add(
            &world,
            Path::new(&target),
            "session-1",
            Some("jump"),
            Some(&origin)
        )
        .status
        .success()
    );
    let back_out = run(world.furet().arg("back").arg("--session").arg("session-1"));
    assert!(
        back_out.status.success(),
        "stderr: {}",
        text(&back_out.stderr)
    );
    let expected = paths::canonical(&origin)
        .expect("the origin directory canonicalizes")
        .path;
    assert_eq!(text(&back_out.stdout), format!("{expected}\n"));
}

#[test]
fn query_list_color_wraps_every_path_in_the_ls_colors_directory_code() {
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
    let out = run(world
        .furet()
        .arg("query")
        .arg("tok")
        .arg("--list")
        .arg("--color")
        .env("LS_COLORS", "di=01;34:ln=01;36")
        .current_dir(world.tree.path()));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let first = paths::canonical(&tokio)
        .expect("the best match canonicalizes")
        .path;
    let second = paths::canonical(&stock)
        .expect("the second match canonicalizes")
        .path;
    assert_eq!(
        text(&out.stdout),
        format!("\x1b[01;34m{first}\x1b[0m\n\x1b[01;34m{second}\x1b[0m\n")
    );
}

#[test]
fn query_list_color_is_a_noop_without_ls_colors() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let colored = run(world
        .furet()
        .arg("query")
        .arg("tokio")
        .arg("--list")
        .arg("--color")
        .env_remove("LS_COLORS")
        .current_dir(world.tree.path()));
    let plain = query(&world, "tokio", world.tree.path(), true);
    assert!(colored.status.success());
    assert_eq!(text(&colored.stdout), text(&plain.stdout));
}

#[test]
fn query_list_with_an_empty_query_lists_by_recency_then_breaks_a_tie_on_path() {
    let world = sandbox(&["tokio", "tokei", "bbb", "aaa"]);
    let tokio = world.child("tokio");
    let tokei = world.child("tokei");
    let bbb = world.child("bbb");
    let aaa = world.child("aaa");
    for child in [&tokio, &tokei, &bbb, &aaa] {
        assert!(add(&world, child, "session-1", None, None).status.success());
    }
    visited_at(&world, &tokio, 1_700_000_003);
    visited_at(&world, &tokei, 1_700_000_002);
    visited_at(&world, &bbb, 1_700_000_001);
    visited_at(&world, &aaa, 1_700_000_001);
    let out = query(&world, "", world.tree.path(), true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let lines: Vec<String> = [&tokio, &tokei, &aaa, &bbb]
        .iter()
        .map(|path| {
            paths::canonical(path)
                .expect("the recorded directory canonicalizes")
                .path
        })
        .collect();
    assert_eq!(text(&out.stdout), format!("{}\n", lines.join("\n")));
}

#[test]
fn query_list_without_a_query_argument_lists_by_recency_like_an_empty_query() {
    let world = sandbox(&["tokio", "tokei"]);
    let tokio = world.child("tokio");
    let tokei = world.child("tokei");
    for child in [&tokio, &tokei] {
        assert!(add(&world, child, "session-1", None, None).status.success());
    }
    visited_at(&world, &tokio, 1_700_000_001);
    visited_at(&world, &tokei, 1_700_000_002);
    let mut cmd = world.furet();
    cmd.arg("query")
        .arg("--list")
        .arg("--color")
        .current_dir(world.tree.path());
    let bare = run(&mut cmd);
    assert!(bare.status.success(), "stderr: {}", text(&bare.stderr));
    let empty = query(&world, "", world.tree.path(), true);
    assert_eq!(text(&bare.stdout), text(&empty.stdout));
    assert_eq!(text(&bare.stdout).lines().count(), 2);
}

#[test]
fn query_list_with_an_empty_query_excludes_missing_and_current_directories() {
    let world = sandbox(&["tokio", "gone"]);
    let tokio = world.child("tokio");
    let gone = world.child("gone");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(add(&world, &gone, "session-1", None, None).status.success());
    std::fs::remove_dir_all(&gone).expect("the recorded directory vanishes");
    let out = query(&world, "", &tokio, true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn query_with_an_empty_query_and_no_list_fails_exactly_as_before() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = query(&world, "", world.tree.path(), false);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "furet: no directory matches ''\n"
    );
}

#[test]
fn a_jumping_query_records_its_stage_outcome_and_result_dir_id() {
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
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
    let expected = paths::canonical(&tokio)
        .expect("the winning candidate canonicalizes")
        .path;
    let (query_text, stage, outcome, result_path): (String, String, String, String) = conn
        .query_row(
            "SELECT query, stage, outcome, (SELECT path FROM dirs WHERE id = result_dir_id) FROM queries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the single queries row reads back");
    assert_eq!(query_text, "tokio");
    assert_eq!(stage, "1");
    assert_eq!(outcome, "jump");
    assert_eq!(result_path, expected);
}

#[test]
fn query_list_records_no_queries_row() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = query(&world, "tokio", world.tree.path(), true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn query_explain_records_no_queries_row() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = run(world
        .furet()
        .arg("query")
        .arg("tokio")
        .arg("--explain")
        .current_dir(world.tree.path()));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn the_bare_empty_query_without_list_regression_case_records_no_queries_row() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = query(&world, "", world.tree.path(), false);
    assert!(!out.status.success());
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn queries_without_failures_fails_on_stderr() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("queries"));
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn queries_failures_with_an_empty_journal_prints_nothing_and_exits_zero() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("queries").arg("--failures"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn queries_failures_prints_exactly_the_probable_failure_and_ignores_the_benign_jump() {
    let world = sandbox(&["tokio", "helix"]);
    let tokio = world.child("tokio");
    let helix = world.child("helix");
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
    let conn = db(&world);
    let tokio_id: i64 = conn
        .query_row(
            "SELECT id FROM dirs WHERE path = ?1",
            params![paths::canonical(&tokio).expect("tokio canonicalizes").path],
            |row| row.get(0),
        )
        .expect("the tokio dir row reads back");
    let helix_id: i64 = conn
        .query_row(
            "SELECT id FROM dirs WHERE path = ?1",
            params![paths::canonical(&helix).expect("helix canonicalizes").path],
            |row| row.get(0),
        )
        .expect("the helix dir row reads back");
    conn.execute(
        "INSERT INTO queries (ts, cwd, query, result_dir_id, stage, outcome)
         VALUES (1_700_000_000, 'c:\\dev', 'tok', ?1, '1', 'jump')",
        params![tokio_id],
    )
    .expect("the probable-failure query inserts");
    conn.execute(
        "INSERT INTO visits (dir_id, ts, source, session)
         VALUES (?1, 1_700_000_010, 'jump', 'session-1')",
        params![tokio_id],
    )
    .expect("the landing visit inserts");
    conn.execute(
        "INSERT INTO visits (dir_id, ts, source, session)
         VALUES (?1, 1_700_000_015, 'back', 'session-1')",
        params![helix_id],
    )
    .expect("the backtrack visit inserts");
    conn.execute(
        "INSERT INTO queries (ts, cwd, query, result_dir_id, stage, outcome)
         VALUES (1_700_001_000, 'c:\\dev', 'hel', ?1, '1', 'jump')",
        params![helix_id],
    )
    .expect("the benign query inserts");
    conn.execute(
        "INSERT INTO visits (dir_id, ts, source, session)
         VALUES (?1, 1_700_001_010, 'jump', 'session-1')",
        params![helix_id],
    )
    .expect("the benign landing visit inserts");
    let out = run(world.furet().arg("queries").arg("--failures"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stderr.is_empty());
    let tokio_path = paths::canonical(&tokio).expect("tokio canonicalizes").path;
    assert_eq!(
        text(&out.stdout),
        format!("c:\\dev\ttok\t{tokio_path}\tbacktrack\n")
    );
}

#[test]
fn list_on_an_empty_database_prints_nothing_and_exits_zero() {
    let world = sandbox(&[]);
    let out = list(&world, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn list_prints_path_visit_count_and_both_timestamps_tab_separated() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(&world, &tokio, "session-2", None, None)
            .status
            .success()
    );
    first_seen_at(&world, &tokio, 1_700_000_000);
    let updated = db(&world)
        .execute("UPDATE visits SET ts = 1_700_000_050", [])
        .expect("the visit timestamps are forced");
    assert_eq!(updated, 2, "both visits belong to tokio");
    let out = list(&world, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let conn = db(&world);
    let last = local_time(&conn, 1_700_000_050);
    let first = local_time(&conn, 1_700_000_000);
    let stamp = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$")
        .expect("the timestamp pattern compiles");
    for value in [&last, &first] {
        assert!(
            stamp.is_match(value),
            "'{value}' must be a local ISO timestamp"
        );
    }
    let expected_path = paths::canonical(&tokio)
        .expect("the recorded directory canonicalizes")
        .path;
    assert_eq!(
        text(&out.stdout),
        format!("{expected_path}\t2\t{last}\t{first}\n")
    );
}

#[test]
fn list_sorts_by_last_visit_descending() {
    let world = sandbox(&["alpha", "beta", "gamma"]);
    let alpha = world.child("alpha");
    let beta = world.child("beta");
    let gamma = world.child("gamma");
    for child in [&alpha, &beta, &gamma] {
        assert!(add(&world, child, "session-1", None, None).status.success());
    }
    visited_at(&world, &alpha, 1_700_000_001);
    visited_at(&world, &beta, 1_700_000_003);
    visited_at(&world, &gamma, 1_700_000_002);
    let out = list(&world, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected: Vec<String> = [&beta, &gamma, &alpha]
        .iter()
        .map(|path| {
            paths::canonical(path)
                .expect("the recorded directory canonicalizes")
                .path
        })
        .collect();
    let listed: Vec<String> = text(&out.stdout)
        .lines()
        .map(|line| line.split('\t').next().expect("a path field").to_owned())
        .collect();
    assert_eq!(listed, expected);
}

#[test]
fn list_breaks_a_last_visit_tie_by_path_ascending() {
    let world = sandbox(&["zzz", "aaa"]);
    let zzz = world.child("zzz");
    let aaa = world.child("aaa");
    assert!(add(&world, &zzz, "session-1", None, None).status.success());
    assert!(add(&world, &aaa, "session-1", None, None).status.success());
    visited_at(&world, &zzz, 1_700_000_042);
    visited_at(&world, &aaa, 1_700_000_042);
    let out = list(&world, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let first = paths::canonical(&aaa).expect("aaa canonicalizes").path;
    let second = paths::canonical(&zzz).expect("zzz canonicalizes").path;
    let stdout = text(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].split('\t').next(), Some(first.as_str()));
    assert_eq!(lines[1].split('\t').next(), Some(second.as_str()));
}

#[test]
fn list_excludes_missing_directories_by_default() {
    let world = sandbox(&["tokio", "gone"]);
    let tokio = world.child("tokio");
    let gone = world.child("gone");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(add(&world, &gone, "session-1", None, None).status.success());
    first_seen_at(&world, &tokio, 1_700_000_000);
    first_seen_at(&world, &gone, 1_700_000_000);
    visited_at(&world, &tokio, 1_700_000_002);
    visited_at(&world, &gone, 1_700_000_003);
    missing_since_at(&world, &gone, 1_700_000_100);
    let out = list(&world, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let conn = db(&world);
    let expected_path = paths::canonical(&tokio)
        .expect("the present directory canonicalizes")
        .path;
    let last = local_time(&conn, 1_700_000_002);
    let first = local_time(&conn, 1_700_000_000);
    assert_eq!(
        text(&out.stdout),
        format!("{expected_path}\t1\t{last}\t{first}\n")
    );
}

#[test]
fn list_all_includes_missing_directories_with_a_presence_column() {
    let world = sandbox(&["tokio", "gone"]);
    let tokio = world.child("tokio");
    let gone = world.child("gone");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(add(&world, &gone, "session-1", None, None).status.success());
    first_seen_at(&world, &tokio, 1_700_000_000);
    first_seen_at(&world, &gone, 1_700_000_000);
    visited_at(&world, &tokio, 1_700_000_002);
    visited_at(&world, &gone, 1_700_000_003);
    missing_since_at(&world, &gone, 1_700_000_100);
    let out = list(&world, true);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let conn = db(&world);
    let gone_path = paths::canonical(&gone)
        .expect("the missing directory canonicalizes")
        .path;
    let tokio_path = paths::canonical(&tokio)
        .expect("the present directory canonicalizes")
        .path;
    let first = local_time(&conn, 1_700_000_000);
    let gone_last = local_time(&conn, 1_700_000_003);
    let tokio_last = local_time(&conn, 1_700_000_002);
    assert_eq!(
        text(&out.stdout),
        format!(
            "{gone_path}\t1\t{gone_last}\t{first}\tmissing\n{tokio_path}\t1\t{tokio_last}\t{first}\tpresent\n"
        )
    );
}

#[test]
fn list_writes_nothing_to_stderr_on_success() {
    let world = sandbox(&["tokio", "gone"]);
    let tokio = world.child("tokio");
    let gone = world.child("gone");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(add(&world, &gone, "session-1", None, None).status.success());
    missing_since_at(&world, &gone, 1_700_000_100);
    let plain = list(&world, false);
    let all = list(&world, true);
    assert!(plain.status.success(), "stderr: {}", text(&plain.stderr));
    assert!(all.status.success(), "stderr: {}", text(&all.stderr));
    assert!(
        plain.stderr.is_empty(),
        "plain list writes nothing to stderr: {}",
        text(&plain.stderr)
    );
    assert!(
        all.stderr.is_empty(),
        "list --all writes nothing to stderr: {}",
        text(&all.stderr)
    );
}

#[test]
fn help_prints_the_database_file_path_resolved_at_runtime() {
    let world = sandbox(&[]);
    let expected = world.data.path().join("furet.db");
    for flag in ["-h", "--help"] {
        let out = run(world.furet().arg(flag));
        assert!(out.status.success(), "stderr: {}", text(&out.stderr));
        let help = text(&out.stdout);
        assert!(
            help.contains("Database file:"),
            "the top-level {flag} output must name the database file: {help}"
        );
        assert!(
            help.contains(expected.to_string_lossy().as_ref()),
            "the top-level {flag} output must show the resolved path {}: {help}",
            expected.display()
        );
        assert!(
            help.contains(&format!(
                "Config file: {} (not found, defaults apply)",
                world.data.path().join("config.toml").display()
            )),
            "the top-level {flag} output must name the missing config file: {help}"
        );
        assert!(
            help.contains(&format!(
                "Log directory: {}",
                world.data.path().join("logs").display()
            )),
            "the top-level {flag} output must name the log directory: {help}"
        );
    }
    write_config(&world, "typo_min_length = 5\n");
    let out = run(world.furet().arg("--help"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let help = text(&out.stdout);
    assert!(
        help.contains(&format!(
            "Config file: {} (found)",
            world.data.path().join("config.toml").display()
        )),
        "a written config.toml must flip the trailer to (found): {help}"
    );
}

#[test]
fn add_writes_a_log_file_under_the_data_dir() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    let out = add(&world, &tokio, "session-1", None, None);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let log = log_contents(&world);
    let canonical = paths::canonical(&tokio)
        .expect("the recorded directory canonicalizes")
        .path;
    assert!(
        log.contains("INFO") && log.contains(&canonical),
        "the log must contain an INFO line with the recorded path {canonical}: {log:?}"
    );
}

#[test]
fn a_failing_query_logs_an_error_line() {
    let world = sandbox(&[]);
    let out = query(&world, "nothing", world.tree.path(), false);
    assert!(!out.status.success(), "nothing matches an empty database");
    assert!(out.stdout.is_empty());
    let log = log_contents(&world);
    let errors: Vec<&str> = log.lines().filter(|line| line.contains("ERROR")).collect();
    assert_eq!(errors.len(), 1, "exactly one ERROR line, got: {log:?}");
    assert!(
        errors[0].contains("no directory matches 'nothing'"),
        "the ERROR line carries the failure message: {:?}",
        errors[0]
    );
}

#[test]
fn furet_log_env_var_enables_debug_lines() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    let quiet = add(&world, &tokio, "session-1", None, None);
    assert!(quiet.status.success(), "stderr: {}", text(&quiet.stderr));
    assert!(
        !log_contents(&world).contains("DEBUG"),
        "no DEBUG line without FURET_LOG"
    );
    let verbose = run(world
        .furet()
        .env("FURET_LOG", "debug")
        .arg("add")
        .arg(&tokio)
        .arg("--session")
        .arg("session-2"));
    assert!(
        verbose.status.success(),
        "stderr: {}",
        text(&verbose.stderr)
    );
    assert!(
        log_contents(&world).contains("DEBUG"),
        "FURET_LOG=debug enables DEBUG lines: {:?}",
        log_contents(&world)
    );
}

#[test]
fn an_unwritable_log_dir_does_not_break_the_command() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    std::fs::write(world.data.path().join("logs"), b"not a directory")
        .expect("the blocker file named logs is written");
    let out = add(&world, &tokio, "session-1", None, None);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "add writes nothing to stdout");
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 1);
}

#[test]
fn logging_never_writes_to_stdout_or_stderr() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = run(world
        .furet()
        .env("FURET_LOG", "trace")
        .arg("query")
        .arg("tokio")
        .current_dir(world.tree.path()));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let target = paths::canonical(&tokio)
        .expect("the hit canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{target}\n"));
    assert!(out.stderr.is_empty(), "stderr: {}", text(&out.stderr));
}

#[test]
fn typo_min_length_from_config_disables_short_typo_queries() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let lenient = query(&world, "tokoi", world.tree.path(), false);
    assert!(
        lenient.status.success(),
        "stderr: {}",
        text(&lenient.stderr)
    );
    write_config(&world, "typo_min_length = 6");
    let strict = query(&world, "tokoi", world.tree.path(), false);
    assert!(!strict.status.success());
    assert!(strict.stdout.is_empty());
}

#[test]
fn fallback_up_from_config_climbs_more_ancestors() {
    let world = sandbox(&["a/b/current", "a/sibling"]);
    let current = world.child("a/b/current");
    let sibling = world.child("a/sibling");
    let without_override = query(&world, "sibling", &current, false);
    assert!(!without_override.status.success());
    write_config(&world, "[fallback]\nup = 2");
    let with_override = query(&world, "sibling", &current, false);
    assert!(
        with_override.status.success(),
        "stderr: {}",
        text(&with_override.stderr)
    );
    let expected = paths::canonical(&sibling)
        .expect("the grandparent's child canonicalizes")
        .path;
    assert_eq!(text(&with_override.stdout), format!("{expected}\n"));
}

#[test]
fn fallback_exclude_from_config_replaces_the_defaults() {
    let world = sandbox(&["current/target"]);
    let current = world.child("current");
    let target = world.child("current/target");
    let without_override = query(&world, "target", &current, false);
    assert!(!without_override.status.success());
    write_config(&world, "[fallback]\nexclude = [\"foo\"]");
    let with_override = query(&world, "target", &current, false);
    assert!(
        with_override.status.success(),
        "stderr: {}",
        text(&with_override.stderr)
    );
    let expected = paths::canonical(&target)
        .expect("the once-excluded directory canonicalizes")
        .path;
    assert_eq!(text(&with_override.stdout), format!("{expected}\n"));
}

#[test]
fn fallback_no_ignore_from_config_matches_the_flag() {
    let world = sandbox(&["current/ignored", "current/.git"]);
    let current = world.child("current");
    let ignored = world.child("current/ignored");
    std::fs::write(current.join(".gitignore"), "ignored\n")
        .expect("the fixture .gitignore is written");
    let respected = query(&world, "ignored", &current, false);
    assert!(!respected.status.success());
    write_config(&world, "[fallback]\nno_ignore = true");
    let overridden = query(&world, "ignored", &current, false);
    assert!(
        overridden.status.success(),
        "stderr: {}",
        text(&overridden.stderr)
    );
    let expected = paths::canonical(&ignored)
        .expect("the gitignored directory canonicalizes")
        .path;
    assert_eq!(text(&overridden.stdout), format!("{expected}\n"));
}

#[test]
fn an_invalid_value_warns_and_falls_back_to_the_default() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    write_config(&world, "typo_min_length = \"six\"");
    let out = query(&world, "tokio", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&tokio)
        .expect("the recorded directory canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
    let log = log_contents(&world);
    assert!(
        log.lines()
            .any(|line| line.contains("WARN") && line.contains("typo_min_length")),
        "{log:?}"
    );
}

#[test]
fn a_malformed_config_never_breaks_a_query() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    write_config(&world, "this is not [ valid toml");
    let out = query(&world, "tokio", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&tokio)
        .expect("the recorded directory canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
    assert!(
        log_contents(&world).contains("WARN"),
        "{}",
        log_contents(&world)
    );
}

#[test]
fn home_prints_the_configured_directory_canonicalized() {
    let world = sandbox(&["home"]);
    let home = world.child("home");
    let forward = home.to_string_lossy().replace('\\', "/");
    write_config(&world, &format!("home = \"{forward}\""));
    let out = run(world.furet().arg("home"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&home)
        .expect("the configured home canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
}

#[test]
fn home_prints_nothing_and_exits_zero_when_unset() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("home"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
}

#[test]
fn home_prints_nothing_and_warns_when_the_directory_is_missing() {
    let world = sandbox(&[]);
    let missing = world.child("nope");
    let forward = missing.to_string_lossy().replace('\\', "/");
    write_config(&world, &format!("home = \"{forward}\""));
    let out = run(world.furet().arg("home"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    let log = log_contents(&world);
    assert!(
        log.lines()
            .any(|line| line.contains("WARN") && line.contains("home")),
        "{log:?}"
    );
}

#[test]
fn home_prints_nothing_and_warns_when_home_is_a_file() {
    let world = sandbox(&[]);
    let file = world.tree.path().join("readme.txt");
    std::fs::write(&file, b"content").expect("the scratch file is written");
    let forward = file.to_string_lossy().replace('\\', "/");
    write_config(&world, &format!("home = \"{forward}\""));
    let out = run(world.furet().arg("home"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    let log = log_contents(&world);
    assert!(
        log.lines()
            .any(|line| line.contains("WARN") && line.contains("home")),
        "{log:?}"
    );
}

#[test]
fn home_prints_nothing_and_warns_on_a_relative_path() {
    let world = sandbox(&[]);
    write_config(&world, "home = \"relative/path\"");
    let out = run(world.furet().arg("home"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    let log = log_contents(&world);
    assert!(
        log.lines()
            .any(|line| line.contains("WARN") && line.contains("home")),
        "{log:?}"
    );
}

#[test]
fn import_zoxide_records_directories_with_the_import_source_and_session() {
    let world = sandbox(&["tokei", "tokio"]);
    let tokei = world.child("tokei");
    let tokio = world.child("tokio");
    let stdin = format!(
        "12.5 {}\n3 {}\n",
        tokei.to_string_lossy(),
        tokio.to_string_lossy()
    );
    let out = import_zoxide(&world, &stdin);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).starts_with("imported 2, skipped 0"));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 2);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 2);
    assert_eq!(
        scalar(
            &conn,
            "SELECT COUNT(*) FROM visits WHERE source = 'import' AND session = 'import'"
        ),
        2
    );
}

#[test]
fn importing_the_same_zoxide_export_twice_imports_nothing_the_second_time() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    let stdin = format!("12.5 {}\n", tokio.to_string_lossy());
    let first = import_zoxide(&world, &stdin);
    assert!(first.status.success(), "stderr: {}", text(&first.stderr));
    assert!(text(&first.stderr).starts_with("imported 1, skipped 0"));
    let second = import_zoxide(&world, &stdin);
    assert!(second.status.success(), "stderr: {}", text(&second.stderr));
    assert!(text(&second.stderr).starts_with("imported 0, skipped 1 (known 1"));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 1);
}

#[test]
fn case_variant_lines_for_the_same_directory_collapse_into_one_import() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    let path = tokio.to_string_lossy().into_owned();
    let stdin = format!("3 {}\n9 {}\n", path.to_lowercase(), path.to_uppercase());
    let out = import_zoxide(&world, &stdin);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr),
        "imported 1, skipped 1 (known 0, not a directory 0, malformed 0, duplicate 1)\n"
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 1);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
}

#[test]
fn a_directory_already_visited_through_add_is_skipped_and_keeps_its_visits() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let conn = db(&world);
    let original_ts: i64 = conn
        .query_row("SELECT ts FROM visits", [], |row| row.get(0))
        .expect("the original visit reads back");
    let stdin = format!("12.5 {}\n", tokio.to_string_lossy());
    let out = import_zoxide(&world, &stdin);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(text(&out.stderr).starts_with("imported 0, skipped 1 (known 1"));
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    let (source, ts): (String, i64) = conn
        .query_row("SELECT source, ts FROM visits", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("the untouched visit reads back");
    assert_eq!(source, "hook");
    assert_eq!(ts, original_ts);
}

#[test]
fn a_missing_path_a_file_and_a_malformed_line_are_each_skipped_and_counted() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    let file = world.tree.path().join("readme.txt");
    std::fs::write(&file, b"content").expect("the scratch file is written");
    let missing = world.tree.path().join("nope");
    let stdin = format!(
        "12.5 {}\n3 {}\n2 {}\nnot-a-score {}\n",
        tokio.to_string_lossy(),
        missing.to_string_lossy(),
        file.to_string_lossy(),
        tokio.to_string_lossy(),
    );
    let out = import_zoxide(&world, &stdin);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr),
        "imported 1, skipped 3 (known 0, not a directory 2, malformed 1, duplicate 0)\n"
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn importing_empty_stdin_exits_zero_and_imports_nothing() {
    let world = sandbox(&[]);
    let out = import_zoxide(&world, "");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert_eq!(
        text(&out.stderr),
        "imported 0, skipped 0 (known 0, not a directory 0, malformed 0, duplicate 0)\n"
    );
}

#[test]
fn an_imported_directory_is_reachable_by_query() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    let stdin = format!("12.5 {}\n", tokio.to_string_lossy());
    assert!(import_zoxide(&world, &stdin).status.success());
    let out = query(&world, "tokio", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&tokio)
        .expect("the imported directory canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
}

#[test]
fn no_config_file_writes_no_warning() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = query(&world, "tokio", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(
        !log_contents(&world).contains("WARN"),
        "{}",
        log_contents(&world)
    );
}
