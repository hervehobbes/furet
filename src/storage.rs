use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use thiserror::Error;
use tracing::debug;

use crate::calibration;
use crate::clock::Timestamp;

/// Failure to resolve the database location, or to open, configure, or
/// migrate the database file.
#[derive(Debug, Error)]
pub enum StorageError {
    /// The platform offers no local data directory to store the database in.
    #[error("the platform local data directory is unavailable")]
    NoDataDir,
    /// The directory meant to hold the database could not be created.
    #[error("cannot create the data directory {path}: {source}")]
    CreateDataDir {
        path: PathBuf,
        source: std::io::Error,
    },
    /// SQLite rejected an open, pragma, or migration statement.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

const DB_FILE_NAME: &str = "furet.db";
const APP_DIR_NAME: &str = "furet";

// WHY: ts columns are INTEGER Unix seconds to match clock::Timestamp.
const MIGRATIONS: &[&str] = &["CREATE TABLE dirs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        path TEXT NOT NULL,
        key TEXT NOT NULL,
        first_seen INTEGER NOT NULL,
        missing_since INTEGER
    );
    CREATE UNIQUE INDEX idx_dirs_key ON dirs (key);
    CREATE TABLE visits (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        dir_id INTEGER NOT NULL REFERENCES dirs (id),
        ts INTEGER NOT NULL,
        source TEXT NOT NULL CHECK (source IN ('hook', 'jump', 'back', 'up', 'fallback', 'import')),
        session TEXT NOT NULL,
        from_dir_id INTEGER REFERENCES dirs (id)
    );
    CREATE TABLE queries (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ts INTEGER NOT NULL,
        cwd TEXT NOT NULL,
        query TEXT NOT NULL,
        result_dir_id INTEGER REFERENCES dirs (id),
        stage TEXT NOT NULL CHECK (stage IN ('1', '2', 'fallback', 'menu')),
        outcome TEXT NOT NULL
    );"];

/// Resolves the directory holding the database and the log files:
/// `FURET_DATA_DIR` when set, else the platform local data dir plus `furet`.
pub fn data_dir() -> Result<PathBuf, StorageError> {
    if let Some(dir) = env::var_os("FURET_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    dirs::data_local_dir()
        .map(|base| base.join(APP_DIR_NAME))
        .ok_or(StorageError::NoDataDir)
}

/// Resolves the database file path: `FURET_DATA_DIR/furet.db` when the
/// variable is set (tests rely on this), else the platform data directory.
pub fn db_path() -> Result<PathBuf, StorageError> {
    Ok(data_dir()?.join(DB_FILE_NAME))
}

/// Opens the database at the resolved location, creating the file and its
/// parent directory if needed, then configures and migrates it.
pub fn open() -> Result<Connection, StorageError> {
    let path = db_path()?;
    open_at(&path)
}

fn open_at(path: &Path) -> Result<Connection, StorageError> {
    debug!(path = %path.display(), "database open/migration");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| StorageError::CreateDataDir {
            path: parent.to_owned(),
            source,
        })?;
    }
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<(), StorageError> {
    conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    })?;
    // WHY: foreign_keys is a per-connection setting, never persisted by SQLite.
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}

fn migrate(conn: &mut Connection) -> Result<(), StorageError> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (offset, script) in MIGRATIONS.iter().enumerate() {
        let version = offset as i64 + 1;
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(script)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

/// One `dirs` row flattened for ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The `dirs.id` of this row.
    pub id: i64,
    /// Canonical displayable path.
    pub path: String,
    /// Most recent visit, or `first_seen` when no visit exists yet.
    pub last_visit: Timestamp,
    /// Whether `missing_since` is set (soft delete, SPEC section 10).
    pub missing: bool,
}

/// Inserts a `dirs` row for `key` and returns its id; on conflict the row
/// keeps its `path` and `first_seen` and is reactivated (SPEC section 10).
pub fn upsert_dir(
    conn: &Connection,
    path: &str,
    key: &str,
    first_seen: Timestamp,
) -> Result<i64, StorageError> {
    conn.execute(
        "INSERT INTO dirs (path, key, first_seen) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE SET missing_since = NULL",
        params![path, key, first_seen.unix_seconds()],
    )?;
    conn.query_row("SELECT id FROM dirs WHERE key = ?1", params![key], |row| {
        row.get(0)
    })
    .map_err(StorageError::from)
}

