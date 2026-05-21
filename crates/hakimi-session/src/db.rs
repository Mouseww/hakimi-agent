//! Database connection wrapper with WAL mode and busy timeout.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use tracing::info;

use crate::schema;

/// Helper: create an in-memory `SessionDB` with schema fully initialized.
#[cfg(test)]
pub(crate) fn test_db() -> SessionDB {
    let db = SessionDB::new(std::path::Path::new(":memory:")).unwrap();
    db.initialize().unwrap();
    db
}

/// Thread-safe wrapper around a SQLite connection for session storage.
pub struct SessionDB {
    conn: Mutex<Connection>,
}

impl SessionDB {
    /// Open (or create) a SQLite database at `path` with WAL mode enabled.
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )
        .context("Failed to set PRAGMA options")?;

        info!("Opened session database at {}", path.display());

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Run schema migrations: create core tables and FTS virtual table.
    pub fn initialize(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(schema::SCHEMA_SQL)
            .context("Failed to create core tables")?;
        conn.execute_batch(schema::FTS_SQL)
            .context("Failed to create FTS tables")?;
        info!("Session database schema initialized");
        Ok(())
    }

    /// Access the underlying connection mutex.
    pub fn conn(&self) -> &Mutex<Connection> {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_in_memory() {
        let db = SessionDB::new(std::path::Path::new(":memory:")).unwrap();
        // Should be able to acquire the connection lock
        let _conn = db.conn().lock().unwrap();
    }

    #[test]
    fn test_initialize_creates_tables() {
        let db = test_db();
        let conn = db.conn().lock().unwrap();

        // sessions table should exist
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // messages table should exist
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_initialize_creates_fts5_table() {
        let db = test_db();
        let conn = db.conn().lock().unwrap();

        // FTS5 virtual table should exist and be queryable
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_initialize_is_idempotent() {
        let db = test_db();
        // Calling initialize() a second time should not fail (IF NOT EXISTS)
        db.initialize().unwrap();
        db.initialize().unwrap();
    }

    #[test]
    fn test_pragma_settings() {
        let db = SessionDB::new(std::path::Path::new(":memory:")).unwrap();
        let conn = db.conn().lock().unwrap();

        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1, "foreign_keys should be ON");

        let busy: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy, 5000);
    }
}
