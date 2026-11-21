//! SQLite persistence foundation for the trading center.
//!
//! The database lives at `data/database/dexo.db` in WAL mode. The
//! exchange remains the authoritative source for real positions and
//! fills — this database is the local record used for persistence,
//! tracking and recovery.
//!
//! Schema handling follows the strict rules of AGENTS.md section 11.4:
//! - Only a genuinely empty database gets the initial schema and a
//!   version row, in a single transaction.
//! - A database that already has tables but no `dexo_meta` table, or a
//!   `dexo_meta` table without a parseable `schema_version`, is REFUSED:
//!   never auto-initialised, never silently rebuilt, never overwritten.
//! - Version problems are never masked with `INSERT OR IGNORE`.
//! - Migrations run in explicit version order, each in a transaction;
//!   a failure leaves the database untouched and the error diagnosable.
//! - A database whose recorded version is newer than this build is
//!   refused, and a version whose business tables are missing is
//!   reported as damaged rather than silently repaired.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;

/// Current schema version understood by this build.
/// v1: `dexo_meta` only (Phase 1). v2: trading records (Phase 6).
pub const SCHEMA_VERSION: i64 = 2;

const BUSINESS_TABLES: [&str; 4] = ["orders", "fills", "position_snapshots", "anomalies"];

#[derive(Debug)]
pub enum DatabaseError {
    OpenFailed(rusqlite::Error),
    InitFailed(rusqlite::Error),
    StoreFailed(rusqlite::Error),
    /// Tables exist but schema metadata is missing or unusable; the
    /// database is refused rather than guessed at.
    MissingMetadata(String),
    /// The database was written by a newer build.
    UnsupportedVersion(i64),
    /// A migration or a schema consistency check failed.
    MigrationFailed(String),
    /// An order update tried to move the stored record through a status
    /// transition the state machine forbids.
    InvalidTransition(String),
    /// A stored value could not be parsed back into the unified model.
    CorruptRecord(String),
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenFailed(e) => write!(f, "failed to open database: {e}"),
            Self::InitFailed(e) => write!(f, "failed to initialise database: {e}"),
            Self::StoreFailed(e) => write!(f, "database operation failed: {e}"),
            Self::MissingMetadata(m) => write!(f, "schema metadata missing: {m}"),
            Self::UnsupportedVersion(v) => {
                write!(f, "database schema version {v} is newer than this build")
            }
            Self::MigrationFailed(m) => write!(f, "schema migration failed: {m}"),
            Self::InvalidTransition(m) => write!(f, "invalid order status transition: {m}"),
            Self::CorruptRecord(m) => write!(f, "corrupt stored record: {m}"),
        }
    }
}

impl std::error::Error for DatabaseError {}

/// Handle to the application database, shared through application state.
pub struct Database {
    conn: Connection,
    path: std::path::PathBuf,
}

impl Database {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Crate-internal access for the trading record stores.
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Schema version recorded in the database metadata table.
    pub fn schema_version(&self) -> Result<Option<i64>, DatabaseError> {
        read_schema_version(&self.conn)
    }
}

fn existing_tables(conn: &Connection) -> Result<HashSet<String>, DatabaseError> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .map_err(DatabaseError::InitFailed)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(DatabaseError::InitFailed)?;
    let mut tables = HashSet::new();
    for row in rows {
        tables.insert(row.map_err(DatabaseError::InitFailed)?);
    }
    Ok(tables)
}

fn read_schema_version(conn: &Connection) -> Result<Option<i64>, DatabaseError> {
    let mut stmt = conn
        .prepare("SELECT value FROM dexo_meta WHERE key = 'schema_version'")
        .map_err(DatabaseError::InitFailed)?;
    let mut rows = stmt.query([]).map_err(DatabaseError::InitFailed)?;
    match rows.next().map_err(DatabaseError::InitFailed)? {
        Some(row) => {
            let raw: String = row.get(0).map_err(DatabaseError::InitFailed)?;
            Ok(raw.trim().parse::<i64>().ok())
        }
        None => Ok(None),
    }
}

const CREATE_META_SQL: &str = "CREATE TABLE dexo_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
)";

