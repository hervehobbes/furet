use std::env;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use thiserror::Error;

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

/// Resolves the database file path: `FURET_DATA_DIR/furet.db` when the
/// variable is set (tests rely on this), else the platform data directory.
pub fn db_path() -> Result<PathBuf, StorageError> {
    if let Some(dir) = env::var_os("FURET_DATA_DIR") {
        return Ok(Path::new(&dir).join(DB_FILE_NAME));
    }
    let base = dirs::data_local_dir().ok_or(StorageError::NoDataDir)?;
    Ok(base.join(APP_DIR_NAME).join(DB_FILE_NAME))
}

/// Opens the database at the resolved location, creating the file and its
/// parent directory if needed, then configures and migrates it.
pub fn open() -> Result<Connection, StorageError> {
    let path = db_path()?;
    open_at(&path)
}

fn open_at(path: &Path) -> Result<Connection, StorageError> {
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

#[cfg(test)]
mod tests {
    use super::{db_path, open, open_at};
    use rusqlite::{Connection, params};
    use std::path::{Path, PathBuf};

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
}
