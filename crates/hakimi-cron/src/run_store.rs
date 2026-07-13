//! Persistent storage for cron job run history.

use crate::retry::{AttemptStatus, CronJobRun, RunAttempt, RunStatus};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;
use tracing::{debug, info};

/// Persistent store for cron job run history.
pub struct CronRunStore {
    conn: Mutex<Connection>,
}

impl CronRunStore {
    /// Open or create a run store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Create tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cron_runs (
                id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                completed_at INTEGER,
                status TEXT NOT NULL,
                error TEXT,
                duration_ms INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS cron_run_attempts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                attempt_number INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                completed_at INTEGER,
                status TEXT NOT NULL,
                error TEXT,
                duration_ms INTEGER,
                FOREIGN KEY(run_id) REFERENCES cron_runs(id) ON DELETE CASCADE
            );",
            [],
        )?;

        // Create indices for efficient queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cron_runs_job_id ON cron_runs(job_id);",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cron_runs_status ON cron_runs(status);",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cron_runs_started_at ON cron_runs(started_at);",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cron_run_attempts_run_id ON cron_run_attempts(run_id);",
            [],
        )?;

        info!(path = %path.display(), "Cron run store opened");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Save or update a run.
    pub fn save_run(&self, run: &CronJobRun) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT OR REPLACE INTO cron_runs (
                id, job_id, started_at, completed_at, status, error, duration_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run.id,
                run.job_id,
                run.started_at,
                run.completed_at,
                format!("{:?}", run.status),
                run.error.as_deref(),
                run.duration_ms.map(|d| d as i64),
            ],
        )?;

        // Delete existing attempts and re-insert (simpler than update logic)
        conn.execute(
            "DELETE FROM cron_run_attempts WHERE run_id = ?1",
            params![run.id],
        )?;

        for attempt in &run.attempts {
            conn.execute(
                "INSERT INTO cron_run_attempts (
                    run_id, attempt_number, started_at, completed_at, status, error, duration_ms
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    run.id,
                    attempt.attempt_number as i64,
                    attempt.started_at,
                    attempt.completed_at,
                    format!("{:?}", attempt.status),
                    attempt.error.as_deref(),
                    attempt.duration_ms.map(|d| d as i64),
                ],
            )?;
        }

        debug!(run_id = %run.id, job_id = %run.job_id, "Run saved to store");
        Ok(())
    }

    /// Load a single run by ID.
    pub fn get_run(&self, run_id: &str) -> Result<Option<CronJobRun>> {
        let conn = self.conn.lock().unwrap();

        let run_row = conn
            .query_row(
                "SELECT id, job_id, started_at, completed_at, status, error, duration_ms
                 FROM cron_runs WHERE id = ?1",
                params![run_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                    ))
                },
            )
            .optional()?;

        let Some((id, job_id, started_at, completed_at, status_str, error, duration_ms)) = run_row
        else {
            return Ok(None);
        };

        let status = parse_run_status(&status_str);

        // Load attempts
        let mut stmt = conn.prepare(
            "SELECT attempt_number, started_at, completed_at, status, error, duration_ms
             FROM cron_run_attempts WHERE run_id = ?1 ORDER BY attempt_number",
        )?;

        let attempts = stmt
            .query_map(params![run_id], |row| {
                let attempt_status_str: String = row.get(3)?;
                Ok(RunAttempt {
                    attempt_number: row.get::<_, i64>(0)? as usize,
                    started_at: row.get(1)?,
                    completed_at: row.get(2)?,
                    status: parse_attempt_status(&attempt_status_str),
                    error: row.get(4)?,
                    duration_ms: row.get::<_, Option<i64>>(5)?.map(|d| d as u64),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(Some(CronJobRun {
            id,
            job_id,
            started_at,
            completed_at,
            status,
            attempts,
            error,
            duration_ms: duration_ms.map(|d| d as u64),
        }))
    }

    /// Get all runs for a specific job, ordered by start time (newest first).
    pub fn get_job_runs(&self, job_id: &str, limit: usize) -> Result<Vec<CronJobRun>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id FROM cron_runs
             WHERE job_id = ?1
             ORDER BY started_at DESC
             LIMIT ?2",
        )?;

        let run_ids: Vec<String> = stmt
            .query_map(params![job_id, limit as i64], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        drop(stmt);
        drop(conn);

        // Load each run with its attempts
        run_ids
            .into_iter()
            .filter_map(|run_id| self.get_run(&run_id).ok().flatten())
            .collect::<Vec<_>>()
            .pipe(Ok)
    }

    /// Get recent runs across all jobs, ordered by start time (newest first).
    pub fn get_recent_runs(&self, limit: usize) -> Result<Vec<CronJobRun>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id FROM cron_runs
             ORDER BY started_at DESC
             LIMIT ?1",
        )?;

        let run_ids: Vec<String> = stmt
            .query_map(params![limit as i64], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        drop(stmt);
        drop(conn);

        run_ids
            .into_iter()
            .filter_map(|run_id| self.get_run(&run_id).ok().flatten())
            .collect::<Vec<_>>()
            .pipe(Ok)
    }

    /// Get all failed runs (after all retries exhausted).
    pub fn get_failed_runs(&self, limit: usize) -> Result<Vec<CronJobRun>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id FROM cron_runs
             WHERE status = 'FailedAfterRetries'
             ORDER BY started_at DESC
             LIMIT ?1",
        )?;

        let run_ids: Vec<String> = stmt
            .query_map(params![limit as i64], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        drop(stmt);
        drop(conn);

        run_ids
            .into_iter()
            .filter_map(|run_id| self.get_run(&run_id).ok().flatten())
            .collect::<Vec<_>>()
            .pipe(Ok)
    }

    /// Delete old runs to manage storage. Keeps the most recent N runs per job.
    pub fn prune_old_runs(&self, keep_per_job: usize) -> Result<usize> {
        let conn = self.conn.lock().unwrap();

        // Get list of all job IDs
        let mut stmt = conn.prepare("SELECT DISTINCT job_id FROM cron_runs")?;
        let job_ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut deleted = 0;

        for job_id in job_ids {
            // Get run IDs to delete (keep only the most recent N)
            let mut stmt = conn.prepare(
                "SELECT id FROM cron_runs
                 WHERE job_id = ?1
                 ORDER BY started_at DESC
                 LIMIT -1 OFFSET ?2",
            )?;

            let to_delete: Vec<String> = stmt
                .query_map(params![job_id, keep_per_job as i64], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            for run_id in to_delete {
                conn.execute("DELETE FROM cron_runs WHERE id = ?1", params![run_id])?;
                deleted += 1;
            }
        }

        if deleted > 0 {
            info!(deleted, "Pruned old cron run records");
        }

        Ok(deleted)
    }
}