const CREATE_BUSINESS_SQL: &str = "
CREATE TABLE orders (
    client_order_id TEXT PRIMARY KEY,
    exchange_order_id TEXT,
    account_id TEXT NOT NULL,
    strategy_id TEXT NOT NULL,
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    side TEXT NOT NULL,
    order_type TEXT NOT NULL,
    price TEXT,
    quantity TEXT NOT NULL,
    filled_quantity TEXT NOT NULL,
    average_fill_price TEXT,
    reduce_only INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_orders_account_status ON orders (account_id, status);
CREATE TABLE fills (
    fill_key TEXT PRIMARY KEY,
    client_order_id TEXT NOT NULL,
    exchange_order_id TEXT,
    exchange_fill_id TEXT,
    account_id TEXT NOT NULL,
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    side TEXT NOT NULL,
    quantity TEXT NOT NULL,
    price TEXT NOT NULL,
    fee TEXT NOT NULL,
    filled_at INTEGER NOT NULL
);
CREATE INDEX idx_fills_order ON fills (client_order_id);
CREATE INDEX idx_fills_account_time ON fills (account_id, filled_at);
CREATE TABLE position_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL,
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    quantity TEXT NOT NULL,
    entry_price TEXT,
    leverage TEXT,
    unrealized_pnl TEXT,
    observed_at INTEGER NOT NULL
);
CREATE INDEX idx_positions_account ON position_snapshots (account_id, exchange, symbol, id);
CREATE TABLE anomalies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    account_id TEXT NOT NULL,
    detail TEXT NOT NULL,
    detected_at INTEGER NOT NULL,
    resolved INTEGER NOT NULL DEFAULT 0
);
";

