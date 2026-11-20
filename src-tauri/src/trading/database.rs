//! SQLite persistence foundation for the trading center.
//!
//! The database lives at `data/database/dexo.db` in WAL mode. Phase 1 only
//! creates the database and its schema-version metadata; order, fill and
//! position tables land in Phase 6. The exchange remains the authoritative
//! source for real positions and fills — this database is the local record
//! used for persistence, tracking and recovery.

use std::path::Path;

use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug)]
pub enum DatabaseError {
    OpenFailed(rusqlite::Error),
    InitFailed(rusqlite::Error),
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenFailed(e) => write!(f, "failed to open database: {e}"),
            Self::InitFailed(e) => write!(f, "failed to initialise database: {e}"),
        }
    }
}

impl std::error::Error for DatabaseError {}

/// Handle to the application database. The connection is opened on the
/// startup path and shared through application state; heavier trading
/// access patterns are introduced with the trading center in Phase 6.
pub struct Database {
    conn: Connection,
    path: std::path::PathBuf,
}

impl Database {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Schema version recorded in the database metadata table.
    pub fn schema_version(&self) -> Result<Option<i64>, DatabaseError> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM dexo_meta WHERE key = 'schema_version'")
            .map_err(DatabaseError::InitFailed)?;
        let mut rows = stmt.query([]).map_err(DatabaseError::InitFailed)?;
        match rows.next().map_err(DatabaseError::InitFailed)? {
            Some(row) => {
                let raw: String = row.get(0).map_err(DatabaseError::InitFailed)?;
                Ok(raw.parse::<i64>().ok())
            }
            None => Ok(None),
        }
    }
}

/// Opens (creating if needed) the database at `db_path`, enables WAL and
/// foreign keys, and seeds the schema version on a fresh database.
pub fn initialise(db_path: &Path) -> Result<Database, DatabaseError> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            DatabaseError::OpenFailed(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(format!("cannot create database directory: {e}")),
            ))
        })?;
    }
    let conn = Connection::open(db_path).map_err(DatabaseError::OpenFailed)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(DatabaseError::InitFailed)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(DatabaseError::InitFailed)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS dexo_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
    .map_err(DatabaseError::InitFailed)?;
    conn.execute(
        "INSERT OR IGNORE INTO dexo_meta (key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )
    .map_err(DatabaseError::InitFailed)?;
    Ok(Database {
        conn,
        path: db_path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialise_creates_database_with_schema_version() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("database").join("dexo.db");
        let db = initialise(&db_path).expect("initialise");
        assert!(db_path.exists());
        assert_eq!(db.schema_version().expect("version"), Some(SCHEMA_VERSION));
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
}
