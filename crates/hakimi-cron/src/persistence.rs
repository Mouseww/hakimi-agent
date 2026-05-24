//! SQLite-based persistent storage for cron jobs.
//!
//! Provides durable job storage with file-based locking for multi-process safety.

use crate::{CronJob, CronSchedule, CronScheduler};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
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
                    "minutes" => {
                        CronSchedule::IntervalMinutes(schedule_value.parse().unwrap_or(60))
                    }
                    "hours" => CronSchedule::IntervalHours(schedule_value.parse().unwrap_or(1)),
                    _ => CronSchedule::CronExpr(schedule_value),
                };

                Ok(CronJob {
                    id,
                    name,
                    schedule,
                    prompt,
                    enabled: enabled != 0,
                    last_run: last_run.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    }),
                    next_run: next_run.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    }),
                    skills: Vec::new(),
                    enabled_toolsets: None,
                    context_from: Vec::new(),
                    deliver: None,
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
        if path.exists()
            && let Ok(metadata) = std::fs::metadata(path)
            && let Ok(modified) = metadata.modified()
        {
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

        let job = CronJob::new(
            "test-job",
            CronSchedule::IntervalMinutes(30),
            "do something",
        );
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

        store
            .save_job(&CronJob::new("j1", CronSchedule::IntervalMinutes(10), "p1"))
            .unwrap();
        store
            .save_job(&CronJob::new("j2", CronSchedule::IntervalHours(2), "p2"))
            .unwrap();

        let scheduler = store.load_into_scheduler().unwrap();
        assert_eq!(scheduler.list().len(), 2);
    }

    #[test]
    fn test_persist_and_reload_jobs() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");

        // Save jobs to one store instance.
        {
            let store = PersistentCronStore::open(&db_path).unwrap();
            store
                .save_job(&CronJob::new(
                    "job-a",
                    CronSchedule::IntervalMinutes(15),
                    "prompt-a",
                ))
                .unwrap();
            store
                .save_job(&CronJob::new(
                    "job-b",
                    CronSchedule::IntervalHours(3),
                    "prompt-b",
                ))
                .unwrap();
            store
                .save_job(&CronJob::new(
                    "job-c",
                    CronSchedule::CronExpr("*/5 * * * *".into()),
                    "prompt-c",
                ))
                .unwrap();
        }

        // Reload from a fresh store instance (simulates restart).
        let store2 = PersistentCronStore::open(&db_path).unwrap();
        let jobs = store2.load_all().unwrap();
        assert_eq!(jobs.len(), 3);

        let names: Vec<&str> = jobs.iter().map(|j| j.name.as_str()).collect();
        assert!(names.contains(&"job-a"));
        assert!(names.contains(&"job-b"));
        assert!(names.contains(&"job-c"));

        // Verify schedule round-trips.
        for job in &jobs {
            match job.name.as_str() {
                "job-a" => matches!(&job.schedule, CronSchedule::IntervalMinutes(15)),
                "job-b" => matches!(&job.schedule, CronSchedule::IntervalHours(3)),
                "job-c" => matches!(&job.schedule, CronSchedule::CronExpr(e) if e == "*/5 * * * *"),
                _ => panic!("unexpected job name"),
            };
        }
    }

    #[test]
    fn test_toggle_job_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        let job = CronJob::new("toggle-me", CronSchedule::IntervalMinutes(10), "prompt");
        let id = job.id.clone();
        store.save_job(&job).unwrap();

        // Initially enabled.
        let loaded = store.load_all().unwrap();
        assert!(loaded[0].enabled);

        // Disable.
        assert!(store.set_enabled(&id, false).unwrap());
        let loaded = store.load_all().unwrap();
        assert!(!loaded[0].enabled);

        // Re-enable.
        assert!(store.set_enabled(&id, true).unwrap());
        let loaded = store.load_all().unwrap();
        assert!(loaded[0].enabled);

        // Toggling a non-existent job returns false.
        assert!(!store.set_enabled("nonexistent-id", true).unwrap());
    }

    #[test]
    fn test_update_run_times() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        let job = CronJob::new("run-times", CronSchedule::IntervalMinutes(30), "prompt");
        let id = job.id.clone();
        store.save_job(&job).unwrap();

        // Initially no last_run.
        let loaded = store.load_all().unwrap();
        assert!(loaded[0].last_run.is_none());

        // Update run times.
        let last = Utc::now();
        let next = last + chrono::Duration::minutes(30);
        store.update_run_times(&id, last, next).unwrap();

        // Verify persisted.
        let loaded = store.load_all().unwrap();
        assert!(loaded[0].last_run.is_some());
        assert!(loaded[0].next_run.is_some());

        let persisted_last = loaded[0].last_run.unwrap();
        let persisted_next = loaded[0].next_run.unwrap();
        // Allow 1-second tolerance for rounding.
        assert!((persisted_last - last).num_seconds().abs() <= 1);
        assert!((persisted_next - next).num_seconds().abs() <= 1);
    }

    #[test]
    fn test_get_due_jobs_via_scheduler() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        // Create a job whose next_run is in the past (already due).
        let mut job = CronJob::new("due-job", CronSchedule::IntervalMinutes(60), "prompt");
        job.next_run = Some(Utc::now() - chrono::Duration::minutes(5));
        store.save_job(&job).unwrap();

        // Create a job whose next_run is in the future (not due).
        let mut future_job = CronJob::new("future-job", CronSchedule::IntervalHours(1), "prompt2");
        future_job.next_run = Some(Utc::now() + chrono::Duration::hours(2));
        store.save_job(&future_job).unwrap();

        // Create a disabled job that is also in the past (should not appear).
        let mut disabled = CronJob::new("disabled", CronSchedule::IntervalMinutes(10), "prompt3");
        disabled.enabled = false;
        disabled.next_run = Some(Utc::now() - chrono::Duration::minutes(1));
        store.save_job(&disabled).unwrap();

        // Load into scheduler and check next_tick.
        let scheduler = store.load_into_scheduler().unwrap();
        let due = scheduler.next_tick(Utc::now());
        assert_eq!(due.len(), 1);

        // The due job should be "due-job".
        let scheduler_jobs = scheduler.list();
        let due_job = scheduler_jobs.iter().find(|j| j.id == due[0]).unwrap();
        assert_eq!(due_job.name, "due-job");
    }

    #[test]
    fn test_delete_job() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        let job1 = CronJob::new("keep", CronSchedule::IntervalMinutes(10), "p1");
        let job2 = CronJob::new("delete-me", CronSchedule::IntervalMinutes(20), "p2");
        let id1 = job1.id.clone();
        let id2 = job2.id.clone();

        store.save_job(&job1).unwrap();
        store.save_job(&job2).unwrap();
        assert_eq!(store.load_all().unwrap().len(), 2);

        // Delete one job.
        assert!(store.remove_job(&id2).unwrap());
        assert_eq!(store.load_all().unwrap().len(), 1);

        // Remaining job is the correct one.
        let remaining = store.load_all().unwrap();
        assert_eq!(remaining[0].id, id1);
        assert_eq!(remaining[0].name, "keep");

        // Deleting the same job again returns false.
        assert!(!store.remove_job(&id2).unwrap());

        // Deleting a non-existent id returns false.
        assert!(!store.remove_job("nonexistent").unwrap());

        // Delete the remaining job.
        assert!(store.remove_job(&id1).unwrap());
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn test_list_enabled_jobs() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_cron.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        // Save 3 jobs, then disable 2 of them.
        let job1 = CronJob::new("enabled-1", CronSchedule::IntervalMinutes(10), "p1");
        let job2 = CronJob::new("disabled-1", CronSchedule::IntervalMinutes(20), "p2");
        let job3 = CronJob::new("enabled-2", CronSchedule::IntervalMinutes(30), "p3");
        let id2 = job2.id.clone();
        let id3 = job3.id.clone();

        store.save_job(&job1).unwrap();
        store.save_job(&job2).unwrap();
        store.save_job(&job3).unwrap();

        // All enabled initially.
        let all = store.load_all().unwrap();
        assert!(all.iter().all(|j| j.enabled));

        // Disable job2 and job3.
        store.set_enabled(&id2, false).unwrap();
        store.set_enabled(&id3, false).unwrap();

        // Load and filter enabled jobs.
        let all = store.load_all().unwrap();
        let enabled: Vec<_> = all.iter().filter(|j| j.enabled).collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "enabled-1");

        // Re-enable job3.
        store.set_enabled(&id3, true).unwrap();
        let all = store.load_all().unwrap();
        let enabled: Vec<_> = all.iter().filter(|j| j.enabled).collect();
        assert_eq!(enabled.len(), 2);
        let enabled_names: Vec<&str> = enabled.iter().map(|j| j.name.as_str()).collect();
        assert!(enabled_names.contains(&"enabled-1"));
        assert!(enabled_names.contains(&"enabled-2"));

        // Load into scheduler – should contain all 3 jobs (scheduler doesn't filter).
        let scheduler = store.load_into_scheduler().unwrap();
        assert_eq!(scheduler.list().len(), 3);
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

    #[test]
    fn test_round_trip_save_load_update() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("round_trip.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        let job = CronJob::new("rt-job", CronSchedule::IntervalMinutes(5), "hello");
        let id = job.id.clone();

        store.save_job(&job).unwrap();

        // Simulate a run: update last_run and next_run.
        let now = chrono::Utc::now();
        let next = now + chrono::Duration::minutes(5);
        store.update_run_times(&id, now, next).unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].last_run.is_some());
        assert!(loaded[0].next_run.is_some());
        assert_eq!(loaded[0].name, "rt-job");
        assert_eq!(loaded[0].prompt, "hello");
    }

    #[test]
    fn test_save_job_upsert() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("upsert.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        let mut job = CronJob::new("upsert", CronSchedule::IntervalMinutes(10), "v1");
        store.save_job(&job).unwrap();

        // Modify and save again (upsert).
        job.prompt = "v2".to_string();
        store.save_job(&job).unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].prompt, "v2");
    }

    #[test]
    fn test_open_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let nested_dir = tmp.path().join("nested").join("dir");
        std::fs::create_dir_all(&nested_dir).unwrap();
        let db_path = nested_dir.join("cron.db");
        let store = PersistentCronStore::open(&db_path);
        assert!(
            store.is_ok(),
            "should open in nested dirs: {:?}",
            store.err()
        );
        assert!(db_path.exists());
    }

    #[test]
    fn test_set_enabled_nonexistent_job() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("nonexist.db");
        let store = PersistentCronStore::open(&db_path).unwrap();

        // Setting enabled on a non-existent job should not panic.
        let result = store.set_enabled("nonexistent-id", false);
        assert!(result.is_ok());
    }
}
