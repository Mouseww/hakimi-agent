//! Progress tracking for batch jobs.

use serde::{Deserialize, Serialize};

/// Progress information for a batch job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    /// Current step in the job (0-based).
    pub current_step: usize,
    /// Total number of steps.
    pub total_steps: usize,
    /// Current stage name.
    pub current_stage: String,
    /// Overall completion percentage (0.0 - 100.0).
    pub percentage: f64,
    /// Number of items processed so far.
    pub items_processed: usize,
    /// Total number of items.
    pub items_total: usize,
    /// Progress for individual stages.
    pub stages: Vec<StageProgress>,
}

/// Progress information for a single stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageProgress {
    /// Stage name.
    pub name: String,
    /// Stage status.
    pub status: StageStatus,
    /// Start time (Unix timestamp).
    pub started_at: Option<i64>,
    /// Completion time (Unix timestamp).
    pub completed_at: Option<i64>,
    /// Items processed in this stage.
    pub items_processed: usize,
    /// Total items in this stage.
    pub items_total: usize,
}

/// Status of a stage.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StageStatus {
    /// Stage has not started yet.
    Pending,
    /// Stage is currently running.
    Running,
    /// Stage completed successfully.
    Completed,
    /// Stage failed.
    Failed,
}

impl JobProgress {
    /// Create a new progress tracker with the given stages.
    pub fn new(total_steps: usize, stages: Vec<String>) -> Self {
        let stage_progresses = stages
            .iter()
            .map(|name| StageProgress {
                name: name.clone(),
                status: StageStatus::Pending,
                started_at: None,
                completed_at: None,
                items_processed: 0,
                items_total: 0,
            })
            .collect();

        Self {
            current_step: 0,
            total_steps,
            current_stage: stages.first().cloned().unwrap_or_default(),
            percentage: 0.0,
            items_processed: 0,
            items_total: 0,
            stages: stage_progresses,
        }
    }

    /// Update the current step and recalculate percentage.
    pub fn update_step(&mut self, step: usize) {
        self.current_step = step;
        self.percentage = if self.total_steps > 0 {
            (step as f64 / self.total_steps as f64) * 100.0
        } else {
            0.0
        };
    }

    /// Start a stage by name.
    pub fn start_stage(&mut self, stage_name: &str) {
        self.current_stage = stage_name.to_string();

        if let Some(stage) = self.stages.iter_mut().find(|s| s.name == stage_name) {
            stage.status = StageStatus::Running;
            stage.started_at = Some(chrono::Utc::now().timestamp());
        }
    }

    /// Complete a stage by name.
    pub fn complete_stage(&mut self, stage_name: &str) {
        if let Some(stage) = self.stages.iter_mut().find(|s| s.name == stage_name) {
            stage.status = StageStatus::Completed;
            stage.completed_at = Some(chrono::Utc::now().timestamp());
        }
    }

    /// Mark a stage as failed.
    pub fn fail_stage(&mut self, stage_name: &str) {
        if let Some(stage) = self.stages.iter_mut().find(|s| s.name == stage_name) {
            stage.status = StageStatus::Failed;
            stage.completed_at = Some(chrono::Utc::now().timestamp());
        }
    }

    /// Update the item count for a specific stage.
    pub fn update_stage_items(&mut self, stage_name: &str, processed: usize, total: usize) {
        if let Some(stage) = self.stages.iter_mut().find(|s| s.name == stage_name) {
            stage.items_processed = processed;
            stage.items_total = total;
        }

        self.items_processed = processed;
        self.items_total = total;

        // Recalculate percentage based on items
        self.percentage = if total > 0 {
            (processed as f64 / total as f64) * 100.0
        } else {
            0.0
        };
    }

    /// Increment the number of processed items.
    pub fn increment_processed(&mut self) {
        self.items_processed += 1;
        if let Some(stage) = self
            .stages
            .iter_mut()
            .find(|s| s.name == self.current_stage)
        {
            stage.items_processed += 1;
        }

        // Recalculate percentage
        self.percentage = if self.items_total > 0 {
            (self.items_processed as f64 / self.items_total as f64) * 100.0
        } else {
            0.0
        };
    }