/// Creates the full current schema (metadata + business tables) and
/// stamps the version. Runs inside the caller's transaction.
fn create_schema_v2(conn: &Connection) -> Result<(), DatabaseError> {
    conn.execute_batch(CREATE_META_SQL)
        .map_err(DatabaseError::InitFailed)?;
    conn.execute_batch(CREATE_BUSINESS_SQL)
        .map_err(DatabaseError::InitFailed)?;
    conn.execute(
        "INSERT INTO dexo_meta (key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )
    .map_err(DatabaseError::InitFailed)?;
    Ok(())
}

/// Migration v1 → v2: add the trading record tables and advance the
/// version with a plain UPDATE (never INSERT OR IGNORE). Runs inside
/// the caller's transaction; existing metadata rows are preserved.
fn migrate_1_to_2(conn: &Connection) -> Result<(), DatabaseError> {
    conn.execute_batch(CREATE_BUSINESS_SQL)
        .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;
    let changed = conn
        .execute(
            "UPDATE dexo_meta SET value = '2' WHERE key = 'schema_version'",
            [],
        )
        .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;
    if changed != 1 {
        return Err(DatabaseError::MigrationFailed(format!(
            "expected to update exactly one schema_version row, updated {changed}"
        )));
    }
    Ok(())
}

/// Opens the database at `db_path` (creating it when genuinely new),
/// enables WAL and foreign keys, and enforces the strict schema rules
/// documented at the top of this module.
pub fn initialise(db_path: &Path) -> Result<Database, DatabaseError> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            DatabaseError::OpenFailed(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(format!("cannot create database directory: {e}")),
            ))
        })?;
    }
    let mut conn = Connection::open(db_path).map_err(DatabaseError::OpenFailed)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(DatabaseError::InitFailed)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(DatabaseError::InitFailed)?;

    let tables = existing_tables(&conn)?;
    if tables.is_empty() {
        // Genuinely fresh database: create everything in one transaction.
        let tx = conn.transaction().map_err(DatabaseError::InitFailed)?;
        create_schema_v2(&tx)?;
        tx.commit().map_err(DatabaseError::InitFailed)?;
    } else if !tables.contains("dexo_meta") {
        let mut names: Vec<&str> = tables.iter().map(String::as_str).collect();
        names.sort_unstable();
        return Err(DatabaseError::MissingMetadata(format!(
            "database has tables [{}] but no dexo_meta; refusing to auto-initialise",
            names.join(", ")
        )));
    } else {
        match read_schema_version(&conn)? {
            None => {
                return Err(DatabaseError::MissingMetadata(
                    "dexo_meta exists but the schema_version row is missing or unparseable; \
                     refusing to guess the schema"
                        .to_string(),
                ));
            }
            Some(v) if v > SCHEMA_VERSION => {
                return Err(DatabaseError::UnsupportedVersion(v));
            }
            Some(1) => {
                let tx = conn.transaction().map_err(DatabaseError::InitFailed)?;
                migrate_1_to_2(&tx)?;
                tx.commit().map_err(DatabaseError::InitFailed)?;
            }
            Some(v) if v == SCHEMA_VERSION => {
                let missing: Vec<&str> = BUSINESS_TABLES
                    .iter()
                    .filter(|t| !tables.contains(**t))
                    .copied()
                    .collect();
                if !missing.is_empty() {
                    return Err(DatabaseError::MigrationFailed(format!(
                        "schema_version is {SCHEMA_VERSION} but tables are missing: {}; \
                         the database may be damaged and will not be silently rebuilt",
                        missing.join(", ")
                    )));
                }
            }
            Some(v) => {
                return Err(DatabaseError::MissingMetadata(format!(
                    "unrecognised schema_version {v}"
                )));
            }
        }
    }
    Ok(Database {
        conn,
        path: db_path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_db_with(path: &Path, sql: &str) {
        let conn = Connection::open(path).expect("raw open");
        conn.execute_batch(sql).expect("raw sql");
    }

    #[test]
    fn initialise_creates_database_with_schema_version() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("database").join("dexo.db");
        let db = initialise(&db_path).expect("initialise");
        assert!(db_path.exists());
        assert_eq!(db.schema_version().expect("version"), Some(SCHEMA_VERSION));
        let tables = existing_tables(&db.conn).expect("tables");
        for t in BUSINESS_TABLES {
            assert!(tables.contains(t), "missing table {t}");
        }
    }

    #[test]
    fn initialise_enables_wal_mode() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("dexo.db");
        let db = initialise(&db_path).expect("initialise");
        let mode: String = db
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal_mode");
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn reopening_keeps_existing_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("dexo.db");
        {
            let db = initialise(&db_path).expect("initialise");
            db.conn
                .execute(
                    "INSERT INTO dexo_meta (key, value) VALUES ('marker', 'kept')",
                    [],
                )
                .expect("insert marker");
        }
        let db = initialise(&db_path).expect("reopen");
        let marker: String = db
            .conn
            .query_row(
                "SELECT value FROM dexo_meta WHERE key = 'marker'",
                [],
                |row| row.get(0),
            )
            .expect("marker survives reopen");
        assert_eq!(marker, "kept");
        assert_eq!(db.schema_version().expect("version"), Some(SCHEMA_VERSION));
    }

    #[test]
    fn v1_database_is_migrated_and_data_preserved() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("dexo.db");
        raw_db_with(
            &db_path,
            "CREATE TABLE dexo_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO dexo_meta VALUES ('schema_version', '1');
             INSERT INTO dexo_meta VALUES ('marker', 'from-v1');",
        );
        let db = initialise(&db_path).expect("migrate");
        assert_eq!(db.schema_version().expect("version"), Some(2));
        let marker: String = db
            .conn
            .query_row(
                "SELECT value FROM dexo_meta WHERE key = 'marker'",
                [],
                |row| row.get(0),
            )
            .expect("marker preserved");
        assert_eq!(marker, "from-v1");
        assert!(existing_tables(&db.conn)
            .expect("tables")
            .contains("orders"));
    }

    #[test]
    fn tables_without_meta_are_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("dexo.db");
        raw_db_with(&db_path, "CREATE TABLE something (id INTEGER);");
        let result = initialise(&db_path);
        assert!(matches!(result, Err(DatabaseError::MissingMetadata(_))));
    }

    #[test]
    fn meta_without_version_row_is_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("dexo.db");
        raw_db_with(
            &db_path,
            "CREATE TABLE dexo_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        );
        let result = initialise(&db_path);
        assert!(matches!(result, Err(DatabaseError::MissingMetadata(_))));
    }

    #[test]
    fn newer_version_is_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("dexo.db");
        raw_db_with(
            &db_path,
            "CREATE TABLE dexo_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO dexo_meta VALUES ('schema_version', '99');",
        );
        let result = initialise(&db_path);
        assert!(matches!(result, Err(DatabaseError::UnsupportedVersion(99))));
    }

    #[test]
    fn current_version_with_missing_tables_is_reported_not_rebuilt() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("dexo.db");
        raw_db_with(
            &db_path,
            "CREATE TABLE dexo_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO dexo_meta VALUES ('schema_version', '2');",
        );
        let result = initialise(&db_path);
        assert!(matches!(result, Err(DatabaseError::MigrationFailed(_))));
        // And nothing was created behind our back.
        let conn = Connection::open(&db_path).expect("raw open");
        let tables = existing_tables(&conn).expect("tables");
        assert!(!tables.contains("orders"));
    }
}
