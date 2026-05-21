//! Cron scheduling for the Hakimi Agent.
//!
//! Provides recurring task scheduling with simple interval expressions
//! (e.g. "30m", "2h") and standard cron syntax.
//! Supports SQLite persistent storage with file-based locking.

pub mod persistence;

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Schedule representation
// ---------------------------------------------------------------------------

/// A parsed schedule that can compute the next run time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CronSchedule {
    /// Run every N minutes.
    IntervalMinutes(u64),
    /// Run every N hours.
    IntervalHours(u64),
    /// Raw cron expression (stored for display; full parsing is a TODO).
    CronExpr(String),
}

impl CronSchedule {
    /// Compute the next tick time given a reference instant.
    pub fn next_after(&self, after: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            CronSchedule::IntervalMinutes(m) => after + chrono::Duration::minutes(*m as i64),
            CronSchedule::IntervalHours(h) => after + chrono::Duration::hours(*h as i64),
            CronSchedule::CronExpr(_expr) => {
                // TODO: integrate a proper cron parser crate.
                // For now, fall back to a 1-hour default.
                after + chrono::Duration::hours(1)
            }
        }
    }
}

/// Parse a human-friendly schedule string.
///
/// Supported formats:
/// - `"30m"` – every 30 minutes
/// - `"2h"`  – every 2 hours
/// - anything else is stored as a raw cron expression
pub fn parse_schedule(s: &str) -> anyhow::Result<CronSchedule> {
    let s = s.trim();

    if let Some(rest) = s.strip_suffix('m') {
        let mins: u64 = rest
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid minute interval: {s}"))?;
        if mins == 0 {
            anyhow::bail!("interval must be > 0");
        }
        return Ok(CronSchedule::IntervalMinutes(mins));
    }

    if let Some(rest) = s.strip_suffix('h') {
        let hours: u64 = rest
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid hour interval: {s}"))?;
        if hours == 0 {
            anyhow::bail!("interval must be > 0");
        }
        return Ok(CronSchedule::IntervalHours(hours));
    }

    // Treat as a raw cron expression.
    Ok(CronSchedule::CronExpr(s.to_string()))
}

// ---------------------------------------------------------------------------
// CronJob
// ---------------------------------------------------------------------------

/// A single scheduled job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Parsed schedule.
    pub schedule: CronSchedule,
    /// The prompt / payload to execute.
    pub prompt: String,
    /// Whether the job is enabled.
    pub enabled: bool,
    /// Last execution timestamp, if any.
    pub last_run: Option<DateTime<Utc>>,
    /// Next scheduled execution, if any.
    pub next_run: Option<DateTime<Utc>>,
}

impl CronJob {
    /// Create a new job with a parsed schedule and computed `next_run`.
    pub fn new(name: impl Into<String>, schedule: CronSchedule, prompt: impl Into<String>) -> Self {
        let now = Utc::now();
        let next = schedule.next_after(now);
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            schedule,
            prompt: prompt.into(),
            enabled: true,
            last_run: None,
            next_run: Some(next),
        }
    }
}

// ---------------------------------------------------------------------------
// CronScheduler
// ---------------------------------------------------------------------------

/// In-memory scheduler that tracks [`CronJob`]s.
#[derive(Debug, Default)]
pub struct CronScheduler {
    jobs: HashMap<String, CronJob>,
}

