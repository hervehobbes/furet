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

fn query_answering(sandbox: &Sandbox, query: &str, cwd: &Path, answer: &str) -> Output {
    let mut cmd = sandbox.furet();
    cmd.arg("query")
        .arg(query)
        .current_dir(cwd)
        .write_stdin(answer);
    run(&mut cmd)
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
    assert!(!out.stderr.is_empty());
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
