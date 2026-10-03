// WHY: this whole file is layer-3 test code, where expect() is the norm.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

use assert_cmd::Command;
use furet::paths;
use furet::project;
use rusqlite::{Connection, params};
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

fn add_with_query(
    sandbox: &Sandbox,
    path: &Path,
    session: &str,
    from: &Path,
    query: &str,
    source: Option<&str>,
) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("add")
        .arg(path)
        .arg("--session")
        .arg(session)
        .arg("--from")
        .arg(from)
        .arg("--query")
        .arg(query);
    if let Some(source) = source {
        cmd.arg("--source").arg(source);
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

fn query_with(sandbox: &Sandbox, args: &[&str], cwd: &Path) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("query");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.current_dir(cwd);
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

fn visit_source_at(sandbox: &Sandbox, path: &Path, source: &str, seconds: i64) {
    let canonical = paths::canonical(path)
        .expect("the recorded directory canonicalizes")
        .path;
    let updated = db(sandbox)
        .execute(
            "UPDATE visits SET ts = ?3
         WHERE source = ?2 AND dir_id = (SELECT id FROM dirs WHERE path = ?1)",
            params![canonical, source, seconds],
        )
        .expect("the visit timestamp is forced");
    assert_eq!(
        updated,
        1,
        "exactly one {source} visit belongs to {}",
        path.display()
    );
}

fn dir_row_id(sandbox: &Sandbox, path: &str) -> i64 {
    db(sandbox)
        .query_row(
            "SELECT id FROM dirs WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )
        .expect("the dir row reads back")
}

fn journal_row(
    sandbox: &Sandbox,
    ts: i64,
    query: &str,
    result_path: Option<&str>,
    stage: &str,
    outcome: &str,
) {
    let result_dir_id = result_path.map(|path| dir_row_id(sandbox, path));
    db(sandbox)
        .execute(
            "INSERT INTO queries (ts, cwd, query, result_dir_id, stage, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![ts, "c:\\dev", query, result_dir_id, stage, outcome],
        )
        .expect("the journal row inserts");
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the system clock reads after the epoch")
        .as_secs() as i64
}

fn stats(sandbox: &Sandbox, extra: &[&str]) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("stats");
    for arg in extra {
        cmd.arg(arg);
    }
    run(&mut cmd)
}

fn list(sandbox: &Sandbox, all: bool) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("list");
    if all {
        cmd.arg("--all");
    }
    run(&mut cmd)
}

fn list_with(sandbox: &Sandbox, extra: &[&str]) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("list");
    for arg in extra {
        cmd.arg(arg);
    }
    run(&mut cmd)
}

fn remove(sandbox: &Sandbox, pattern: &str, cwd: &Path) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("remove").arg(pattern).current_dir(cwd);
    run(&mut cmd)
}

fn remove_answering(sandbox: &Sandbox, pattern: &str, cwd: &Path, answer: &str) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("remove")
        .arg(pattern)
        .arg("--confirm")
        .current_dir(cwd)
        .write_stdin(answer);
    run(&mut cmd)
}

fn remove_with(
    sandbox: &Sandbox,
    pattern: &str,
    cwd: &Path,
    flags: &[&str],
    answer: &str,
) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("remove").arg(pattern).current_dir(cwd);
    for flag in flags {
        cmd.arg(flag);
    }
    cmd.write_stdin(answer);
    run(&mut cmd)
}

fn remove_missing(
    sandbox: &Sandbox,
    pattern: Option<&str>,
    cwd: &Path,
    flags: &[&str],
    answer: &str,
) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("remove");
    if let Some(pattern) = pattern {
        cmd.arg(pattern);
    }
    for flag in flags {
        cmd.arg(flag);
    }
    cmd.current_dir(cwd).write_stdin(answer);
    run(&mut cmd)
}

fn alias(sandbox: &Sandbox, args: &[&str], cwd: &Path) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("alias");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.current_dir(cwd);
    run(&mut cmd)
}

fn mark(sandbox: &Sandbox, args: &[&str], cwd: &Path) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("mark");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.current_dir(cwd);
    run(&mut cmd)
}

fn missing_since_values(conn: &Connection) -> Vec<i64> {
    let mut statement = conn
        .prepare("SELECT COALESCE(missing_since, -1) FROM dirs ORDER BY path")
        .expect("the missing_since query prepares");
    statement
        .query_map([], |row| row.get(0))
        .expect("the missing_since rows read")
        .collect::<Result<Vec<i64>, _>>()
        .expect("the missing_since values collect")
}

