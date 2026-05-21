//! SQLite-based persistent storage for cron jobs.
//!
//! Provides durable job storage with file-based locking for multi-process safety.

use crate::{CronJob, CronSchedule, CronScheduler};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;
use tracing::{debug, info, warn};

/// Persistent cron job store backed by SQLite.
pub struct PersistentCronStore {
    conn: Mutex<Connection>,
}

impl PersistentCronStore {
    /// Open or create a persistent store at the given path.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrent access.
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;

        // Create the jobs table if it doesn't exist.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cron_jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                schedule_type TEXT NOT NULL,
                schedule_value TEXT NOT NULL,
                prompt TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run TEXT,
                next_run TEXT,
                toolsets TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;

        info!(path = %path.display(), "Persistent cron store opened");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Save a job to the store (upsert).
    pub fn save_job(&self, job: &CronJob) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let (schedule_type, schedule_value) = match &job.schedule {
            CronSchedule::IntervalMinutes(m) => ("minutes", m.to_string()),
            CronSchedule::IntervalHours(h) => ("hours", h.to_string()),
            CronSchedule::CronExpr(expr) => ("cron", expr.clone()),
        };

        conn.execute(
            "INSERT OR REPLACE INTO cron_jobs (id, name, schedule_type, schedule_value, prompt, enabled, last_run, next_run)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                job.id,
                job.name,
                schedule_type,
                schedule_value,
                job.prompt,
                job.enabled as i32,
                job.last_run.map(|t| t.to_rfc3339()),
                job.next_run.map(|t| t.to_rfc3339()),
            ],
        )?;

        debug!(job_id = %job.id, name = %job.name, "Job saved to store");
        Ok(())
    }

    /// Load all jobs from the store.
    pub fn load_all(&self) -> anyhow::Result<Vec<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule_type, schedule_value, prompt, enabled, last_run, next_run FROM cron_jobs",
        )?;

        let jobs = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let schedule_type: String = row.get(2)?;
                let schedule_value: String = row.get(3)?;
                let prompt: String = row.get(4)?;
                let enabled: i32 = row.get(5)?;
                let last_run: Option<String> = row.get(6)?;
                let next_run: Option<String> = row.get(7)?;

                let schedule = match schedule_type.as_str() {
                    "minutes" => CronSchedule::IntervalMinutes(
                        schedule_value.parse().unwrap_or(60),
                    ),
                    "hours" => CronSchedule::IntervalHours(
                        schedule_value.parse().unwrap_or(1),
                    ),
                    _ => CronSchedule::CronExpr(schedule_value),
                };

                Ok(CronJob {
                    id,
                    name,
                    schedule,
                    prompt,
                    enabled: enabled != 0,
                    last_run: last_run.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
                    next_run: next_run.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(jobs)
    }

    /// Remove a job by ID.
    pub fn remove_job(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute("DELETE FROM cron_jobs WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// Enable or disable a job.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE cron_jobs SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i32],
        )?;
        Ok(changed > 0)
    }

    /// Update the last_run and next_run for a job.
    pub fn update_run_times(
        &self,
        id: &str,
        last_run: DateTime<Utc>,
        next_run: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE cron_jobs SET last_run = ?2, next_run = ?3 WHERE id = ?1",
            params![id, last_run.to_rfc3339(), next_run.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Load jobs into a CronScheduler.
    pub fn load_into_scheduler(&self) -> anyhow::Result<CronScheduler> {
        let jobs = self.load_all()?;
        let mut scheduler = CronScheduler::new();
        for job in jobs {
            scheduler.add(job);
        }
        info!(count = scheduler.list().len(), "Loaded jobs into scheduler");
        Ok(scheduler)
    }
}

/// File-based lock for multi-process cron safety.
pub struct FileLock {
    path: std::path::PathBuf,
}

impl FileLock {
    /// Acquire a file lock at the given path.
    pub fn acquire(path: &Path) -> anyhow::Result<Self> {
        // Create a lock file. If it exists and is recent, another process holds it.
        if path.exists() {
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(modified) = metadata.modified() {
                    let age = std::time::SystemTime::now()
                        .duration_since(modified)
                        .unwrap_or_default();
                    // If lock is older than 60 seconds, consider it stale.
                    if age < std::time::Duration::from_secs(60) {
                        anyhow::bail!(
                            "Cron lock file exists at {} (held by another process)",
                            path.display()
                        );
                    }
                    warn!("Removing stale lock file at {}", path.display());
                }
            }
        }
        std::fs::write(path, std::process::id().to_string())?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persistent_store_open() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        // Should be empty initially.
        let jobs = store.load_all().unwrap();
        assert!(jobs.is_empty());
    }

    #[test]
    fn test_save_and_load_job() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        let job = CronJob::new("test-job", CronSchedule::IntervalMinutes(30), "do something");
        let id = job.id.clone();
        store.save_job(&job).unwrap();

        let jobs = store.load_all().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
        assert_eq!(jobs[0].name, "test-job");
    }

    #[test]
    fn test_remove_job() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        let job = CronJob::new("test", CronSchedule::IntervalHours(1), "prompt");
        let id = job.id.clone();
        store.save_job(&job).unwrap();

        assert!(store.remove_job(&id).unwrap());
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn test_set_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        let job = CronJob::new("test", CronSchedule::IntervalHours(1), "prompt");
        let id = job.id.clone();
        store.save_job(&job).unwrap();

        assert!(store.set_enabled(&id, false).unwrap());
        let jobs = store.load_all().unwrap();
        assert!(!jobs[0].enabled);
    }

    #[test]
    fn test_load_into_scheduler() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        store.save_job(&CronJob::new("j1", CronSchedule::IntervalMinutes(10), "p1")).unwrap();
        store.save_job(&CronJob::new("j2", CronSchedule::IntervalHours(2), "p2")).unwrap();

        let scheduler = store.load_into_scheduler().unwrap();
        assert_eq!(scheduler.list().len(), 2);
    }

    #[test]
    fn test_file_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_path = tmp.path().join("test.lock");

        let lock = FileLock::acquire(&lock_path).unwrap();
        assert!(lock_path.exists());

        // Second acquire should fail.
        assert!(FileLock::acquire(&lock_path).is_err());

        drop(lock);
        // After releasing, should be able to acquire again.
        let _lock2 = FileLock::acquire(&lock_path).unwrap();
    }
}
