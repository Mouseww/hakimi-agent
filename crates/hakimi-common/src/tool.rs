use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// A tool call requested by the assistant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,

    /// Name of the tool/function to invoke.
    pub name: String,

    /// JSON-encoded arguments string.
    pub arguments: String,

    /// Index of this tool call in a batch (provider-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

/// The result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// ID of the tool call this result corresponds to.
    pub tool_call_id: String,

    /// Name of the tool that was executed.
    pub name: String,

    /// Text content of the result.
    pub content: String,

    /// Whether the tool execution resulted in an error.
    #[serde(default)]
    pub is_error: bool,
}

/// Definition of a tool that can be called by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Name of the tool.
    pub name: String,

    /// Human-readable description of what the tool does.
    pub description: String,

    /// JSON Schema describing the tool's parameters.
    pub parameters: JsonValue,

    /// Toolset/category that owns this tool.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub toolset: String,
}

/// Runtime mode for progressive tool disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSearchMode {
    /// Activate only when deferred tool schemas exceed the configured threshold.
    Auto,
    /// Always activate when any deferrable tool exists.
    On,
    /// Never activate.
    Off,
}

fn default_tool_search_mode() -> ToolSearchMode {
    ToolSearchMode::Auto
}

fn default_tool_search_threshold_pct() -> f64 {
    10.0
}

fn default_tool_search_default_limit() -> usize {
    5
}

fn default_tool_search_max_limit() -> usize {
    20
}

pub const DEFAULT_TOOL_OUTPUT_MAX_BYTES: usize = 50_000;
const MAX_TOOL_OUTPUT_MAX_BYTES: usize = 10 * 1024 * 1024;

/// Configuration for framework-level tool-result size limits.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolOutputConfig {
    /// Default maximum size for a tool result when the tool does not provide
    /// its own per-tool limit.
    pub max_bytes: usize,
}

impl<'de> Deserialize<'de> for ToolOutputConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = JsonValue::deserialize(deserializer)?;
        Ok(Self::from_json_value(&raw).normalized())
    }
}

impl Default for ToolOutputConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_TOOL_OUTPUT_MAX_BYTES,
        }
    }
}

impl ToolOutputConfig {
    fn from_json_value(raw: &JsonValue) -> Self {
        match raw {
            JsonValue::Object(map) => Self {
                max_bytes: parse_usize(map.get("max_bytes"), DEFAULT_TOOL_OUTPUT_MAX_BYTES).max(1),
            },
            _ => Self::default(),
        }
    }

    /// Return a copy with numeric fields clamped to safe runtime bounds.
    pub fn normalized(&self) -> Self {
        Self {
            max_bytes: self.max_bytes.clamp(1, MAX_TOOL_OUTPUT_MAX_BYTES),
        }
    }
}

/// Configuration for Hermes-style progressive tool disclosure.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolSearchConfig {
    /// Whether tool search is enabled.
    pub enabled: ToolSearchMode,

    /// Percentage of the context window that deferred tool schemas may occupy
    /// before `Auto` mode activates.
    pub threshold_pct: f64,

    /// Default number of search hits returned by `tool_search`.
    pub search_default_limit: usize,

    /// Maximum number of search hits a model can request.
    pub max_search_limit: usize,
}

impl<'de> Deserialize<'de> for ToolSearchConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = JsonValue::deserialize(deserializer)?;
        Ok(Self::from_json_value(&raw).normalized())
    }
}

impl Default for ToolSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_tool_search_mode(),
            threshold_pct: default_tool_search_threshold_pct(),
            search_default_limit: default_tool_search_default_limit(),
            max_search_limit: default_tool_search_max_limit(),
        }
    }
}

impl ToolSearchConfig {
    fn from_json_value(raw: &JsonValue) -> Self {
        match raw {
            JsonValue::Bool(true) => Self::default(),
            JsonValue::Bool(false) => Self {
                enabled: ToolSearchMode::Off,
                ..Self::default()
            },
            JsonValue::Object(map) => Self {
                enabled: parse_tool_search_mode(map.get("enabled"), ToolSearchMode::Auto),
                threshold_pct: parse_f64(
                    map.get("threshold_pct"),
                    default_tool_search_threshold_pct(),
                ),
                search_default_limit: parse_usize(
                    map.get("search_default_limit"),
                    default_tool_search_default_limit(),
                ),
                max_search_limit: parse_usize(
                    map.get("max_search_limit"),
                    default_tool_search_max_limit(),
                ),
            },
            _ => Self::default(),
        }
    }

    /// Return a copy with numeric fields clamped to safe runtime bounds.
    pub fn normalized(&self) -> Self {
        let max_search_limit = self.max_search_limit.clamp(1, 50);
        Self {
            enabled: self.enabled,
            threshold_pct: self.threshold_pct.clamp(0.0, 100.0),
            search_default_limit: self.search_default_limit.clamp(1, max_search_limit),
            max_search_limit,
        }
    }
}

fn parse_tool_search_mode(raw: Option<&JsonValue>, fallback: ToolSearchMode) -> ToolSearchMode {
    match raw {
        Some(JsonValue::Bool(true)) => ToolSearchMode::On,
        Some(JsonValue::Bool(false)) => ToolSearchMode::Off,
        Some(JsonValue::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
            "on" | "true" | "1" | "yes" => ToolSearchMode::On,
            "off" | "false" | "0" | "no" => ToolSearchMode::Off,
            "auto" => ToolSearchMode::Auto,
            _ => fallback,
        },
        _ => fallback,
    }
}

fn parse_f64(raw: Option<&JsonValue>, fallback: f64) -> f64 {
    raw.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
    })
    .unwrap_or(fallback)
}

