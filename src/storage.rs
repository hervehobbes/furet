use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use thiserror::Error;
use tracing::debug;

use crate::calibration;
use crate::clock::Timestamp;
use crate::memory::{self, Recall};
use crate::stats;

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
const CONFIG_FILE_NAME: &str = "config.toml";
const LOGS_DIR_NAME: &str = "logs";
const APP_DIR_NAME: &str = "furet";

// WHY: ts columns are INTEGER Unix seconds to match clock::Timestamp.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE dirs (
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
    );",
    // WHY: these indexes keep the ranking reads from scanning all of visits as it grows.
    "CREATE INDEX idx_visits_dir_ts ON visits (dir_id, ts);
    CREATE INDEX idx_visits_session_ts ON visits (session, ts);",
    // WHY: the retention purge deletes by age on every add, which must not scan either journal.
    "CREATE INDEX idx_visits_ts ON visits (ts);
    CREATE INDEX idx_queries_ts ON queries (ts);",
    // WHY: aliases and marks keep a plain path, so remove, the purge and exclude_dirs never touch them.
    "CREATE TABLE aliases (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        key TEXT NOT NULL,
        path TEXT NOT NULL,
        created INTEGER NOT NULL
    );
    CREATE UNIQUE INDEX idx_aliases_key ON aliases (key);",
];

/// Resolves the directory holding the database and the log files:
/// `FURET_DATA_DIR` when set, else the platform local data dir plus `furet`.
pub fn data_dir() -> Result<PathBuf, StorageError> {
    resolve_data_dir(env::var_os("FURET_DATA_DIR"))
}

// WHY: an empty override would otherwise put furet.db in whatever directory the hook runs from.
fn resolve_data_dir(overridden: Option<OsString>) -> Result<PathBuf, StorageError> {
    if let Some(dir) = overridden.filter(|dir| !dir.is_empty()) {
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

/// Resolves the config file path: `config.toml` in the data directory.
pub fn config_path() -> Result<PathBuf, StorageError> {
    Ok(data_dir()?.join(CONFIG_FILE_NAME))
}

/// Resolves the log directory: `logs` in the data directory.
pub fn logs_dir() -> Result<PathBuf, StorageError> {
    Ok(data_dir()?.join(LOGS_DIR_NAME))
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
    // WHY: NORMAL is WAL's pairing; it drops the per-commit fsync every `furet add` pays.
    conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    // WHY: concurrent prompt-hook writers wait up to 5 s instead of failing locked.
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
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
    conn.prepare_cached(
        "INSERT INTO dirs (path, key, first_seen) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE SET missing_since = NULL
         RETURNING id",
    )?
    .query_row(params![path, key, first_seen.unix_seconds()], |row| {
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

/// Every `dirs` row id with a set `missing_since`, read before overlaying
/// a reconcile's in-memory updates.
pub fn missing_since_by_id(conn: &Connection) -> Result<HashMap<i64, Timestamp>, StorageError> {
    let mut stmt =
        conn.prepare("SELECT id, missing_since FROM dirs WHERE missing_since IS NOT NULL")?;
    let markers = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                Timestamp::from_unix_seconds(row.get(1)?),
            ))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(markers)
}

/// Formats `ts` in local time with the same SQLite expression as
/// `dir_listing`.
pub fn format_local_time(conn: &Connection, ts: Timestamp) -> Result<String, StorageError> {
    conn.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S', ?1, 'unixepoch', 'localtime')",
        params![ts.unix_seconds()],
        |row| row.get(0),
    )
    .map_err(StorageError::from)
}

/// Deletes the `visits` and `queries` rows older than `cutoff`, keeping every
/// `dirs` row; returns how many visits and queries were deleted.
pub fn purge_before(conn: &Connection, cutoff: Timestamp) -> Result<(usize, usize), StorageError> {
    let visits = conn.execute(
        "DELETE FROM visits WHERE ts < ?1",
        params![cutoff.unix_seconds()],
    )?;
    let queries = conn.execute(
        "DELETE FROM queries WHERE ts < ?1",
        params![cutoff.unix_seconds()],
    )?;
    Ok((visits, queries))
}

/// Deletes the `dirs` rows with these ids and their `visits`/`queries` rows
/// in one transaction, setting other visits' `from_dir_id` back to `NULL`.
pub fn remove_dirs(conn: &Connection, ids: &[i64]) -> Result<usize, StorageError> {
    let tx = conn.unchecked_transaction()?;
    let mut removed = 0usize;
    for id in ids {
        tx.prepare_cached("UPDATE visits SET from_dir_id = NULL WHERE from_dir_id = ?1")?
            .execute(params![*id])?;
        tx.prepare_cached("DELETE FROM visits WHERE dir_id = ?1")?
            .execute(params![*id])?;
        tx.prepare_cached("DELETE FROM queries WHERE result_dir_id = ?1")?
            .execute(params![*id])?;
        removed += tx
            .prepare_cached("DELETE FROM dirs WHERE id = ?1")?
            .execute(params![*id])?;
    }
    tx.commit()?;
    Ok(removed)
}

/// One `aliases` row, looked up by its case-insensitive key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    /// The name as the user typed it, shown in listings.
    pub name: String,
    /// Canonical displayable path of the target directory.
    pub path: String,
}

/// One `aliases` row flattened for `furet alias list`, its `created` already
/// formatted in local time by SQLite itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasListing {
    /// The name as the user typed it, shown in listings.
    pub name: String,
    /// Canonical displayable path of the target directory.
    pub path: String,
    /// When the entry was created or last overwritten, in local time.
    pub created: String,
}

/// The `aliases` row matching `key`, or `None` when no such row exists.
pub fn alias_by_key(conn: &Connection, key: &str) -> Result<Option<Alias>, StorageError> {
    let mut stmt = conn.prepare("SELECT name, path FROM aliases WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    match rows.next()? {
        Some(row) => Ok(Some(Alias {
            name: row.get(0)?,
            path: row.get(1)?,
        })),
        None => Ok(None),
    }
}