/// The id of the `dirs` row matching `key`, or `None` when no such row
/// exists; this lookup never creates a row.
pub fn dir_id_by_key(conn: &Connection, key: &str) -> Result<Option<i64>, StorageError> {
    let mut stmt = conn.prepare("SELECT id FROM dirs WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

/// Inserts one `visits` row; `source` must satisfy the table CHECK.
pub fn insert_visit(
    conn: &Connection,
    dir_id: i64,
    ts: Timestamp,
    source: &str,
    session: &str,
    from_dir_id: Option<i64>,
) -> Result<(), StorageError> {
    debug!(dir_id, source, session, "visit insert");
    conn.execute(
        "INSERT INTO visits (dir_id, ts, source, session, from_dir_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![dir_id, ts.unix_seconds(), source, session, from_dir_id],
    )?;
    Ok(())
}

/// Reads every `dirs` row with its most recent visit and its soft-delete
/// flag, ready to be turned into ranking candidates.
pub fn dir_entries(conn: &Connection) -> Result<Vec<DirEntry>, StorageError> {
    // WHY: a dir upserted before any visit falls back to first_seen.
    let mut stmt = conn.prepare(
        "SELECT dirs.id,
                dirs.path,
                COALESCE(latest.ts, dirs.first_seen),
                dirs.missing_since IS NOT NULL
         FROM dirs
         LEFT JOIN (SELECT dir_id, MAX(ts) AS ts
                    FROM visits
                    GROUP BY dir_id) AS latest
           ON latest.dir_id = dirs.id",
    )?;
    let entries = stmt
        .query_map([], |row| {
            Ok(DirEntry {
                id: row.get(0)?,
                path: row.get(1)?,
                last_visit: Timestamp::from_unix_seconds(row.get(2)?),
                missing: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

/// Sets or clears one directory's soft-delete timestamp by row id.
pub fn set_missing_since(
    conn: &Connection,
    dir_id: i64,
    missing_since: Option<Timestamp>,
) -> Result<(), StorageError> {
    conn.execute(
        "UPDATE dirs SET missing_since = ?1 WHERE id = ?2",
        params![missing_since.map(|ts| ts.unix_seconds()), dir_id],
    )?;
    Ok(())
}

/// The path of the second-to-last visited directory for `session`, ordered
/// by `ts` then `id` descending; `None` when fewer than two visits exist.
pub fn last_visited_dir(conn: &Connection, session: &str) -> Result<Option<String>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT dirs.path
         FROM visits
         JOIN dirs ON dirs.id = visits.dir_id
         WHERE visits.session = ?1
         ORDER BY visits.ts DESC, visits.id DESC
         LIMIT 1 OFFSET 1",
    )?;
    let mut rows = stmt.query(params![session])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

/// Inserts one `queries` row, journaling a real query decision (SPEC section 15).
pub fn insert_query(
    conn: &Connection,
    ts: Timestamp,
    cwd: &str,
    query: &str,
    result_dir_id: Option<i64>,
    stage: &str,
    outcome: &str,
) -> Result<(), StorageError> {
    debug!(cwd, query, stage, outcome, "query journal insert");
    conn.execute(
        "INSERT INTO queries (ts, cwd, query, result_dir_id, stage, outcome)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ts.unix_seconds(), cwd, query, result_dir_id, stage, outcome],
    )?;
    Ok(())
}

/// Reads every `queries` row oldest first, ready for calibration (SPEC section 15).
pub fn query_log(conn: &Connection) -> Result<Vec<calibration::QueryRecord>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, cwd, query, result_dir_id, stage, outcome FROM queries ORDER BY ts ASC",
    )?;
    let records = stmt
        .query_map([], |row| {
            Ok(calibration::QueryRecord {
                id: row.get(0)?,
                ts: Timestamp::from_unix_seconds(row.get(1)?),
                cwd: row.get(2)?,
                query: row.get(3)?,
                result_dir_id: row.get(4)?,
                stage: row.get(5)?,
                outcome: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

/// Reads every `visits` row oldest first, ready for calibration (SPEC section 15).
pub fn visit_log(conn: &Connection) -> Result<Vec<calibration::VisitRecord>, StorageError> {
    let mut stmt =
        conn.prepare("SELECT dir_id, ts, source, session FROM visits ORDER BY ts ASC")?;
    let records = stmt
        .query_map([], |row| {
            Ok(calibration::VisitRecord {
                dir_id: row.get(0)?,
                ts: Timestamp::from_unix_seconds(row.get(1)?),
                source: row.get(2)?,
                session: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

/// Every `dirs.key` currently stored, used to keep `furet import` idempotent.
pub fn known_keys(conn: &Connection) -> Result<HashSet<String>, StorageError> {
    let mut stmt = conn.prepare("SELECT key FROM dirs")?;
    let keys = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<HashSet<String>, _>>()?;
    Ok(keys)
}

/// The `path` of the `dirs` row `dir_id`, or `None` when no such row exists.
pub fn dir_path_by_id(conn: &Connection, dir_id: i64) -> Result<Option<String>, StorageError> {
    let mut stmt = conn.prepare("SELECT path FROM dirs WHERE id = ?1")?;
    let mut rows = stmt.query(params![dir_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        db_path, dir_entries, dir_id_by_key, dir_path_by_id, insert_query, insert_visit,
        known_keys, last_visited_dir, open, open_at, query_log, set_missing_since, upsert_dir,
        visit_log,
    };
    use crate::clock::Timestamp;
    use rusqlite::{Connection, params};
    use std::path::{Path, PathBuf};

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_seconds(1_000_000 + seconds)
    }

    fn temp_db() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a fresh temporary directory");
        let path = dir.path().join("furet.db");
        (dir, path)
    }

    fn opened(path: &Path) -> Connection {
        open_at(path).expect("the fixture database opens and migrates")
    }

    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user_version is readable")
    }

    fn managed_table_count(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('dirs', 'visits', 'queries')",
            [],
            |row| row.get(0),
        )
        .expect("sqlite_master is readable")
    }

    fn insert_dir(conn: &Connection, path: &str, key: &str) -> i64 {
        conn.execute(
            "INSERT INTO dirs (path, key, first_seen) VALUES (?1, ?2, 1_700_000_000)",
            params![path, key],
        )
        .expect("the fixture dir row inserts");
        conn.last_insert_rowid()
    }

    #[test]
    fn fresh_database_creates_all_tables_and_reaches_user_version_1() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        assert_eq!(user_version(&conn), 1);
        assert_eq!(managed_table_count(&conn), 3);
    }

    #[test]
    fn migrating_an_already_migrated_database_is_a_noop() {
        let (_dir, path) = temp_db();
        drop(opened(&path));
        let conn = opened(&path);
        assert_eq!(user_version(&conn), 1);
        assert_eq!(managed_table_count(&conn), 3);
        let dirs_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'dirs'",
                [],
                |row| row.get(0),
            )
            .expect("sqlite_master is readable");
        assert_eq!(dirs_tables, 1);
    }

    #[test]
    fn opened_databases_run_in_wal_mode() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal_mode is readable");
        assert_eq!(mode, "wal");
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let rejected = conn.execute(
            "INSERT INTO visits (dir_id, ts, source, session) VALUES (999, 1, 'hook', 's')",
            [],
        );
        assert!(
            rejected.is_err(),
            "a visit referencing a missing dir must be rejected"
        );
    }

    #[test]
    fn source_check_accepts_every_valid_value_and_rejects_others() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let dir_id = insert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio");
        for source in ["hook", "jump", "back", "up", "fallback", "import"] {
            let accepted = conn.execute(
                "INSERT INTO visits (dir_id, ts, source, session) VALUES (?1, 1, ?2, 's')",
                params![dir_id, source],
            );
            assert!(accepted.is_ok(), "source '{source}' must be accepted");
        }
        let rejected = conn.execute(
            "INSERT INTO visits (dir_id, ts, source, session) VALUES (?1, 1, 'manual', 's')",
            params![dir_id],
        );
        assert!(rejected.is_err(), "an unlisted source must be rejected");
    }

    #[test]
    fn stage_check_accepts_every_valid_value_and_rejects_others() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        for stage in ["1", "2", "fallback", "menu"] {
            let accepted = conn.execute(
                "INSERT INTO queries (ts, cwd, query, stage, outcome) VALUES (1, 'c:\\dev', 'tok', ?1, 'jump')",
                params![stage],
            );
            assert!(accepted.is_ok(), "stage '{stage}' must be accepted");
        }
        let rejected = conn.execute(
            "INSERT INTO queries (ts, cwd, query, stage, outcome) VALUES (1, 'c:\\dev', 'tok', '3', 'jump')",
            [],
        );
        assert!(rejected.is_err(), "an unlisted stage must be rejected");
    }

    #[test]
    fn rows_round_trip_with_their_expected_values() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let dir_id = insert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio");
        conn.execute(
            "INSERT INTO visits (dir_id, ts, source, session) VALUES (?1, 1_700_000_050, 'jump', 'session-1')",
            params![dir_id],
        )
        .expect("the fixture visit inserts");
        conn.execute(
            "INSERT INTO queries (ts, cwd, query, result_dir_id, stage, outcome) VALUES (1_700_000_060, 'c:\\dev', 'tok', ?1, '1', 'jump')",
            params![dir_id],
        )
        .expect("the fixture query inserts");

        let dir = conn
            .query_row(
                "SELECT path, key, first_seen, missing_since FROM dirs WHERE id = ?1",
                params![dir_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .expect("the dir row reads back");
        assert_eq!(dir.0, "c:\\dev\\tokio");
        assert_eq!(dir.1, "c:\\dev\\tokio");
        assert_eq!(dir.2, 1_700_000_000);
        assert_eq!(dir.3, None);

        let visit = conn
            .query_row(
                "SELECT ts, source, session, from_dir_id FROM visits WHERE dir_id = ?1",
                params![dir_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .expect("the visit row reads back");
        assert_eq!(visit.0, 1_700_000_050);
        assert_eq!(visit.1, "jump");
        assert_eq!(visit.2, "session-1");
        assert_eq!(visit.3, None);

        let query = conn
            .query_row(
                "SELECT cwd, query, result_dir_id, stage, outcome FROM queries WHERE query = 'tok'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .expect("the query row reads back");
        assert_eq!(query.0, "c:\\dev");
        assert_eq!(query.1, "tok");
        assert_eq!(query.2, Some(dir_id));
        assert_eq!(query.3, "1");
        assert_eq!(query.4, "jump");

        conn.execute(
            "INSERT INTO queries (ts, cwd, query, result_dir_id, stage, outcome) VALUES (1, 'c:\\', 'nope', NULL, 'fallback', 'none')",
            [],
        )
        .expect("the fixture failed query inserts");
        let result_dir_id: Option<i64> = conn
            .query_row(
                "SELECT result_dir_id FROM queries WHERE query = 'nope'",
                [],
                |row| row.get(0),
            )
            .expect("the failed query reads back");
        assert_eq!(result_dir_id, None);
    }

    #[test]
    fn duplicate_comparison_keys_are_rejected() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        insert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio");
        let rejected = conn.execute(
            "INSERT INTO dirs (path, key, first_seen) VALUES ('C:\\Dev\\Tokio', 'c:\\dev\\tokio', 1)",
            [],
        );
        assert!(
            rejected.is_err(),
            "two dirs must never share a comparison key"
        );
    }

    #[test]
    fn furet_data_dir_overrides_the_database_location() {
        let dir = tempfile::tempdir().expect("a fresh temporary directory");
        let nested = dir.path().join("nested");
        unsafe { std::env::set_var("FURET_DATA_DIR", &nested) };
        let resolved = db_path();
        let connection = open();
        unsafe { std::env::remove_var("FURET_DATA_DIR") };
        assert_eq!(
            resolved.expect("db_path resolves under FURET_DATA_DIR"),
            nested.join("furet.db")
        );
        let conn = connection.expect("open works under FURET_DATA_DIR");
        assert_eq!(user_version(&conn), 1);
        assert!(nested.join("furet.db").exists());
    }

    #[test]
    fn upsert_dir_reuses_the_existing_row_without_touching_it() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let first = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the first upsert runs");
        let second = upsert_dir(&conn, "C:\\Dev\\TOKIO", "c:\\dev\\tokio", at(200))
            .expect("the second upsert runs");
        assert_eq!(first, second);
        assert_eq!(row_count(&conn, "dirs"), 1);
        let (path, first_seen): (String, i64) = conn
            .query_row("SELECT path, first_seen FROM dirs", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("the single dirs row reads back");
        assert_eq!(path, "c:\\dev\\tokio");
        assert_eq!(first_seen, at(100).unix_seconds());
    }

    #[test]
    fn dir_id_by_key_reports_known_and_unknown_keys() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let inserted = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture upsert runs");
        let known = dir_id_by_key(&conn, "c:\\dev\\tokio").expect("the known lookup runs");
        let unknown = dir_id_by_key(&conn, "c:\\dev\\nope").expect("the unknown lookup runs");
        assert_eq!(known, Some(inserted));
        assert_eq!(unknown, None);
    }

    #[test]
    fn insert_visit_stores_every_column() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let dir = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture dir upserts");
        let origin = upsert_dir(&conn, "c:\\dev\\helix", "c:\\dev\\helix", at(100))
            .expect("the fixture origin upserts");
        insert_visit(&conn, dir, at(150), "jump", "session-7", Some(origin))
            .expect("the visit inserts");
        let (ts, source, session, from_dir_id): (i64, String, String, Option<i64>) = conn
            .query_row(
                "SELECT ts, source, session, from_dir_id FROM visits",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("the single visits row reads back");
        assert_eq!(ts, at(150).unix_seconds());
        assert_eq!(source, "jump");
        assert_eq!(session, "session-7");
        assert_eq!(from_dir_id, Some(origin));
    }

    #[test]
    fn dir_entries_take_the_latest_visit_and_report_missing() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture dir upserts");
        insert_visit(&conn, tokio, at(110), "hook", "s", None).expect("the first visit inserts");
        insert_visit(&conn, tokio, at(140), "hook", "s", None).expect("the second visit inserts");
        let orphan = upsert_dir(&conn, "c:\\dev\\orphan", "c:\\dev\\orphan", at(120))
            .expect("the visit-less dir upserts");
        conn.execute(
            "UPDATE dirs SET missing_since = 900 WHERE id = ?1",
            params![tokio],
        )
        .expect("the fixture marks the dir missing");
        let mut entries = dir_entries(&conn).expect("the entries read");
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, orphan);
        assert_eq!(entries[0].path, "c:\\dev\\orphan");
        assert_eq!(entries[0].last_visit, at(120));
        assert!(!entries[0].missing);
        assert_eq!(entries[1].id, tokio);
        assert_eq!(entries[1].path, "c:\\dev\\tokio");
        assert_eq!(entries[1].last_visit, at(140));
        assert!(entries[1].missing);
    }

    #[test]
    fn set_missing_since_round_trips_a_timestamp_and_none() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let dir_id = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture dir upserts");
        set_missing_since(&conn, dir_id, Some(at(500))).expect("marking the dir missing runs");
        let marked: Option<i64> = conn
            .query_row(
                "SELECT missing_since FROM dirs WHERE id = ?1",
                params![dir_id],
                |row| row.get(0),
            )
            .expect("the marked row reads back");
        assert_eq!(marked, Some(at(500).unix_seconds()));
        set_missing_since(&conn, dir_id, None).expect("reactivating the dir runs");
        let cleared: Option<i64> = conn
            .query_row(
                "SELECT missing_since FROM dirs WHERE id = ?1",
                params![dir_id],
                |row| row.get(0),
            )
            .expect("the reactivated row reads back");
        assert_eq!(cleared, None);
    }

    #[test]
    fn upsert_dir_reactivates_a_missing_row_without_touching_path_or_first_seen() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let first = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the first upsert runs");
        set_missing_since(&conn, first, Some(at(150))).expect("the fixture marks the dir missing");
        let second = upsert_dir(&conn, "C:\\Dev\\TOKIO", "c:\\dev\\tokio", at(200))
            .expect("the second upsert runs");
        assert_eq!(first, second);
        assert_eq!(row_count(&conn, "dirs"), 1);
        let (path, first_seen, missing_since): (String, i64, Option<i64>) = conn
            .query_row(
                "SELECT path, first_seen, missing_since FROM dirs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("the single dirs row reads back");
        assert_eq!(path, "c:\\dev\\tokio");
        assert_eq!(first_seen, at(100).unix_seconds());
        assert_eq!(missing_since, None);
    }

    #[test]
    fn last_visited_dir_returns_the_second_to_last_visit_for_the_session() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the tokio dir upserts");
        let helix = upsert_dir(&conn, "c:\\dev\\helix", "c:\\dev\\helix", at(100))
            .expect("the helix dir upserts");
        let tokei = upsert_dir(&conn, "c:\\dev\\tokei", "c:\\dev\\tokei", at(100))
            .expect("the tokei dir upserts");
        insert_visit(&conn, tokio, at(110), "jump", "session-1", None)
            .expect("the first visit inserts");
        insert_visit(&conn, helix, at(120), "jump", "session-1", None)
            .expect("the second visit inserts");
        insert_visit(&conn, tokei, at(130), "jump", "session-1", None)
            .expect("the third visit inserts");
        insert_visit(&conn, tokio, at(200), "hook", "decoy-session", None)
            .expect("the decoy visit inserts");
        let previous =
            last_visited_dir(&conn, "session-1").expect("the second-to-last lookup runs");
        assert_eq!(previous, Some("c:\\dev\\helix".to_owned()));
    }

    #[test]
    fn last_visited_dir_returns_none_below_two_visits() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let empty = last_visited_dir(&conn, "no-visits").expect("the empty lookup runs");
        assert_eq!(empty, None);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the tokio dir upserts");
        insert_visit(&conn, tokio, at(110), "jump", "one-visit", None)
            .expect("the single visit inserts");
        let single = last_visited_dir(&conn, "one-visit").expect("the single-visit lookup runs");
        assert_eq!(single, None);
    }

    #[test]
    fn known_keys_reports_every_stored_key() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the first fixture dir upserts");
        upsert_dir(&conn, "c:\\dev\\helix", "c:\\dev\\helix", at(100))
            .expect("the second fixture dir upserts");
        let keys = known_keys(&conn).expect("known keys read back");
        assert_eq!(keys.len(), 2);
        assert!(keys.contains("c:\\dev\\tokio"));
        assert!(keys.contains("c:\\dev\\helix"));
    }

    fn row_count(conn: &Connection, table: &str) -> i64 {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        conn.query_row(&sql, [], |row| row.get(0))
            .expect("the count reads")
    }

    #[test]
    fn insert_query_and_query_log_round_trip_every_field() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture dir upserts");
        insert_query(&conn, at(150), "c:\\dev", "tok", Some(tokio), "1", "jump")
            .expect("the jump query inserts");
        insert_query(&conn, at(160), "c:\\dev", "nope", None, "fallback", "none")
            .expect("the failed query inserts");
        let records = query_log(&conn).expect("the query log reads");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].ts, at(150));
        assert_eq!(records[0].cwd, "c:\\dev");
        assert_eq!(records[0].query, "tok");
        assert_eq!(records[0].result_dir_id, Some(tokio));
        assert_eq!(records[0].stage, "1");
        assert_eq!(records[0].outcome, "jump");
        assert_eq!(records[1].ts, at(160));
        assert_eq!(records[1].result_dir_id, None);
        assert_eq!(records[1].stage, "fallback");
        assert_eq!(records[1].outcome, "none");
    }

    #[test]
    fn visit_log_reads_inserted_visits_in_ts_order() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture dir upserts");
        insert_visit(&conn, tokio, at(120), "hook", "session-1", None)
            .expect("the second visit inserts");
        insert_visit(&conn, tokio, at(110), "jump", "session-1", None)
            .expect("the first visit inserts");
        let records = visit_log(&conn).expect("the visit log reads");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].ts, at(110));
        assert_eq!(records[0].dir_id, tokio);
        assert_eq!(records[0].source, "jump");
        assert_eq!(records[0].session, "session-1");
        assert_eq!(records[1].ts, at(120));
        assert_eq!(records[1].source, "hook");
    }

    #[test]
    fn dir_path_by_id_reports_known_and_unknown_ids() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture dir upserts");
        assert_eq!(
            dir_path_by_id(&conn, tokio).expect("the known lookup runs"),
            Some("c:\\dev\\tokio".to_owned())
        );
        assert_eq!(
            dir_path_by_id(&conn, tokio + 1).expect("the unknown lookup runs"),
            None
        );
    }
}