// Helper trait for pipe-style method chaining
trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}

impl<T> Pipe for T {}

fn parse_run_status(s: &str) -> RunStatus {
    match s {
        "Running" => RunStatus::Running,
        "Success" => RunStatus::Success,
        "FailedAfterRetries" => RunStatus::FailedAfterRetries,
        "Cancelled" => RunStatus::Cancelled,
        _ => RunStatus::FailedAfterRetries, // Default for unknown
    }
}

fn parse_attempt_status(s: &str) -> AttemptStatus {
    match s {
        "Running" => AttemptStatus::Running,
        "Success" => AttemptStatus::Success,
        "Failed" => AttemptStatus::Failed,
        "Cancelled" => AttemptStatus::Cancelled,
        _ => AttemptStatus::Failed, // Default for unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retry::CronJobRun;
    use tempfile::NamedTempFile;

    #[test]
    fn test_save_and_load_run() {
        let temp = NamedTempFile::new().unwrap();
        let store = CronRunStore::open(temp.path()).unwrap();

        let mut run = CronJobRun::new("job-123");
        let mut attempt = RunAttempt::new(1);
        attempt.complete(AttemptStatus::Success, None);
        run.attempts.push(attempt);
        run.complete(RunStatus::Success, None);

        store.save_run(&run).unwrap();

        let loaded = store.get_run(&run.id).unwrap().unwrap();
        assert_eq!(loaded.id, run.id);
        assert_eq!(loaded.job_id, "job-123");
        assert_eq!(loaded.status, RunStatus::Success);
        assert_eq!(loaded.attempts.len(), 1);
        assert_eq!(loaded.attempts[0].attempt_number, 1);
        assert_eq!(loaded.attempts[0].status, AttemptStatus::Success);
    }

    #[test]
    fn test_get_job_runs() {
        let temp = NamedTempFile::new().unwrap();
        let store = CronRunStore::open(temp.path()).unwrap();

        // Create multiple runs for the same job
        for _i in 0..5 {
            let mut run = CronJobRun::new("job-123");
            run.complete(RunStatus::Success, None);
            // Ensure different timestamps
            std::thread::sleep(std::time::Duration::from_millis(10));
            store.save_run(&run).unwrap();
        }

        let runs = store.get_job_runs("job-123", 10).unwrap();
        assert_eq!(runs.len(), 5);

        // Should be ordered newest first
        for i in 0..4 {
            assert!(runs[i].started_at >= runs[i + 1].started_at);
        }
    }

    #[test]
    fn test_get_failed_runs() {
        let temp = NamedTempFile::new().unwrap();
        let store = CronRunStore::open(temp.path()).unwrap();

        // Create mix of successful and failed runs
        let mut success_run = CronJobRun::new("job-1");
        success_run.complete(RunStatus::Success, None);
        store.save_run(&success_run).unwrap();

        let mut failed_run = CronJobRun::new("job-2");
        failed_run.complete(RunStatus::FailedAfterRetries, Some("error".to_string()));
        store.save_run(&failed_run).unwrap();

        let failed = store.get_failed_runs(10).unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].status, RunStatus::FailedAfterRetries);
    }

    #[test]
    fn test_prune_old_runs() {
        let temp = NamedTempFile::new().unwrap();
        let store = CronRunStore::open(temp.path()).unwrap();

        // Create 10 runs for a job
        for _ in 0..10 {
            let mut run = CronJobRun::new("job-123");
            run.complete(RunStatus::Success, None);
            std::thread::sleep(std::time::Duration::from_millis(10));
            store.save_run(&run).unwrap();
        }

        // Keep only 5 most recent
        let deleted = store.prune_old_runs(5).unwrap();
        assert_eq!(deleted, 5);

        let remaining = store.get_job_runs("job-123", 100).unwrap();
        assert_eq!(remaining.len(), 5);
    }
}