fn dry_run_state(conn: &Connection) -> (i64, i64, i64, Vec<i64>) {
    (
        scalar(conn, "SELECT COUNT(*) FROM dirs"),
        scalar(conn, "SELECT COUNT(*) FROM visits"),
        scalar(conn, "SELECT COUNT(*) FROM queries"),
        missing_since_values(conn),
    )
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
    // WHY: query memory would send the second query to the first pick without a menu.
    write_config(&world, "query_memory = false");
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

fn single_query_row(sandbox: &Sandbox) -> (i64, String, String, Option<String>, String, String) {
    db(sandbox)
        .query_row(
            "SELECT ts, cwd, query, (SELECT path FROM dirs WHERE id = result_dir_id), stage, outcome FROM queries",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .expect("the single queries row reads back")
}

#[test]
fn query_menu_choice_journals_a_pick_with_the_chosen_directory() {
    let (world, first, second) = ambiguous_world();
    let out = query_answering(&world, "tokoi", world.tree.path(), "2\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("{second}\n"));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
    let (_, _, query_text, result_path, stage, outcome) = single_query_row(&world);
    assert_eq!(query_text, "tokoi");
    assert_eq!(stage, "menu");
    assert_eq!(outcome, "pick");
    assert_eq!(result_path.as_deref(), Some(second.as_str()));
    assert_ne!(result_path.as_deref(), Some(first.as_str()));
}

#[test]
fn query_menu_cancel_still_journals_menu_without_a_result() {
    for answer in ["\n", ""] {
        let (world, _, _) = ambiguous_world();
        let out = query_answering(&world, "tokoi", world.tree.path(), answer);
        assert!(!out.status.success(), "answer: {answer:?}");
        let conn = db(&world);
        assert_eq!(
            scalar(&conn, "SELECT COUNT(*) FROM queries"),
            1,
            "answer: {answer:?}"
        );
        let (_, _, _, result_path, stage, outcome) = single_query_row(&world);
        assert_eq!(stage, "menu", "answer: {answer:?}");
        assert_eq!(outcome, "menu", "answer: {answer:?}");
        assert_eq!(result_path, None, "answer: {answer:?}");
    }
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
fn add_purges_visits_and_queries_older_than_the_retention() {
    let world = sandbox(&["tokio", "cwd"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        query(&world, "tokio", &world.child("cwd"), false)
            .status
            .success()
    );
    let conn = db(&world);
    conn.execute_batch("UPDATE visits SET ts = 1000; UPDATE queries SET ts = 1000;")
        .expect("the journal is aged past the retention");
    write_config(&world, "retention_days = 30");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM visits WHERE ts = 1000"),
        0
    );
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn a_zero_retention_keeps_every_visit_and_query() {
    let world = sandbox(&["tokio", "cwd"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(
        query(&world, "tokio", &world.child("cwd"), false)
            .status
            .success()
    );
    let conn = db(&world);
    conn.execute_batch("UPDATE visits SET ts = 1000; UPDATE queries SET ts = 1000;")
        .expect("the journal is aged past any retention");
    write_config(&world, "retention_days = 0");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 2);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
}

#[test]
fn add_skips_a_directory_matching_an_excluded_name() {
    let world = sandbox(&["zzskip", "zzskip/sub", "keep"]);
    write_config(&world, "exclude_dirs = ['zzskip']");
    let skipped = add(&world, &world.child("zzskip"), "session-1", None, None);
    assert!(skipped.status.success());
    assert!(skipped.stdout.is_empty());
    assert!(skipped.stderr.is_empty());
    let nested = add(&world, &world.child("zzskip/sub"), "session-1", None, None);
    assert!(nested.status.success());
    assert!(nested.stdout.is_empty());
    assert!(nested.stderr.is_empty());
    let listed = list_with(&world, &["--all", "--paths"]);
    assert!(listed.status.success(), "stderr: {}", text(&listed.stderr));
    assert!(text(&listed.stdout).is_empty());
    assert!(
        add(&world, &world.child("keep"), "session-1", None, None)
            .status
            .success()
    );
    assert_eq!(
        text(&list_with(&world, &["--paths"]).stdout),
        format!("{}\n", canonical_child(&world, "keep"))
    );
}

#[test]
fn add_skips_a_directory_matching_an_absolute_excluded_pattern() {
    let world = sandbox(&["zz", "zz/a"]);
    let root = paths::canonical(world.tree.path())
        .expect("the sandbox root canonicalizes")
        .path;
    write_config(&world, &format!("exclude_dirs = ['{}\\zz\\*']", root));
    let skipped = add(&world, &world.child("zz/a"), "session-1", None, None);
    assert!(skipped.status.success());
    assert!(skipped.stdout.is_empty());
    assert!(
        add(&world, &world.child("zz"), "session-1", None, None)
            .status
            .success()
    );
    assert_eq!(
        text(&list_with(&world, &["--paths"]).stdout),
        format!("{}\n", canonical_child(&world, "zz"))
    );
}

#[test]
fn add_skips_a_directory_matching_a_star_prefixed_pattern() {
    let world = sandbox(&["zz/a", "zz/b"]);
    write_config(&world, "exclude_dirs = ['*\\zz\\a']");
    let skipped = add(&world, &world.child("zz/a"), "session-1", None, None);
    assert!(skipped.status.success());
    assert!(skipped.stdout.is_empty());
    assert!(
        add(&world, &world.child("zz/b"), "session-1", None, None)
            .status
            .success()
    );
    assert_eq!(
        text(&list_with(&world, &["--paths"]).stdout),
        format!("{}\n", canonical_child(&world, "zz/b"))
    );
}

#[test]
fn an_already_known_directory_keeps_its_rows_but_gets_no_new_visit() {
    let world = sandbox(&["zzskip"]);
    assert!(
        add(&world, &world.child("zzskip"), "session-1", None, None)
            .status
            .success()
    );
    write_config(&world, "exclude_dirs = ['zzskip']");
    let again = add(&world, &world.child("zzskip"), "session-1", None, None);
    assert!(again.status.success());
    assert!(again.stdout.is_empty());
    assert!(again.stderr.is_empty());
    assert_eq!(
        text(&list_with(&world, &["--paths"]).stdout),
        format!("{}\n", canonical_child(&world, "zzskip"))
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 1);
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
fn init_pwsh_starts_with_the_clap_completion_block() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let script = text(&out.stdout);
    // WHY: clap's block opens with a blank line, kept byte-identical; pwsh only requires `using` ahead of other statements.
    let first = script
        .lines()
        .find(|line| !line.is_empty())
        .expect("a non-empty stdout line");
    assert_eq!(first, "using namespace System.Management.Automation");
    assert_eq!(
        script
            .matches("Register-ArgumentCompleter -Native -CommandName 'furet'")
            .count(),
        1,
        "the clap block registers the furet native completer exactly once"
    );
}

#[test]
fn init_pwsh_with_a_custom_cmd_keeps_completing_furet() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("init").arg("pwsh").arg("--cmd").arg("j"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let script = text(&out.stdout);
    assert_eq!(
        script
            .matches("Register-ArgumentCompleter -Native -CommandName 'furet'")
            .count(),
        1,
        "--cmd must not retarget clap's completer away from furet"
    );
    assert!(script.contains("function global:j "));
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
        .find("function global:__furet_record($target, $from, $source, $query) {")
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
    let explain_call = "furet query --explain @scope -- $query";
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
    let explain_call = "furet query --explain @scope -- $query";
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
fn a_fallback_jump_to_an_excluded_directory_jumps_but_records_nothing() {
    let world = sandbox(&["projects/tokio"]);
    write_config(&world, "exclude_dirs = ['tokio']");
    let cwd = world.child("projects");
    let out = query(&world, "tokio", &cwd, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&cwd.join("tokio"))
        .expect("the fallback hit canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 0);
    let result_dir_id: Option<i64> = conn
        .query_row("SELECT result_dir_id FROM queries", [], |row| row.get(0))
        .expect("the fallback query is journalled");
    assert_eq!(result_dir_id, None);
}

// WHY: wd/tokio and the sibling tokio tie in stage 2; path order puts the sibling first.
fn fallback_tie_world() -> (Sandbox, String, String) {
    let world = sandbox(&["tokio", "wd/tokio"]);
    let sibling = canonical_child(&world, "tokio");
    let nested = canonical_child(&world, "wd/tokio");
    (world, sibling, nested)
}

#[test]
fn query_fallback_menu_choice_journals_a_pick_with_stage_fallback() {
    let (world, sibling, nested) = fallback_tie_world();
    let cwd = world.child("wd");
    let out = query_answering(&world, "tokoi", &cwd, "2\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("{nested}\n"));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
    let (_, _, query_text, result_path, stage, outcome) = single_query_row(&world);
    assert_eq!(query_text, "tokoi");
    assert_eq!(stage, "fallback");
    assert_eq!(outcome, "pick");
    assert_eq!(result_path.as_deref(), Some(nested.as_str()));
    assert_ne!(result_path.as_deref(), Some(sibling.as_str()));
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM dirs"),
        1,
        "the chosen fallback directory gets its dirs row"
    );
    let (visit_source, visit_session): (String, String) = conn
        .query_row("SELECT source, session FROM visits", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("the fallback visit reads back");
    assert_eq!(visit_source, "fallback");
    assert_eq!(visit_session, "fallback");
}

#[test]
fn query_fallback_menu_choice_of_an_excluded_directory_journals_a_pick_without_result() {
    let (world, _sibling, nested) = fallback_tie_world();
    write_config(&world, "exclude_dirs = ['wd']");
    let cwd = world.child("wd");
    let out = query_answering(&world, "tokoi", &cwd, "2\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("{nested}\n"));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
    let (_, _, _, result_path, stage, outcome) = single_query_row(&world);
    assert_eq!(stage, "fallback");
    assert_eq!(outcome, "pick");
    assert_eq!(result_path, None);
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
fn add_query_journals_one_pick_row() {
    let world = sandbox(&["tokio", "origin"]);
    let tokio = world.child("tokio");
    let origin = world.child("origin");
    let out = add_with_query(&world, &tokio, "session-1", &origin, "Tok io", None);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
    let visit_ts: i64 = conn
        .query_row("SELECT ts FROM visits", [], |row| row.get(0))
        .expect("the visit reads back");
    let (ts, cwd, query_text, result_path, stage, outcome) = single_query_row(&world);
    assert_eq!(ts, visit_ts, "the pick row shares the visit's timestamp");
    let from_canonical = paths::canonical(&origin)
        .expect("the origin canonicalizes")
        .path;
    assert_eq!(cwd, from_canonical);
    assert_eq!(query_text, "Tok io");
    let tokio_canonical = paths::canonical(&tokio)
        .expect("the recorded directory canonicalizes")
        .path;
    assert_eq!(result_path.as_deref(), Some(tokio_canonical.as_str()));
    assert_eq!(stage, "menu");
    assert_eq!(outcome, "pick");
}

#[test]
fn a_fallback_query_skips_the_query_memory_lookup() {
    // WHY: the pick's key is "zebra" but its directory's name does not match, so the query falls back.
    let world = sandbox(&["projects/zebra", "origin"]);
    let cwd = world.child("projects");
    let picked = add_with_query(
        &world,
        &world.child("origin"),
        "session-1",
        &cwd,
        "zebra",
        None,
    );
    assert!(picked.status.success(), "stderr: {}", text(&picked.stderr));
    let out = run(world
        .furet()
        .env("FURET_LOG", "debug")
        .arg("query")
        .arg("zebra")
        .current_dir(&cwd));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = paths::canonical(&cwd.join("zebra"))
        .expect("the fallback hit canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{expected}\n"));
    let log = log_contents(&world);
    let ranking: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("ranking"))
        .collect();
    assert_eq!(ranking.len(), 1, "one ranking line, got: {log:?}");
    assert!(
        ranking[0].contains("fallback=true"),
        "the query fell back to the disk walk: {ranking:?}"
    );
    let memory: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("query memory"))
        .collect();
    assert_eq!(memory.len(), 1, "one query memory line, got: {log:?}");
    assert!(
        memory[0].contains("recall=Nothing"),
        "the fallback path skips the memory lookup: {memory:?}"
    );
}

#[test]
fn add_query_without_from_is_refused() {
    let world = sandbox(&["tokio"]);
    let mut cmd = world.furet();
    cmd.arg("add")
        .arg(world.child("tokio"))
        .arg("--session")
        .arg("session-1")
        .arg("--query")
        .arg("tok");
    let out = run(&mut cmd);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(!world.data.path().join("furet.db").exists());
}

#[test]
fn add_query_with_a_blank_text_writes_no_queries_row() {
    let world = sandbox(&["tokio", "origin"]);
    let tokio = world.child("tokio");
    let origin = world.child("origin");
    for blank in ["", "   "] {
        let out = add_with_query(&world, &tokio, "session-1", &origin, blank, None);
        assert!(out.status.success(), "blank: {blank:?}");
        let conn = db(&world);
        assert_eq!(
            scalar(&conn, "SELECT COUNT(*) FROM queries"),
            0,
            "blank: {blank:?}"
        );
    }
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 2);
}

#[test]
fn add_query_of_an_excluded_directory_writes_nothing() {
    let world = sandbox(&["zzskip", "origin"]);
    write_config(&world, "exclude_dirs = ['zzskip']");
    let out = add_with_query(
        &world,
        &world.child("zzskip"),
        "session-1",
        &world.child("origin"),
        "tok",
        None,
    );
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
    assert!(!world.data.path().join("furet.db").exists());
}

#[test]
fn queries_failures_flags_a_pick_followed_by_a_quick_back() {
    let world = sandbox(&["tokio", "helix", "origin"]);
    let tokio = world.child("tokio");
    let helix = world.child("helix");
    let origin = world.child("origin");
    let picked = add_with_query(&world, &tokio, "session-1", &origin, "tok", None);
    assert!(picked.status.success(), "stderr: {}", text(&picked.stderr));
    let back = add(&world, &helix, "session-1", Some("back"), Some(&tokio));
    assert!(back.status.success(), "stderr: {}", text(&back.stderr));
    visited_at(&world, &tokio, 1_700_000_000);
    visited_at(&world, &helix, 1_700_000_005);
    db(&world)
        .execute("UPDATE queries SET ts = 1_700_000_000", [])
        .expect("the pick row keeps the visit's aged timestamp");
    let out = run(world.furet().arg("queries").arg("--failures"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let tokio_canonical = canonical_child(&world, "tokio");
    let origin_canonical = canonical_child(&world, "origin");
    assert_eq!(
        text(&out.stdout),
        format!("{origin_canonical}\ttok\t{tokio_canonical}\tbacktrack\n")
    );
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
fn list_sorts_by_path_ignoring_case() {
    let world = sandbox(&["alpha", "Beta", "gamma"]);
    let alpha = world.child("alpha");
    let beta = world.child("Beta");
    let gamma = world.child("gamma");
    for child in [&alpha, &beta, &gamma] {
        assert!(add(&world, child, "session-1", None, None).status.success());
    }
    visited_at(&world, &alpha, 1_700_000_001);
    visited_at(&world, &beta, 1_700_000_002);
    visited_at(&world, &gamma, 1_700_000_003);
    let out = list(&world, false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected: Vec<String> = [&alpha, &beta, &gamma]
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
fn list_paths_prints_only_the_path_of_each_directory_in_list_order() {
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
    let out = list_with(&world, &["--paths"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected: Vec<String> = [&alpha, &beta, &gamma]
        .iter()
        .map(|path| {
            paths::canonical(path)
                .expect("the recorded directory canonicalizes")
                .path
        })
        .collect();
    let stdout = text(&out.stdout);
    assert_eq!(stdout, format!("{}\n", expected.join("\n")));
    assert!(
        !stdout.contains('\t'),
        "--paths never prints a tab: {stdout:?}"
    );
}

#[test]
fn list_paths_short_flag_matches_the_long_flag() {
    let world = sandbox(&["tokio", "tokei"]);
    let tokio = world.child("tokio");
    let tokei = world.child("tokei");
    for child in [&tokio, &tokei] {
        assert!(add(&world, child, "session-1", None, None).status.success());
    }
    visited_at(&world, &tokio, 1_700_000_002);
    visited_at(&world, &tokei, 1_700_000_001);
    let long = list_with(&world, &["--paths"]);
    let short = list_with(&world, &["-p"]);
    assert!(long.status.success(), "stderr: {}", text(&long.stderr));
    assert!(short.status.success(), "stderr: {}", text(&short.stderr));
    assert_eq!(text(&long.stdout), text(&short.stdout));
}

#[test]
fn list_all_paths_includes_missing_directories_without_a_presence_column() {
    let world = sandbox(&["tokio", "gone"]);
    let tokio = world.child("tokio");
    let gone = world.child("gone");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    assert!(add(&world, &gone, "session-1", None, None).status.success());
    visited_at(&world, &tokio, 1_700_000_002);
    visited_at(&world, &gone, 1_700_000_003);
    missing_since_at(&world, &gone, 1_700_000_100);
    let out = list_with(&world, &["--all", "--paths"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let gone_path = paths::canonical(&gone)
        .expect("the missing directory canonicalizes")
        .path;
    let tokio_path = paths::canonical(&tokio)
        .expect("the present directory canonicalizes")
        .path;
    let stdout = text(&out.stdout);
    assert_eq!(stdout, format!("{gone_path}\n{tokio_path}\n"));
    assert!(
        !stdout.contains('\t'),
        "--paths never prints a tab: {stdout:?}"
    );
    assert!(
        !stdout.contains("present") && !stdout.contains("missing"),
        "--all --paths never prints a presence column: {stdout:?}"
    );
}

#[test]
fn list_paths_on_an_empty_database_prints_nothing_and_exits_zero() {
    let world = sandbox(&[]);
    let out = list_with(&world, &["--paths"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
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
fn stats_prints_the_exact_report_for_a_built_database() {
    let world = sandbox(&["tokio", "helix", "gone", "cwd"]);
    let tokio = world.child("tokio");
    let helix = world.child("helix");
    let gone = world.child("gone");
    let cwd = world.child("cwd");
    let now = unix_now();
    let recent = now - 3_600;
    let old = now - 40 * 86_400;
    let calib = now - 60;
    for (path, session) in [(&tokio, "s1"), (&helix, "s2"), (&gone, "s3")] {
        assert!(add(&world, path, session, None, None).status.success());
    }
    assert!(
        add(&world, &tokio, "s4", Some("jump"), None)
            .status
            .success()
    );
    assert!(add(&world, &tokio, "s5", Some("up"), None).status.success());
    assert!(
        add(&world, &helix, "s6", Some("back"), None)
            .status
            .success()
    );
    assert!(
        add(&world, &helix, "s7", Some("fallback"), None)
            .status
            .success()
    );
    assert!(
        add(&world, &gone, "s8", Some("import"), None)
            .status
            .success()
    );
    visit_source_at(&world, &tokio, "up", old);
    assert!(query(&world, "tokio", &cwd, false).status.success());
    let tokio_path = paths::canonical(&tokio).expect("tokio canonicalizes").path;
    let helix_path = paths::canonical(&helix).expect("helix canonicalizes").path;
    let quiet_path = format!(
        "{}\\quiet",
        paths::canonical(world.tree.path())
            .expect("the sandbox tree canonicalizes")
            .path
    );
    let conn = db(&world);
    conn.execute(
        "INSERT INTO dirs (path, key, first_seen) VALUES (?1, ?2, ?3)",
        params![quiet_path, quiet_path.to_lowercase(), now - 100],
    )
    .expect("the visit-less dir inserts");
    conn.execute(
        "INSERT INTO visits (dir_id, ts, source, session) VALUES (?1, ?2, 'jump', 'calib')",
        params![dir_row_id(&world, &tokio_path), calib + 1],
    )
    .expect("the landing visit inserts");
    conn.execute(
        "INSERT INTO visits (dir_id, ts, source, session) VALUES (?1, ?2, 'back', 'calib')",
        params![dir_row_id(&world, &helix_path), calib + 6],
    )
    .expect("the backtrack visit inserts");
    journal_row(&world, calib, "tok", Some(&tokio_path), "1", "jump");
    journal_row(&world, recent, "hel", Some(&quiet_path), "2", "jump");
    journal_row(&world, recent, "gon", Some(&quiet_path), "fallback", "jump");
    journal_row(&world, recent, "men", None, "menu", "menu");
    journal_row(&world, recent, "no1", None, "1", "none");
    for name in ["j2", "j3", "j4", "j5"] {
        journal_row(&world, recent, name, Some(&quiet_path), "1", "jump");
    }
    journal_row(&world, old, "old", None, "2", "none");
    drop(conn);
    missing_since_at(&world, &gone, old);
    let out = stats(&world, &[]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        format!(
            "known_directories\t3\n\
             missing_directories\t1\n\
             visits\t10\n\
             visits_last_30_days\t9\n\
             queries\t11\n\
             queries_last_30_days\t10\n\
             jumps\t8\n\
             probable_failures\t1\n\
             failure_rate\t12.5%\n\
             stage_1\t7\n\
             stage_2\t2\n\
             stage_fallback\t1\n\
             stage_menu\t1\n\
             source_hook\t3\n\
             source_jump\t2\n\
             source_back\t2\n\
             source_up\t1\n\
             source_fallback\t1\n\
             source_import\t1\n\
             top\t4\t{helix_path}\n\
             top\t4\t{tokio_path}\n\
             top\t0\t{quiet_path}\n"
        )
    );
}

#[test]
fn stats_on_an_empty_database_prints_zeros_and_no_top_line() {
    let world = sandbox(&[]);
    let out = stats(&world, &[]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        "known_directories\t0\n\
         missing_directories\t0\n\
         visits\t0\n\
         visits_last_30_days\t0\n\
         queries\t0\n\
         queries_last_30_days\t0\n\
         jumps\t0\n\
         probable_failures\t0\n\
         failure_rate\t0.0%\n\
         stage_1\t0\n\
         stage_2\t0\n\
         stage_fallback\t0\n\
         stage_menu\t0\n\
         source_hook\t0\n\
         source_jump\t0\n\
         source_back\t0\n\
         source_up\t0\n\
         source_fallback\t0\n\
         source_import\t0\n"
    );
}

#[test]
fn stats_top_zero_prints_no_top_line() {
    let world = sandbox(&["tokio"]);
    assert!(
        add(&world, &world.child("tokio"), "session-1", None, None)
            .status
            .success()
    );
    let out = stats(&world, &["--top", "0"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout).lines().count(), 19);
    assert!(!text(&out.stdout).contains("top\t"));
    assert!(text(&out.stdout).starts_with("known_directories\t1\n"));
}

#[test]
fn stats_top_two_prints_two_top_lines() {
    let world = sandbox(&["aaa", "bbb", "ccc"]);
    for (name, visits) in [("aaa", 3), ("bbb", 2), ("ccc", 1)] {
        for i in 0..visits {
            assert!(
                add(
                    &world,
                    &world.child(name),
                    &format!("session-{i}"),
                    None,
                    None
                )
                .status
                .success()
            );
        }
    }
    let out = stats(&world, &["--top", "2"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert_eq!(stdout.lines().count(), 21);
    let lines: Vec<&str> = stdout.lines().collect();
    let aaa = canonical_child(&world, "aaa");
    let bbb = canonical_child(&world, "bbb");
    assert_eq!(lines[19], format!("top\t3\t{aaa}"));
    assert_eq!(lines[20], format!("top\t2\t{bbb}"));
}

#[test]
fn stats_writes_nothing() {
    let world = sandbox(&["tokio", "gone", "marked", "cwd"]);
    let tokio = world.child("tokio");
    let gone = world.child("gone");
    let marked = world.child("marked");
    let cwd = world.child("cwd");
    for path in [&tokio, &gone, &marked] {
        assert!(add(&world, path, "session-1", None, None).status.success());
    }
    assert!(query(&world, "tokio", &cwd, false).status.success());
    missing_since_at(&world, &marked, 1_700_000_000);
    std::fs::remove_dir_all(&gone).expect("the recorded directory vanishes from disk");
    let before = dry_run_state(&db(&world));
    let out = stats(&world, &[]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let after = dry_run_state(&db(&world));
    assert_eq!(
        before, after,
        "stats must not write anything, least of all mark the vanished directory"
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
fn help_ends_with_the_utc_build_date() {
    let world = sandbox(&[]);
    let expected = format!("Build date: {}", env!("FURET_BUILD_DATE"));
    for flag in ["-h", "--help"] {
        let out = run(world.furet().arg(flag));
        assert!(out.status.success(), "stderr: {}", text(&out.stderr));
        let help = text(&out.stdout);
        let last = help.lines().rev().find(|line| !line.trim().is_empty());
        assert_eq!(
            last,
            Some(expected.as_str()),
            "the top-level {flag} output must end with the build date: {help}"
        );
    }
    let digits = env!("FURET_BUILD_DATE");
    assert!(
        digits.len() == 8 && digits.bytes().all(|byte| byte.is_ascii_digit()),
        "the build date must be 8 ASCII digits: {digits}"
    );
    let year: i32 = digits[0..4].parse().expect("the year parses");
    let month: i32 = digits[4..6].parse().expect("the month parses");
    let day: i32 = digits[6..8].parse().expect("the day parses");
    assert!(year >= 2026, "the build year must be >= 2026: {digits}");
    assert!(
        (1..=12).contains(&month),
        "the build month must be in 1..=12: {digits}"
    );
    assert!(
        (1..=31).contains(&day),
        "the build day must be in 1..=31: {digits}"
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
        "imported 1, skipped 1 (known 0, not a directory 0, malformed 0, duplicate 1, excluded 0)\n"
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
        "imported 1, skipped 3 (known 0, not a directory 2, malformed 1, duplicate 0, excluded 0)\n"
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
        "imported 0, skipped 0 (known 0, not a directory 0, malformed 0, duplicate 0, excluded 0)\n"
    );
}

#[test]
fn import_zoxide_skips_excluded_directories_and_counts_them() {
    let world = sandbox(&["tokio", "tokei"]);
    write_config(&world, "exclude_dirs = ['tokei']");
    let tokei = world.child("tokei");
    let tokio = world.child("tokio");
    let stdin = format!(
        "12.5 {}\n3 {}\n",
        tokei.to_string_lossy(),
        tokio.to_string_lossy()
    );
    let out = import_zoxide(&world, &stdin);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr),
        "imported 1, skipped 1 (known 0, not a directory 0, malformed 0, duplicate 0, excluded 1)\n"
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 1);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 1);
}

#[test]
fn a_relative_exclude_dirs_entry_warns_in_the_log_and_keeps_the_others() {
    let world = sandbox(&["zzskip", "zz"]);
    write_config(&world, "exclude_dirs = ['zz\\a', 'zzskip']");
    let skipped = add(&world, &world.child("zzskip"), "session-1", None, None);
    assert!(skipped.status.success());
    assert!(skipped.stdout.is_empty());
    assert!(
        add(&world, &world.child("zz"), "session-1", None, None)
            .status
            .success()
    );
    assert_eq!(
        text(&list_with(&world, &["--paths"]).stdout),
        format!("{}\n", canonical_child(&world, "zz"))
    );
    let log = log_contents(&world);
    assert!(
        log.contains("exclude_dirs entry 'zz\\a' is a relative path; ignored"),
        "{log:?}"
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

fn canonical_child(world: &Sandbox, name: &str) -> String {
    paths::canonical(&world.child(name))
        .expect("the recorded directory canonicalizes")
        .path
}

#[test]
fn remove_by_name_glob_removes_every_match_and_reports_on_stderr_only() {
    let world = sandbox(&["ombi", "ombi-v4", "other"]);
    for name in ["ombi", "ombi-v4", "other"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let out = remove(&world, "ombi*", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "remove never writes to stdout");
    let expected = [
        canonical_child(&world, "ombi"),
        canonical_child(&world, "ombi-v4"),
    ]
    .map(|path| format!("removed {path}"));
    assert_eq!(text(&out.stderr), format!("{}\n", expected.join("\n")));
    let remaining = list_with(&world, &["--paths"]);
    assert_eq!(
        text(&remaining.stdout),
        format!("{}\n", canonical_child(&world, "other"))
    );
}

#[test]
fn remove_by_name_ignores_case() {
    let world = sandbox(&["Ombi"]);
    assert!(
        add(&world, &world.child("Ombi"), "session-1", None, None)
            .status
            .success()
    );
    let out = remove(&world, "ombi", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = [format!("removed {}", canonical_child(&world, "Ombi"))];
    assert_eq!(text(&out.stderr), format!("{}\n", expected.join("\n")));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
}

#[test]
fn remove_by_name_selects_the_known_descendants_of_a_matching_folder() {
    let world = sandbox(&["zz/cache", "zz/cache/x", "zz/cache/x/y", "zz/keep"]);
    for child in ["zz/cache", "zz/cache/x", "zz/cache/x/y", "zz/keep"] {
        assert!(
            add(&world, &world.child(child), "session-1", None, None)
                .status
                .success()
        );
    }
    let out = remove(&world, "cach*", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = [
        canonical_child(&world, "zz/cache"),
        canonical_child(&world, "zz/cache/x"),
        canonical_child(&world, "zz/cache/x/y"),
    ]
    .map(|path| format!("removed {path}"));
    assert_eq!(text(&out.stderr), format!("{}\n", expected.join("\n")));
    let remaining = list_with(&world, &["--paths"]);
    assert_eq!(
        text(&remaining.stdout),
        format!("{}\n", canonical_child(&world, "zz/keep"))
    );
}

#[test]
fn remove_path_pattern_starting_with_a_star_is_not_anchored_at_the_cwd() {
    let world = sandbox(&[
        "zz/cache",
        "zz/cache/x",
        "zz/cache/x/y",
        "zz/keep",
        "elsewhere",
    ]);
    for child in ["zz/cache", "zz/cache/x", "zz/cache/x/y", "zz/keep"] {
        assert!(
            add(&world, &world.child(child), "session-1", None, None)
                .status
                .success()
        );
    }
    let out = remove(&world, "*\\cache\\*", &world.child("elsewhere"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = [
        canonical_child(&world, "zz/cache/x"),
        canonical_child(&world, "zz/cache/x/y"),
    ]
    .map(|path| format!("removed {path}"));
    assert_eq!(text(&out.stderr), format!("{}\n", expected.join("\n")));
    let remaining = list_with(&world, &["--paths"]);
    let expected = [
        canonical_child(&world, "zz/cache"),
        canonical_child(&world, "zz/keep"),
    ];
    assert_eq!(
        text(&remaining.stdout),
        format!("{}\n", expected.join("\n"))
    );
}

#[test]
fn remove_by_relative_path_removes_only_that_directory() {
    let world = sandbox(&["a/src", "b/src"]);
    for child in ["a/src", "b/src"] {
        assert!(
            add(&world, &world.child(child), "session-1", None, None)
                .status
                .success()
        );
    }
    let out = remove(&world, "a\\src", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let expected = [format!("removed {}", canonical_child(&world, "a/src"))];
    assert_eq!(text(&out.stderr), format!("{}\n", expected.join("\n")));
    let remaining = list_with(&world, &["--paths"]);
    assert_eq!(
        text(&remaining.stdout),
        format!("{}\n", canonical_child(&world, "b/src"))
    );
}

#[test]
fn remove_dot_removes_the_current_directory() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = remove(&world, ".", &tokio);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
}

#[test]
fn remove_by_path_forgets_a_directory_already_deleted_from_disk() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let canonical = canonical_child(&world, "tokio");
    std::fs::remove_dir_all(&tokio).expect("the recorded directory vanishes from disk");
    let out = remove(&world, tokio.to_string_lossy().as_ref(), world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), format!("removed {canonical}\n"));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
}

#[test]
fn remove_by_path_glob_removes_the_descendants_not_the_parent() {
    let world = sandbox(&["apps", "apps/x", "apps/x/y"]);
    for child in ["apps", "apps/x", "apps/x/y"] {
        assert!(
            add(&world, &world.child(child), "session-1", None, None)
                .status
                .success()
        );
    }
    let out = remove(&world, "apps\\*", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
    let remaining = list_with(&world, &["--paths"]);
    assert_eq!(
        text(&remaining.stdout),
        format!("{}\n", canonical_child(&world, "apps"))
    );
}

#[test]
fn remove_includes_directories_marked_missing() {
    let world = sandbox(&["tokio", "gone"]);
    for name in ["tokio", "gone"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let gone_path = canonical_child(&world, "gone");
    std::fs::remove_dir_all(world.child("gone")).expect("gone vanishes from disk");
    let out = remove(&world, "gone", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), format!("removed {gone_path}\n"));
    let remaining = list_with(&world, &["--paths"]);
    assert_eq!(
        text(&remaining.stdout),
        format!("{}\n", canonical_child(&world, "tokio"))
    );
}

#[test]
fn remove_with_no_match_fails_with_exit_one_and_removes_nothing() {
    let world = sandbox(&["tokio"]);
    assert!(
        add(&world, &world.child("tokio"), "session-1", None, None)
            .status
            .success()
    );
    let out = remove(&world, "nope*", world.tree.path());
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(
        text(&out.stderr),
        "furet: no known directory matches 'nope*'\n"
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn remove_empty_pattern_fails_with_exit_one() {
    let world = sandbox(&[]);
    for pattern in ["", "   "] {
        let out = remove(&world, pattern, world.tree.path());
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(text(&out.stderr), "furet: empty pattern\n");
    }
}

#[test]
fn remove_drops_the_visits_and_queries_of_the_removed_directory() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let jumped = query(&world, "tokio", world.tree.path(), false);
    assert!(jumped.status.success(), "stderr: {}", text(&jumped.stderr));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 1);
    let out = remove(&world, "tokio", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn remove_confirm_yes_to_each_removes_both() {
    let world = sandbox(&["ombi", "ombi-v4"]);
    for name in ["ombi", "ombi-v4"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let first = canonical_child(&world, "ombi");
    let second = canonical_child(&world, "ombi-v4");
    let out = remove_answering(&world, "ombi*", world.tree.path(), "y\ny\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "remove never writes to stdout");
    assert_eq!(
        text(&out.stderr),
        format!(
            "Remove {first}? [y/N/a/q] Remove {second}? [y/N/a/q] removed {first}\nremoved {second}\n"
        )
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
}

#[test]
fn remove_confirm_declined_or_eof_removes_nothing_and_exits_one() {
    let world = sandbox(&["tokio"]);
    assert!(
        add(&world, &world.child("tokio"), "session-1", None, None)
            .status
            .success()
    );
    let expected = canonical_child(&world, "tokio");
    for answer in ["n\n", "\n", ""] {
        let out = remove_answering(&world, "tokio", world.tree.path(), answer);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(
            text(&out.stderr),
            format!("Remove {expected}? [y/N/a/q] furet: nothing removed\n")
        );
        assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
    }
}

#[test]
fn remove_confirm_asks_once_per_directory_and_removes_the_yes_ones() {
    let world = sandbox(&["ombi", "ombi-v4"]);
    for name in ["ombi", "ombi-v4"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let first = canonical_child(&world, "ombi");
    let second = canonical_child(&world, "ombi-v4");
    let out = remove_answering(&world, "ombi*", world.tree.path(), "y\nn\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "remove never writes to stdout");
    assert_eq!(
        text(&out.stderr),
        format!("Remove {first}? [y/N/a/q] Remove {second}? [y/N/a/q] removed {first}\n")
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
    let remaining = list_with(&world, &["--paths"]);
    assert_eq!(text(&remaining.stdout), format!("{second}\n"));
}

#[test]
fn remove_confirm_empty_and_no_answers_keep_everything() {
    let world = sandbox(&["ombi", "ombi-v4"]);
    for name in ["ombi", "ombi-v4"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let first = canonical_child(&world, "ombi");
    let second = canonical_child(&world, "ombi-v4");
    for answer in ["\n\n", "n\nno\n"] {
        let out = remove_answering(&world, "ombi*", world.tree.path(), answer);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(
            text(&out.stderr),
            format!(
                "Remove {first}? [y/N/a/q] Remove {second}? [y/N/a/q] furet: nothing removed\n"
            )
        );
        assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 2);
    }
}

#[test]
fn remove_confirm_all_removes_the_rest_without_asking() {
    let world = sandbox(&["ombi", "ombi-v2", "ombi-v4"]);
    for name in ["ombi", "ombi-v2", "ombi-v4"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let first = canonical_child(&world, "ombi");
    let second = canonical_child(&world, "ombi-v2");
    let third = canonical_child(&world, "ombi-v4");
    let out = remove_answering(&world, "ombi*", world.tree.path(), "n\na\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr),
        format!(
            "Remove {first}? [y/N/a/q] Remove {second}? [y/N/a/q] removed {second}\nremoved {third}\n"
        )
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
    let remaining = list_with(&world, &["--paths"]);
    assert_eq!(text(&remaining.stdout), format!("{first}\n"));
}

#[test]
fn remove_confirm_yes_then_quit_removes_only_the_first() {
    let world = sandbox(&["ombi", "ombi-v2", "ombi-v4"]);
    for name in ["ombi", "ombi-v2", "ombi-v4"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let first = canonical_child(&world, "ombi");
    let second = canonical_child(&world, "ombi-v2");
    let out = remove_answering(&world, "ombi*", world.tree.path(), "y\nq\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr),
        format!("Remove {first}? [y/N/a/q] Remove {second}? [y/N/a/q] removed {first}\n")
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 2);
}

#[test]
fn remove_confirm_reasks_after_an_invalid_answer() {
    let world = sandbox(&["ombi"]);
    assert!(
        add(&world, &world.child("ombi"), "session-1", None, None)
            .status
            .success()
    );
    let path = canonical_child(&world, "ombi");
    let out = remove_answering(&world, "ombi", world.tree.path(), "maybe\ny\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr),
        format!("Remove {path}? [y/N/a/q] Remove {path}? [y/N/a/q] removed {path}\n")
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
}

#[test]
fn remove_confirm_eof_removes_nothing() {
    let world = sandbox(&["tokio"]);
    assert!(
        add(&world, &world.child("tokio"), "session-1", None, None)
            .status
            .success()
    );
    let path = canonical_child(&world, "tokio");
    let out = remove_answering(&world, "tokio", world.tree.path(), "");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(
        text(&out.stderr),
        format!("Remove {path}? [y/N/a/q] furet: nothing removed\n")
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn remove_yes_asks_nothing_and_removes() {
    let world = sandbox(&["ombi", "ombi-v4"]);
    for name in ["ombi", "ombi-v4"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let out = remove_with(&world, "ombi*", world.tree.path(), &["--yes"], "");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "remove never writes to stdout");
    let expected = [
        canonical_child(&world, "ombi"),
        canonical_child(&world, "ombi-v4"),
    ]
    .map(|path| format!("removed {path}"));
    assert_eq!(text(&out.stderr), format!("{}\n", expected.join("\n")));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
}

#[test]
fn remove_confirm_and_yes_conflict() {
    let world = sandbox(&["tokio"]);
    assert!(
        add(&world, &world.child("tokio"), "session-1", None, None)
            .status
            .success()
    );
    let out = remove_with(
        &world,
        "tokio",
        world.tree.path(),
        &["--confirm", "--yes"],
        "",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn remove_dry_run_changes_nothing() {
    let world = sandbox(&["tokio/keep", "tokio/gone"]);
    for child in ["tokio/keep", "tokio/gone"] {
        assert!(
            add(&world, &world.child(child), "session-1", None, None)
                .status
                .success()
        );
    }
    let jumped = query(&world, "keep", world.tree.path(), false);
    assert!(jumped.status.success(), "stderr: {}", text(&jumped.stderr));
    missing_since_at(&world, &world.child("tokio/gone"), 1_000);
    let conn = db(&world);
    let before = dry_run_state(&conn);
    assert!(before.2 >= 1, "at least one queries row was journaled");
    let out = remove_with(
        &world,
        "tokio",
        world.tree.path(),
        &["--dry-run", "--confirm"],
        "",
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "remove never writes to stdout");
    let expected = [
        canonical_child(&world, "tokio/gone"),
        canonical_child(&world, "tokio/keep"),
    ]
    .map(|path| format!("would remove {path}"));
    assert_eq!(text(&out.stderr), format!("{}\n", expected.join("\n")));
    assert_eq!(dry_run_state(&conn), before);
}

#[test]
fn remove_dry_run_with_no_match_fails_like_a_real_remove() {
    let world = sandbox(&["tokio"]);
    assert!(
        add(&world, &world.child("tokio"), "session-1", None, None)
            .status
            .success()
    );
    let out = remove_with(&world, "nope*", world.tree.path(), &["--dry-run"], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert_eq!(
        text(&out.stderr),
        "furet: no known directory matches 'nope*'\n"
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn remove_missing_yes_removes_a_vanished_directory_and_keeps_a_returned_one() {
    let world = sandbox(&["gone", "back", "present"]);
    for name in ["gone", "back", "present"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let gone = canonical_child(&world, "gone");
    std::fs::remove_dir_all(world.child("gone")).expect("gone vanishes from disk");
    missing_since_at(&world, &world.child("back"), 1_000);
    let out = remove_missing(&world, None, world.tree.path(), &["--missing", "--yes"], "");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "remove never writes to stdout");
    assert_eq!(text(&out.stderr), format!("removed {gone}\n"));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 2);
    let back_missing: Option<i64> = conn
        .query_row(
            "SELECT missing_since FROM dirs WHERE path = ?1",
            params![canonical_child(&world, "back")],
            |row| row.get(0),
        )
        .expect("back's row reads back");
    assert_eq!(back_missing, None, "the stale marker is cleared");
}

#[test]
fn remove_missing_asks_by_default_with_the_absence_date() {
    let world = sandbox(&["gone"]);
    assert!(
        add(&world, &world.child("gone"), "session-1", None, None)
            .status
            .success()
    );
    missing_since_at(&world, &world.child("gone"), 1_700_000_000);
    let gone = canonical_child(&world, "gone");
    std::fs::remove_dir_all(world.child("gone")).expect("gone vanishes from disk");
    let out = remove_missing(&world, None, world.tree.path(), &["--missing"], "y\n");
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "remove never writes to stdout");
    let date = local_time(&db(&world), 1_700_000_000);
    assert_eq!(
        text(&out.stderr),
        format!("Remove {gone} (missing since {date})? [y/N/a/q] removed {gone}\n")
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
}

#[test]
fn remove_missing_confirm_is_accepted_and_still_asks() {
    let world = sandbox(&["gone"]);
    assert!(
        add(&world, &world.child("gone"), "session-1", None, None)
            .status
            .success()
    );
    missing_since_at(&world, &world.child("gone"), 1_700_000_000);
    let gone = canonical_child(&world, "gone");
    std::fs::remove_dir_all(world.child("gone")).expect("gone vanishes from disk");
    let out = remove_missing(
        &world,
        None,
        world.tree.path(),
        &["--missing", "--confirm"],
        "n\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let date = local_time(&db(&world), 1_700_000_000);
    assert_eq!(
        text(&out.stderr),
        format!("Remove {gone} (missing since {date})? [y/N/a/q] furet: nothing removed\n")
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn remove_missing_with_a_pattern_keeps_other_missing_directories() {
    let world = sandbox(&["gone-a", "gone-b"]);
    for name in ["gone-a", "gone-b"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let gone_a = canonical_child(&world, "gone-a");
    let gone_b = canonical_child(&world, "gone-b");
    std::fs::remove_dir_all(world.child("gone-a")).expect("gone-a vanishes from disk");
    std::fs::remove_dir_all(world.child("gone-b")).expect("gone-b vanishes from disk");
    let out = remove_missing(
        &world,
        Some("gone-a"),
        world.tree.path(),
        &["--missing", "--yes"],
        "",
    );
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), format!("removed {gone_a}\n"));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM dirs"), 1);
    let kept: Option<i64> = conn
        .query_row(
            "SELECT missing_since FROM dirs WHERE path = ?1",
            params![gone_b],
            |row| row.get(0),
        )
        .expect("gone-b's row reads back");
    assert!(
        kept.is_some(),
        "gone-b stays, marked missing by the reconcile"
    );
}

#[test]
fn remove_missing_without_missing_directories_fails() {
    let world = sandbox(&["tokio"]);
    assert!(
        add(&world, &world.child("tokio"), "session-1", None, None)
            .status
            .success()
    );
    let plain = remove_missing(&world, None, world.tree.path(), &["--missing"], "");
    assert_eq!(plain.status.code(), Some(1));
    assert!(plain.stdout.is_empty());
    assert_eq!(text(&plain.stderr), "furet: no missing known directory\n");
    std::fs::remove_dir_all(world.child("tokio")).expect("tokio vanishes from disk");
    let with_pattern = remove_missing(&world, Some("nope"), world.tree.path(), &["--missing"], "");
    assert_eq!(with_pattern.status.code(), Some(1));
    assert!(with_pattern.stdout.is_empty());
    assert_eq!(
        text(&with_pattern.stderr),
        "furet: no missing known directory matches 'nope'\n"
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn remove_missing_dry_run_changes_nothing() {
    let world = sandbox(&["gone", "back"]);
    for name in ["gone", "back"] {
        assert!(
            add(&world, &world.child(name), "session-1", None, None)
                .status
                .success()
        );
    }
    let jumped = query(&world, "back", world.tree.path(), false);
    assert!(jumped.status.success(), "stderr: {}", text(&jumped.stderr));
    let gone = canonical_child(&world, "gone");
    missing_since_at(&world, &world.child("back"), 1_000);
    std::fs::remove_dir_all(world.child("gone")).expect("gone vanishes from disk");
    let conn = db(&world);
    let before = dry_run_state(&conn);
    assert!(before.2 >= 1, "at least one queries row was journaled");
    let out = remove_missing(
        &world,
        None,
        world.tree.path(),
        &["--missing", "--dry-run"],
        "",
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "remove never writes to stdout");
    assert_eq!(text(&out.stderr), format!("would remove {gone}\n"));
    assert_eq!(dry_run_state(&conn), before);
}

#[test]
fn remove_without_a_pattern_or_missing_is_refused() {
    let world = sandbox(&["tokio"]);
    assert!(
        add(&world, &world.child("tokio"), "session-1", None, None)
            .status
            .success()
    );
    for flags in [&[] as &[&str], &["--yes"], &["--dry-run"]] {
        let out = remove_missing(&world, None, world.tree.path(), flags, "");
        assert_eq!(out.status.code(), Some(2), "flags: {flags:?}");
        assert!(out.stdout.is_empty(), "flags: {flags:?}");
    }
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 1);
}

#[test]
fn remove_missing_with_an_empty_pattern_fails_like_a_plain_remove() {
    let world = sandbox(&[]);
    for pattern in ["", "   "] {
        let out = remove_missing(&world, Some(pattern), world.tree.path(), &["--missing"], "");
        assert_eq!(out.status.code(), Some(1), "pattern: {pattern:?}");
        assert!(out.stdout.is_empty(), "pattern: {pattern:?}");
        assert_eq!(text(&out.stderr), "furet: empty pattern\n");
    }
}

#[test]
fn a_removed_directory_comes_back_on_the_next_add() {
    let world = sandbox(&["tokio"]);
    let tokio = world.child("tokio");
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let out = remove(&world, "tokio", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
    assert!(
        add(&world, &tokio, "session-1", None, None)
            .status
            .success()
    );
    let remaining = list_with(&world, &["--paths"]);
    assert_eq!(
        text(&remaining.stdout),
        format!("{}\n", canonical_child(&world, "tokio"))
    );
}

#[test]
fn query_local_ignores_a_better_match_outside_the_project() {
    let world = sandbox(&["proj/.git", "proj/tokio", "elsewhere/tokio"]);
    assert!(
        add(&world, &world.child("proj/tokio"), "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(
            &world,
            &world.child("elsewhere/tokio"),
            "session-1",
            None,
            None
        )
        .status
        .success()
    );
    visited_at(&world, &world.child("proj/tokio"), 1_700_000_001);
    visited_at(&world, &world.child("elsewhere/tokio"), 1_700_000_002);
    let cwd = world.child("proj");
    let global = query_with(&world, &["tokio"], &cwd);
    assert!(global.status.success(), "stderr: {}", text(&global.stderr));
    let outside = paths::canonical(&world.child("elsewhere/tokio"))
        .expect("the outside candidate canonicalizes")
        .path;
    assert_eq!(text(&global.stdout), format!("{outside}\n"));
    let local = query_with(&world, &["--local", "tokio"], &cwd);
    assert!(local.status.success(), "stderr: {}", text(&local.stderr));
    let inside = paths::canonical(&world.child("proj/tokio"))
        .expect("the in-project candidate canonicalizes")
        .path;
    assert_eq!(text(&local.stdout), format!("{inside}\n"));
}

#[test]
fn query_local_outside_a_repository_fails_and_records_nothing() {
    let world = sandbox(&["cwd"]);
    let cwd = world.child("cwd");
    assert!(
        project::root(
            world.tree.path().to_string_lossy().as_ref(),
            &project::RealGitMarker
        )
        .is_none(),
        "the sandbox tree must not sit inside a git repository"
    );
    let cases: Vec<Vec<&str>> = vec![
        vec!["--local", "zigzag"],
        vec!["--list", "--local", "zigzag"],
        vec!["--explain", "--local", "zigzag"],
    ];
    for args in &cases {
        let out = query_with(&world, args, &cwd);
        assert!(!out.status.success(), "args: {args:?}");
        assert!(out.stdout.is_empty(), "args: {args:?}");
        assert_eq!(
            text(&out.stderr),
            "furet: not inside a git repository\n",
            "args: {args:?}"
        );
        assert!(
            !world.data.path().join("furet.db").exists()
                || scalar(&db(&world), "SELECT COUNT(*) FROM queries") == 0,
            "args: {args:?}"
        );
    }
}

#[test]
fn query_local_with_an_empty_query_prints_the_project_root() {
    let world = sandbox(&["proj/.git", "proj/sub"]);
    let cwd = world.child("proj/sub");
    let root = paths::canonical(&world.child("proj"))
        .expect("the project root canonicalizes")
        .path;
    for args in [vec!["--local"], vec!["--local", "   "]] {
        let out = query_with(&world, &args, &cwd);
        assert!(out.status.success(), "stderr: {}", text(&out.stderr));
        assert_eq!(text(&out.stdout), format!("{root}\n"));
        assert!(
            !world.data.path().join("furet.db").exists()
                || scalar(&db(&world), "SELECT COUNT(*) FROM queries") == 0
        );
    }
}

#[test]
fn query_local_fallback_never_climbs_above_the_root() {
    let world = sandbox(&["proj/.git", "outproject"]);
    let cwd = world.child("proj");
    let global = query_with(&world, &["outproject"], &cwd);
    assert!(global.status.success(), "stderr: {}", text(&global.stderr));
    let expected = paths::canonical(&world.child("outproject"))
        .expect("the sibling of the root canonicalizes")
        .path;
    assert_eq!(text(&global.stdout), format!("{expected}\n"));
    let local = query_with(&world, &["--local", "outproject"], &cwd);
    assert!(!local.status.success());
    assert!(local.stdout.is_empty());
}

#[test]
fn query_list_local_with_an_empty_query_lists_the_project_by_recency() {
    let world = sandbox(&["proj/.git", "proj/alpha", "proj/beta", "outside/gamma"]);
    for child in ["proj/alpha", "proj/beta", "outside/gamma"] {
        assert!(
            add(&world, &world.child(child), "session-1", None, None)
                .status
                .success()
        );
    }
    visited_at(&world, &world.child("proj/alpha"), 1_700_000_001);
    visited_at(&world, &world.child("proj/beta"), 1_700_000_002);
    let cwd = world.child("proj");
    let out = query_with(&world, &["--list", "--local"], &cwd);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let beta = paths::canonical(&world.child("proj/beta"))
        .expect("beta canonicalizes")
        .path;
    let alpha = paths::canonical(&world.child("proj/alpha"))
        .expect("alpha canonicalizes")
        .path;
    assert_eq!(text(&out.stdout), format!("{beta}\n{alpha}\n"));
}

#[test]
fn query_explain_local_prints_the_project_root_line() {
    let world = sandbox(&["proj/.git", "proj/tokio"]);
    assert!(
        add(&world, &world.child("proj/tokio"), "session-1", None, None)
            .status
            .success()
    );
    let cwd = world.child("proj");
    let root = paths::canonical(&world.child("proj"))
        .expect("the project root canonicalizes")
        .path;
    let out = query_with(&world, &["--explain", "--local", "tokio"], &cwd);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with(&format!(
            "normalized query: tokio\nengine: reference\nmemory: none\nproject root: {root}\n"
        )),
        "{stderr}"
    );
    let global = query_with(&world, &["--explain", "tokio"], &cwd);
    assert!(global.status.success(), "stderr: {}", text(&global.stderr));
    assert!(
        !text(&global.stderr).contains("project root:"),
        "{}",
        text(&global.stderr)
    );
}

fn write_home_config(sandbox: &Sandbox, home: &Path) {
    let forward = home.to_string_lossy().replace('\\', "/");
    write_config(sandbox, &format!("home = \"{forward}\""));
}

fn user_profile_home() -> String {
    let home = dirs::home_dir().expect("the user profile resolves");
    paths::canonical(&home)
        .expect("the user profile canonicalizes")
        .path
}

#[test]
fn query_home_ignores_a_better_match_outside_the_home() {
    let world = sandbox(&["home/tokio", "elsewhere/tokio"]);
    assert!(
        add(&world, &world.child("home/tokio"), "session-1", None, None)
            .status
            .success()
    );
    assert!(
        add(
            &world,
            &world.child("elsewhere/tokio"),
            "session-1",
            None,
            None
        )
        .status
        .success()
    );
    visited_at(&world, &world.child("home/tokio"), 1_700_000_001);
    visited_at(&world, &world.child("elsewhere/tokio"), 1_700_000_002);
    write_home_config(&world, &world.child("home"));
    let cwd = world.tree.path();
    let global = query_with(&world, &["tokio"], cwd);
    assert!(global.status.success(), "stderr: {}", text(&global.stderr));
    let outside = canonical_child(&world, "elsewhere/tokio");
    assert_eq!(text(&global.stdout), format!("{outside}\n"));
    let scoped = query_with(&world, &["--home", "tokio"], cwd);
    assert!(scoped.status.success(), "stderr: {}", text(&scoped.stderr));
    let inside = canonical_child(&world, "home/tokio");
    assert_eq!(text(&scoped.stdout), format!("{inside}\n"));
}

#[test]
fn query_home_with_an_empty_query_prints_the_home_root() {
    let world = sandbox(&["home/sub"]);
    write_home_config(&world, &world.child("home"));
    let cwd = world.tree.path();
    let root = canonical_child(&world, "home");
    for args in [vec!["--home"], vec!["--home", "   "]] {
        let out = query_with(&world, &args, cwd);
        assert!(
            out.status.success(),
            "args: {args:?}, stderr: {}",
            text(&out.stderr)
        );
        assert_eq!(text(&out.stdout), format!("{root}\n"), "args: {args:?}");
        assert!(
            !world.data.path().join("furet.db").exists()
                || scalar(&db(&world), "SELECT COUNT(*) FROM queries") == 0,
            "args: {args:?}"
        );
    }
}

#[test]
fn query_home_without_a_configured_home_uses_the_user_profile() {
    let world = sandbox(&[]);
    let out = query_with(&world, &["--home"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("{}\n", user_profile_home()));
}

#[test]
fn query_home_with_an_invalid_home_uses_the_user_profile() {
    let world = sandbox(&[]);
    let expected = format!("{}\n", user_profile_home());
    write_config(&world, "home = \"C:/definitely/missing/furet-lot-55\"");
    let missing = query_with(&world, &["--home"], world.tree.path());
    assert!(
        missing.status.success(),
        "stderr: {}",
        text(&missing.stderr)
    );
    assert_eq!(text(&missing.stdout), expected);
    write_config(&world, "home = \"relative/home\"");
    let relative = query_with(&world, &["--home"], world.tree.path());
    assert!(
        relative.status.success(),
        "stderr: {}",
        text(&relative.stderr)
    );
    assert_eq!(text(&relative.stdout), expected);
}

#[test]
fn query_home_fallback_inside_the_home_never_climbs_above_it() {
    let world = sandbox(&["home", "outhome"]);
    write_home_config(&world, &world.child("home"));
    let cwd = world.child("home");
    let global = query_with(&world, &["outhome"], &cwd);
    assert!(global.status.success(), "stderr: {}", text(&global.stderr));
    let expected = canonical_child(&world, "outhome");
    assert_eq!(text(&global.stdout), format!("{expected}\n"));
    let scoped = query_with(&world, &["--home", "outhome"], &cwd);
    assert!(!scoped.status.success());
    assert!(scoped.stdout.is_empty());
}

#[test]
fn query_home_fallback_from_outside_walks_the_home() {
    let world = sandbox(&["home/zebra", "cwd/zulu"]);
    write_home_config(&world, &world.child("home"));
    let cwd = world.child("cwd");
    let scoped = query_with(&world, &["--home", "zebra"], &cwd);
    assert!(scoped.status.success(), "stderr: {}", text(&scoped.stderr));
    let zebra = canonical_child(&world, "home/zebra");
    assert_eq!(text(&scoped.stdout), format!("{zebra}\n"));
    let outside = query_with(&world, &["--home", "zulu"], &cwd);
    assert!(!outside.status.success());
    assert!(outside.stdout.is_empty());
    let global = query_with(&world, &["zulu"], &cwd);
    assert!(global.status.success(), "stderr: {}", text(&global.stderr));
    let zulu = canonical_child(&world, "cwd/zulu");
    assert_eq!(text(&global.stdout), format!("{zulu}\n"));
}

#[test]
fn query_list_home_with_an_empty_query_lists_the_home_by_recency() {
    let world = sandbox(&["home/alpha", "home/beta", "outside/gamma"]);
    for child in ["home/alpha", "home/beta", "outside/gamma"] {
        assert!(
            add(&world, &world.child(child), "session-1", None, None)
                .status
                .success()
        );
    }
    visited_at(&world, &world.child("home/alpha"), 1_700_000_001);
    visited_at(&world, &world.child("home/beta"), 1_700_000_002);
    write_home_config(&world, &world.child("home"));
    let cwd = world.tree.path();
    let out = query_with(&world, &["--list", "--home"], cwd);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let beta = canonical_child(&world, "home/beta");
    let alpha = canonical_child(&world, "home/alpha");
    assert_eq!(text(&out.stdout), format!("{beta}\n{alpha}\n"));
}

#[test]
fn query_explain_home_prints_the_home_root_line() {
    let world = sandbox(&["home/tokio"]);
    assert!(
        add(&world, &world.child("home/tokio"), "session-1", None, None)
            .status
            .success()
    );
    write_home_config(&world, &world.child("home"));
    let cwd = world.tree.path();
    let root = canonical_child(&world, "home");
    let out = query_with(&world, &["--explain", "--home", "tokio"], cwd);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with(&format!(
            "normalized query: tokio\nengine: reference\nmemory: none\nhome root: {root}\n"
        )),
        "{stderr}"
    );
    assert!(!stderr.contains("project root:"), "{stderr}");
    let global = query_with(&world, &["--explain", "tokio"], cwd);
    assert!(global.status.success(), "stderr: {}", text(&global.stderr));
    assert!(
        !text(&global.stderr).contains("home root:"),
        "{}",
        text(&global.stderr)
    );
}

#[test]
fn query_memory_respects_home_scope() {
    let world = aged_world(
        &["proj/om", "proj/omega", "elsewhere/ombi", "origin"],
        &["proj/om", "proj/omega", "elsewhere/ombi"],
    );
    pick(&world, "elsewhere/ombi", "om");
    write_home_config(&world, &world.child("proj"));
    let cwd = world.tree.path();
    let explained = query_with(&world, &["--home", "--explain", "om"], cwd);
    assert!(
        text(&explained.stderr).contains("\nmemory: not applied (no longer matches)\n"),
        "{}",
        text(&explained.stderr)
    );
    assert_eq!(
        jumped_to(&query_with(&world, &["om"], cwd)),
        format!("{}\n", canonical_child(&world, "elsewhere/ombi"))
    );
    assert_eq!(
        jumped_to(&query_with(&world, &["--home", "om"], cwd)),
        format!("{}\n", canonical_child(&world, "proj/om"))
    );
}

#[test]
fn query_home_and_local_together_fail() {
    let world = sandbox(&["tokio"]);
    let out = query_with(&world, &["--home", "--local", "tokio"], world.tree.path());
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(
        text(&out.stderr).contains("cannot be used with"),
        "stderr: {}",
        text(&out.stderr)
    );
    assert!(
        !world.data.path().join("furet.db").exists()
            || scalar(&db(&world), "SELECT COUNT(*) FROM queries") == 0
    );
}

#[test]
fn query_alias_with_home_fails() {
    let world = sandbox(&["zebra"]);
    assert!(
        alias(&world, &["add", "ombi", "zebra"], world.tree.path())
            .status
            .success()
    );
    let out = query_with(&world, &["--home", "!ombi"], world.tree.path());
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(
        text(&out.stderr).contains("furet: --home cannot be combined with an alias"),
        "stderr: {}",
        text(&out.stderr)
    );
}

#[test]
fn query_engine_flag_overrides_the_config() {
    let world = sandbox(&["my-dev", "d-e-v"]);
    for child in ["my-dev", "d-e-v"] {
        assert!(
            add(&world, &world.child(child), "session-1", None, None)
                .status
                .success()
        );
    }
    let winner = |args: &[&str]| {
        let out = query_with(&world, args, world.tree.path());
        assert!(out.status.success(), "stderr: {}", text(&out.stderr));
        text(&out.stdout)
    };
    let reference = winner(&["dev"]);
    let expected = paths::canonical(&world.child("d-e-v"))
        .expect("the reference winner canonicalizes")
        .path;
    assert_eq!(reference, format!("{expected}\n"));
    // WHY: query memory would send "dev" back to the reference winner's journaled jump.
    write_config(&world, "engine = \"nucleo\"\nquery_memory = false");
    let nucleo = winner(&["dev"]);
    let nucleo_winner = paths::canonical(&world.child("my-dev"))
        .expect("the nucleo winner canonicalizes")
        .path;
    assert_eq!(nucleo, format!("{nucleo_winner}\n"));
    assert_eq!(winner(&["dev", "--engine", "reference"]), reference);
    assert_eq!(winner(&["--engine", "nucleo", "dev"]), nucleo);
}

fn write_entry(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the parent directory exists");
    }
    std::fs::write(path, contents).expect("the entry is written on disk");
}

#[test]
fn preview_lists_directories_then_files_sorted_ignoring_case() {
    let world = sandbox(&[]);
    let dir = world.tree.path().join("listed");
    for child in ["Beta", "alpha", ".git"] {
        std::fs::create_dir_all(dir.join(child)).expect("the child directory exists");
    }
    write_entry(&dir.join("Zeta.txt"), b"x");
    write_entry(&dir.join("a-file.md"), b"x");
    let out = run(world.furet().arg("preview").arg(&dir));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        ".git\\\nalpha\\\nBeta\\\na-file.md\nZeta.txt\n"
    );
}

#[test]
fn preview_truncates_at_fifty_entries() {
    let world = sandbox(&[]);
    let dir = world.tree.path().join("many");
    std::fs::create_dir_all(&dir).expect("the listed directory exists");
    for i in 0..51 {
        write_entry(&dir.join(format!("e{i:02}")), b"x");
    }
    let out = run(world.furet().arg("preview").arg(&dir));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 51);
    for (i, line) in lines[..50].iter().enumerate() {
        assert_eq!(*line, format!("e{i:02}"));
    }
    assert_eq!(lines[50], "… +1 more");
}

#[test]
fn preview_of_a_missing_path_or_a_file_says_not_a_directory() {
    let world = sandbox(&[]);
    let missing = world.tree.path().join("nope");
    let file = world.tree.path().join("file.txt");
    write_entry(&file, b"x");
    for path in [&missing, &file] {
        let out = run(world.furet().arg("preview").arg(path));
        assert!(out.status.success(), "stderr: {}", text(&out.stderr));
        assert_eq!(text(&out.stdout), "(not a directory)\n");
    }
}

#[test]
fn preview_strips_ansi_codes_from_its_argument() {
    let world = sandbox(&[]);
    let dir = world.tree.path().join("sneaky");
    std::fs::create_dir_all(&dir).expect("the listed directory exists");
    write_entry(&dir.join("entry.txt"), b"x");
    let colored = format!("\x1b[01;34m{}\x1b[0m", dir.display());
    let out = run(world.furet().arg("preview").arg(&colored));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "entry.txt\n");
}

#[test]
fn preview_never_opens_the_database() {
    let world = sandbox(&[]);
    let out = run(world.furet().arg("preview").arg(world.tree.path()));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(
        !world.data.path().join("furet.db").exists(),
        "preview must not create the database file"
    );
}

// WHY: setup visits are aged and in their own session, so no later pick reads them as a probable failure.
fn aged_world(children: &[&str], known: &[&str]) -> Sandbox {
    let world = sandbox(children);
    for (age, child) in known.iter().enumerate() {
        let path = world.child(child);
        assert!(add(&world, &path, "setup", None, None).status.success());
        visited_at(&world, &path, unix_now() - 100 - age as i64);
    }
    world
}

fn pick(world: &Sandbox, child: &str, query_text: &str) {
    let out = add_with_query(
        world,
        &world.child(child),
        "session-1",
        &world.child("origin"),
        query_text,
        None,
    );
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
}

fn jumped_to(out: &Output) -> String {
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stderr.is_empty(), "stderr: {}", text(&out.stderr));
    text(&out.stdout)
}

fn latest_query_row(world: &Sandbox) -> (String, String, Option<String>) {
    db(world)
        .query_row(
            "SELECT stage, outcome, (SELECT path FROM dirs WHERE id = result_dir_id)
             FROM queries ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the latest queries row reads back")
}

fn om_world() -> (Sandbox, String, String) {
    let world = aged_world(&["om", "dev/ombi", "origin"], &["om", "dev/ombi"]);
    let om = canonical_child(&world, "om");
    let ombi = canonical_child(&world, "dev/ombi");
    (world, om, ombi)
}

#[test]
fn query_memory_sends_the_same_query_to_the_last_picked_directory() {
    let (world, om, ombi) = om_world();
    let root = world.tree.path();
    assert_eq!(
        jumped_to(&query(&world, "om", root, false)),
        format!("{om}\n")
    );
    pick(&world, "dev/ombi", "om");
    for variant in ["om", "OM", "  om  "] {
        assert_eq!(
            jumped_to(&query(&world, variant, root, false)),
            format!("{ombi}\n"),
            "variant {variant:?}"
        );
    }
    assert_eq!(
        jumped_to(&query(&world, "ombi", root, false)),
        format!("{ombi}\n")
    );
    assert_eq!(
        jumped_to(&query(&world, "o m", root, false)),
        format!("{om}\n"),
        "a different key keeps the normal winner"
    );
}

#[test]
fn query_memory_is_cancelled_by_a_probable_failure() {
    let (world, om, _) = om_world();
    pick(&world, "dev/ombi", "om");
    let back = add(
        &world,
        &world.child("om"),
        "session-1",
        Some("back"),
        Some(&world.child("dev/ombi")),
    );
    assert!(back.status.success(), "stderr: {}", text(&back.stderr));
    let picked_at = unix_now() - 50;
    let conn = db(&world);
    conn.execute(
        "UPDATE visits SET ts = ?1 WHERE session = 'session-1' AND source = 'hook'",
        params![picked_at],
    )
    .expect("the pick landing is dated");
    conn.execute(
        "UPDATE visits SET ts = ?1 WHERE source = 'back'",
        params![picked_at + 5],
    )
    .expect("the backtrack is dated five seconds later");
    conn.execute("UPDATE queries SET ts = ?1", params![picked_at])
        .expect("the pick row is dated");
    let explained = query_with(&world, &["--explain", "om"], world.tree.path());
    assert!(
        text(&explained.stderr).contains("\nmemory: not applied (probable failure)\n"),
        "{}",
        text(&explained.stderr)
    );
    assert_eq!(
        jumped_to(&query(&world, "om", world.tree.path(), false)),
        format!("{om}\n")
    );
}

#[test]
fn query_memory_is_ignored_when_disabled_in_config() {
    let (world, om, _) = om_world();
    write_config(&world, "query_memory = false");
    pick(&world, "dev/ombi", "om");
    assert_eq!(
        scalar(
            &db(&world),
            "SELECT COUNT(*) FROM queries WHERE outcome = 'pick'"
        ),
        1,
        "part A keeps journaling the pick"
    );
    let root = world.tree.path();
    assert_eq!(
        jumped_to(&query(&world, "om", root, false)),
        format!("{om}\n")
    );
    let listed = query(&world, "om", root, true);
    assert!(text(&listed.stdout).starts_with(&format!("{om}\n")));
    let explained = query_with(&world, &["--explain", "om"], root);
    let report = text(&explained.stderr);
    assert!(
        report.starts_with("normalized query: om\nengine: reference\nmemory: disabled\n"),
        "{report}"
    );
    assert!(report.contains("deciding criterion: score\n"), "{report}");
}

#[test]
fn query_memory_respects_local_scope() {
    let world = aged_world(
        &[
            "proj/.git",
            "proj/om",
            "proj/omega",
            "elsewhere/ombi",
            "origin",
        ],
        &["proj/om", "proj/omega", "elsewhere/ombi"],
    );
    pick(&world, "elsewhere/ombi", "om");
    let cwd = world.child("proj");
    let explained = query_with(&world, &["--local", "--explain", "om"], &cwd);
    assert!(
        text(&explained.stderr).contains("\nmemory: not applied (no longer matches)\n"),
        "{}",
        text(&explained.stderr)
    );
    assert_eq!(
        jumped_to(&query_with(&world, &["om"], &cwd)),
        format!("{}\n", canonical_child(&world, "elsewhere/ombi"))
    );
    assert_eq!(
        jumped_to(&query_with(&world, &["--local", "om"], &cwd)),
        format!("{}\n", canonical_child(&world, "proj/om"))
    );
}

#[test]
fn query_memory_jump_journals_the_real_stage() {
    let world = aged_world(
        &["aaa/tokio", "zzz/tokio", "origin"],
        &["aaa/tokio", "zzz/tokio"],
    );
    let zzz = canonical_child(&world, "zzz/tokio");
    pick(&world, "zzz/tokio", "tokoi");
    let out = query_answering(&world, "tokoi", world.tree.path(), "");
    assert_eq!(jumped_to(&out), format!("{zzz}\n"), "no menu opens");
    assert_eq!(
        latest_query_row(&world),
        ("2".to_owned(), "jump".to_owned(), Some(zzz.clone()))
    );
    let (om_world, _, ombi) = om_world();
    pick(&om_world, "dev/ombi", "om");
    let out = query(&om_world, "om", om_world.tree.path(), false);
    assert_eq!(jumped_to(&out), format!("{ombi}\n"));
    assert_eq!(
        latest_query_row(&om_world),
        ("1".to_owned(), "jump".to_owned(), Some(ombi))
    );
}

#[test]
fn query_list_puts_the_remembered_directory_first() {
    let (world, om, ombi) = om_world();
    let root = world.tree.path();
    let before = query(&world, "om", root, true);
    assert_eq!(text(&before.stdout), format!("{om}\n{ombi}\n"));
    pick(&world, "dev/ombi", "om");
    let queries_before = scalar(&db(&world), "SELECT COUNT(*) FROM queries");
    let after = query(&world, "Om", root, true);
    assert!(after.status.success(), "stderr: {}", text(&after.stderr));
    assert_eq!(text(&after.stdout), format!("{ombi}\n{om}\n"));
    assert_eq!(
        scalar(&db(&world), "SELECT COUNT(*) FROM queries"),
        queries_before
    );
}

#[test]
fn query_memory_never_applies_to_the_disk_fallback() {
    let world = aged_world(
        &["tokio", "wd/tokio", "gone/tokio", "origin"],
        &["gone/tokio"],
    );
    let sibling = canonical_child(&world, "tokio");
    let nested = canonical_child(&world, "wd/tokio");
    pick(&world, "gone/tokio", "tokoi");
    std::fs::remove_dir_all(world.child("gone/tokio")).expect("the remembered directory vanishes");
    let out = query_answering(&world, "tokoi", &world.child("wd"), "");
    assert!(!out.status.success(), "stdout: {}", text(&out.stdout));
    assert!(out.stdout.is_empty());
    assert_eq!(
        text(&out.stderr),
        format!(
            "{}furet: no directory selected\n",
            expected_menu(&sibling, &nested)
        )
    );
    assert_eq!(
        latest_query_row(&world),
        ("fallback".to_owned(), "menu".to_owned(), None)
    );
}

#[test]
fn alias_add_defaults_to_the_current_directory() {
    let world = sandbox(&["ombi"]);
    let out = alias(&world, &["add", "Ombi"], &world.child("ombi"));
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    let ombi = canonical_child(&world, "ombi");
    assert_eq!(text(&out.stderr), format!("alias Ombi -> {ombi}\n"));
    let listed = alias(&world, &["list"], world.tree.path());
    assert!(listed.status.success(), "stderr: {}", text(&listed.stderr));
    let stdout = text(&listed.stdout);
    assert_eq!(stdout.lines().count(), 1);
    let mut fields = stdout.trim_end().split('\t');
    assert_eq!(fields.next(), Some("Ombi"));
    assert_eq!(fields.next(), Some(ombi.as_str()));
    let created = fields.next().expect("the created field exists");
    let stamp = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$")
        .expect("the timestamp pattern compiles");
    assert!(
        stamp.is_match(created),
        "created field '{created}' is not a local timestamp"
    );
    assert_eq!(fields.next(), None);
}

#[test]
fn alias_add_canonicalizes_a_relative_path() {
    let world = sandbox(&["ombi"]);
    let out = alias(&world, &["add", "om", "ombi"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let listed = alias(&world, &["list"], world.tree.path());
    let ombi = canonical_child(&world, "ombi");
    let stdout = text(&listed.stdout);
    assert_eq!(stdout.lines().count(), 1);
    let mut fields = stdout.trim_end().split('\t');
    assert_eq!(fields.next(), Some("om"));
    assert_eq!(fields.next(), Some(ombi.as_str()));
}

#[test]
fn alias_add_refuses_an_existing_name_ignoring_case() {
    let world = sandbox(&["ombi", "other"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = alias(&world, &["add", "OMBI", "other"], world.tree.path());
    assert_eq!(out.status.code(), Some(1));
    let ombi = canonical_child(&world, "ombi");
    assert_eq!(
        text(&out.stderr),
        format!("furet: alias 'ombi' already exists ({ombi}); use --force to replace it\n")
    );
    assert!(out.stdout.is_empty());
    let listed = alias(&world, &["list"], world.tree.path());
    let stdout = text(&listed.stdout);
    assert_eq!(stdout.lines().count(), 1);
    let mut fields = stdout.trim_end().split('\t');
    assert_eq!(fields.next(), Some("ombi"));
    assert_eq!(fields.next(), Some(ombi.as_str()));
}

#[test]
fn alias_add_force_replaces_name_and_path() {
    let world = sandbox(&["ombi", "other"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = alias(
        &world,
        &["add", "OMBI", "other", "--force"],
        world.tree.path(),
    );
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let other = canonical_child(&world, "other");
    let listed = alias(&world, &["list"], world.tree.path());
    let stdout = text(&listed.stdout);
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.starts_with(&format!("OMBI\t{other}\t")));
}

#[test]
fn alias_add_rejects_invalid_names() {
    let world = sandbox(&["ombi"]);
    assert!(alias(&world, &["list"], world.tree.path()).status.success());
    for name in ["!ombi", "=ombi", "om bi", "a/b", "a.b", "é", ""] {
        let out = alias(&world, &["add", name, "ombi"], world.tree.path());
        assert_eq!(out.status.code(), Some(1), "name '{name}' must be rejected");
        assert!(
            text(&out.stderr).contains("invalid alias name"),
            "name '{name}' explains the refusal"
        );
    }
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM aliases"), 0);
}

#[test]
fn alias_add_rejects_a_missing_path_and_a_file() {
    let world = sandbox(&["ombi"]);
    assert!(alias(&world, &["list"], world.tree.path()).status.success());
    let file = world.tree.path().join("notes.txt");
    std::fs::write(&file, b"content").expect("the sandbox file is written");
    let missing = alias(&world, &["add", "m", "nope"], world.tree.path());
    assert_eq!(missing.status.code(), Some(1));
    let as_file = alias(&world, &["add", "f", "notes.txt"], world.tree.path());
    assert_eq!(as_file.status.code(), Some(1));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM aliases"), 0);
}

#[test]
fn alias_add_accepts_a_digit_name() {
    let world = sandbox(&["ombi"]);
    let out = alias(&world, &["add", "1", "ombi"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM aliases"), 1);
}

#[test]
fn alias_list_is_empty_then_sorted_by_name_ignoring_case() {
    let world = sandbox(&["ombi"]);
    let empty = alias(&world, &["list"], world.tree.path());
    assert!(empty.status.success(), "stderr: {}", text(&empty.stderr));
    assert!(empty.stdout.is_empty());
    for name in ["zz", "1", "Ab"] {
        assert!(
            alias(&world, &["add", name, "ombi"], world.tree.path())
                .status
                .success()
        );
    }
    let listed = alias(&world, &["list"], world.tree.path());
    let stdout = text(&listed.stdout);
    let names: Vec<&str> = stdout
        .lines()
        .map(|line| line.split('\t').next().unwrap_or_default())
        .collect();
    assert_eq!(names, ["1", "Ab", "zz"]);
}

#[test]
fn alias_remove_ignores_case() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = alias(&world, &["remove", "OMBI"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(text(&out.stderr).contains("removed alias OMBI"));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM aliases"), 0);
}

#[test]
fn alias_remove_of_an_unknown_name_fails() {
    let world = sandbox(&["ombi"]);
    let out = alias(&world, &["remove", "nope"], world.tree.path());
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stderr), "furet: unknown alias 'nope'\n");
    assert!(out.stdout.is_empty());
}

#[test]
fn furet_remove_leaves_aliases_untouched() {
    let world = sandbox(&["ombi"]);
    assert!(
        add(&world, &world.child("ombi"), "session-1", None, None)
            .status
            .success()
    );
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = remove(&world, "ombi", world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
    let ombi = canonical_child(&world, "ombi");
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM aliases"), 1);
    let stored: String = db(&world)
        .query_row("SELECT path FROM aliases", [], |row| row.get(0))
        .expect("the surviving alias reads back");
    assert_eq!(stored, ombi);
}

#[test]
fn alias_add_does_not_record_a_visit() {
    let world = sandbox(&["ombi"]);
    let out = alias(&world, &["add", "ombi", "ombi"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM dirs"), 0);
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM visits"), 0);
}

#[test]
fn query_alias_prints_the_target_and_writes_nothing() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "!ombi", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!("{}\n", canonical_child(&world, "ombi"))
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 0);
}

#[test]
fn query_alias_ignores_case() {
    let world = sandbox(&["ombi", "zebra"]);
    assert!(
        alias(&world, &["add", "ombi", "zebra"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "!OMBI", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!("{}\n", canonical_child(&world, "zebra"))
    );
}

#[test]
fn query_alias_bypasses_the_ranking() {
    let world = sandbox(&["ombi", "apps"]);
    assert!(
        add(&world, &world.child("ombi"), "session-1", None, None)
            .status
            .success()
    );
    assert!(
        alias(&world, &["add", "ombi", "apps"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "!ombi", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!("{}\n", canonical_child(&world, "apps"))
    );
}

#[test]
fn query_unknown_alias_hints_at_a_close_name() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "!mbi", world.tree.path(), false);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(
        text(&out.stderr).contains("furet: unknown alias 'mbi'; did you mean 'ombi'?"),
        "stderr: {}",
        text(&out.stderr)
    );
}

#[test]
fn query_unknown_alias_far_from_every_name_has_no_hint() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "!zzzz", world.tree.path(), false);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("furet: unknown alias 'zzzz'"),
        "stderr: {}",
        text(&out.stderr)
    );
    assert!(!text(&out.stderr).contains("did you mean"));
}

#[test]
fn query_alias_with_another_token_fails() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "!ombi src", world.tree.path(), false);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("furet: an alias takes no other token"),
        "stderr: {}",
        text(&out.stderr)
    );
}

#[test]
fn query_alias_with_local_fails() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = query_with(&world, &["--local", "!ombi"], world.tree.path());
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("furet: --local cannot be combined with an alias"),
        "stderr: {}",
        text(&out.stderr)
    );
    assert!(!text(&out.stderr).contains("not inside a git repository"));
}

#[test]
fn query_alias_to_a_deleted_directory_fails() {
    let world = sandbox(&["ombi"]);
    let ombi = canonical_child(&world, "ombi");
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    std::fs::remove_dir_all(world.child("ombi")).expect("the aliased directory vanishes");
    let out = query(&world, "!ombi", world.tree.path(), false);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains(&format!(
            "furet: alias 'ombi' points to a missing directory: {ombi}"
        )),
        "stderr: {}",
        text(&out.stderr)
    );
}

#[test]
fn query_invalid_alias_names_fail() {
    let world = sandbox(&[]);
    for name in ["!om.bi", "!"] {
        let out = query(&world, name, world.tree.path(), false);
        assert_eq!(out.status.code(), Some(1), "query '{name}' must fail");
        assert!(
            text(&out.stderr).contains("invalid alias name"),
            "query '{name}' explains the refusal: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn query_list_of_an_alias_prints_the_target_alone() {
    let world = sandbox(&["ombi", "zebra"]);
    assert!(
        alias(&world, &["add", "ombi", "zebra"], world.tree.path())
            .status
            .success()
    );
    let out = query_with(&world, &["--list", "!ombi"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!("{}\n", canonical_child(&world, "zebra"))
    );
}

#[test]
fn query_explain_of_an_alias_prints_one_line_on_stderr() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = query_with(&world, &["--explain", "!ombi"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(
        text(&out.stderr).contains(&format!(
            "alias: ombi -> {}",
            canonical_child(&world, "ombi")
        )),
        "stderr: {}",
        text(&out.stderr)
    );
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn alias_prefix_equals_resolves_equals_and_leaves_bang_fuzzy() {
    let world = sandbox(&["ombi", "zebra"]);
    write_config(&world, "alias_prefix = \"=\"");
    assert!(
        alias(&world, &["add", "ombi", "zebra"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "=ombi", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!("{}\n", canonical_child(&world, "zebra"))
    );
    let fuzzy = query(&world, "!ombi", world.tree.path(), false);
    assert!(
        !text(&fuzzy.stdout).contains(&canonical_child(&world, "zebra")),
        "stdout: {}",
        text(&fuzzy.stdout)
    );
}

#[test]
fn an_invalid_alias_prefix_keeps_bang() {
    let world = sandbox(&["ombi", "zebra"]);
    write_config(&world, "alias_prefix = \"@\"");
    assert!(
        alias(&world, &["add", "ombi", "zebra"], world.tree.path())
            .status
            .success()
    );
    for token in ["!ombi", "!OMBI"] {
        let out = query(&world, token, world.tree.path(), false);
        assert!(out.status.success(), "stderr: {}", text(&out.stderr));
        assert_eq!(
            text(&out.stdout),
            format!("{}\n", canonical_child(&world, "zebra"))
        );
    }
}

#[test]
fn alias_complete_lists_matching_names_ignoring_case_in_key_order() {
    let world = sandbox(&["ombi", "omnitool", "apps", "1"]);
    for name in ["ombi", "OmniTool", "apps", "1"] {
        assert!(
            alias(&world, &["add", name, name], world.tree.path())
                .status
                .success()
        );
    }
    let out = alias(&world, &["complete", "!OM"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!(
            "!ombi\t{}\n!OmniTool\t{}\n",
            canonical_child(&world, "ombi"),
            canonical_child(&world, "omnitool")
        )
    );
}

#[test]
fn alias_complete_on_the_bare_prefix_lists_every_alias() {
    let world = sandbox(&["ombi", "omnitool", "apps", "1"]);
    for name in ["ombi", "OmniTool", "apps", "1"] {
        assert!(
            alias(&world, &["add", name, name], world.tree.path())
                .status
                .success()
        );
    }
    let out = alias(&world, &["complete", "!"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let names: Vec<&str> = stdout
        .lines()
        .map(|line| line.split('\t').next().unwrap_or_default())
        .collect();
    assert_eq!(names, ["!1", "!apps", "!ombi", "!OmniTool"]);
}

#[test]
fn alias_complete_ignores_a_word_without_the_configured_prefix() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = alias(&world, &["complete", "om"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    write_config(&world, "alias_prefix = \"=\"");
    let bang = alias(&world, &["complete", "!om"], world.tree.path());
    assert!(bang.status.success(), "stderr: {}", text(&bang.stderr));
    assert!(bang.stdout.is_empty());
    let equals = alias(&world, &["complete", "=om"], world.tree.path());
    assert!(equals.status.success(), "stderr: {}", text(&equals.stderr));
    assert_eq!(
        text(&equals.stdout),
        format!("=ombi\t{}\n", canonical_child(&world, "ombi"))
    );
}

#[test]
fn alias_complete_writes_nothing() {
    let world = sandbox(&["ombi"]);
    assert!(
        alias(&world, &["add", "ombi", "ombi"], world.tree.path())
            .status
            .success()
    );
    let out = alias(&world, &["complete", "!om"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn mark_set_defaults_to_the_cwd_and_overwrites_silently() {
    let world = sandbox(&["a", "b"]);
    let first = mark(&world, &["set", "1"], &world.child("a"));
    assert!(first.status.success(), "stderr: {}", text(&first.stderr));
    assert!(first.stdout.is_empty());
    assert_eq!(
        text(&first.stderr),
        format!("mark 1 -> {}\n", canonical_child(&world, "a"))
    );
    let second = mark(&world, &["set", "1"], &world.child("b"));
    assert!(second.status.success(), "stderr: {}", text(&second.stderr));
    assert_eq!(
        text(&second.stderr),
        format!("mark 1 -> {}\n", canonical_child(&world, "b"))
    );
    assert!(
        !text(&second.stderr).contains("already exists"),
        "overwriting a mark is silent"
    );
    let listed = mark(&world, &["list"], world.tree.path());
    assert!(listed.status.success(), "stderr: {}", text(&listed.stderr));
    assert_eq!(
        text(&listed.stdout),
        format!("1\t{}\n", canonical_child(&world, "b"))
    );
}

#[test]
fn mark_set_takes_an_explicit_path() {
    let world = sandbox(&["a"]);
    let out = mark(&world, &["set", "2", "a"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let listed = mark(&world, &["list"], world.tree.path());
    assert_eq!(
        text(&listed.stdout),
        format!("2\t{}\n", canonical_child(&world, "a"))
    );
}

#[test]
fn mark_set_rejects_anything_but_one_to_nine() {
    let world = sandbox(&["a"]);
    assert!(mark(&world, &["list"], world.tree.path()).status.success());
    for digit in ["0", "10", "a"] {
        let out = mark(&world, &["set", digit, "a"], world.tree.path());
        assert_eq!(
            out.status.code(),
            Some(1),
            "digit '{digit}' must be rejected"
        );
        assert!(
            text(&out.stderr).contains(&format!("invalid mark '{digit}': use a digit from 1 to 9")),
            "digit '{digit}' explains the refusal: {}",
            text(&out.stderr)
        );
    }
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM aliases"), 0);
}

#[test]
fn mark_set_rejects_a_missing_path_and_a_file() {
    let world = sandbox(&["a"]);
    assert!(mark(&world, &["list"], world.tree.path()).status.success());
    let file = world.tree.path().join("notes.txt");
    std::fs::write(&file, b"content").expect("the sandbox file is written");
    let missing = mark(&world, &["set", "1", "nope"], world.tree.path());
    assert_eq!(missing.status.code(), Some(1));
    let as_file = mark(&world, &["set", "1", "notes.txt"], world.tree.path());
    assert_eq!(as_file.status.code(), Some(1));
    assert_eq!(scalar(&db(&world), "SELECT COUNT(*) FROM aliases"), 0);
}

#[test]
fn mark_list_prints_only_marks_in_digit_order() {
    let world = sandbox(&["a", "b", "c"]);
    let empty = mark(&world, &["list"], world.tree.path());
    assert!(empty.status.success(), "stderr: {}", text(&empty.stderr));
    assert!(empty.stdout.is_empty());
    for name in ["ombi", "0"] {
        assert!(
            alias(&world, &["add", name, "a"], world.tree.path())
                .status
                .success()
        );
    }
    assert!(
        mark(&world, &["set", "3", "b"], world.tree.path())
            .status
            .success()
    );
    assert!(
        mark(&world, &["set", "1", "c"], world.tree.path())
            .status
            .success()
    );
    let listed = mark(&world, &["list"], world.tree.path());
    assert!(listed.status.success(), "stderr: {}", text(&listed.stderr));
    assert_eq!(
        text(&listed.stdout),
        format!(
            "1\t{}\n3\t{}\n",
            canonical_child(&world, "c"),
            canonical_child(&world, "b")
        )
    );
}

#[test]
fn mark_delete_one_and_a_range_and_stays_silent_on_unset() {
    let world = sandbox(&["a", "b", "c", "d"]);
    for (digit, child) in [("1", "a"), ("2", "b"), ("4", "c"), ("5", "d")] {
        assert!(
            mark(&world, &["set", digit, child], world.tree.path())
                .status
                .success()
        );
    }
    let ranged = mark(&world, &["delete", "2-4"], world.tree.path());
    assert!(ranged.status.success(), "stderr: {}", text(&ranged.stderr));
    assert_eq!(text(&ranged.stderr), "removed mark 2\nremoved mark 4\n");
    let listed = mark(&world, &["list"], world.tree.path());
    assert_eq!(
        text(&listed.stdout),
        format!(
            "1\t{}\n5\t{}\n",
            canonical_child(&world, "a"),
            canonical_child(&world, "d")
        )
    );
    let unset = mark(&world, &["delete", "3"], world.tree.path());
    assert!(unset.status.success(), "stderr: {}", text(&unset.stderr));
    assert!(unset.stderr.is_empty());
}

#[test]
fn mark_delete_rejects_invalid_specs() {
    let world = sandbox(&["a"]);
    assert!(mark(&world, &["list"], world.tree.path()).status.success());
    for spec in ["0", "4-2", "2-", "2-10", "a"] {
        let out = mark(&world, &["delete", spec], world.tree.path());
        assert_eq!(out.status.code(), Some(1), "spec '{spec}' must be rejected");
        assert!(
            text(&out.stderr).contains("invalid mark range"),
            "spec '{spec}' explains the refusal: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn mark_delete_all_keeps_named_aliases() {
    let world = sandbox(&["a", "b"]);
    for name in ["ombi", "0"] {
        assert!(
            alias(&world, &["add", name, "a"], world.tree.path())
                .status
                .success()
        );
    }
    assert!(
        mark(&world, &["set", "1", "a"], world.tree.path())
            .status
            .success()
    );
    assert!(
        mark(&world, &["set", "9", "b"], world.tree.path())
            .status
            .success()
    );
    let out = mark(&world, &["delete", "--all"], world.tree.path());
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "removed mark 1\nremoved mark 9\n");
    let listed = mark(&world, &["list"], world.tree.path());
    assert!(listed.status.success(), "stderr: {}", text(&listed.stderr));
    assert!(listed.stdout.is_empty());
    let aliases = alias(&world, &["list"], world.tree.path());
    let listing = text(&aliases.stdout);
    let names: Vec<&str> = listing
        .lines()
        .map(|line| line.split('\t').next().unwrap_or_default())
        .collect();
    assert_eq!(names, ["0", "ombi"]);
}

#[test]
fn mark_delete_needs_a_spec_or_all_but_not_both() {
    let world = sandbox(&["a"]);
    assert!(mark(&world, &["list"], world.tree.path()).status.success());
    let bare = mark(&world, &["delete"], world.tree.path());
    assert_eq!(bare.status.code(), Some(2));
    let both = mark(&world, &["delete", "2", "--all"], world.tree.path());
    assert_eq!(both.status.code(), Some(2));
}

#[test]
fn query_unset_mark_says_not_set_without_a_hint() {
    let world = sandbox(&["a"]);
    assert!(
        mark(&world, &["set", "2", "a"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "!3", world.tree.path(), false);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(
        text(&out.stderr).contains("furet: mark 3 not set"),
        "stderr: {}",
        text(&out.stderr)
    );
    assert!(
        !text(&out.stderr).contains("did you mean"),
        "an unset mark gets no suggestion"
    );
    let zero = query(&world, "!0", world.tree.path(), false);
    assert_eq!(zero.status.code(), Some(1));
    assert!(
        text(&zero.stderr).contains("unknown alias '0'"),
        "stderr: {}",
        text(&zero.stderr)
    );
}

#[test]
fn query_mark_to_a_missing_directory_fails() {
    let world = sandbox(&["gone"]);
    assert!(
        mark(&world, &["set", "1", "gone"], world.tree.path())
            .status
            .success()
    );
    let gone = canonical_child(&world, "gone");
    std::fs::remove_dir_all(world.child("gone")).expect("the marked directory vanishes");
    let out = query(&world, "!1", world.tree.path(), false);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains(&format!(
            "furet: mark 1 points to a missing directory: {gone}"
        )),
        "stderr: {}",
        text(&out.stderr)
    );
}

#[test]
fn query_mark_jumps_to_its_target() {
    let world = sandbox(&["zebra"]);
    assert!(
        mark(&world, &["set", "1", "zebra"], world.tree.path())
            .status
            .success()
    );
    let out = query(&world, "!1", world.tree.path(), false);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!("{}\n", canonical_child(&world, "zebra"))
    );
}

#[test]
fn mark_next_and_prev_follow_the_digits_skipping_holes() {
    let world = sandbox(&["a", "b", "c"]);
    for (digit, child) in [("1", "a"), ("3", "b"), ("7", "c")] {
        assert!(
            mark(&world, &["set", digit, child], world.tree.path())
                .status
                .success()
        );
    }
    let next = mark(&world, &["next"], &world.child("b"));
    assert!(next.status.success(), "stderr: {}", text(&next.stderr));
    assert_eq!(
        text(&next.stdout),
        format!("{}\n", canonical_child(&world, "c"))
    );
    let prev = mark(&world, &["prev"], &world.child("b"));
    assert!(prev.status.success(), "stderr: {}", text(&prev.stderr));
    assert_eq!(
        text(&prev.stdout),
        format!("{}\n", canonical_child(&world, "a"))
    );
}

#[test]
fn mark_next_and_prev_wrap_around() {
    let world = sandbox(&["a", "b", "c"]);
    for (digit, child) in [("1", "a"), ("3", "b"), ("7", "c")] {
        assert!(
            mark(&world, &["set", digit, child], world.tree.path())
                .status
                .success()
        );
    }
    let next = mark(&world, &["next"], &world.child("c"));
    assert!(next.status.success(), "stderr: {}", text(&next.stderr));
    assert_eq!(
        text(&next.stdout),
        format!("{}\n", canonical_child(&world, "a"))
    );
    let prev = mark(&world, &["prev"], &world.child("a"));
    assert!(prev.status.success(), "stderr: {}", text(&prev.stderr));
    assert_eq!(
        text(&prev.stdout),
        format!("{}\n", canonical_child(&world, "c"))
    );
}

#[test]
fn mark_cycling_from_an_unmarked_directory_starts_at_an_end() {
    let world = sandbox(&["a", "b", "c", "x"]);
    for (digit, child) in [("1", "a"), ("3", "b"), ("7", "c")] {
        assert!(
            mark(&world, &["set", digit, child], world.tree.path())
                .status
                .success()
        );
    }
    let next = mark(&world, &["next"], &world.child("x"));
    assert!(next.status.success(), "stderr: {}", text(&next.stderr));
    assert_eq!(
        text(&next.stdout),
        format!("{}\n", canonical_child(&world, "a"))
    );
    let prev = mark(&world, &["prev"], &world.child("x"));
    assert!(prev.status.success(), "stderr: {}", text(&prev.stderr));
    assert_eq!(
        text(&prev.stdout),
        format!("{}\n", canonical_child(&world, "c"))
    );
}

#[test]
fn mark_cycling_counts_several_marks_as_the_lowest_and_skips_them() {
    let world = sandbox(&["a", "b", "c"]);
    for (digit, child) in [("1", "c"), ("2", "a"), ("3", "a"), ("5", "b")] {
        assert!(
            mark(&world, &["set", digit, child], world.tree.path())
                .status
                .success()
        );
    }
    let next = mark(&world, &["next"], &world.child("a"));
    assert!(next.status.success(), "stderr: {}", text(&next.stderr));
    assert_eq!(
        text(&next.stdout),
        format!("{}\n", canonical_child(&world, "b"))
    );
    let prev = mark(&world, &["prev"], &world.child("a"));
    assert!(prev.status.success(), "stderr: {}", text(&prev.stderr));
    assert_eq!(
        text(&prev.stdout),
        format!("{}\n", canonical_child(&world, "c"))
    );
}

#[test]
fn mark_cycling_skips_a_missing_directory_with_a_stderr_line() {
    let world = sandbox(&["a", "b", "c"]);
    for (digit, child) in [("1", "a"), ("3", "b"), ("7", "c")] {
        assert!(
            mark(&world, &["set", digit, child], world.tree.path())
                .status
                .success()
        );
    }
    std::fs::remove_dir_all(world.child("b")).expect("the marked directory vanishes");
    let next = mark(&world, &["next"], &world.child("a"));
    assert!(next.status.success(), "stderr: {}", text(&next.stderr));
    assert_eq!(
        text(&next.stdout),
        format!("{}\n", canonical_child(&world, "c"))
    );
    assert!(
        text(&next.stderr).contains("furet: skipped mark 3: missing directory"),
        "stderr: {}",
        text(&next.stderr)
    );
}

#[test]
fn mark_cycling_without_any_mark_fails() {
    let world = sandbox(&["a"]);
    assert!(
        alias(&world, &["add", "ombi", "a"], world.tree.path())
            .status
            .success()
    );
    let next = mark(&world, &["next"], world.tree.path());
    assert_eq!(next.status.code(), Some(1));
    assert!(next.stdout.is_empty());
    assert!(
        text(&next.stderr).contains("furet: no marks set"),
        "stderr: {}",
        text(&next.stderr)
    );
}

#[test]
fn mark_cycling_with_nothing_eligible_fails() {
    let world = sandbox(&["a", "gone"]);
    assert!(
        mark(&world, &["set", "4", "a"], world.tree.path())
            .status
            .success()
    );
    let only = mark(&world, &["next"], &world.child("a"));
    assert_eq!(only.status.code(), Some(1));
    assert!(
        text(&only.stderr).contains("furet: no other mark"),
        "stderr: {}",
        text(&only.stderr)
    );
    assert!(
        mark(&world, &["set", "6", "gone"], world.tree.path())
            .status
            .success()
    );
    std::fs::remove_dir_all(world.child("gone")).expect("the marked directory vanishes");
    let after = mark(&world, &["next"], &world.child("a"));
    assert_eq!(after.status.code(), Some(1));
    let stderr = text(&after.stderr);
    assert!(
        stderr.contains("furet: skipped mark 6: missing directory")
            && stderr.contains("furet: no other mark"),
        "stderr: {stderr}"
    );
}

#[test]
fn mark_cycling_to_a_single_mark_from_elsewhere() {
    let world = sandbox(&["a", "x"]);
    assert!(
        mark(&world, &["set", "4", "a"], world.tree.path())
            .status
            .success()
    );
    let next = mark(&world, &["next"], &world.child("x"));
    assert!(next.status.success(), "stderr: {}", text(&next.stderr));
    assert_eq!(
        text(&next.stdout),
        format!("{}\n", canonical_child(&world, "a"))
    );
    let prev = mark(&world, &["prev"], &world.child("x"));
    assert!(prev.status.success(), "stderr: {}", text(&prev.stderr));
    assert_eq!(
        text(&prev.stdout),
        format!("{}\n", canonical_child(&world, "a"))
    );
}

#[test]
fn mark_cycling_writes_nothing() {
    let world = sandbox(&["a", "b", "c", "x"]);
    for (digit, child) in [("1", "a"), ("3", "b"), ("7", "c")] {
        assert!(
            mark(&world, &["set", digit, child], world.tree.path())
                .status
                .success()
        );
    }
    assert!(mark(&world, &["next"], &world.child("b")).status.success());
    assert!(mark(&world, &["prev"], &world.child("b")).status.success());
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn mark_commands_write_no_visit_and_no_query() {
    let world = sandbox(&["a"]);
    assert!(mark(&world, &["list"], world.tree.path()).status.success());
    assert!(
        mark(&world, &["set", "1", "a"], world.tree.path())
            .status
            .success()
    );
    assert!(mark(&world, &["list"], world.tree.path()).status.success());
    assert!(
        mark(&world, &["delete", "1"], world.tree.path())
            .status
            .success()
    );
    let conn = db(&world);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM visits"), 0);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM queries"), 0);
}

#[test]
fn sandbox_tree_names_hold_no_letters() {
    let first = sandbox(&[]);
    let second = sandbox(&[]);
    for world in [&first, &second] {
        let name = world
            .tree
            .path()
            .file_name()
            .expect("the tree has a name")
            .to_string_lossy();
        assert!(
            name.chars().all(|c| c.is_ascii_digit() || c == '-'),
            "the tree name holds letters: {name}"
        );
    }
    assert_ne!(
        first.tree.path().file_name(),
        second.tree.path().file_name()
    );
}
