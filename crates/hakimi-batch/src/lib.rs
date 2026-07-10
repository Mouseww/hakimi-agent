//! Batch runner for parallel prompt processing.
//!
//! Loads datasets, processes prompts in parallel, with checkpointing
//! and trajectory saving for evaluation/benchmarking workflows.

pub mod progress;
pub mod progress_notifier;
pub mod progress_store;

pub use progress::{JobProgress, StageProgress, StageStatus};
pub use progress_notifier::{ProgressNotifier, ProgressUpdate};
pub use progress_store::ProgressStore;

use anyhow::Result;
use chrono::Utc;
use futures::StreamExt;
use hakimi_core::AIAgentBuilder;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// A single item in a batch dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItem {
    /// Unique identifier for this item.
    pub id: String,
    /// The prompt to process.
    pub prompt: String,
    /// Optional expected output for evaluation.
    pub expected: Option<String>,
    /// Optional metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Result of processing a single batch item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    /// The input item ID.
    pub item_id: String,
    /// The generated response.
    pub response: String,
    /// Whether the processing succeeded.
    pub success: bool,
    /// Error message if processing failed.
    pub error: Option<String>,
    /// Processing duration in milliseconds.
    pub duration_ms: u64,
    /// Token usage (prompt, completion).
    pub tokens_used: Option<(u32, u32)>,
    /// Tool calls made during processing.
    pub tool_calls: Vec<ToolCallRecord>,
}

/// Record of a tool call made during batch processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub arguments: String,
    pub result_preview: String,
}

/// Checkpoint for fault-tolerant batch processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCheckpoint {
    /// Index of the next item to process.
    pub next_index: usize,
    /// Total items in the dataset.
    pub total_items: usize,
    /// Results completed so far.
    pub completed: usize,
    /// Timestamp of the checkpoint.
    pub timestamp: String,
}

/// Configuration for batch processing.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum number of concurrent tasks.
    pub concurrency: usize,
    /// Whether to save checkpoints.
    pub checkpoint_enabled: bool,
    /// Checkpoint interval (save every N items).
    pub checkpoint_interval: usize,
    /// Output directory for results and checkpoints.
    pub output_dir: PathBuf,
    /// Whether to save full trajectories.
    pub save_trajectories: bool,
    /// Batch metadata.
    pub metadata: Option<serde_json::Value>,
    /// Whether to enable progress tracking.
    pub progress_tracking_enabled: bool,
    /// Progress database path (None for in-memory).
    pub progress_db_path: Option<PathBuf>,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            checkpoint_enabled: true,
            checkpoint_interval: 10,
            output_dir: PathBuf::from("./batch-output"),
            save_trajectories: false,
            metadata: None,
            progress_tracking_enabled: true,
            progress_db_path: None, // Use in-memory by default
        }
    }
}

/// Load a dataset from a JSONL file.
pub fn load_dataset(path: &Path) -> Result<Vec<BatchItem>> {
    let content = std::fs::read_to_string(path)?;
    let mut items = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match serde_json::from_str::<BatchItem>(trimmed) {
            Ok(item) => items.push(item),
            Err(e) => {
                warn!(line = line_num + 1, error = %e, "Skipping malformed dataset line");
            }
        }
    }

    info!(count = items.len(), path = %path.display(), "Loaded dataset");
    Ok(items)
}

/// Save results to a JSONL file.
pub fn save_results(path: &Path, results: &[BatchResult]) -> Result<()> {
    let mut output = String::new();
    for result in results {
        output.push_str(&serde_json::to_string(result)?);
        output.push('\n');
    }
    std::fs::write(path, output)?;
    info!(count = results.len(), path = %path.display(), "Saved batch results");
    Ok(())
}

/// Save a checkpoint.
pub fn save_checkpoint(path: &Path, checkpoint: &BatchCheckpoint) -> Result<()> {
    let json = serde_json::to_string_pretty(checkpoint)?;
    std::fs::write(path, json)?;
    debug!(next_index = checkpoint.next_index, "Saved checkpoint");
    Ok(())
}

/// Load a checkpoint if it exists.
pub fn load_checkpoint(path: &Path) -> Option<BatchCheckpoint> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Batch processor.
pub struct BatchProcessor {
    builder: AIAgentBuilder,
    config: BatchConfig,
    progress_store: Option<ProgressStore>,
    progress_notifier: Option<ProgressNotifier>,
}

impl BatchProcessor {
    pub fn new(builder: AIAgentBuilder, config: BatchConfig) -> Self {
        let progress_store = if config.progress_tracking_enabled {
            let db_path = config
                .progress_db_path
                .as_ref()
                .and_then(|p| p.to_str())
                .unwrap_or(":memory:");
            ProgressStore::new(db_path).ok()
        } else {
            None
        };

        let progress_notifier = if config.progress_tracking_enabled {
            Some(ProgressNotifier::new())
        } else {
            None
        };

        Self {
            builder,
            config,
            progress_store,
            progress_notifier,
        }
    }

    /// Get a reference to the progress notifier.
    pub fn progress_notifier(&self) -> Option<&ProgressNotifier> {
        self.progress_notifier.as_ref()
    }