fn parse_usize(raw: Option<&JsonValue>, fallback: usize) -> usize {
    raw.and_then(|value| {
        value
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
    })
    .unwrap_or(fallback)
}

/// Callback used by long-running tools to surface progress back to the parent
/// agent UI without waiting for the tool's final result.
pub type ToolProgressCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Trait for executing delegated sub-tasks via child agents.
///
/// Implementors hold the shared resources (transport, context engine, model,
/// tool registry) needed to spawn and run a child agent. The `delegate_task`
/// tool calls through this trait to perform actual delegation.
#[async_trait]
pub trait DelegateExecutor: Send + Sync {
    /// Spawn a child agent to accomplish `goal` with the given `context` and
    /// restricted to the listed `toolsets`. Returns the child agent's final
    /// text response.
    async fn execute_delegation(
        &self,
        goal: &str,
        context: &str,
        toolsets: &[String],
    ) -> crate::Result<String>;

    /// Spawn multiple child agents to accomplish a batch of tasks concurrently.
    /// Returns a list of the child agents' final text responses in the same order.
    async fn execute_batch_delegation(
        &self,
        tasks: Vec<(String, String, Vec<String>)>, // (goal, context, toolsets)
    ) -> crate::Result<Vec<String>>;

    /// Submit a task to the delegation queue.
    async fn enqueue_task(&self, goal: &str, priority: u32) -> crate::Result<String>;
}

/// Metadata describing a teammate persona that can be consulted via the `team` tool.
#[derive(Debug, Clone)]
pub struct TeammateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// A single consultation request handed to a [`TeamExecutor`].
///
/// `depth` and `lineage` are NOT carried here: they live on the executor instance
/// bound to the calling agent (each consult descends into a child executor).
pub struct TeamCallContext {
    /// Target teammate persona id.
    pub teammate_id: String,
    /// The sub-task / question for the teammate.
    pub task: String,
    /// Optional shared context and constraints.
    pub context: String,
    /// Progress callback (reuses the delegate bubble protocol).
    pub progress: Option<ToolProgressCallback>,
}

/// Executes a sub-task on a named teammate persona and returns its answer.
///
/// Implemented by `hakimi-core`'s `PersonaTeamExecutor`. Tools reach it through
/// [`ToolContext::team_executor`].
#[async_trait]
pub trait TeamExecutor: Send + Sync {
    /// List teammate personas this agent may consult (id, name, description).
    async fn roster(&self) -> Vec<TeammateInfo>;

    /// Consult a single teammate; returns its final structured answer.
    async fn consult(&self, call: TeamCallContext) -> crate::Result<String>;

    /// Consult several teammates concurrently; returns one answer per input
    /// (failures become `"Teammate <id> failed: ..."` strings, never aborting the batch).
    async fn consult_many(&self, calls: Vec<TeamCallContext>) -> crate::Result<Vec<String>>;
}

/// Trait for searching the knowledge base.
#[async_trait]
pub trait KnowledgeSearcher: Send + Sync {
    /// Search for knowledge entities or snippets.
    async fn search(&self, query: &str, limit: usize) -> crate::Result<JsonValue>;
}

/// Contextual information available during tool execution.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct ToolContext {
    /// ID of the current session.
    pub session_id: String,

    /// ID of the user who initiated the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// ID of the current task, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    /// Working directory for the tool execution.
    pub workdir: String,

    /// Model identifier for spawning child agents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Executor for delegating sub-tasks to child agents.
    /// Holds shared resources (transport, context engine, tool registry).
    #[serde(skip)]
    pub delegate_executor: Option<Arc<dyn DelegateExecutor>>,

    /// Optional team executor for delegating to named teammate personas
    /// (`team` tool). Set by the dispatch layer; `None` disables team collaboration.
    #[serde(skip)]
    pub team_executor: Option<Arc<dyn TeamExecutor>>,

    /// TTS Provider setting
    #[serde(skip)]
    pub tts_provider: Option<String>,

    /// TTS Model setting
    #[serde(skip)]
    pub tts_model: Option<String>,

    /// TTS base URL override.
    #[serde(skip)]
    pub tts_base_url: Option<String>,

    /// TTS API key override.
    #[serde(skip)]
    pub tts_api_key: Option<String>,

    /// TTS voice override.
    #[serde(skip)]
    pub tts_voice: Option<String>,

    /// Whether voice-mode TTS output should auto-start local playback.
    #[serde(skip)]
    pub tts_auto_play: bool,

    /// Transcription provider setting.
    #[serde(skip)]
    pub transcription_provider: Option<String>,

    /// Transcription model setting.
    #[serde(skip)]
    pub transcription_model: Option<String>,

    /// Transcription base URL override.
    #[serde(skip)]
    pub transcription_base_url: Option<String>,

    /// Transcription API key override.
    #[serde(skip)]
    pub transcription_api_key: Option<String>,

    /// Searcher for accessing the knowledge base.
    #[serde(skip)]
    pub knowledge_searcher: Option<Arc<dyn KnowledgeSearcher>>,

    /// Optional callback for long-running tools to stream progress/status back
    /// through the parent agent UI.
    #[serde(skip)]
    pub progress_callback: Option<ToolProgressCallback>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("session_id", &self.session_id)
            .field("user_id", &self.user_id)
            .field("task_id", &self.task_id)
            .field("workdir", &self.workdir)
            .field("model", &self.model)
            .field("delegate_executor", &self.delegate_executor.is_some())
            .field("team_executor", &self.team_executor.is_some())
            .field("knowledge_searcher", &self.knowledge_searcher.is_some())
            .field("progress_callback", &self.progress_callback.is_some())
            .field("tts_auto_play", &self.tts_auto_play)
            .finish()
    }
}
