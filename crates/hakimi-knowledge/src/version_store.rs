use crate::version::KnowledgeVersion;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Storage for knowledge entry versions
pub struct VersionStore {
    conn: Arc<Mutex<Connection>>,
}

impl VersionStore {
    /// Create a new VersionStore with the given SQLite database path
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS knowledge_versions (
                id TEXT PRIMARY KEY,
                knowledge_key TEXT NOT NULL,
                version INTEGER NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                created_by TEXT,
                change_summary TEXT,
                UNIQUE(knowledge_key, version)
            )
            "#,
            [],
        )?;

        conn.execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_knowledge_key 
            ON knowledge_versions(knowledge_key)
            "#,
            [],
        )?;

        conn.execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_knowledge_key_version 
            ON knowledge_versions(knowledge_key, version DESC)
            "#,
            [],
        )?;

        Ok(())
    }

    /// Save a version to the database
    pub fn save_version(&self, version: &KnowledgeVersion) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let metadata_json = serde_json::to_string(&version.metadata)?;

        conn.execute(
            r#"
            INSERT INTO knowledge_versions 
            (id, knowledge_key, version, content, metadata, created_at, created_by, change_summary)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                version.id,
                version.knowledge_key,
                version.version,
                version.content,
                metadata_json,
                version.created_at,
                version.created_by,
                version.change_summary,
            ],
        )
        .context("Failed to save knowledge version")?;

        Ok(())
    }

    /// Get a specific version of a knowledge entry
    pub fn get_version(
        &self,
        knowledge_key: &str,
        version: i32,
    ) -> Result<Option<KnowledgeVersion>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, knowledge_key, version, content, metadata, created_at, created_by, change_summary
            FROM knowledge_versions
            WHERE knowledge_key = ?1 AND version = ?2
            "#,
        )?;

        let result = stmt
            .query_row(params![knowledge_key, version], |row| {
                let metadata_json: String = row.get(4)?;
                Ok(KnowledgeVersion {
                    id: row.get(0)?,
                    knowledge_key: row.get(1)?,
                    version: row.get(2)?,
                    content: row.get(3)?,
                    metadata: serde_json::from_str(&metadata_json).unwrap_or(serde_json::json!({})),
                    created_at: row.get(5)?,
                    created_by: row.get(6)?,
                    change_summary: row.get(7)?,
                })
            })
            .optional()?;

        Ok(result)
    }

    /// Get all versions of a knowledge entry, ordered by version descending
    pub fn get_all_versions(&self, knowledge_key: &str) -> Result<Vec<KnowledgeVersion>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, knowledge_key, version, content, metadata, created_at, created_by, change_summary
            FROM knowledge_versions
            WHERE knowledge_key = ?1
            ORDER BY version DESC
            "#,
        )?;

        let rows = stmt.query_map(params![knowledge_key], |row| {
            let metadata_json: String = row.get(4)?;
            Ok(KnowledgeVersion {
                id: row.get(0)?,
                knowledge_key: row.get(1)?,
                version: row.get(2)?,
                content: row.get(3)?,
                metadata: serde_json::from_str(&metadata_json).unwrap_or(serde_json::json!({})),
                created_at: row.get(5)?,
                created_by: row.get(6)?,
                change_summary: row.get(7)?,
            })
        })?;

        let mut versions = Vec::new();
        for row in rows {
            versions.push(row?);
        }

        Ok(versions)
    }

    /// Get the latest version number for a knowledge entry
    pub fn get_latest_version_number(&self, knowledge_key: &str) -> Result<Option<i32>> {
        let conn = self.conn.lock().unwrap();
        let result: Option<i32> = conn
            .query_row(
                r#"
                SELECT version FROM knowledge_versions
                WHERE knowledge_key = ?1
                ORDER BY version DESC
                LIMIT 1
                "#,
                params![knowledge_key],
                |row| row.get(0),
            )
            .optional()?;

        Ok(result)
    }

    /// Delete all versions for a knowledge entry
    pub fn delete_versions(&self, knowledge_key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM knowledge_versions WHERE knowledge_key = ?1",
            params![knowledge_key],
        )?;
        Ok(())
    }

    /// Get version count for a knowledge entry
    pub fn get_version_count(&self, knowledge_key: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM knowledge_versions WHERE knowledge_key = ?1",
            params![knowledge_key],
            |row| row.get(0),
        )?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_store() -> (VersionStore, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let store = VersionStore::new(temp_file.path()).unwrap();
        (store, temp_file)
    }

    #[test]
    fn test_save_and_retrieve_version() {
        let (store, _temp) = create_test_store();

        let version = KnowledgeVersion::new(
            "test_key".to_string(),
            1,
            "test content".to_string(),
            serde_json::json!({"type": "test"}),
            Some("Initial version".to_string()),
        );

        store.save_version(&version).unwrap();

        let retrieved = store.get_version("test_key", 1).unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.knowledge_key, "test_key");
        assert_eq!(retrieved.version, 1);
        assert_eq!(retrieved.content, "test content");
        assert_eq!(
            retrieved.change_summary,
            Some("Initial version".to_string())
        );
    }

    #[test]
    fn test_version_history_ordering() {
        let (store, _temp) = create_test_store();

        // Create multiple versions
        for i in 1..=5 {
            let version = KnowledgeVersion::new(
                "test_key".to_string(),
                i,
                format!("content version {}", i),
                serde_json::json!({}),
                Some(format!("Version {}", i)),
            );
            store.save_version(&version).unwrap();
        }

        let versions = store.get_all_versions("test_key").unwrap();
        assert_eq!(versions.len(), 5);

        // Should be in descending order
        for (i, version) in versions.iter().enumerate() {
            assert_eq!(version.version, (5 - i) as i32);
        }
    }

    #[test]
    fn test_get_latest_version_number() {
        let (store, _temp) = create_test_store();

        // No versions yet
        let latest = store.get_latest_version_number("test_key").unwrap();
        assert!(latest.is_none());

        // Add versions
        for i in 1..=3 {
            let version = KnowledgeVersion::new(
                "test_key".to_string(),
                i,
                format!("content {}", i),
                serde_json::json!({}),
                None,
            );
            store.save_version(&version).unwrap();
        }

        let latest = store.get_latest_version_number("test_key").unwrap();
        assert_eq!(latest, Some(3));
    }

    #[test]
    fn test_version_count() {
        let (store, _temp) = create_test_store();

        let count = store.get_version_count("test_key").unwrap();
        assert_eq!(count, 0);

        for i in 1..=7 {
            let version = KnowledgeVersion::new(
                "test_key".to_string(),
                i,
                format!("content {}", i),
                serde_json::json!({}),
                None,
            );
            store.save_version(&version).unwrap();
        }

        let count = store.get_version_count("test_key").unwrap();
        assert_eq!(count, 7);
    }

    #[test]
    fn test_delete_versions() {
        let (store, _temp) = create_test_store();

        for i in 1..=3 {
            let version = KnowledgeVersion::new(
                "test_key".to_string(),
                i,
                format!("content {}", i),
                serde_json::json!({}),
                None,
            );
            store.save_version(&version).unwrap();
        }

        store.delete_versions("test_key").unwrap();

        let count = store.get_version_count("test_key").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_unique_constraint() {
        let (store, _temp) = create_test_store();

        let version = KnowledgeVersion::new(
            "test_key".to_string(),
            1,
            "content v1".to_string(),
            serde_json::json!({}),
            None,
        );

        store.save_version(&version).unwrap();

        // Try to save same key+version again
        let duplicate = KnowledgeVersion::new(
            "test_key".to_string(),
            1,
            "different content".to_string(),
            serde_json::json!({}),
            None,
        );

        let result = store.save_version(&duplicate);
        assert!(result.is_err());
    }
}