    /// Get the current stage progress.
    pub fn current_stage_progress(&self) -> Option<&StageProgress> {
        self.stages.iter().find(|s| s.name == self.current_stage)
    }

    /// Check if all stages are completed.
    pub fn is_complete(&self) -> bool {
        self.stages
            .iter()
            .all(|s| s.status == StageStatus::Completed)
    }

    /// Check if any stage has failed.
    pub fn has_failed(&self) -> bool {
        self.stages.iter().any(|s| s.status == StageStatus::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_initialization() {
        let stages = vec![
            "load".to_string(),
            "process".to_string(),
            "save".to_string(),
        ];
        let progress = JobProgress::new(100, stages);

        assert_eq!(progress.total_steps, 100);
        assert_eq!(progress.current_step, 0);
        assert_eq!(progress.percentage, 0.0);
        assert_eq!(progress.stages.len(), 3);
        assert_eq!(progress.current_stage, "load");
        assert_eq!(progress.stages[0].status, StageStatus::Pending);
    }

    #[test]
    fn test_stage_progression() {
        let stages = vec!["load".to_string(), "process".to_string()];
        let mut progress = JobProgress::new(100, stages);

        progress.start_stage("load");
        assert_eq!(progress.current_stage, "load");
        assert_eq!(progress.stages[0].status, StageStatus::Running);
        assert!(progress.stages[0].started_at.is_some());

        progress.complete_stage("load");
        assert_eq!(progress.stages[0].status, StageStatus::Completed);
        assert!(progress.stages[0].completed_at.is_some());
    }

    #[test]
    fn test_percentage_calculation() {
        let stages = vec!["process".to_string()];
        let mut progress = JobProgress::new(100, stages);

        progress.update_step(25);
        assert_eq!(progress.percentage, 25.0);

        progress.update_step(50);
        assert_eq!(progress.percentage, 50.0);

        progress.update_step(100);
        assert_eq!(progress.percentage, 100.0);
    }

    #[test]
    fn test_item_progress() {
        let stages = vec!["process".to_string()];
        let mut progress = JobProgress::new(100, stages);
        progress.start_stage("process");

        progress.update_stage_items("process", 10, 100);
        assert_eq!(progress.items_processed, 10);
        assert_eq!(progress.items_total, 100);
        assert_eq!(progress.percentage, 10.0);

        progress.update_stage_items("process", 50, 100);
        assert_eq!(progress.percentage, 50.0);
    }

    #[test]
    fn test_increment_processed() {
        let stages = vec!["process".to_string()];
        let mut progress = JobProgress::new(10, stages);
        progress.items_total = 10;
        progress.start_stage("process");

        for i in 1..=10 {
            progress.increment_processed();
            assert_eq!(progress.items_processed, i);
        }

        assert_eq!(progress.percentage, 100.0);
    }

    #[test]
    fn test_stage_failure() {
        let stages = vec!["load".to_string()];
        let mut progress = JobProgress::new(10, stages);

        progress.start_stage("load");
        progress.fail_stage("load");

        assert_eq!(progress.stages[0].status, StageStatus::Failed);
        assert!(progress.has_failed());
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_is_complete() {
        let stages = vec!["load".to_string(), "process".to_string()];
        let mut progress = JobProgress::new(10, stages);

        assert!(!progress.is_complete());

        progress.start_stage("load");
        progress.complete_stage("load");
        assert!(!progress.is_complete());

        progress.start_stage("process");
        progress.complete_stage("process");
        assert!(progress.is_complete());
    }

    #[test]
    fn test_current_stage_progress() {
        let stages = vec!["load".to_string(), "process".to_string()];
        let mut progress = JobProgress::new(10, stages);

        progress.start_stage("load");
        let current = progress.current_stage_progress();
        assert!(current.is_some());
        assert_eq!(current.unwrap().name, "load");
        assert_eq!(current.unwrap().status, StageStatus::Running);
    }

    #[test]
    fn test_empty_stages() {
        let progress = JobProgress::new(0, vec![]);
        assert_eq!(progress.stages.len(), 0);
        assert_eq!(progress.current_stage, "");
        assert_eq!(progress.percentage, 0.0);
    }
}
