//! Batch runner for parallel prompt processing.
//!
//! Loads datasets, processes prompts in parallel, with checkpointing
//! and trajectory saving for evaluation/benchmarking workflows.

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
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            checkpoint_enabled: true,
            checkpoint_interval: 10,
            output_dir: PathBuf::from("./batch-output"),
            save_trajectories: false,
        }
    }
}

/// Load a dataset from a JSONL file.
pub fn load_dataset(path: &Path) -> anyhow::Result<Vec<BatchItem>> {
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
pub fn save_results(path: &Path, results: &[BatchResult]) -> anyhow::Result<()> {
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
pub fn save_checkpoint(path: &Path, checkpoint: &BatchCheckpoint) -> anyhow::Result<()> {
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

    #[test]
    fn test_checkpoint_serialization() {
        let checkpoint = BatchCheckpoint {
            next_index: 10,
            total_items: 100,
            completed: 10,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&checkpoint).unwrap();
        let back: BatchCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.next_index, 10);
    }

    #[test]
    fn test_load_dataset_nonexistent() {
        let result = load_dataset(Path::new("/nonexistent/dataset.jsonl"));
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_call_record() {
        let record = ToolCallRecord {
            tool_name: "read_file".to_string(),
            arguments: r#"{"path":"/tmp"}"#.to_string(),
            result_preview: "contents...".to_string(),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("read_file"));
    }

    #[test]
    fn test_load_checkpoint_nonexistent() {
        let result = load_checkpoint(Path::new("/nonexistent/checkpoint.json"));
        assert!(result.is_none());
    }
}
