//! Progress storage for batch jobs using SQLite.

use crate::progress::JobProgress;
use anyhow::Result;
use rusqlite::{Connection, params};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// Store for persisting job progress.
pub struct ProgressStore {
    conn: Arc<Mutex<Connection>>,
}

impl ProgressStore {
    /// Create a new progress store with the given database path.
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create an in-memory progress store for testing.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Initialize the database schema.
    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS job_progress (
                job_id TEXT PRIMARY KEY,
                progress_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        info!("Initialized job_progress table");
        Ok(())
    }

    /// Save progress for a job.
    pub fn save_progress(&self, job_id: &str, progress: &JobProgress) -> Result<()> {
        let progress_json = serde_json::to_string(progress)?;
        let updated_at = chrono::Utc::now().timestamp();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO job_progress (job_id, progress_json, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(job_id) DO UPDATE SET
                progress_json = excluded.progress_json,
                updated_at = excluded.updated_at
            "#,
            params![job_id, progress_json, updated_at],
        )?;

        debug!(job_id, "Saved job progress");
        Ok(())
    }

    /// Get progress for a job.
    pub fn get_progress(&self, job_id: &str) -> Result<Option<JobProgress>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT progress_json FROM job_progress WHERE job_id = ?")?;

        let result = stmt.query_row([job_id], |row| {
            let progress_json: String = row.get(0)?;
            Ok(progress_json)
        });

        match result {
            Ok(progress_json) => {
                let progress = serde_json::from_str(&progress_json)?;
                Ok(Some(progress))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete progress for a job.
    pub fn delete_progress(&self, job_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM job_progress WHERE job_id = ?", params![job_id])?;

        debug!(job_id, "Deleted job progress");
        Ok(())
    }

    /// List all job IDs with progress.
    pub fn list_job_ids(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT job_id FROM job_progress ORDER BY updated_at DESC")?;

        let job_ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;

        Ok(job_ids)
    }

    /// Clean up old progress entries (older than specified days).
    pub fn cleanup_old(&self, days: i64) -> Result<usize> {
        let cutoff = chrono::Utc::now().timestamp() - (days * 86400);

        let conn = self.conn.lock().unwrap();
        let deleted = conn.execute(
            "DELETE FROM job_progress WHERE updated_at < ?",
            params![cutoff],
        )?;

        info!(deleted, days, "Cleaned up old progress entries");
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::{JobProgress, StageStatus};

    fn create_test_store() -> ProgressStore {
        ProgressStore::in_memory().unwrap()
    }

    #[test]
    fn test_save_and_get_progress() {
        let store = create_test_store();
        let stages = vec!["load".to_string(), "process".to_string()];
        let mut progress = JobProgress::new(100, stages);
        progress.start_stage("load");
        progress.update_step(25);

        store.save_progress("job-1", &progress).unwrap();

        let loaded = store.get_progress("job-1").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.current_step, 25);
        assert_eq!(loaded.current_stage, "load");
        assert_eq!(loaded.stages[0].status, StageStatus::Running);
    }

    #[test]
    fn test_get_nonexistent_progress() {
        let store = create_test_store();
        let result = store.get_progress("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_update_progress() {
        let store = create_test_store();
        let stages = vec!["process".to_string()];
        let mut progress = JobProgress::new(10, stages);

        store.save_progress("job-1", &progress).unwrap();
        progress.update_step(5);
        store.save_progress("job-1", &progress).unwrap();

        let loaded = store.get_progress("job-1").unwrap().unwrap();
        assert_eq!(loaded.current_step, 5);
    }

    #[test]
    fn test_delete_progress() {
        let store = create_test_store();
        let progress = JobProgress::new(10, vec!["test".to_string()]);

        store.save_progress("job-1", &progress).unwrap();
        assert!(store.get_progress("job-1").unwrap().is_some());

        store.delete_progress("job-1").unwrap();
        assert!(store.get_progress("job-1").unwrap().is_none());
    }

    #[test]
    fn test_list_job_ids() {
        let store = create_test_store();
        let progress = JobProgress::new(10, vec!["test".to_string()]);

        store.save_progress("job-1", &progress).unwrap();
        store.save_progress("job-2", &progress).unwrap();
        store.save_progress("job-3", &progress).unwrap();

        let job_ids = store.list_job_ids().unwrap();
        assert_eq!(job_ids.len(), 3);
        assert!(job_ids.contains(&"job-1".to_string()));
        assert!(job_ids.contains(&"job-2".to_string()));
        assert!(job_ids.contains(&"job-3".to_string()));
    }

    #[test]
    fn test_cleanup_old() {
        let store = create_test_store();
        let progress = JobProgress::new(10, vec!["test".to_string()]);

        store.save_progress("job-1", &progress).unwrap();

        // Cleanup entries older than 1000 days should not delete anything
        let deleted = store.cleanup_old(1000).unwrap();
        assert_eq!(deleted, 0);
        assert!(store.get_progress("job-1").unwrap().is_some());
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let store = Arc::new(create_test_store());
        let mut handles = vec![];

        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = thread::spawn(move || {
                let progress = JobProgress::new(100, vec!["test".to_string()]);
                let job_id = format!("job-{}", i);
                store_clone.save_progress(&job_id, &progress).unwrap();
                store_clone.get_progress(&job_id).unwrap()
            });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.join().unwrap();
            assert!(result.is_some());
        }

        let job_ids = store.list_job_ids().unwrap();
        assert_eq!(job_ids.len(), 10);
    }
}