impl CronScheduler {
    /// Create an empty scheduler.
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }

    /// Add a job and return its id.
    pub fn add(&mut self, job: CronJob) -> String {
        let id = job.id.clone();
        self.jobs.insert(id.clone(), job);
        id
    }

    /// Remove a job by id. Returns `true` if it existed.
    pub fn remove(&mut self, id: &str) -> bool {
        self.jobs.remove(id).is_some()
    }

    /// List all registered jobs.
    pub fn list(&self) -> Vec<&CronJob> {
        self.jobs.values().collect()
    }

    /// Return the ids of all enabled jobs whose `next_run` is at or before `now`.
    pub fn next_tick(&self, now: DateTime<Utc>) -> Vec<String> {
        self.jobs
            .values()
            .filter(|j| j.enabled)
            .filter_map(|j| j.next_run.filter(|nr| *nr <= now).map(|_| j.id.clone()))
            .collect()
    }

    /// Mark a job as executed: update `last_run` and recompute `next_run`.
    pub fn mark_executed(&mut self, id: &str) {
        if let Some(job) = self.jobs.get_mut(id) {
            let now = Utc::now();
            job.last_run = Some(now);
            job.next_run = Some(job.schedule.next_after(now));
        }
    }

    /// Get a mutable reference to a job by id.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut CronJob> {
        self.jobs.get_mut(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_schedule_minutes() {
        let s = parse_schedule("30m").unwrap();
        match s {
            CronSchedule::IntervalMinutes(30) => {}
            _ => panic!("expected IntervalMinutes(30)"),
        }
    }

    #[test]
    fn test_parse_schedule_hours() {
        let s = parse_schedule("2h").unwrap();
        match s {
            CronSchedule::IntervalHours(2) => {}
            _ => panic!("expected IntervalHours(2)"),
        }
    }

    #[test]
    fn test_parse_schedule_cron() {
        let s = parse_schedule("*/5 * * * *").unwrap();
        match s {
            CronSchedule::CronExpr(ref e) => assert_eq!(e, "*/5 * * * *"),
            _ => panic!("expected CronExpr"),
        }
    }

    #[test]
    fn test_scheduler_add_remove_list() {
        let mut sched = CronScheduler::new();
        let job = CronJob::new("test", CronSchedule::IntervalMinutes(10), "hello");
        let id = sched.add(job);
        assert_eq!(sched.list().len(), 1);
        assert!(sched.remove(&id));
        assert_eq!(sched.list().len(), 0);
    }

    #[test]
    fn test_parse_schedule_trims_whitespace() {
        let s = parse_schedule("  45m  ").unwrap();
        match s {
            CronSchedule::IntervalMinutes(45) => {}
            _ => panic!("expected IntervalMinutes(45)"),
        }
    }

    #[test]
    fn test_parse_schedule_zero_minutes_fails() {
        assert!(parse_schedule("0m").is_err());
    }

    #[test]
    fn test_parse_schedule_zero_hours_fails() {
        assert!(parse_schedule("0h").is_err());
    }

    #[test]
    fn test_parse_schedule_invalid_number_fails() {
        assert!(parse_schedule("abc").is_ok()); // treated as cron expr
        assert!(parse_schedule("xm").is_err());
        assert!(parse_schedule("xh").is_err());
    }

    #[test]
    fn test_cron_expr_next_after_adds_one_hour() {
        let schedule = CronSchedule::CronExpr("0 * * * *".to_string());
        let base = Utc::now();
        let next = schedule.next_after(base);
        let diff = next - base;
        assert_eq!(diff, chrono::Duration::hours(1));
    }

    #[test]
    fn test_cron_job_new_sets_fields() {
        let job = CronJob::new("my-job", CronSchedule::IntervalMinutes(5), "do something");
        assert_eq!(job.name, "my-job");
        assert_eq!(job.prompt, "do something");
        assert!(job.enabled);
        assert!(job.last_run.is_none());
        assert!(job.next_run.is_some());
        // next_run should be ~5 minutes from now (within a small tolerance)
        let diff = job.next_run.unwrap() - Utc::now();
        assert!(diff >= chrono::Duration::minutes(4));
    }

    #[test]
    fn test_scheduler_next_tick() {
        let mut sched = CronScheduler::new();

        // Create a job whose next_run is in the past
        let mut job = CronJob::new("past", CronSchedule::IntervalMinutes(1), "run");
        job.next_run = Some(Utc::now() - chrono::Duration::minutes(1));
        sched.add(job);

        // Create a job whose next_run is in the future
        let mut job2 = CronJob::new("future", CronSchedule::IntervalMinutes(60), "later");
        job2.next_run = Some(Utc::now() + chrono::Duration::hours(1));
        sched.add(job2);

        let due = sched.next_tick(Utc::now());
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn test_scheduler_remove_nonexistent_returns_false() {
        let mut sched = CronScheduler::new();
        assert!(!sched.remove("nonexistent-id"));
    }

    #[test]
    fn test_mark_executed_updates_timestamps() {
        let mut sched = CronScheduler::new();
        let job = CronJob::new("exec", CronSchedule::IntervalMinutes(10), "run");
        let id = sched.add(job);

        sched.mark_executed(&id);

        let binding = sched.list();
        let job = binding.iter().find(|j| j.id == id).unwrap();
        assert!(job.last_run.is_some());
        assert!(job.next_run.is_some());
        // next_run should be ~10 minutes after last_run
        let diff = job.next_run.unwrap() - job.last_run.unwrap();
        assert_eq!(diff, chrono::Duration::minutes(10));
    }

    #[test]
    fn test_get_mut_returns_none_for_missing() {
        let mut sched = CronScheduler::new();
        assert!(sched.get_mut("missing").is_none());
    }

    #[test]
    fn test_disabled_job_not_in_next_tick() {
        let mut sched = CronScheduler::new();
        let mut job = CronJob::new("disabled", CronSchedule::IntervalMinutes(1), "skip");
        job.enabled = false;
        job.next_run = Some(Utc::now() - chrono::Duration::minutes(5));
        sched.add(job);

        let due = sched.next_tick(Utc::now());
        assert!(due.is_empty());
    }
}
