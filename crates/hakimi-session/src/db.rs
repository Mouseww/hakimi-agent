//! Database connection wrapper with WAL mode and busy timeout.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use tracing::info;

use crate::schema;

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