/// Inserts or replaces the `aliases` row for `key`, refreshing `name`,
/// `path`, and `created`.
pub fn upsert_alias(
    conn: &Connection,
    name: &str,
    key: &str,
    path: &str,
    created: Timestamp,
) -> Result<(), StorageError> {
    conn.prepare_cached(
        "INSERT INTO aliases (name, key, path, created) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (key) DO UPDATE SET name = excluded.name, path = excluded.path,
         created = excluded.created",
    )?
    .execute(params![name, key, path, created.unix_seconds()])?;
    Ok(())
}

/// Lists every alias for `furet alias list`, ordered by `key` ascending.
pub fn alias_listing(conn: &Connection) -> Result<Vec<AliasListing>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT name, path, strftime('%Y-%m-%dT%H:%M:%S', created, 'unixepoch', 'localtime')
         FROM aliases ORDER BY key ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(AliasListing {
                name: row.get(0)?,
                path: row.get(1)?,
                created: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Deletes the `aliases` row matching `key`; `true` when exactly one row went.
pub fn remove_alias(conn: &Connection, key: &str) -> Result<bool, StorageError> {
    let removed = conn
        .prepare_cached("DELETE FROM aliases WHERE key = ?1")?
        .execute(params![key])?;
    Ok(removed == 1)
}

/// One `dirs` row flattened for `furet list`, its timestamps already
/// formatted in local time by SQLite itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirListing {
    /// Canonical displayable path.
    pub path: String,
    /// Number of `visits` rows recorded for this directory.
    pub visits: i64,
    /// Most recent visit in local time, or `None` when no visit exists yet.
    pub last_visit: Option<String>,
    /// First time this directory was recorded, in local time.
    pub first_seen: String,
    /// Whether `missing_since` is set (soft delete, SPEC section 10).
    pub missing: bool,
}

/// Lists every known directory for `furet list`, last visit descending then
/// path ascending, zero-visit rows last; `include_missing` keeps missing rows.
pub fn dir_listing(
    conn: &Connection,
    include_missing: bool,
) -> Result<Vec<DirListing>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT dirs.path,
                COUNT(visits.id),
                strftime('%Y-%m-%dT%H:%M:%S', MAX(visits.ts), 'unixepoch', 'localtime'),
                strftime('%Y-%m-%dT%H:%M:%S', dirs.first_seen, 'unixepoch', 'localtime'),
                dirs.missing_since IS NOT NULL
         FROM dirs
         LEFT JOIN visits ON visits.dir_id = dirs.id
         WHERE ?1 OR dirs.missing_since IS NULL
         GROUP BY dirs.id
         ORDER BY dirs.key ASC",
    )?;
    let rows = stmt
        .query_map(params![include_missing], |row| {
            Ok(DirListing {
                path: row.get(0)?,
                visits: row.get(1)?,
                last_visit: row.get(2)?,
                first_seen: row.get(3)?,
                missing: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The path of the second-to-last visited directory for `session`, ordered
/// by `ts` then `id` descending; `None` when fewer than two visits exist.
pub fn last_visited_dir(conn: &Connection, session: &str) -> Result<Option<String>, StorageError> {
    visited_dir_back(conn, session, 1)
}

/// The directory `steps` visits before the latest one in `session`
/// (`steps = 1` is the previous directory), or `None` past the history.
pub fn visited_dir_back(
    conn: &Connection,
    session: &str,
    steps: u32,
) -> Result<Option<String>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT dirs.path
         FROM visits
         JOIN dirs ON dirs.id = visits.dir_id
         WHERE visits.session = ?1
         ORDER BY visits.ts DESC, visits.id DESC
         LIMIT 1 OFFSET ?2",
    )?;
    let mut rows = stmt.query(params![session, steps])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

/// One visit as listed by `furet history`.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    /// Visit time in local time, `YYYY-MM-DDTHH:MM:SS`.
    pub time: String,
    /// What triggered the visit (`hook`, `jump`, ...).
    pub source: String,
    /// Canonical displayable path of the visited directory.
    pub path: String,
}

/// The newest `limit` visits of `session` (every session when `None`),
/// newest first; `limit = 0` returns them all.
pub fn visit_history(
    conn: &Connection,
    session: Option<&str>,
    limit: u32,
) -> Result<Vec<HistoryRow>, StorageError> {
    // WHY: the order stays `ts DESC, id DESC`, the same as `visited_dir_back`, so line N is `f -N`'s target.
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S', visits.ts, 'unixepoch', 'localtime'),
                visits.source,
                dirs.path
         FROM visits
         JOIN dirs ON dirs.id = visits.dir_id
         WHERE ?1 IS NULL OR visits.session = ?1
         ORDER BY visits.ts DESC, visits.id DESC
         LIMIT ?2",
    )?;
    // WHY: SQLite treats a negative LIMIT as no limit, so 0 maps to -1.
    let bound = if limit == 0 { -1_i64 } else { i64::from(limit) };
    let rows = stmt
        .query_map(params![session, bound], |row| {
            Ok(HistoryRow {
                time: row.get(0)?,
                source: row.get(1)?,
                path: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
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

/// What the journal remembers for memory key `key` (SPEC-v2 §24): the latest
/// `jump` or `pick` row with a result, unless it is a probable failure.
pub fn recall(conn: &Connection, key: &str) -> Result<Recall, StorageError> {
    if key.is_empty() {
        return Ok(Recall::Nothing);
    }
    let Some(latest) = latest_choice(conn, key)? else {
        return Ok(Recall::Nothing);
    };
    let visits = visits_since(conn, latest.ts)?;
    if calibration::failure_of(&latest, &visits).is_some() {
        debug!(id = latest.id, "remembered choice is a probable failure");
        return Ok(Recall::ProbableFailure);
    }
    let Some(path) = latest
        .result_dir_id
        .map(|dir_id| dir_path_by_id(conn, dir_id))
        .transpose()?
        .flatten()
    else {
        return Ok(Recall::Nothing);
    };
    let chosen = format_local_time(conn, latest.ts)?;
    Ok(Recall::Remembered { path, chosen })
}

fn latest_choice(
    conn: &Connection,
    key: &str,
) -> Result<Option<calibration::QueryRecord>, StorageError> {
    // WHY: no index covers the query text, so one sequential scan keeping the max (ts, id) beats walking idx_queries_ts row by row.
    let mut stmt = conn.prepare(
        "SELECT id, ts, query FROM queries
         WHERE outcome IN ('jump', 'pick') AND result_dir_id IS NOT NULL",
    )?;
    let mut rows = stmt.query([])?;
    let mut latest: Option<(i64, i64)> = None;
    while let Some(row) = rows.next()? {
        if !memory::matches(
            row.get_ref(2)?.as_str().map_err(rusqlite::Error::from)?,
            key,
        ) {
            continue;
        }
        let found = (row.get::<_, i64>(1)?, row.get::<_, i64>(0)?);
        if latest.is_none_or(|best| found > best) {
            latest = Some(found);
        }
    }
    let Some((_, id)) = latest else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT id, ts, cwd, query, result_dir_id, stage, outcome FROM queries WHERE id = ?1",
        params![id],
        |row| {
            Ok(calibration::QueryRecord {
                id: row.get(0)?,
                ts: Timestamp::from_unix_seconds(row.get(1)?),
                cwd: row.get(2)?,
                query: row.get(3)?,
                result_dir_id: row.get(4)?,
                stage: row.get(5)?,
                outcome: row.get(6)?,
            })
        },
    )
    .map(Some)
    .map_err(StorageError::from)
}

fn visits_since(
    conn: &Connection,
    since: Timestamp,
) -> Result<Vec<calibration::VisitRecord>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT dir_id, ts, source, session FROM visits WHERE ts >= ?1 ORDER BY ts ASC, id ASC",
    )?;
    let records = stmt
        .query_map(params![since.unix_seconds()], |row| {
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

/// Every aggregate `furet stats` prints (SPEC-v2 §22), with the 30-day
/// windows counted from `since`.
pub fn stats_counts(conn: &Connection, since: Timestamp) -> Result<stats::Counts, StorageError> {
    let since = since.unix_seconds();
    let count = |sql: &str| {
        conn.query_row(sql, [], |row| row.get::<_, i64>(0))
            .map_err(StorageError::from)
    };
    let window = |sql: &str| {
        conn.query_row(sql, params![since], |row| row.get::<_, i64>(0))
            .map_err(StorageError::from)
    };
    let mut stmt = conn.prepare("SELECT stage, COUNT(*) FROM queries GROUP BY stage")?;
    let stages: HashMap<String, i64> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<HashMap<_, _>, _>>()?;
    let mut stmt = conn.prepare("SELECT source, COUNT(*) FROM visits GROUP BY source")?;
    let sources: HashMap<String, i64> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<HashMap<_, _>, _>>()?;
    let grouped = |map: &HashMap<String, i64>, key: &str| map.get(key).copied().unwrap_or(0);
    Ok(stats::Counts {
        known_directories: count("SELECT COUNT(*) FROM dirs WHERE missing_since IS NULL")?,
        missing_directories: count("SELECT COUNT(*) FROM dirs WHERE missing_since IS NOT NULL")?,
        visits: count("SELECT COUNT(*) FROM visits")?,
        visits_last_30_days: window("SELECT COUNT(*) FROM visits WHERE ts >= ?1")?,
        queries: count("SELECT COUNT(*) FROM queries")?,
        queries_last_30_days: window("SELECT COUNT(*) FROM queries WHERE ts >= ?1")?,
        jumps: count("SELECT COUNT(*) FROM queries WHERE outcome IN ('jump', 'pick')")?,
        stage_1: grouped(&stages, "1"),
        stage_2: grouped(&stages, "2"),
        stage_fallback: grouped(&stages, "fallback"),
        stage_menu: grouped(&stages, "menu"),
        source_hook: grouped(&sources, "hook"),
        source_jump: grouped(&sources, "jump"),
        source_back: grouped(&sources, "back"),
        source_up: grouped(&sources, "up"),
        source_fallback: grouped(&sources, "fallback"),
        source_import: grouped(&sources, "import"),
    })
}

/// The `--top` rows of `furet stats`: every present directory, visit count
/// descending then `key` ascending, truncated to `limit`.
pub fn top_dirs(conn: &Connection, limit: usize) -> Result<Vec<(i64, String)>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT COUNT(visits.id), dirs.path
         FROM dirs
         LEFT JOIN visits ON visits.dir_id = dirs.id
         WHERE dirs.missing_since IS NULL
         GROUP BY dirs.id
         ORDER BY COUNT(visits.id) DESC, dirs.key ASC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::{
        alias_by_key, alias_listing, config_path, db_path, dir_entries, dir_id_by_key, dir_listing,
        dir_path_by_id, format_local_time, insert_query, insert_visit, known_keys,
        last_visited_dir, logs_dir, missing_since_by_id, open, open_at, purge_before, query_log,
        recall, remove_alias, remove_dirs, resolve_data_dir, set_missing_since, stats_counts,
        top_dirs, upsert_alias, upsert_dir, visit_history, visit_log, visited_dir_back,
    };
    use crate::clock::Timestamp;
    use crate::memory::{self, Recall};
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
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('dirs', 'visits', 'queries', 'aliases')",
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

    fn managed_index_count(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_dirs_key', 'idx_visits_dir_ts', 'idx_visits_session_ts', 'idx_visits_ts', 'idx_queries_ts', 'idx_aliases_key')",
            [],
            |row| row.get(0),
        )
        .expect("sqlite_master is readable")
    }

    fn schema_names(conn: &Connection, kind: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name",
            )
            .expect("sqlite_master is queryable");
        stmt.query_map([kind], |row| row.get::<_, String>(0))
            .expect("sqlite_master rows read")
            .collect::<Result<Vec<_>, _>>()
            .expect("sqlite_master names decode")
    }

    fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
            .expect("pragma_table_info is queryable");
        stmt.query_map([table], |row| row.get::<_, String>(0))
            .expect("table_info rows read")
            .collect::<Result<Vec<_>, _>>()
            .expect("table_info names decode")
    }

    fn section_of<'a>(doc: &'a str, table: &str) -> Option<&'a str> {
        let heading = format!("### `{table}`");
        let start = doc.lines().position(|l| l.starts_with(&heading))?;
        let lines: Vec<&str> = doc.lines().collect();
        let end = lines[start + 1..]
            .iter()
            .position(|l| l.starts_with("### ") || l.starts_with("## "))
            .map_or(lines.len(), |n| start + 1 + n);
        let begin = doc.lines().take(start).map(|l| l.len() + 1).sum::<usize>();
        let len = lines[start..end].iter().map(|l| l.len() + 1).sum::<usize>();
        Some(&doc[begin..(begin + len).min(doc.len())])
    }

    #[test]
    fn database_md_documents_every_table_column_and_index() {
        let mut conn = Connection::open_in_memory().expect("an in-memory database opens");
        super::migrate(&mut conn).expect("the migrations apply in memory");
        let doc = include_str!("../DATABASE.md").replace("\r\n", "\n");
        let tables = schema_names(&conn, "table");
        let indexes = schema_names(&conn, "index");

        for table in &tables {
            let section = section_of(&doc, table).unwrap_or_else(|| {
                panic!("DATABASE.md lacks a `### `{table}`` heading; update DATABASE.md")
            });
            for column in table_columns(&conn, table) {
                let row = format!("| `{column}` |");
                assert!(
                    section.lines().any(|l| l.starts_with(&row)),
                    "DATABASE.md lacks the `{column}` column row in the `{table}` section; update DATABASE.md"
                );
            }
        }
        for index in &indexes {
            let row = format!("| `{index}` |");
            assert!(
                doc.lines().any(|l| l.starts_with(&row)),
                "DATABASE.md lacks the `{index}` index row; update DATABASE.md"
            );
        }
        let version = user_version(&conn);
        let pragma_row = format!("| `user_version` | `{version}` |");
        assert!(
            doc.lines().any(|l| l.starts_with(&pragma_row)),
            "DATABASE.md does not show user_version {version} in its pragma table; update DATABASE.md"
        );
        for heading in doc.lines().filter_map(|l| l.strip_prefix("### `")) {
            let name = heading.split('`').next().unwrap_or_default();
            assert!(
                tables.iter().any(|t| t == name),
                "DATABASE.md documents table `{name}` that the schema does not have; update DATABASE.md"
            );
        }
        for row in doc.lines().filter_map(|l| l.strip_prefix("| `idx_")) {
            let name = format!("idx_{}", row.split('`').next().unwrap_or_default());
            assert!(
                indexes.contains(&name),
                "DATABASE.md documents index `{name}` that the schema does not have; update DATABASE.md"
            );
        }
    }

    #[test]
    fn fresh_database_creates_all_tables_and_reaches_user_version_4() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        assert_eq!(user_version(&conn), 4);
        assert_eq!(managed_table_count(&conn), 4);
        assert_eq!(managed_index_count(&conn), 6);
    }

    #[test]
    fn migrating_an_already_migrated_database_is_a_noop() {
        let (_dir, path) = temp_db();
        drop(opened(&path));
        let conn = opened(&path);
        assert_eq!(user_version(&conn), 4);
        assert_eq!(managed_table_count(&conn), 4);
        assert_eq!(managed_index_count(&conn), 6);
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
    fn a_version_2_database_migrates_to_version_4() {
        let (_dir, path) = temp_db();
        {
            let old = Connection::open(&path).expect("the legacy database opens");
            old.execute_batch(super::MIGRATIONS[0])
                .expect("the version 1 schema applies");
            old.execute_batch(super::MIGRATIONS[1])
                .expect("the version 2 indexes apply");
            old.execute_batch("PRAGMA user_version = 2;")
                .expect("the legacy version is stamped");
        }
        let conn = opened(&path);
        assert_eq!(user_version(&conn), 4);
        assert_eq!(managed_table_count(&conn), 4);
        assert_eq!(managed_index_count(&conn), 6);
    }

    #[test]
    fn a_version_3_database_migrates_to_version_4_keeping_its_rows() {
        let (_dir, path) = temp_db();
        {
            let old = Connection::open(&path).expect("the legacy database opens");
            old.execute_batch(super::MIGRATIONS[0])
                .expect("the version 1 schema applies");
            old.execute_batch(super::MIGRATIONS[1])
                .expect("the version 2 indexes apply");
            old.execute_batch(super::MIGRATIONS[2])
                .expect("the version 3 indexes apply");
            old.execute_batch("PRAGMA user_version = 3;")
                .expect("the legacy version is stamped");
            insert_dir(&old, "C:\\legacy", "c:\\legacy");
        }
        let conn = opened(&path);
        assert_eq!(user_version(&conn), 4);
        assert_eq!(managed_table_count(&conn), 4);
        assert_eq!(managed_index_count(&conn), 6);
        assert_eq!(row_count(&conn, "dirs"), 1);
        assert_eq!(row_count(&conn, "aliases"), 0);
    }

    #[test]
    fn a_version_1_database_migrates_to_version_4() {
        let (_dir, path) = temp_db();
        {
            let old = Connection::open(&path).expect("the legacy database opens");
            old.execute_batch(super::MIGRATIONS[0])
                .expect("the version 1 schema applies");
            old.execute_batch("PRAGMA user_version = 1;")
                .expect("the legacy version is stamped");
        }
        let conn = opened(&path);
        assert_eq!(user_version(&conn), 4);
        assert_eq!(managed_table_count(&conn), 4);
        assert_eq!(managed_index_count(&conn), 6);
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
    fn opened_databases_run_synchronous_normal_with_a_busy_timeout() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .expect("synchronous is readable");
        assert_eq!(synchronous, 1, "SQLite's constant for NORMAL is 1");
        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .expect("busy_timeout is readable");
        assert_eq!(busy_timeout, 5000);
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
    fn aliases_key_is_unique() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let inserted = conn.execute(
            "INSERT INTO aliases (name, key, path, created) VALUES ('Ombi', 'ombi', 'C:\\apps\\ombi', 1_700_000_000)",
            [],
        );
        assert!(inserted.is_ok(), "the first alias inserts");
        let rejected = conn.execute(
            "INSERT INTO aliases (name, key, path, created) VALUES ('OMBI', 'ombi', 'C:\\other', 1_700_000_001)",
            [],
        );
        assert!(rejected.is_err(), "two aliases must never share a key");
        let stored: String = conn
            .query_row("SELECT path FROM aliases WHERE key = 'ombi'", [], |row| {
                row.get(0)
            })
            .expect("the surviving alias reads back");
        assert_eq!(stored, "C:\\apps\\ombi");
    }

    #[test]
    fn upsert_alias_inserts_then_replaces_by_key() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        upsert_alias(&conn, "Ombi", "ombi", "C:\\apps\\ombi", at(100))
            .expect("the first alias upserts");
        upsert_alias(&conn, "OMBI", "ombi", "C:\\other", at(200))
            .expect("the second alias upserts");
        assert_eq!(row_count(&conn, "aliases"), 1);
        let (name, stored_path, created): (String, String, i64) = conn
            .query_row("SELECT name, path, created FROM aliases", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .expect("the single aliases row reads back");
        assert_eq!(name, "OMBI");
        assert_eq!(stored_path, "C:\\other");
        assert_eq!(created, at(200).unix_seconds());
    }

    #[test]
    fn alias_by_key_misses_an_unknown_key() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        upsert_alias(&conn, "Ombi", "ombi", "C:\\apps\\ombi", at(100))
            .expect("the fixture alias upserts");
        assert_eq!(
            alias_by_key(&conn, "ombi").expect("the known lookup runs"),
            Some(super::Alias {
                name: "Ombi".to_owned(),
                path: "C:\\apps\\ombi".to_owned()
            })
        );
        assert_eq!(
            alias_by_key(&conn, "nope").expect("the unknown lookup runs"),
            None
        );
    }

    #[test]
    fn alias_listing_orders_by_key() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        upsert_alias(&conn, "zz", "zz", "C:\\zz", at(100)).expect("the zz alias upserts");
        upsert_alias(&conn, "1", "1", "C:\\one", at(100)).expect("the 1 alias upserts");
        upsert_alias(&conn, "Ab", "ab", "C:\\ab", at(100)).expect("the Ab alias upserts");
        let rows = alias_listing(&conn).expect("the listing reads");
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["1", "Ab", "zz"]);
    }

    #[test]
    fn remove_alias_reports_whether_a_row_went() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        upsert_alias(&conn, "ombi", "ombi", "C:\\apps\\ombi", at(100))
            .expect("the fixture alias upserts");
        assert!(remove_alias(&conn, "ombi").expect("the first remove runs"));
        assert!(!remove_alias(&conn, "ombi").expect("the second remove runs"));
        assert_eq!(row_count(&conn, "aliases"), 0);
    }

    // WHY: edition 2024 makes env mutation unsafe; this test must set FURET_DATA_DIR.
    #[allow(unsafe_code)]
    #[test]
    fn furet_data_dir_overrides_the_database_location() {
        let dir = tempfile::tempdir().expect("a fresh temporary directory");
        let nested = dir.path().join("nested");
        unsafe { std::env::set_var("FURET_DATA_DIR", &nested) };
        let resolved = db_path();
        let config = config_path();
        let logs = logs_dir();
        let connection = open();
        unsafe { std::env::remove_var("FURET_DATA_DIR") };
        assert_eq!(
            resolved.expect("db_path resolves under FURET_DATA_DIR"),
            nested.join("furet.db")
        );
        assert_eq!(
            config.expect("config_path resolves under FURET_DATA_DIR"),
            nested.join("config.toml")
        );
        assert_eq!(
            logs.expect("logs_dir resolves under FURET_DATA_DIR"),
            nested.join("logs")
        );
        let conn = connection.expect("open works under FURET_DATA_DIR");
        assert_eq!(user_version(&conn), 4);
        assert!(nested.join("furet.db").exists());
    }

    #[test]
    fn an_empty_furet_data_dir_is_treated_as_unset() {
        let unset = resolve_data_dir(None).expect("the platform data dir resolves");
        let empty = resolve_data_dir(Some("".into())).expect("an empty override resolves");
        assert_eq!(empty, unset);
        assert!(empty.is_absolute());
        let set = resolve_data_dir(Some("elsewhere".into())).expect("an override resolves");
        assert_eq!(set, PathBuf::from("elsewhere"));
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
    fn dir_listing_counts_visits_and_formats_local_time() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture dir upserts");
        insert_visit(&conn, tokio, at(150), "hook", "s", None).expect("the first visit inserts");
        insert_visit(&conn, tokio, at(120), "hook", "s", None).expect("the second visit inserts");
        let rows = dir_listing(&conn, false).expect("the listing reads");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "c:\\dev\\tokio");
        assert_eq!(rows[0].visits, 2);
        let local = |seconds: i64| -> String {
            conn.query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%S', ?1, 'unixepoch', 'localtime')",
                params![seconds],
                |row| row.get(0),
            )
            .expect("the expected timestamp formats")
        };
        assert_eq!(
            rows[0].last_visit.as_deref(),
            Some(local(at(150).unix_seconds()).as_str())
        );
        assert_eq!(rows[0].first_seen, local(at(100).unix_seconds()));
        assert!(!rows[0].missing);
    }

    #[test]
    fn dir_listing_orders_by_key_whatever_the_case_and_the_visits() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let zeta = upsert_dir(&conn, "C:\\dev\\Zeta", "c:\\dev\\zeta", at(100))
            .expect("the zeta dir upserts");
        let alpha = upsert_dir(&conn, "c:\\dev\\alpha", "c:\\dev\\alpha", at(100))
            .expect("the alpha dir upserts");
        upsert_dir(&conn, "C:\\dev\\Beta", "c:\\dev\\beta", at(100))
            .expect("the visit-less dir upserts");
        insert_visit(&conn, zeta, at(300), "hook", "s", None).expect("the zeta visit inserts");
        insert_visit(&conn, alpha, at(150), "hook", "s", None).expect("the alpha visit inserts");
        let rows = dir_listing(&conn, false).expect("the listing reads");
        let paths: Vec<&str> = rows.iter().map(|row| row.path.as_str()).collect();
        assert_eq!(paths, ["c:\\dev\\alpha", "C:\\dev\\Beta", "C:\\dev\\Zeta"]);
        assert_eq!(rows[0].visits, 1);
        assert_eq!(rows[1].visits, 0);
        assert_eq!(rows[1].last_visit, None);
        assert_eq!(rows[2].visits, 1);
    }

    #[test]
    fn dir_listing_filters_missing_rows_unless_included() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the tokio dir upserts");
        let gone = upsert_dir(&conn, "c:\\dev\\gone", "c:\\dev\\gone", at(100))
            .expect("the gone dir upserts");
        insert_visit(&conn, tokio, at(150), "hook", "s", None).expect("the tokio visit inserts");
        insert_visit(&conn, gone, at(160), "hook", "s", None).expect("the gone visit inserts");
        conn.execute(
            "UPDATE dirs SET missing_since = 900 WHERE id = ?1",
            params![gone],
        )
        .expect("the fixture marks the dir missing");
        let plain = dir_listing(&conn, false).expect("the filtered listing reads");
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].path, "c:\\dev\\tokio");
        let all = dir_listing(&conn, true).expect("the full listing reads");
        assert_eq!(all.len(), 2);
        assert!(all[0].missing, "gone sorts before tokio");
        assert_eq!(all[0].path, "c:\\dev\\gone");
        assert!(!all[1].missing);
        assert_eq!(all[1].path, "c:\\dev\\tokio");
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
    fn missing_since_by_id_returns_only_marked_rows() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let gone = upsert_dir(&conn, "c:\\dev\\gone", "c:\\dev\\gone", at(100))
            .expect("the gone fixture dir upserts");
        upsert_dir(&conn, "c:\\dev\\ok", "c:\\dev\\ok", at(100))
            .expect("the ok fixture dir upserts");
        set_missing_since(&conn, gone, Some(at(500))).expect("marking the dir missing runs");
        let marked = missing_since_by_id(&conn).expect("the stored markers read");
        assert_eq!(marked.len(), 1);
        assert_eq!(marked.get(&gone), Some(&at(500)));
        set_missing_since(&conn, gone, None).expect("reactivating the dir runs");
        let cleared = missing_since_by_id(&conn).expect("the stored markers read");
        assert!(cleared.is_empty());
    }

    #[test]
    fn format_local_time_matches_the_list_format() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let formatted =
            format_local_time(&conn, at(150)).expect("the timestamp formats in local time");
        let expected: String = conn
            .query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%S', ?1, 'unixepoch', 'localtime')",
                params![at(150).unix_seconds()],
                |row| row.get(0),
            )
            .expect("the expected timestamp formats");
        assert_eq!(formatted, expected);
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
    fn visited_dir_back_walks_the_session_history() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let a = upsert_dir(&conn, "c:\\dev\\a", "c:\\dev\\a", at(100)).expect("the a dir upserts");
        let b = upsert_dir(&conn, "c:\\dev\\b", "c:\\dev\\b", at(100)).expect("the b dir upserts");
        let c = upsert_dir(&conn, "c:\\dev\\c", "c:\\dev\\c", at(100)).expect("the c dir upserts");
        let d = upsert_dir(&conn, "c:\\dev\\d", "c:\\dev\\d", at(100)).expect("the d dir upserts");
        let x = upsert_dir(&conn, "c:\\dev\\x", "c:\\dev\\x", at(100)).expect("the x dir upserts");
        insert_visit(&conn, a, at(110), "jump", "s1", None).expect("the first visit inserts");
        insert_visit(&conn, b, at(120), "jump", "s1", None).expect("the second visit inserts");
        insert_visit(&conn, x, at(125), "jump", "s2", None).expect("the decoy visit inserts");
        insert_visit(&conn, c, at(130), "jump", "s1", None).expect("the third visit inserts");
        insert_visit(&conn, d, at(140), "jump", "s1", None).expect("the fourth visit inserts");
        let one = visited_dir_back(&conn, "s1", 1).expect("the one-step lookup runs");
        assert_eq!(one, Some("c:\\dev\\c".to_owned()));
        let three = visited_dir_back(&conn, "s1", 3).expect("the three-step lookup runs");
        assert_eq!(three, Some("c:\\dev\\a".to_owned()));
        let four = visited_dir_back(&conn, "s1", 4).expect("the past-the-end lookup runs");
        assert_eq!(four, None);
    }

    #[test]
    fn visit_history_lists_newest_first_per_session_or_all() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let a = upsert_dir(&conn, "c:\\dev\\a", "c:\\dev\\a", at(100)).expect("the a dir upserts");
        let b = upsert_dir(&conn, "c:\\dev\\b", "c:\\dev\\b", at(100)).expect("the b dir upserts");
        let c = upsert_dir(&conn, "c:\\dev\\c", "c:\\dev\\c", at(100)).expect("the c dir upserts");
        let d = upsert_dir(&conn, "c:\\dev\\d", "c:\\dev\\d", at(100)).expect("the d dir upserts");
        let x = upsert_dir(&conn, "c:\\dev\\x", "c:\\dev\\x", at(100)).expect("the x dir upserts");
        insert_visit(&conn, a, at(110), "jump", "s1", None).expect("the first visit inserts");
        insert_visit(&conn, b, at(120), "jump", "s1", None).expect("the second visit inserts");
        insert_visit(&conn, x, at(125), "jump", "s2", None).expect("the decoy visit inserts");
        insert_visit(&conn, c, at(130), "jump", "s1", None).expect("the third visit inserts");
        insert_visit(&conn, d, at(140), "jump", "s1", None).expect("the fourth visit inserts");
        fn paths(rows: &[super::HistoryRow]) -> Vec<&str> {
            rows.iter().map(|row| row.path.as_str()).collect()
        }
        let session = visit_history(&conn, Some("s1"), 0).expect("the session history reads");
        assert_eq!(
            paths(&session),
            ["c:\\dev\\d", "c:\\dev\\c", "c:\\dev\\b", "c:\\dev\\a"]
        );
        let limited = visit_history(&conn, Some("s1"), 2).expect("the limited history reads");
        assert_eq!(paths(&limited), ["c:\\dev\\d", "c:\\dev\\c"]);
        let every = visit_history(&conn, None, 0).expect("the whole history reads");
        assert_eq!(
            paths(&every),
            [
                "c:\\dev\\d",
                "c:\\dev\\c",
                "c:\\dev\\x",
                "c:\\dev\\b",
                "c:\\dev\\a"
            ]
        );
        let unknown = visit_history(&conn, Some("nope"), 0).expect("the unknown session reads");
        assert_eq!(unknown, vec![]);
        assert_eq!(session[0].source, "jump");
        assert_eq!(
            session[0].time,
            format_local_time(&conn, at(140)).expect("the newest visit formats")
        );
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
    fn purge_before_deletes_only_older_visits_and_queries_and_keeps_dirs() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let tokio = upsert_dir(&conn, "c:\\dev\\tokio", "c:\\dev\\tokio", at(100))
            .expect("the fixture dir upserts");
        for ts in [110, 150, 200] {
            insert_visit(&conn, tokio, at(ts), "hook", "session-1", None)
                .expect("the visit inserts");
            insert_query(&conn, at(ts), "c:\\", "tok", Some(tokio), "1", "jump")
                .expect("the query inserts");
        }
        let purged = purge_before(&conn, at(150)).expect("the purge runs");
        assert_eq!(purged, (1, 1));
        let visits: Vec<_> = visit_log(&conn)
            .expect("the visit log reads")
            .into_iter()
            .map(|record| record.ts)
            .collect();
        assert_eq!(visits, vec![at(150), at(200)]);
        let queries: Vec<_> = query_log(&conn)
            .expect("the query log reads")
            .into_iter()
            .map(|record| record.ts)
            .collect();
        assert_eq!(queries, vec![at(150), at(200)]);
        assert_eq!(
            dir_path_by_id(&conn, tokio).expect("the dir lookup runs"),
            Some("c:\\dev\\tokio".to_owned())
        );
    }

    #[test]
    fn remove_dirs_deletes_the_dir_its_visits_and_its_queries_and_unlinks_other_visits() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let a = upsert_dir(&conn, "c:\\dev\\a", "c:\\dev\\a", at(100))
            .expect("the fixture dir a upserts");
        let b = upsert_dir(&conn, "c:\\dev\\b", "c:\\dev\\b", at(100))
            .expect("the fixture dir b upserts");
        insert_visit(&conn, a, at(110), "hook", "s", None).expect("a's first visit inserts");
        insert_visit(&conn, a, at(120), "jump", "s", None).expect("a's second visit inserts");
        insert_visit(&conn, b, at(130), "jump", "s", Some(a)).expect("b's visit from a inserts");
        insert_query(&conn, at(140), "c:\\dev", "a", Some(a), "1", "jump")
            .expect("a's query inserts");
        insert_query(&conn, at(150), "c:\\dev", "b", Some(b), "1", "jump")
            .expect("b's query inserts");
        let removed = remove_dirs(&conn, &[a]).expect("remove_dirs runs");
        assert_eq!(removed, 1);
        assert_eq!(
            dir_path_by_id(&conn, a).expect("the a lookup runs"),
            None,
            "a's dirs row must be gone"
        );
        assert_eq!(row_count(&conn, "visits"), 1);
        let (dir_id, from_dir_id): (i64, Option<i64>) = conn
            .query_row("SELECT dir_id, from_dir_id FROM visits", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("the surviving visit reads back");
        assert_eq!(dir_id, b);
        assert_eq!(from_dir_id, None, "b's visit must be unlinked from a");
        let result_dir_id: Option<i64> = conn
            .query_row("SELECT result_dir_id FROM queries", [], |row| row.get(0))
            .expect("the surviving query reads back");
        assert_eq!(result_dir_id, Some(b), "only b's query must survive");
        let violations = conn
            .prepare("PRAGMA foreign_key_check")
            .expect("the check prepares")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("the check runs")
            .collect::<Result<Vec<_>, _>>()
            .expect("the check collects");
        assert!(
            violations.is_empty(),
            "no foreign key violation: {violations:?}"
        );
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

    #[test]
    fn stats_counts_splits_by_stage_source_and_window() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let alpha = upsert_dir(&conn, "c:\\dev\\alpha", "c:\\dev\\alpha", at(100))
            .expect("the alpha fixture dir upserts");
        let beta = upsert_dir(&conn, "c:\\dev\\beta", "c:\\dev\\beta", at(100))
            .expect("the beta fixture dir upserts");
        let gone = upsert_dir(&conn, "c:\\dev\\gone", "c:\\dev\\gone", at(100))
            .expect("the gone fixture dir upserts");
        set_missing_since(&conn, gone, Some(at(500))).expect("the fixture marks the dir missing");
        insert_visit(&conn, alpha, at(100), "hook", "s", None)
            .expect("the pre-window visit inserts");
        insert_visit(&conn, alpha, at(150), "jump", "s", None)
            .expect("the window-edge visit inserts");
        insert_visit(&conn, alpha, at(200), "hook", "s", None)
            .expect("the post-window visit inserts");
        insert_visit(&conn, beta, at(120), "up", "s", None).expect("the beta visit inserts");
        insert_query(&conn, at(100), "c:\\dev", "a", Some(alpha), "1", "jump")
            .expect("the pre-window jump query inserts");
        insert_query(&conn, at(150), "c:\\dev", "b", Some(alpha), "2", "jump")
            .expect("the stage-2 jump query inserts");
        insert_query(&conn, at(200), "c:\\dev", "c", None, "fallback", "jump")
            .expect("the fallback jump query inserts");
        insert_query(&conn, at(200), "c:\\dev", "d", None, "menu", "menu")
            .expect("the menu query inserts");
        insert_query(&conn, at(200), "c:\\dev", "e", None, "1", "none")
            .expect("the failed query inserts");
        insert_query(&conn, at(200), "c:\\dev", "f", Some(beta), "menu", "pick")
            .expect("the pick query inserts");
        let counts = stats_counts(&conn, at(150)).expect("the counts read");
        assert_eq!(counts.known_directories, 2);
        assert_eq!(counts.missing_directories, 1);
        assert_eq!(counts.visits, 4);
        assert_eq!(counts.visits_last_30_days, 2);
        assert_eq!(counts.queries, 6);
        assert_eq!(counts.queries_last_30_days, 5);
        assert_eq!(counts.jumps, 4);
        assert_eq!(counts.stage_1, 2);
        assert_eq!(counts.stage_2, 1);
        assert_eq!(counts.stage_fallback, 1);
        assert_eq!(counts.stage_menu, 2);
        assert_eq!(counts.source_hook, 2);
        assert_eq!(counts.source_jump, 1);
        assert_eq!(counts.source_back, 0);
        assert_eq!(counts.source_up, 1);
        assert_eq!(counts.source_fallback, 0);
        assert_eq!(counts.source_import, 0);
    }

    #[test]
    fn top_dirs_orders_by_visits_then_key_and_skips_missing() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let alpha = upsert_dir(&conn, "c:\\dev\\alpha", "c:\\dev\\alpha", at(100))
            .expect("the alpha fixture dir upserts");
        let beta = upsert_dir(&conn, "C:\\Dev\\Beta", "c:\\dev\\beta", at(100))
            .expect("the beta fixture dir upserts");
        upsert_dir(&conn, "c:\\dev\\gamma", "c:\\dev\\gamma", at(100))
            .expect("the visit-less fixture dir upserts");
        let gone = upsert_dir(&conn, "c:\\dev\\gone", "c:\\dev\\gone", at(100))
            .expect("the gone fixture dir upserts");
        set_missing_since(&conn, gone, Some(at(500))).expect("the fixture marks the dir missing");
        for ts in [at(110), at(120)] {
            insert_visit(&conn, alpha, ts, "hook", "s", None).expect("an alpha visit inserts");
            insert_visit(&conn, beta, ts, "hook", "s", None).expect("a beta visit inserts");
        }
        for ts in [at(110), at(120), at(130), at(140), at(150)] {
            insert_visit(&conn, gone, ts, "hook", "s", None).expect("a gone visit inserts");
        }
        let rows = top_dirs(&conn, 10).expect("the top rows read");
        assert_eq!(
            rows,
            vec![
                (2, "c:\\dev\\alpha".to_owned()),
                (2, "C:\\Dev\\Beta".to_owned()),
                (0, "c:\\dev\\gamma".to_owned()),
            ]
        );
        let limited = top_dirs(&conn, 2).expect("the limited top rows read");
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0], (2, "c:\\dev\\alpha".to_owned()));
    }

    fn journal(conn: &Connection, seconds: i64, text: &str, result: Option<i64>, outcome: &str) {
        insert_query(conn, at(seconds), "c:\\dev", text, result, "1", outcome)
            .expect("the journal row inserts");
    }

    fn remembered(conn: &Connection, path: &str, seconds: i64) -> Recall {
        Recall::Remembered {
            path: path.to_owned(),
            chosen: format_local_time(conn, at(seconds)).expect("the choice date formats"),
        }
    }

    #[test]
    fn recall_takes_the_latest_row_by_ts_then_by_id() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let a = upsert_dir(&conn, "c:\\a", "c:\\a", at(0)).expect("a inserts");
        let b = upsert_dir(&conn, "c:\\b", "c:\\b", at(0)).expect("b inserts");
        let c = upsert_dir(&conn, "c:\\c", "c:\\c", at(0)).expect("c inserts");
        journal(&conn, 100, "cl", Some(a), "pick");
        journal(&conn, 300, "CL", Some(b), "jump");
        journal(&conn, 200, " cl ", Some(c), "pick");
        let key = memory::key("cl");
        assert_eq!(
            recall(&conn, &key).expect("the recall reads"),
            remembered(&conn, "c:\\b", 300),
            "a later ts wins over a larger id"
        );
        journal(&conn, 300, "Cl", Some(c), "pick");
        assert_eq!(
            recall(&conn, &key).expect("the recall reads"),
            remembered(&conn, "c:\\c", 300),
            "a ts tie goes to the larger id"
        );
    }

    #[test]
    fn recall_ignores_other_keys_other_outcomes_and_rows_without_a_result() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let a = upsert_dir(&conn, "c:\\a", "c:\\a", at(0)).expect("a inserts");
        let b = upsert_dir(&conn, "c:\\b", "c:\\b", at(0)).expect("b inserts");
        let key = memory::key("cl");
        assert_eq!(
            recall(&conn, &key).expect("the recall reads"),
            Recall::Nothing
        );
        journal(&conn, 100, "cl", Some(a), "jump");
        journal(&conn, 200, "cl", None, "jump");
        journal(&conn, 300, "cl", Some(b), "menu");
        journal(&conn, 400, "cl", Some(b), "none");
        journal(&conn, 500, "cl x", Some(b), "pick");
        journal(&conn, 600, "lc", Some(b), "pick");
        assert_eq!(
            recall(&conn, &key).expect("the recall reads"),
            remembered(&conn, "c:\\a", 100)
        );
    }

    #[test]
    fn a_latest_row_that_is_a_probable_failure_cancels_the_memory() {
        let (_dir, path) = temp_db();
        let conn = opened(&path);
        let a = upsert_dir(&conn, "c:\\a", "c:\\a", at(0)).expect("a inserts");
        let b = upsert_dir(&conn, "c:\\b", "c:\\b", at(0)).expect("b inserts");
        journal(&conn, 100, "cl", Some(a), "pick");
        insert_visit(&conn, a, at(100), "jump", "s", None).expect("the good landing inserts");
        journal(&conn, 200, "cl", Some(b), "jump");
        insert_visit(&conn, b, at(200), "jump", "s", None).expect("the bad landing inserts");
        let key = memory::key("cl");
        assert_eq!(
            recall(&conn, &key).expect("the recall reads"),
            remembered(&conn, "c:\\b", 200),
            "no follow-up visit yet: the latest row stands"
        );
        insert_visit(&conn, a, at(205), "back", "s", None).expect("the backtrack inserts");
        assert_eq!(
            recall(&conn, &key).expect("the recall reads"),
            Recall::ProbableFailure,
            "the latest word wins; the older good row is never used"
        );
    }
}