    /// Get a reference to the progress store.
    pub fn progress_store(&self) -> Option<&ProgressStore> {
        self.progress_store.as_ref()
    }

    /// Save and notify progress for a job.
    fn save_and_notify_progress(&self, job_id: &str, progress: &JobProgress) -> Result<()> {
        if let Some(store) = &self.progress_store {
            store.save_progress(job_id, progress)?;
        }

        if let Some(notifier) = &self.progress_notifier {
            notifier.notify(job_id, progress)?;
        }

        Ok(())
    }

    /// Process a dataset.
    pub async fn run(&self, items: Vec<BatchItem>) -> Result<Vec<BatchResult>> {
        if !self.config.output_dir.exists() {
            std::fs::create_dir_all(&self.config.output_dir)?;
        }

        let checkpoint_path = self.config.output_dir.join("checkpoint.json");
        let results_path = self.config.output_dir.join("results.jsonl");

        let mut start_index = 0;
        let mut results = Vec::new();

        if self.config.checkpoint_enabled
            && let Some(checkpoint) = load_checkpoint(&checkpoint_path)
        {
            info!(index = checkpoint.next_index, "Resuming from checkpoint");
            start_index = checkpoint.next_index;
            // Load existing results if resuming
            if results_path.exists() {
                let _items = load_dataset(&results_path)?;
                // This is a bit hacky since load_dataset returns BatchItem,
                // but results.jsonl contains BatchResult.
                // Let's just re-read manually.
                let content = std::fs::read_to_string(&results_path)?;
                for line in content.lines() {
                    if let Ok(res) = serde_json::from_str::<BatchResult>(line) {
                        results.push(res);
                    }
                }
            }
        }

        let total = items.len();

        // Initialize progress tracking
        let job_id = format!("batch-{}", uuid::Uuid::new_v4());
        if self.progress_store.is_some() || self.progress_notifier.is_some() {
            let mut progress = JobProgress::new(
                total,
                vec![
                    "initialization".to_string(),
                    "processing".to_string(),
                    "finalization".to_string(),
                ],
            );
            progress.items_total = total;
            progress.items_processed = start_index;
            progress.start_stage("initialization");
            progress.complete_stage("initialization");
            progress.start_stage("processing");
            self.save_and_notify_progress(&job_id, &progress)?;
        }

        let stream = futures::stream::iter(items.into_iter().enumerate().skip(start_index))
            .map(|(_idx, item)| {
                let builder = self.builder.clone();
                async move {
                    let start_time = std::time::Instant::now();
                    let mut agent = match builder.build() {
                        Ok(a) => a,
                        Err(e) => {
                            return BatchResult {
                                item_id: item.id,
                                response: "".into(),
                                success: false,
                                error: Some(format!("Build error: {e}")),
                                duration_ms: 0,
                                tokens_used: None,
                                tool_calls: vec![],
                            };
                        }
                    };

                    match agent.run_conversation(&item.prompt).await {
                        Ok(res) => BatchResult {
                            item_id: item.id,
                            response: res.final_response,
                            success: true,
                            error: None,
                            duration_ms: start_time.elapsed().as_millis() as u64,
                            tokens_used: Some((
                                res.usage.prompt_tokens,
                                res.usage.completion_tokens,
                            )),
                            tool_calls: res
                                .messages
                                .iter()
                                .filter_map(|m| {
                                    use hakimi_common::MessageRole;
                                    if m.role == MessageRole::Assistant {
                                        m.tool_calls.as_ref().map(|tcs| {
                                            tcs.iter()
                                                .map(|tc| ToolCallRecord {
                                                    tool_name: tc.name.clone(),
                                                    arguments: tc.arguments.clone(),
                                                    result_preview: "".into(),
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                    } else {
                                        None
                                    }
                                })
                                .flatten()
                                .collect(),
                        },
                        Err(e) => BatchResult {
                            item_id: item.id,
                            response: "".into(),
                            success: false,
                            error: Some(format!("Execution error: {e}")),
                            duration_ms: start_time.elapsed().as_millis() as u64,
                            tokens_used: None,
                            tool_calls: vec![],
                        },
                    }
                }
            })
            .buffer_unordered(self.config.concurrency);

        let mut current_results = Vec::new();
        let mut stream = std::pin::pin!(stream);
        let mut processed_count = start_index;

        // Collect results and update progress
        while let Some(result) = futures::StreamExt::next(&mut stream).await {
            current_results.push(result);
            processed_count += 1;

            // Update progress periodically
            if self.progress_store.is_some() || self.progress_notifier.is_some() {
                if let Some(store) = &self.progress_store {
                    if let Ok(Some(mut progress)) = store.get_progress(&job_id) {
                        progress.items_processed = processed_count;
                        progress.update_stage_items("processing", processed_count, total);
                        let _ = self.save_and_notify_progress(&job_id, &progress);
                    }
                } else if self.progress_notifier.is_some() {
                    // Fallback if progress not in store yet
                    let mut progress = JobProgress::new(
                        total,
                        vec![
                            "initialization".to_string(),
                            "processing".to_string(),
                            "finalization".to_string(),
                        ],
                    );
                    progress.items_total = total;
                    progress.items_processed = processed_count;
                    progress.start_stage("processing");
                    progress.update_stage_items("processing", processed_count, total);
                    let _ = self.save_and_notify_progress(&job_id, &progress);
                }
            }
        }

        // Sort results to match input order if possible, though buffer_unordered loses it.
        // For simplicity, we'll just append.
        results.append(&mut current_results);

        // Complete processing stage and start finalization
        if self.progress_store.is_some() || self.progress_notifier.is_some() {
            if let Ok(Some(mut progress)) = self
                .progress_store
                .as_ref()
                .map(|store| store.get_progress(&job_id))
                .transpose()
                .and_then(|opt| opt.ok_or(anyhow::anyhow!("No progress found")))
            {
                progress.complete_stage("processing");
                progress.start_stage("finalization");
                let _ = self.save_and_notify_progress(&job_id, &progress);
            }
        }

        if self.config.checkpoint_enabled {
            let checkpoint = BatchCheckpoint {
                next_index: total,
                total_items: total,
                completed: total,
                timestamp: Utc::now().to_rfc3339(),
            };
            save_checkpoint(&checkpoint_path, &checkpoint)?;
        }

        save_results(&results_path, &results)?;

        // Complete finalization stage
        if self.progress_store.is_some() || self.progress_notifier.is_some() {
            if let Ok(Some(mut progress)) = self
                .progress_store
                .as_ref()
                .map(|store| store.get_progress(&job_id))
                .transpose()
                .and_then(|opt| opt.ok_or(anyhow::anyhow!("No progress found")))
            {
                progress.complete_stage("finalization");
                progress.percentage = 100.0;
                let _ = self.save_and_notify_progress(&job_id, &progress);
            }
        }

        Ok(results)
    }
}

/// Statistics for a completed batch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchStats {
    pub total_items: usize,
    pub successful: usize,
    pub failed: usize,
    pub total_duration_ms: u64,
    pub total_tokens_prompt: u64,
    pub total_tokens_completion: u64,
    pub total_tool_calls: usize,
}

impl BatchStats {
    /// Compute stats from results.
    pub fn from_results(results: &[BatchResult]) -> Self {
        let successful = results.iter().filter(|r| r.success).count();
        let failed = results.len() - successful;
        let total_duration_ms = results.iter().map(|r| r.duration_ms).sum();
        let (total_prompt, total_completion) = results.iter().fold((0u64, 0u64), |(p, c), r| {
            if let Some((prompt, comp)) = r.tokens_used {
                (p + prompt as u64, c + comp as u64)
            } else {
                (p, c)
            }
        });
        let total_tool_calls = results.iter().map(|r| r.tool_calls.len()).sum();

        Self {
            total_items: results.len(),
            successful,
            failed,
            total_duration_ms,
            total_tokens_prompt: total_prompt,
            total_tokens_completion: total_completion,
            total_tool_calls,
        }
    }

    /// Compute detailed performance metrics.
    pub fn performance_summary(&self) -> serde_json::Value {
        let avg_duration = if self.total_items > 0 {
            self.total_duration_ms as f64 / self.total_items as f64
        } else {
            0.0
        };
        serde_json::json!({
            "total": self.total_items,
            "success_rate": if self.total_items > 0 { self.successful as f64 / self.total_items as f64 } else { 0.0 },
            "avg_duration_ms": avg_duration,
            "tokens_per_item": if self.total_items > 0 { (self.total_tokens_prompt + self.total_tokens_completion) as f64 / self.total_items as f64 } else { 0.0 }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.concurrency, 4);
        assert!(config.checkpoint_enabled);
        assert_eq!(config.checkpoint_interval, 10);
    }

    #[test]
    fn test_batch_item_serialization() {
        let item = BatchItem {
            id: "test-1".to_string(),
            prompt: "Hello".to_string(),
            expected: Some("Hi".to_string()),
            metadata: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        let back: BatchItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "test-1");
    }

    #[test]
    fn test_batch_result_serialization() {
        let result = BatchResult {
            item_id: "test-1".to_string(),
            response: "Hello!".to_string(),
            success: true,
            error: None,
            duration_ms: 1500,
            tokens_used: Some((100, 50)),
            tool_calls: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-1"));
    }

    #[test]
    fn test_batch_stats_from_results() {
        let results = vec![
            BatchResult {
                item_id: "1".to_string(),
                response: "ok".to_string(),
                success: true,
                error: None,
                duration_ms: 1000,
                tokens_used: Some((50, 30)),
                tool_calls: vec![],
            },
            BatchResult {
                item_id: "2".to_string(),
                response: "".to_string(),
                success: false,
                error: Some("timeout".to_string()),
                duration_ms: 5000,
                tokens_used: None,
                tool_calls: vec![],
            },
        ];
        let stats = BatchStats::from_results(&results);
        assert_eq!(stats.total_items, 2);
        assert_eq!(stats.successful, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.total_duration_ms, 6000);
        assert_eq!(stats.total_tokens_prompt, 50);
    }
}
