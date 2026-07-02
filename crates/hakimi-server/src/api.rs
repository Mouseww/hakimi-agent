//! REST API route definitions.
//!
//! Endpoints:
//! - `GET  /health`          — Health check
//! - `POST /chat`            — Send a message, get a response
//! - `GET  /sessions`        — List recent sessions
//! - `POST /sessions`        — Create an empty API-visible session
//! - `GET  /sessions/:id`    — Get session details
//! - `PATCH /sessions/:id`   — Update client-safe session metadata
//! - `DELETE /sessions/:id`  — Delete a session and its messages
//! - `GET  /sessions/search` — Search saved session messages
//! - `GET  /sessions/:id/messages` — Get sanitized session messages
//! - `POST /sessions/:id/fork` — Branch a session and carry messages forward
//! - `GET  /tools`           — List available tools
//! - `GET  /config`          — Get current config (sanitized)
//! - `POST /config`          — Update config
//! - `GET  /status`          — Dashboard runtime status
//! - `GET  /mcp/servers`     — Dashboard MCP server summaries
//! - `POST /mcp/servers`     — Add a runtime MCP server
//! - `DELETE /mcp/servers/:name` — Remove a runtime MCP server
//! - `GET  /credentials/pool`— Dashboard credential pool summaries
//! - `POST /credentials/pool`— Add a runtime credential-pool entry
//! - `DELETE /credentials/pool/:provider/:index` — Remove a runtime credential
//! - `GET  /webhooks`        — Dashboard webhook gateway summary
//! - `POST /webhooks`        — Update runtime webhook gateway config
//! - `GET  /kanban`          — Dashboard Kanban board snapshot
//! - `GET  /kanban/boards`   — Dashboard Kanban board inventory
//! - `GET  /kanban/tasks/:id` — Dashboard Kanban task detail
//! - `POST /kanban/tasks`    — Create a dashboard Kanban task
//! - `PATCH /kanban/tasks/:id` — Update a dashboard Kanban task status/assignee
//! - `POST /kanban/tasks/:id/comments` — Append a dashboard Kanban task comment
//! - `GET  /cron/jobs`       — Dashboard cron job inventory
//! - `GET  /v1/models`       — OpenAI-compatible model discovery
//! - `GET  /v1/capabilities` — Machine-readable API capability discovery
//! - `GET  /v1/skills`       — List loaded runtime skills without skill bodies
//! - `GET  /v1/toolsets`     — List registered toolsets and their tool schemas
//! - `POST /v1/chat/completions` — OpenAI-compatible chat/SSE snapshots
//! - `POST /v1/responses`    — OpenAI Responses-compatible chat/SSE snapshots
//! - `GET  /v1/responses/:id` — Retrieve a stored Responses API result
//! - `DELETE /v1/responses/:id` — Delete a stored Responses API result
//! - `POST /v1/runs`         — Submit an asynchronous text run
//! - `GET  /v1/runs/:id`     — Poll an asynchronous run status/result
//! - `GET  /v1/runs/:id/events` — Stream run lifecycle events as SSE
//! - `POST /v1/runs/:id/stop` — Cancel an asynchronous run

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::Infallible;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, patch, post},
};
use chrono::Utc;
use futures::{StreamExt, stream};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::server::AppState;
use hakimi_common::Message as CoreMessage;
use hakimi_cron::persistence::PersistentCronStore;
use hakimi_cron::{CronJob, CronRepeat, parse_schedule, validate_cron_prompt};
use hakimi_session::{MessageOps, SessionOps};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Request body for POST /chat.
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub session_id: Option<String>,
}

/// Request body for POST /v1/chat/completions.
#[derive(Debug, Deserialize)]
pub struct ChatCompletionsRequest {
    pub model: Option<String>,
    #[serde(default)]
    pub messages: Vec<ChatCompletionsMessage>,
    #[serde(default)]
    pub stream: Option<JsonValue>,
}

/// OpenAI-style chat message input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionsMessage {
    pub role: String,
    #[serde(default)]
    pub content: JsonValue,
}

/// Request body for POST /v1/responses.
#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    pub model: Option<String>,
    #[serde(default)]
    pub input: JsonValue,
    pub instructions: Option<String>,
    pub previous_response_id: Option<String>,
    #[serde(default)]
    pub stream: Option<JsonValue>,
}

/// Request body for POST /v1/runs.
#[derive(Debug, Deserialize)]
pub struct RunCreateRequest {
    pub model: Option<String>,
    #[serde(default)]
    pub input: Option<JsonValue>,
    pub instructions: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub messages: Vec<ChatCompletionsMessage>,
    #[serde(default)]
    pub stream: Option<JsonValue>,
}

// ---------------------------------------------------------------------------
// Workspace API types
// ---------------------------------------------------------------------------

/// Query parameters for GET /api/workspace/list and GET /api/workspace/read.
#[derive(Debug, Deserialize)]
pub struct WorkspacePathQuery {
    pub path: Option<String>,
}

/// Request body for POST /api/workspace/create.
#[derive(Debug, Deserialize)]
pub struct WorkspaceCreateRequest {
    pub path: String,
    #[serde(default)]
    pub is_dir: bool,
}

/// Request body for POST /api/workspace/rename.
#[derive(Debug, Deserialize)]
pub struct WorkspaceRenameRequest {
    pub old_path: String,
    pub new_path: String,
}

/// Request body for POST /api/workspace/delete.
#[derive(Debug, Deserialize)]
pub struct WorkspaceDeleteRequest {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
}

/// Single directory entry returned by workspace/list.
#[derive(Debug, Serialize)]
pub struct WorkspaceListEntry {
    pub name: String,
    pub entry_type: String, // "file" or "dir"
    pub size: u64,
    pub is_dir: bool,
    pub is_git_tracked: bool,
    pub git_status: Option<String>,
}

/// Response for GET /api/workspace/list.
#[derive(Debug, Serialize)]
pub struct WorkspaceListResponse {
    pub entries: Vec<WorkspaceListEntry>,
}

/// Response for GET /api/workspace/read.
#[derive(Debug, Serialize)]
pub struct WorkspaceReadResponse {
    pub content: String,
}

/// Generic success response.
#[derive(Debug, Serialize)]
pub struct WorkspaceSuccessResponse {
    pub success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CronJobsResponse {
    pub object: String,
    pub total: usize,
    pub jobs: Vec<CronJobInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CronJobInfo {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub schedule_type: String,
    pub prompt: String,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub created_at: Option<String>,
    pub deliver: Option<String>,
    pub repeat_times: Option<i64>,
    pub repeat_completed: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CronJobCreateRequest {
    pub name: Option<String>,
    pub schedule: String,
    pub prompt: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub enabled_toolsets: Vec<String>,
    #[serde(default)]
    pub context_from: Vec<String>,
    pub deliver: Option<String>,
    pub repeat: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct CronJobMutationResponse {
    pub object: String,
    pub success: bool,
    pub job: Option<CronJobInfo>,
}

/// Response body for POST /chat.
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub response: String,
    pub session_id: String,
}

/// Response body for GET /api/agents.
#[derive(Debug, Serialize)]
struct AgentsListResponse {
    agents: Vec<hakimi_core::PersonaConfig>,
    default: String,
}

/// Request body for PATCH /api/agents/{id}. Every field is optional; only
/// provided fields are applied. An empty-string `reasoning_effort` clears it.
#[derive(Debug, Default, Deserialize)]
struct AgentUpdateRequest {
    name: Option<String>,
    avatar: Option<String>,
    description: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    system_prompt: Option<String>,
    enabled_skills: Option<Vec<String>>,
    bindings: Option<Vec<String>>,
    is_default: Option<bool>,
    addressable: Option<bool>,
}

/// Response body for DELETE /api/agents/{id}.
#[derive(Debug, Serialize)]
struct AgentDeleteResponse {
    id: String,
    deleted: bool,
}

/// Response body for GET /api/bindings (`platform:bot_id` -> persona id).
#[derive(Debug, Serialize)]
struct BindingsResponse {
    bindings: std::collections::BTreeMap<String, String>,
    default: String,
}

/// One entry of GET /api/agents/{id}/skills.
#[derive(Debug, Serialize)]
struct AgentSkillInfo {
    name: String,
    description: String,
    tags: Vec<String>,
    enabled: bool,
}

/// Response body for GET /api/agents/{id}/skills.
#[derive(Debug, Serialize)]
struct AgentSkillsResponse {
    available: Vec<AgentSkillInfo>,
    enabled: Vec<String>,
}

/// Response body for GET /api/agents/{id}/memory.
#[derive(Debug, Serialize)]
struct AgentMemoryResponse {
    dir: String,
    files: Vec<String>,
    memory_md: Option<String>,
}

/// Response body for GET /health.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Response body for GET /v1/models.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

/// OpenAI-compatible model descriptor.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
    pub permission: Vec<serde_json::Value>,
    pub root: String,
    pub parent: Option<String>,
}

/// Describes a single tool in GET /tools.
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Response body for GET /v1/skills.
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillsResponse {
    pub object: String,
    pub total: usize,
    pub active: Vec<String>,
    pub data: Vec<SkillInfo>,
}

/// Public skill metadata. The markdown body is intentionally not exposed.
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub trigger: Option<String>,
    pub tags: Vec<String>,
    pub phases: Vec<String>,
    pub platforms: Vec<String>,
    pub provenance: String,
    pub active: bool,
}

/// Response body for GET /v1/toolsets.
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolsetsResponse {
    pub object: String,
    pub total_toolsets: usize,
    pub total_tools: usize,
    pub data: Vec<ToolsetInfo>,
}

/// Toolset inventory for external API clients.
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolsetInfo {
    pub name: String,
    pub source: String,
    pub deferrable: bool,
    pub tool_count: usize,
    pub tools: Vec<ToolsetToolInfo>,
}

/// Tool metadata grouped under a toolset.
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolsetToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Describes a session in GET /sessions and GET /sessions/:id.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub source: Option<String>,
    pub user_id: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub message_count: i32,
    pub tool_call_count: i32,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub title: Option<String>,
}

/// Query parameters for GET /sessions/search.
#[derive(Debug, Deserialize)]
pub struct SessionSearchQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

/// Response body for GET /sessions/search.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSearchResponse {
    pub object: String,
    pub query: String,
    pub count: usize,
    pub data: Vec<SessionSearchResultInfo>,
}

/// A dashboard-safe session search hit.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSearchResultInfo {
    pub session_id: String,
    pub message_id: i64,
    pub content: Option<String>,
    pub rank: f64,
    pub title: Option<String>,
    pub source: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<String>,
}

/// Query parameters for GET /sessions/:id/messages.
#[derive(Debug, Deserialize)]
pub struct SessionMessagesQuery {
    pub limit: Option<usize>,
}

/// Request body for POST /sessions.
#[derive(Debug, Deserialize)]
pub struct SessionCreateRequest {
    pub id: Option<String>,
    pub session_id: Option<String>,
    pub source: Option<String>,
    pub user_id: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub title: Option<String>,
}

/// Request body for POST /sessions/:id/fork.
#[derive(Debug, Deserialize)]
pub struct SessionForkRequest {
    pub id: Option<String>,
    pub session_id: Option<String>,
    pub title: Option<String>,
}

/// Response body for GET /sessions/:id/messages.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionMessagesResponse {
    pub object: String,
    pub session: SessionInfo,
    pub count: usize,
    pub messages: Vec<SessionMessageInfo>,
}

/// A sanitized message row for dashboard/session inspection.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionMessageInfo {
    pub role: String,
    pub content: Option<String>,
    pub timestamp: Option<String>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
    pub tool_call_count: usize,
    pub has_reasoning: bool,
    pub token_count: Option<u32>,
    pub finish_reason: Option<String>,
}

impl From<hakimi_session::SessionMeta> for SessionInfo {
    fn from(meta: hakimi_session::SessionMeta) -> Self {
        Self {
            id: meta.id,
            source: meta.source,
            user_id: meta.user_id,
            model: meta.model,
            started_at: meta.started_at,
            ended_at: meta.ended_at,
            message_count: meta.message_count,
            tool_call_count: meta.tool_call_count,
            input_tokens: meta.input_tokens,
            output_tokens: meta.output_tokens,
            title: meta.title,
        }
    }
}

impl From<hakimi_common::Message> for SessionMessageInfo {
    fn from(message: hakimi_common::Message) -> Self {
        let tool_call_count = message.tool_calls.as_ref().map_or(0, Vec::len);
        Self {
            role: message.role.to_string(),
            content: message.content,
            timestamp: message.timestamp.map(|timestamp| timestamp.to_rfc3339()),
            tool_call_id: message.tool_call_id,
            name: message.name,
            tool_call_count,
            has_reasoning: message.reasoning.is_some() || message.reasoning_content.is_some(),
            token_count: message.token_count,
            finish_reason: message.finish_reason,
        }
    }
}

/// Sanitized config response (no secrets).
#[derive(Debug, Serialize, Deserialize)]
pub struct SanitizedConfig {
    pub model_default: String,
    pub model_provider: String,
    // Model tiers for auto-dispatch
    pub model_tiers: Option<ModelTiersDto>,
    pub auto_dispatch_enabled: bool,
    pub auto_dispatch_show_decision: bool,
    pub auto_dispatch_two_stage_enabled: bool,
    pub agent_max_turns: usize,
    pub agent_verbose: bool,
    pub agent_system_prompt: String,
    pub agent_reasoning_effort: String,
    pub agent_save_trajectories: bool,
    pub agent_trajectory_dir: String,
    pub terminal_env_type: String,
    pub terminal_cwd: String,
    pub terminal_timeout: u64,
    pub terminal_docker_image: String,
    pub compression_enabled: bool,
    pub compression_engine: String,
    pub compression_model: String,
    pub compression_context_length: usize,
    pub display_streaming: bool,
    pub display_skin: String,
    pub embedding_enabled: bool,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub embedding_dimension: usize,
    pub embedding_batch_size: usize,
    pub embedding_normalize: bool,
    pub mcp_server_count: usize,
}

/// DTO for model tier configuration (sanitized, no secrets).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TierConfigDto {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub base_url: String,
}

/// DTO for model tiers collection.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelTiersDto {
    pub primary: TierConfigDto,
    pub light: Option<TierConfigDto>,
    pub reasoning: Option<TierConfigDto>,
}

/// Request body for POST /config.
#[derive(Debug, Deserialize)]
pub struct ConfigUpdate {
    pub model_default: Option<String>,
    pub model_provider: Option<String>,
    pub agent_max_turns: Option<usize>,
    pub agent_verbose: Option<bool>,
    pub agent_system_prompt: Option<String>,
    pub agent_save_trajectories: Option<bool>,
    pub agent_trajectory_dir: Option<String>,
    pub terminal_cwd: Option<String>,
    pub terminal_timeout: Option<u64>,
    pub terminal_env_type: Option<String>,
    pub terminal_docker_image: Option<String>,
    pub agent_reasoning_effort: Option<String>,
    pub compression_enabled: Option<bool>,
    pub compression_engine: Option<String>,
    pub compression_model: Option<String>,
    pub compression_context_length: Option<usize>,
    pub display_streaming: Option<bool>,
    pub display_skin: Option<String>,
    pub embedding_enabled: Option<bool>,
    pub embedding_provider: Option<String>,
    pub embedding_model: Option<String>,
    pub embedding_dimension: Option<usize>,
    pub embedding_batch_size: Option<usize>,
    pub embedding_normalize: Option<bool>,
    pub password: Option<String>,
}

/// Request body for POST /mcp/servers.
#[derive(Debug, Deserialize)]
pub struct McpServerCreate {
    pub name: String,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Accepted for dashboard compatibility; runtime writes currently support stdio only.
    pub url: Option<String>,
}

/// Request body for POST /credentials/pool.
#[derive(Debug, Deserialize)]
pub struct CredentialPoolEntryCreate {
    pub provider: String,
    pub api_key: String,
    pub id: Option<String>,
    pub label: Option<String>,
    pub base_url: Option<String>,
    pub org_id: Option<String>,
    pub source: Option<String>,
    pub priority: Option<i32>,
    pub max_concurrent: Option<usize>,
    pub strategy: Option<String>,
}

/// Request body for POST /webhooks.
#[derive(Debug, Deserialize)]
pub struct WebhookUpdate {
    pub enabled: Option<bool>,
    pub bot_id: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub secret: Option<String>,
}

/// Query parameters for GET /kanban.
#[derive(Debug, Deserialize)]
pub struct KanbanDashboardQuery {
    pub board: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub limit: Option<usize>,
}

/// Query parameters for GET /kanban/tasks/:id.
#[derive(Debug, Deserialize)]
pub struct KanbanTaskDashboardQuery {
    pub board: Option<String>,
    pub event_limit: Option<usize>,
}

/// Generic error response.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Clone)]
struct StoredResponse {
    response: JsonValue,
    messages: Vec<ChatCompletionsMessage>,
}

struct ResponsesExecution {
    response: JsonValue,
}

#[derive(Debug, Clone)]
struct RunEvent {
    sequence: usize,
    event: String,
    status: String,
    created_at: u64,
    message: Option<String>,
}

impl RunEvent {
    fn at(
        sequence: usize,
        event: impl Into<String>,
        status: impl Into<String>,
        created_at: u64,
        message: Option<String>,
    ) -> Self {
        Self {
            sequence,
            event: event.into(),
            status: status.into(),
            created_at,
            message,
        }
    }

    fn new(
        sequence: usize,
        event: impl Into<String>,
        status: impl Into<String>,
        message: Option<String>,
    ) -> Self {
        Self::at(sequence, event, status, unix_timestamp_secs(), message)
    }

    fn to_json(&self, run_id: &str) -> JsonValue {
        json!({
            "object": "hakimi.run.event",
            "sequence": self.sequence,
            "event": self.event,
            "run_id": run_id,
            "status": self.status,
            "created_at": self.created_at,
            "message": self.message
        })
    }
}

#[derive(Debug, Clone)]
struct StoredRun {
    id: String,
    status: String,
    created_at: u64,
    updated_at: u64,
    session_id: String,
    model: String,
    output_text: Option<String>,
    usage: Option<JsonValue>,
    error: Option<String>,
    events: Vec<RunEvent>,
}

impl StoredRun {
    fn new(id: String, session_id: String, model: String, created_at: u64) -> Self {
        Self {
            id,
            status: "queued".to_string(),
            created_at,
            updated_at: created_at,
            session_id,
            model,
            output_text: None,
            usage: None,
            error: None,
            events: vec![RunEvent::at(0, "run.queued", "queued", created_at, None)],
        }
    }

    fn to_json(&self) -> JsonValue {
        json!({
            "id": self.id,
            "object": "hakimi.run",
            "status": self.status,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "session_id": self.session_id,
            "model": self.model,
            "output_text": self.output_text,
            "usage": self.usage,
            "error": self.error,
            "events_url": format!("/v1/runs/{}/events", self.id)
        })
    }

    fn push_event(&mut self, event: impl Into<String>, message: Option<String>) -> RunEvent {
        let event = RunEvent::new(self.events.len(), event, self.status.clone(), message);
        self.updated_at = event.created_at;
        self.events.push(event.clone());
        event
    }
}

const RESPONSES_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS responses (
    response_id TEXT PRIMARY KEY,
    response_json TEXT NOT NULL,
    messages_json TEXT NOT NULL,
    accessed_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_responses_accessed_at
    ON responses(accessed_at);
"#;

/// Store for OpenAI Responses-compatible chaining.
pub struct ResponsesStore {
    max_entries: usize,
    entries: HashMap<String, StoredResponse>,
    order: VecDeque<String>,
    db: Option<Connection>,
    db_path: Option<PathBuf>,
}

impl Default for ResponsesStore {
    fn default() -> Self {
        match Self::persistent_default(100) {
            Ok(store) => store,
            Err(err) => {
                warn!(error = %err, "falling back to in-memory Responses API store");
                Self::new(100)
            }
        }
    }
}

impl ResponsesStore {
    /// Create an in-memory store. Tests use this to avoid touching user state.
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
            db: None,
            db_path: None,
        }
    }

    fn persistent_default(max_entries: usize) -> anyhow::Result<Self> {
        let path = default_response_store_path();
        Self::with_path(path, max_entries)
    }

    pub fn with_path(path: impl AsRef<FsPath>, max_entries: usize) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        conn.execute_batch(RESPONSES_SCHEMA_SQL)?;
        restrict_response_store_permissions(path);

        Ok(Self {
            max_entries: max_entries.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
            db: Some(conn),
            db_path: Some(path.to_path_buf()),
        })
    }

    fn insert(
        &mut self,
        response_id: String,
        response: JsonValue,
        messages: Vec<ChatCompletionsMessage>,
    ) {
        if let Some(conn) = &self.db {
            let now = unix_timestamp_millis();
            let response_json = serde_json::to_string(&response);
            let messages_json = serde_json::to_string(&messages);
            match (response_json, messages_json) {
                (Ok(response_json), Ok(messages_json)) => {
                    if let Err(err) = conn.execute(
                        "INSERT OR REPLACE INTO responses
                         (response_id, response_json, messages_json, accessed_at)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![response_id.as_str(), response_json, messages_json, now],
                    ) {
                        warn!(error = %err, "failed to persist Responses API entry");
                    } else if let Err(err) = Self::evict_sqlite(conn, self.max_entries) {
                        warn!(error = %err, "failed to evict old Responses API entries");
                    } else {
                        return;
                    }
                }
                (Err(err), _) | (_, Err(err)) => {
                    warn!(error = %err, "failed to serialize Responses API entry");
                }
            }
        }

        self.insert_memory(response_id, response, messages);
    }

    fn insert_memory(
        &mut self,
        response_id: String,
        response: JsonValue,
        messages: Vec<ChatCompletionsMessage>,
    ) {
        if !self.entries.contains_key(&response_id) {
            self.order.push_back(response_id.clone());
        }
        self.entries
            .insert(response_id.clone(), StoredResponse { response, messages });

        while self.entries.len() > self.max_entries {
            let Some(evicted) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&evicted);
        }
    }

    fn get(&self, response_id: &str) -> Option<JsonValue> {
        if let Some(conn) = &self.db {
            match Self::get_sqlite_response(conn, response_id) {
                Ok(Some(response)) => return Some(response),
                Ok(None) => {}
                Err(err) => warn!(error = %err, "failed to read persisted Responses API entry"),
            }
        }

        self.entries
            .get(response_id)
            .map(|stored| stored.response.clone())
    }

    fn messages(&self, response_id: &str) -> Option<Vec<ChatCompletionsMessage>> {
        if let Some(conn) = &self.db {
            match Self::get_sqlite_messages(conn, response_id) {
                Ok(Some(messages)) => return Some(messages),
                Ok(None) => {}
                Err(err) => warn!(error = %err, "failed to read persisted Responses API messages"),
            }
        }

        self.entries
            .get(response_id)
            .map(|stored| stored.messages.clone())
    }

    fn delete(&mut self, response_id: &str) -> bool {
        let mut removed = false;

        if let Some(conn) = &self.db {
            match conn.execute(
                "DELETE FROM responses WHERE response_id = ?1",
                params![response_id],
            ) {
                Ok(rows) => removed = rows > 0,
                Err(err) => warn!(error = %err, "failed to delete persisted Responses API entry"),
            }
        }

        if self.entries.remove(response_id).is_some() {
            removed = true;
            self.order.retain(|id| id != response_id);
        }

        removed
    }

    fn get_sqlite_response(
        conn: &Connection,
        response_id: &str,
    ) -> anyhow::Result<Option<JsonValue>> {
        let row = conn
            .query_row(
                "SELECT response_json FROM responses WHERE response_id = ?1",
                params![response_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(response_json) = row else {
            return Ok(None);
        };

        conn.execute(
            "UPDATE responses SET accessed_at = ?1 WHERE response_id = ?2",
            params![unix_timestamp_millis(), response_id],
        )?;

        Ok(Some(serde_json::from_str(&response_json)?))
    }

    fn get_sqlite_messages(
        conn: &Connection,
        response_id: &str,
    ) -> anyhow::Result<Option<Vec<ChatCompletionsMessage>>> {
        let row = conn
            .query_row(
                "SELECT messages_json FROM responses WHERE response_id = ?1",
                params![response_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(messages_json) = row else {
            return Ok(None);
        };

        conn.execute(
            "UPDATE responses SET accessed_at = ?1 WHERE response_id = ?2",
            params![unix_timestamp_millis(), response_id],
        )?;

        Ok(Some(serde_json::from_str(&messages_json)?))
    }

    fn evict_sqlite(conn: &Connection, max_entries: usize) -> anyhow::Result<()> {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM responses", [], |row| row.get(0))?;
        let overflow = count - max_entries as i64;
        if overflow <= 0 {
            return Ok(());
        }

        let mut stmt = conn.prepare(
            "SELECT response_id FROM responses ORDER BY accessed_at ASC, response_id ASC LIMIT ?1",
        )?;
        let evicted = stmt
            .query_map(params![overflow], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        for response_id in evicted {
            conn.execute(
                "DELETE FROM responses WHERE response_id = ?1",
                params![response_id],
            )?;
        }

        Ok(())
    }
}

impl std::fmt::Debug for ResponsesStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponsesStore")
            .field("max_entries", &self.max_entries)
            .field("memory_entries", &self.entries.len())
            .field("db_path", &self.db_path)
            .finish()
    }
}

fn default_response_store_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HAKIMI_RESPONSE_STORE_PATH") {
        return PathBuf::from(path);
    }
    if let Some(home) = std::env::var_os("HAKIMI_HOME") {
        return PathBuf::from(home).join("response_store.db");
    }

    dirs::home_dir()
        .map(|home| home.join(".hakimi").join("response_store.db"))
        .unwrap_or_else(|| PathBuf::from(".hakimi").join("response_store.db"))
}

#[cfg(unix)]
fn restrict_response_store_permissions(path: &FsPath) {
    use std::os::unix::fs::PermissionsExt;

    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if let Ok(metadata) = std::fs::metadata(&candidate) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600);
            let _ = std::fs::set_permissions(&candidate, permissions);
        }
    }
}

#[cfg(not(unix))]
fn restrict_response_store_permissions(_path: &FsPath) {}

struct RunControl {
    interrupt: std::sync::Arc<AtomicBool>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for RunControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunControl").finish_non_exhaustive()
    }
}

enum StopRunResult {
    Cancelled(JsonValue),
    AlreadyFinished(String),
    NotFound,
}

fn is_terminal_run_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

struct RunEventsSubscription {
    snapshot: Vec<RunEvent>,
    receiver: Option<broadcast::Receiver<RunEvent>>,
    since_sequence: usize,
}

/// In-memory store for asynchronous API runs.
#[derive(Debug)]
pub struct RunsStore {
    max_entries: usize,
    entries: HashMap<String, StoredRun>,
    order: VecDeque<String>,
    controls: HashMap<String, RunControl>,
    event_streams: HashMap<String, broadcast::Sender<RunEvent>>,
}

impl Default for RunsStore {
    fn default() -> Self {
        Self::new(100)
    }
}

impl RunsStore {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
            controls: HashMap::new(),
            event_streams: HashMap::new(),
        }
    }

    fn insert(&mut self, run: StoredRun) {
        let run_id = run.id.clone();
        if !self.entries.contains_key(&run.id) {
            self.order.push_back(run.id.clone());
        }
        self.event_streams.entry(run_id).or_insert_with(|| {
            let (sender, _receiver) = broadcast::channel(64);
            sender
        });
        self.entries.insert(run.id.clone(), run);

        while self.entries.len() > self.max_entries {
            let Some(evicted) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&evicted);
            self.event_streams.remove(&evicted);
            if let Some(control) = self.controls.remove(&evicted) {
                control.interrupt.store(true, Ordering::Relaxed);
                control.task.abort();
            }
        }
    }

    fn get(&self, run_id: &str) -> Option<JsonValue> {
        self.entries.get(run_id).map(StoredRun::to_json)
    }

    fn subscribe_events(&self, run_id: &str) -> Option<RunEventsSubscription> {
        let run = self.entries.get(run_id)?;
        let receiver = self
            .event_streams
            .get(run_id)
            .map(|sender| sender.subscribe());
        let snapshot = run.events.clone();
        let since_sequence = snapshot
            .iter()
            .map(|event| event.sequence)
            .max()
            .unwrap_or_default();
        Some(RunEventsSubscription {
            snapshot,
            receiver,
            since_sequence,
        })
    }

    fn attach_control(&mut self, run_id: &str, control: RunControl) {
        match self.entries.get(run_id) {
            Some(run) if !is_terminal_run_status(&run.status) => {
                if let Some(previous) = self.controls.insert(run_id.to_string(), control) {
                    previous.task.abort();
                }
            }
            _ => {
                control.task.abort();
            }
        }
    }

    fn set_status(&mut self, run_id: &str, status: &str) {
        let sender = self.event_streams.get(run_id).cloned();
        if let Some(run) = self.entries.get_mut(run_id) {
            if is_terminal_run_status(&run.status) {
                return;
            }
            run.status = status.to_string();
            let event = run.push_event(format!("run.{status}"), None);
            if let Some(sender) = sender {
                let _ = sender.send(event);
            }
        }
    }

    fn complete(&mut self, run_id: &str, output_text: String, usage: JsonValue) {
        let sender = self.event_streams.get(run_id).cloned();
        if let Some(run) = self.entries.get_mut(run_id) {
            if is_terminal_run_status(&run.status) {
                self.controls.remove(run_id);
                self.event_streams.remove(run_id);
                return;
            }
            run.status = "completed".to_string();
            run.output_text = Some(output_text);
            run.usage = Some(usage);
            run.error = None;
            let event = run.push_event("run.completed", None);
            if let Some(sender) = sender {
                let _ = sender.send(event);
            }
        }
        self.controls.remove(run_id);
        self.event_streams.remove(run_id);
    }

    fn fail(&mut self, run_id: &str, error: String) {
        let sender = self.event_streams.get(run_id).cloned();
        if let Some(run) = self.entries.get_mut(run_id) {
            if is_terminal_run_status(&run.status) {
                self.controls.remove(run_id);
                self.event_streams.remove(run_id);
                return;
            }
            run.status = "failed".to_string();
            run.error = Some(error.clone());
            let event = run.push_event("run.failed", Some(error));
            if let Some(sender) = sender {
                let _ = sender.send(event);
            }
        }
        self.controls.remove(run_id);
        self.event_streams.remove(run_id);
    }

    fn stop(&mut self, run_id: &str) -> StopRunResult {
        let sender = self.event_streams.get(run_id).cloned();
        let Some(run) = self.entries.get_mut(run_id) else {
            return StopRunResult::NotFound;
        };
        if is_terminal_run_status(&run.status) {
            return StopRunResult::AlreadyFinished(run.status.clone());
        }

        run.status = "cancelled".to_string();
        let message = "Stop requested via API".to_string();
        run.error = Some(message.clone());
        let event = run.push_event("run.cancelled", Some(message));
        if let Some(sender) = sender {
            let _ = sender.send(event);
        }
        let body = run.to_json();

        if let Some(control) = self.controls.remove(run_id) {
            control.interrupt.store(true, Ordering::Relaxed);
            control.task.abort();
        }
        self.event_streams.remove(run_id);

        StopRunResult::Cancelled(body)
    }
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let password = state.webui_password.lock().await.clone();
    if password.trim().is_empty() {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    if let Some(auth) = auth_header
        && auth == format!("Bearer {}", password)
    {
        return Ok(next.run(req).await);
    }

    Err(StatusCode::UNAUTHORIZED)
}

// ---------------------------------------------------------------------------
// Workspace helpers
// ---------------------------------------------------------------------------

/// Resolve a relative path within the working directory, rejecting `..` escapes.
fn resolve_workspace_path(relative: &str) -> Result<std::path::PathBuf, (StatusCode, String)> {
    let trimmed = relative.trim();
    let normalized = trimmed.trim_start_matches('/');
    if normalized.is_empty() {
        return Ok(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));
    }

    // Reject paths that contain `..` components anywhere.
    for component in std::path::Path::new(normalized).components() {
        if let std::path::Component::ParentDir = component {
            return Err((
                StatusCode::FORBIDDEN,
                "Path contains '..', which is not allowed".to_string(),
            ));
        }
    }

    let base = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let joined = base.join(normalized);

    // Extra safety: canonicalize and ensure it stays within base.
    let canonical_base = std::fs::canonicalize(&base).unwrap_or(base.clone());
    let canonical_joined = std::fs::canonicalize(&joined).unwrap_or(joined.clone());
    if !canonical_joined.starts_with(&canonical_base) {
        return Err((
            StatusCode::FORBIDDEN,
            "Path escapes the working directory".to_string(),
        ));
    }

    Ok(joined)
}

/// Build a map of relative path -> git porcelain status from `git status --porcelain`.
fn git_status_map(dir: &std::path::Path) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if !dir.join(".git").is_dir() {
        return map;
    }
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &dir.to_string_lossy(),
            "status",
            "--porcelain",
            "-uall",
        ])
        .output();
    let Ok(output) = output else {
        return map;
    };
    if !output.status.success() {
        return map;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.len() < 3 {
            continue;
        }
        let status = &line[..2];
        let path_str = line[3..].to_string();
        map.insert(path_str, status.to_string());
    }
    map
}

/// Given a `git_status_map` and a list of entries, set `is_git_tracked` and `git_status`.
fn apply_git_status(
    entries: &mut [WorkspaceListEntry],
    _dir: &std::path::Path,
    git_map: &std::collections::HashMap<String, String>,
) {
    for entry in entries.iter_mut() {
        if let Some(status) = git_map.get(&entry.name) {
            entry.is_git_tracked = true;
            entry.git_status = Some(status.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Workspace handlers
// ---------------------------------------------------------------------------

/// GET /api/workspace/list — list directory contents.
async fn workspace_list(
    Query(query): Query<WorkspacePathQuery>,
) -> Result<Json<WorkspaceListResponse>, (StatusCode, String)> {
    let path = resolve_workspace_path(query.path.as_deref().unwrap_or(""))?;
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(&path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read directory: {e}"),
        )
    })?;

    for entry in read_dir {
        let entry = entry.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read directory entry: {e}"),
            )
        })?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let size = if is_dir {
            0
        } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };
        entries.push(WorkspaceListEntry {
            name,
            entry_type: if is_dir {
                "dir".to_string()
            } else {
                "file".to_string()
            },
            size,
            is_dir,
            is_git_tracked: false,
            git_status: None,
        });
    }

    // Sort: directories first, then files, both alphabetically.
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));

    // Apply git status when available.
    let git_map = git_status_map(&path);
    if !git_map.is_empty() {
        apply_git_status(&mut entries, &path, &git_map);
    }

    Ok(Json(WorkspaceListResponse { entries }))
}

/// GET /api/workspace/read — read file contents.
async fn workspace_read(
    Query(query): Query<WorkspacePathQuery>,
) -> Result<Json<WorkspaceReadResponse>, (StatusCode, String)> {
    let path = resolve_workspace_path(query.path.as_deref().unwrap_or(""))?;
    let content = std::fs::read_to_string(&path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read file: {e}"),
        )
    })?;
    Ok(Json(WorkspaceReadResponse { content }))
}

/// POST /api/workspace/create — create file or directory.
async fn workspace_create(
    Json(req): Json<WorkspaceCreateRequest>,
) -> Result<Json<WorkspaceSuccessResponse>, (StatusCode, String)> {
    let path = resolve_workspace_path(&req.path)?;
    if req.is_dir {
        std::fs::create_dir_all(&path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create directory: {e}"),
            )
        })?;
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to create parent directory: {e}"),
                )
            })?;
        }
        std::fs::write(&path, b"").map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create file: {e}"),
            )
        })?;
    }
    Ok(Json(WorkspaceSuccessResponse { success: true }))
}

/// POST /api/workspace/rename — rename file or directory.
async fn workspace_rename(
    Json(req): Json<WorkspaceRenameRequest>,
) -> Result<Json<WorkspaceSuccessResponse>, (StatusCode, String)> {
    let old = resolve_workspace_path(&req.old_path)?;
    let new = resolve_workspace_path(&req.new_path)?;
    std::fs::rename(&old, &new).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to rename: {e}"),
        )
    })?;
    Ok(Json(WorkspaceSuccessResponse { success: true }))
}

/// POST /api/workspace/delete — delete file or directory.
async fn workspace_delete(
    Json(req): Json<WorkspaceDeleteRequest>,
) -> Result<Json<WorkspaceSuccessResponse>, (StatusCode, String)> {
    let path = resolve_workspace_path(&req.path)?;
    if path.is_dir() {
        if req.recursive {
            std::fs::remove_dir_all(&path).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to delete directory: {e}"),
                )
            })?;
        } else {
            std::fs::remove_dir(&path).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to delete directory: {e}"),
                )
            })?;
        }
    } else {
        std::fs::remove_file(&path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete file: {e}"),
            )
        })?;
    }
    Ok(Json(WorkspaceSuccessResponse { success: true }))
}

// ---------------------------------------------------------------------------
// Cron API handlers
// ---------------------------------------------------------------------------

fn cron_store_path() -> PathBuf {
    std::env::var("HAKIMI_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".hakimi")))
        .unwrap_or_else(|| PathBuf::from(".hakimi"))
        .join("cron.db")
}

fn cron_error(status: StatusCode, error: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
        }),
    )
}

fn cron_job_info(job: &CronJob) -> CronJobInfo {
    let (schedule_type, schedule_value) = match &job.schedule {
        hakimi_cron::CronSchedule::IntervalMinutes(minutes) => {
            ("minutes".to_string(), minutes.to_string())
        }
        hakimi_cron::CronSchedule::IntervalHours(hours) => ("hours".to_string(), hours.to_string()),
        hakimi_cron::CronSchedule::CronExpr(expr) => ("cron".to_string(), expr.clone()),
    };
    CronJobInfo {
        id: job.id.clone(),
        name: job.name.clone(),
        schedule: if schedule_type == "cron" {
            schedule_value.clone()
        } else {
            format!("{} {}", schedule_type, schedule_value)
        },
        schedule_type,
        prompt: job.prompt.clone(),
        enabled: job.enabled,
        last_run: job.last_run.map(|t| t.to_rfc3339()),
        next_run: job.next_run.map(|t| t.to_rfc3339()),
        created_at: None,
        deliver: job.deliver.clone(),
        repeat_times: job.repeat.times.map(i64::from),
        repeat_completed: Some(i64::from(job.repeat.completed)),
    }
}

/// GET /api/cron/jobs — list persisted cron jobs for the WebUI settings panel.
async fn cron_jobs() -> Result<Json<CronJobsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let db_path = cron_store_path();
    if !db_path.exists() {
        return Ok(Json(CronJobsResponse {
            object: "list".to_string(),
            total: 0,
            jobs: Vec::new(),
        }));
    }

    let jobs = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<CronJobInfo>> {
        let store = PersistentCronStore::open(&db_path)?;
        Ok(store.load_all()?.iter().map(cron_job_info).collect())
    })
    .await
    .map_err(|e| {
        cron_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to join cron job query: {e}"),
        )
    })?
    .map_err(|e| {
        cron_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list cron jobs: {e}"),
        )
    })?;

    Ok(Json(CronJobsResponse {
        object: "list".to_string(),
        total: jobs.len(),
        jobs,
    }))
}

/// POST /api/cron/jobs — create a persisted cron job.
async fn cron_create_job(
    Json(req): Json<CronJobCreateRequest>,
) -> Result<Json<CronJobMutationResponse>, (StatusCode, Json<ErrorResponse>)> {
    validate_cron_prompt(&req.prompt)
        .map_err(|e| cron_error(StatusCode::BAD_REQUEST, e.to_string()))?;
    let schedule = parse_schedule(&req.schedule)
        .map_err(|e| cron_error(StatusCode::BAD_REQUEST, format!("Invalid schedule: {e}")))?;
    let name = req
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "WebUI cron job".to_string());
    let mut job = CronJob::new(name, schedule, req.prompt);
    job.skills = req.skills;
    job.enabled_toolsets = (!req.enabled_toolsets.is_empty()).then_some(req.enabled_toolsets);
    job.context_from = req.context_from;
    job.deliver = req.deliver.filter(|deliver| !deliver.trim().is_empty());
    job.repeat = CronRepeat::new(req.repeat);

    let db_path = cron_store_path();
    let saved = tokio::task::spawn_blocking(move || -> anyhow::Result<CronJobInfo> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = PersistentCronStore::open(&db_path)?;
        store.save_job(&job)?;
        Ok(cron_job_info(&job))
    })
    .await
    .map_err(|e| {
        cron_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to join cron job create: {e}"),
        )
    })?
    .map_err(|e| {
        cron_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create cron job: {e}"),
        )
    })?;

    Ok(Json(CronJobMutationResponse {
        object: "cron.job".to_string(),
        success: true,
        job: Some(saved),
    }))
}

async fn cron_delete_job(
    Path(id): Path<String>,
) -> Result<Json<CronJobMutationResponse>, (StatusCode, Json<ErrorResponse>)> {
    cron_store_mutate(id, |store, id| store.remove_job(id)).await
}

async fn cron_pause_job(
    Path(id): Path<String>,
) -> Result<Json<CronJobMutationResponse>, (StatusCode, Json<ErrorResponse>)> {
    cron_store_mutate(id, |store, id| store.set_enabled(id, false)).await
}

async fn cron_resume_job(
    Path(id): Path<String>,
) -> Result<Json<CronJobMutationResponse>, (StatusCode, Json<ErrorResponse>)> {
    cron_store_mutate(id, |store, id| store.set_enabled(id, true)).await
}

async fn cron_run_job_now(
    Path(id): Path<String>,
) -> Result<Json<CronJobMutationResponse>, (StatusCode, Json<ErrorResponse>)> {
    cron_store_mutate(id, |store, id| store.trigger_now(id, Utc::now())).await
}

async fn cron_store_mutate<F>(
    id: String,
    action: F,
) -> Result<Json<CronJobMutationResponse>, (StatusCode, Json<ErrorResponse>)>
where
    F: FnOnce(&PersistentCronStore, &str) -> anyhow::Result<bool> + Send + 'static,
{
    let db_path = cron_store_path();
    let exists_before = db_path.exists();
    let mutation =
        tokio::task::spawn_blocking(move || -> anyhow::Result<(bool, Option<CronJobInfo>)> {
            if !exists_before {
                return Ok((false, None));
            }
            let store = PersistentCronStore::open(&db_path)?;
            let changed = action(&store, &id)?;
            if !changed {
                return Ok((false, None));
            }
            Ok((true, store.get_job(&id)?.as_ref().map(cron_job_info)))
        })
        .await
        .map_err(|e| {
            cron_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to join cron job mutation: {e}"),
            )
        })?
        .map_err(|e| {
            cron_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update cron job: {e}"),
            )
        })?;

    let (changed, updated) = mutation;
    if !changed {
        return Err(cron_error(StatusCode::NOT_FOUND, "Cron job not found"));
    }

    Ok(Json(CronJobMutationResponse {
        object: "cron.job".to_string(),
        success: true,
        job: updated,
    }))
}

// ---------------------------------------------------------------------------
// Memory / Knowledge Base API handlers
// ---------------------------------------------------------------------------

/// Query string for memory search.
#[derive(Debug, Deserialize)]
pub struct MemorySearchParams {
    pub q: Option<String>,
}

/// GET /api/memory/stats — get knowledge base stats.
async fn memory_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let kp = state.knowledge_provider.lock().await;
    let snapshot = kp.graph_snapshot().await;
    let stats = snapshot.stats();
    Json(json!({
        "node_count": stats.node_count,
        "edge_count": stats.edge_count,
        "connected_components": stats.connected_components,
        "avg_degree": stats.avg_degree,
    }))
}

/// GET /api/memory/search?q=... — search the knowledge base.
async fn memory_search(
    State(state): State<AppState>,
    Query(params): Query<MemorySearchParams>,
) -> Json<serde_json::Value> {
    let query = params.q.unwrap_or_default();
    if query.is_empty() {
        return Json(json!({
            "results": [],
            "count": 0,
            "query": "",
        }));
    }

    let kp = state.knowledge_provider.lock().await;
    let snapshot = kp.graph_snapshot().await;
    let nodes = snapshot.search(&query);
    let results: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            json!({
                "key": n.key(),
                "kind": n.kind(),
            })
        })
        .collect();
    Json(json!({
        "results": results,
        "count": results.len(),
        "query": query,
    }))
}

/// GET /api/memory/entities — list recent entities.
async fn memory_entities(State(state): State<AppState>) -> Json<serde_json::Value> {
    let kp = state.knowledge_provider.lock().await;
    let snapshot = kp.graph_snapshot().await;
    let nodes = snapshot.all_nodes();
    let entities: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            json!({
                "key": n.key(),
                "kind": n.kind(),
            })
        })
        .collect();
    Json(json!({
        "entities": entities,
        "count": entities.len(),
    }))
}

/// GET /api/memory/entities/:id — get entity details.
async fn memory_entity_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let kp = state.knowledge_provider.lock().await;
    let snapshot = kp.graph_snapshot().await;

    let node = match snapshot.get_node(&id) {
        Some(n) => n,
        None => {
            return Json(json!({
                "error": "Entity not found",
                "key": id,
            }));
        }
    };

    // Gather neighbors
    let neighbors = snapshot.query_neighbors(&id, 1);
    let neighbor_list: Vec<serde_json::Value> = neighbors
        .iter()
        .map(|n| {
            json!({
                "key": n.key(),
                "kind": n.kind(),
            })
        })
        .collect();

    Json(json!({
        "key": node.key(),
        "kind": node.kind(),
        "neighbors": neighbor_list,
        "neighbor_count": neighbor_list.len(),
    }))
}

// ========== Gateway API handlers ==========

#[derive(Debug, serde::Serialize)]
struct GatewayStatusResponse {
    running: bool,
    platforms: Vec<GatewayPlatformStatus>,
    config_loaded: bool,
}

#[derive(Debug, serde::Serialize)]
struct GatewayPlatformStatus {
    name: String,
    connected: bool,
    bot_count: usize,
}

async fn gateway_status(
    State(state): State<AppState>,
) -> Result<Json<GatewayStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let gateway_opt = state.gateway.as_ref();

    if let Some(_gateway) = gateway_opt {
        // Gateway is Arc<Gateway>, not Arc<Mutex<Gateway>>

        // Get platform connection status from gateway
        let platforms = vec![GatewayPlatformStatus {
            name: "telegram".to_string(),
            connected: true, // TODO: get actual status from gateway
            bot_count: 1,    // TODO: get actual count
        }];

        Ok(Json(GatewayStatusResponse {
            running: true,
            platforms,
            config_loaded: true,
        }))
    } else {
        // Gateway not running in unified mode
        Ok(Json(GatewayStatusResponse {
            running: false,
            platforms: vec![],
            config_loaded: false,
        }))
    }
}

async fn gateway_config(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let config = state.config.lock().await;

    // Return sanitized gateway config
    Ok(Json(json!({
        "busy_input_mode": config.gateways.busy_input_mode,
        "allow_all": config.gateways.allow_all,
        "allowed_users": config.gateways.allowed_users,
        "filter_silence_narration": config.gateways.filter_silence_narration,
        // Don't expose bot tokens or sensitive data
    })))
}

#[derive(Debug, serde::Deserialize)]
struct GatewayConfigUpdate {
    busy_input_mode: Option<String>,
    allow_all: Option<bool>,
    allowed_users: Option<Vec<String>>,
    filter_silence_narration: Option<bool>,
}

async fn gateway_update_config(
    State(state): State<AppState>,
    Json(req): Json<GatewayConfigUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let mut config = state.config.lock().await;

    if let Some(mode) = req.busy_input_mode {
        if mode != "parallel" && mode != "queue" && mode != "interrupt" {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!(
                        "Invalid busy_input_mode: {}. Must be 'parallel', 'queue', or 'interrupt'",
                        mode
                    ),
                }),
            ));
        }
        config.gateways.busy_input_mode = mode.clone();
    }
    if let Some(allow_all) = req.allow_all {
        config.gateways.allow_all = allow_all;
    }
    if let Some(users) = req.allowed_users {
        config.gateways.allowed_users = users;
    }
    if let Some(filter) = req.filter_silence_narration {
        config.gateways.filter_silence_narration = filter;
    }

    save_config_to_disk(&config)?;

    Ok(Json(json!({
        "success": true,
        "message": "Gateway configuration updated"
    })))
}

async fn gateway_restart(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    // Restart hakimi via systemd
    let output = tokio::process::Command::new("systemctl")
        .args(["restart", "hakimi"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => Ok(Json(serde_json::json!({
            "status": "restarting",
            "message": "Hakimi service is restarting. The UI will reconnect automatically."
        }))),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to restart service: {}", stderr),
                }),
            ))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to execute systemctl: {}", e),
            }),
        )),
    }
}

async fn gateway_shutdown(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    // Trigger graceful shutdown via broadcast channel
    if let Some(shutdown_tx) = &state.shutdown_tx {
        let _ = shutdown_tx.send(());
        Ok(Json(serde_json::json!({
            "status": "shutting_down",
            "message": "Hakimi is shutting down gracefully. All running tasks will be completed."
        })))
    } else {
        // WebUI-only mode doesn't support shutdown command
        Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Shutdown command is only available in unified or gateway mode".to_string(),
            }),
        ))
    }
}

/// Mask a secret string for safe display: show first 4 and last 2 chars.
fn mask_secret(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if s.len() <= 8 {
        return "*".repeat(s.len());
    }
    let prefix = &s[..4];
    let suffix = &s[s.len() - 2..];
    format!("{prefix}{}…{suffix}", "*".repeat(4))
}

fn config_home_path() -> String {
    std::env::var("HOME")
        .map(|h| format!("{h}/.hakimi/config.yaml"))
        .unwrap_or_else(|_| "/root/.hakimi/config.yaml".to_string())
}

fn save_config_to_disk(
    config: &hakimi_config::HakimiConfig,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let path = config_home_path();
    std::fs::write(&path, serde_yaml::to_string(config).unwrap()).map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save config: {e}"),
        )
    })
}

/// GET /api/gateways/platforms -- list every gateway platform with its config.
async fn list_gateway_platforms(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let config = state.config.lock().await;
    let gw = &config.gateways;

    let platforms = json!([
        {
            "platform": "telegram",
            "enabled": !gw.telegram.bot_token.is_empty(),
            "bot_id": "telegram_bot",
            "config": {
                "bot_token": mask_secret(&gw.telegram.bot_token),
                "allowed_users": gw.telegram.allowed_users,
            }
        },
        {
            "platform": "qqbot",
            "enabled": gw.qqbot.enabled,
            "bot_id": gw.qqbot.bot_id,
            "config": {
                "enabled": gw.qqbot.enabled,
                "bot_id": gw.qqbot.bot_id,
                "app_id": mask_secret(&gw.qqbot.app_id),
                "client_secret": mask_secret(&gw.qqbot.client_secret),
                "home_channel": gw.qqbot.home_channel,
                "default_chat_type": gw.qqbot.default_chat_type,
                "markdown_support": gw.qqbot.markdown_support,
            }
        },
        {
            "platform": "clawbot",
            "enabled": gw.clawbot.enabled,
            "bot_id": gw.clawbot.bot_id,
            "config": {
                "enabled": gw.clawbot.enabled,
                "bot_id": gw.clawbot.bot_id,
                "mode": gw.clawbot.mode,
                "base_url": gw.clawbot.base_url,
                "token": mask_secret(&gw.clawbot.token),
                "poll_path": gw.clawbot.poll_path,
                "send_path": gw.clawbot.send_path,
                "poll_interval_ms": gw.clawbot.poll_interval_ms,
            }
        },
        {
            "platform": "weixin",
            "enabled": gw.weixin.enabled,
            "bot_id": gw.weixin.bot_id,
            "config": {
                "enabled": gw.weixin.enabled,
                "bot_id": gw.weixin.bot_id,
                "base_url": gw.weixin.base_url,
                "token": mask_secret(&gw.weixin.token),
                "home_channel": gw.weixin.home_channel,
                "login_notify_platform": gw.weixin.login_notify_platform,
                "login_notify_bot_id": gw.weixin.login_notify_bot_id,
                "login_notify_chat_id": gw.weixin.login_notify_chat_id,
            }
        },
        {
            "platform": "discord",
            "enabled": gw.discord.enabled,
            "bot_id": gw.discord.bot_id,
            "config": {
                "enabled": gw.discord.enabled,
                "bot_id": gw.discord.bot_id,
                "token": mask_secret(&gw.discord.token),
                "channel_id": gw.discord.channel_id,
            }
        },
        {
            "platform": "slack",
            "enabled": gw.slack.enabled,
            "bot_id": gw.slack.bot_id,
            "config": {
                "enabled": gw.slack.enabled,
                "bot_id": gw.slack.bot_id,
                "token": mask_secret(&gw.slack.token),
                "channel_id": gw.slack.channel_id,
            }
        },
        {
            "platform": "dingtalk",
            "enabled": gw.dingtalk.enabled,
            "bot_id": gw.dingtalk.bot_id,
            "config": {
                "enabled": gw.dingtalk.enabled,
                "bot_id": gw.dingtalk.bot_id,
            }
        },
        {
            "platform": "feishu",
            "enabled": gw.feishu.enabled,
            "bot_id": gw.feishu.bot_id,
            "config": {
                "enabled": gw.feishu.enabled,
                "bot_id": gw.feishu.bot_id,
            }
        },
        {
            "platform": "wecom",
            "enabled": gw.wecom.enabled,
            "bot_id": gw.wecom.bot_id,
            "config": {
                "enabled": gw.wecom.enabled,
                "bot_id": gw.wecom.bot_id,
            }
        },
        {
            "platform": "webhook",
            "enabled": gw.webhook.enabled,
            "bot_id": gw.webhook.bot_id,
            "config": {
                "enabled": gw.webhook.enabled,
                "bot_id": gw.webhook.bot_id,
                "port": gw.webhook.port,
                "path": gw.webhook.path,
                "secret": mask_secret(&gw.webhook.secret),
            }
        },
        {
            "platform": "mattermost",
            "enabled": gw.mattermost.enabled,
            "bot_id": gw.mattermost.bot_id,
            "config": {
                "enabled": gw.mattermost.enabled,
                "bot_id": gw.mattermost.bot_id,
                "base_url": gw.mattermost.base_url,
                "token": mask_secret(&gw.mattermost.token),
            }
        },
        {
            "platform": "signal",
            "enabled": gw.signal.enabled,
            "bot_id": gw.signal.bot_id,
            "config": {
                "enabled": gw.signal.enabled,
                "bot_id": gw.signal.bot_id,
            }
        },
        {
            "platform": "email",
            "enabled": gw.email.enabled,
            "bot_id": gw.email.bot_id,
            "config": {
                "enabled": gw.email.enabled,
                "bot_id": gw.email.bot_id,
            }
        },
        {
            "platform": "whatsapp",
            "enabled": gw.whatsapp.enabled,
            "bot_id": gw.whatsapp.bot_id,
            "config": {
                "enabled": gw.whatsapp.enabled,
                "bot_id": gw.whatsapp.bot_id,
            }
        },
        {
            "platform": "matrix",
            "enabled": gw.matrix.enabled,
            "bot_id": gw.matrix.bot_id,
            "config": {
                "enabled": gw.matrix.enabled,
                "bot_id": gw.matrix.bot_id,
            }
        },
        {
            "platform": "homeassistant",
            "enabled": gw.homeassistant.enabled,
            "bot_id": gw.homeassistant.bot_id,
            "config": {
                "enabled": gw.homeassistant.enabled,
                "bot_id": gw.homeassistant.bot_id,
            }
        },
        {
            "platform": "bluebubbles",
            "enabled": gw.bluebubbles.enabled,
            "bot_id": gw.bluebubbles.bot_id,
            "config": {
                "enabled": gw.bluebubbles.enabled,
                "bot_id": gw.bluebubbles.bot_id,
            }
        },
        {
            "platform": "sms",
            "enabled": gw.sms.enabled,
            "bot_id": gw.sms.bot_id,
            "config": {
                "enabled": gw.sms.enabled,
                "bot_id": gw.sms.bot_id,
            }
        },
        {
            "platform": "msgraph",
            "enabled": gw.msgraph_webhook.enabled,
            "bot_id": gw.msgraph_webhook.bot_id,
            "config": {
                "enabled": gw.msgraph_webhook.enabled,
                "bot_id": gw.msgraph_webhook.bot_id,
            }
        },
    ]);

    Ok(Json(platforms))
}

/// PATCH /api/gateways/platforms/{platform} -- update a platform's config.
async fn update_gateway_platform(
    State(state): State<AppState>,
    Path(platform): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let mut config = state.config.lock().await;

    macro_rules! apply_str {
        ($target:expr, $field:literal, $req:expr) => {
            if let Some(v) = $req.get($field).and_then(|v| v.as_str()) {
                $target = v.to_string();
            }
        };
    }
    macro_rules! apply_bool {
        ($target:expr, $field:literal, $req:expr) => {
            if let Some(v) = $req.get($field).and_then(|v| v.as_bool()) {
                $target = v;
            }
        };
    }

    match platform.as_str() {
        "telegram" => {
            let tg = &mut config.gateways.telegram;
            apply_str!(tg.bot_token, "bot_token", req);
            if let Some(users) = req.get("allowed_users").and_then(|v| v.as_array()) {
                tg.allowed_users = users.iter().filter_map(|v| v.as_i64()).collect();
            }
        }
        "qqbot" => {
            let qq = &mut config.gateways.qqbot;
            apply_bool!(qq.enabled, "enabled", req);
            apply_str!(qq.bot_id, "bot_id", req);
            apply_str!(qq.app_id, "app_id", req);
            apply_str!(qq.client_secret, "client_secret", req);
            apply_str!(qq.home_channel, "home_channel", req);
            apply_str!(qq.default_chat_type, "default_chat_type", req);
            apply_bool!(qq.markdown_support, "markdown_support", req);
        }
        "clawbot" => {
            let cb = &mut config.gateways.clawbot;
            apply_bool!(cb.enabled, "enabled", req);
            apply_str!(cb.bot_id, "bot_id", req);
            apply_str!(cb.mode, "mode", req);
            apply_str!(cb.base_url, "base_url", req);
            apply_str!(cb.token, "token", req);
            apply_str!(cb.poll_path, "poll_path", req);
            apply_str!(cb.send_path, "send_path", req);
            if let Some(v) = req.get("poll_interval_ms").and_then(|v| v.as_u64()) {
                cb.poll_interval_ms = v;
            }
        }
        "weixin" => {
            let wx = &mut config.gateways.weixin;
            apply_bool!(wx.enabled, "enabled", req);
            apply_str!(wx.bot_id, "bot_id", req);
            apply_str!(wx.base_url, "base_url", req);
            apply_str!(wx.token, "token", req);
            apply_str!(wx.home_channel, "home_channel", req);
            apply_str!(wx.login_notify_platform, "login_notify_platform", req);
            apply_str!(wx.login_notify_bot_id, "login_notify_bot_id", req);
            apply_str!(wx.login_notify_chat_id, "login_notify_chat_id", req);
        }
        "discord" => {
            let dc = &mut config.gateways.discord;
            apply_bool!(dc.enabled, "enabled", req);
            apply_str!(dc.bot_id, "bot_id", req);
            apply_str!(dc.token, "token", req);
            apply_str!(dc.channel_id, "channel_id", req);
        }
        "slack" => {
            let sl = &mut config.gateways.slack;
            apply_bool!(sl.enabled, "enabled", req);
            apply_str!(sl.bot_id, "bot_id", req);
            apply_str!(sl.token, "token", req);
            apply_str!(sl.channel_id, "channel_id", req);
        }
        "dingtalk" => {
            let dt = &mut config.gateways.dingtalk;
            apply_bool!(dt.enabled, "enabled", req);
            apply_str!(dt.bot_id, "bot_id", req);
        }
        "feishu" => {
            let fs = &mut config.gateways.feishu;
            apply_bool!(fs.enabled, "enabled", req);
            apply_str!(fs.bot_id, "bot_id", req);
        }
        "wecom" => {
            let wc = &mut config.gateways.wecom;
            apply_bool!(wc.enabled, "enabled", req);
            apply_str!(wc.bot_id, "bot_id", req);
        }
        "webhook" => {
            let wh = &mut config.gateways.webhook;
            apply_bool!(wh.enabled, "enabled", req);
            apply_str!(wh.bot_id, "bot_id", req);
            if let Some(v) = req.get("port").and_then(|v| v.as_u64()) {
                wh.port = v as u16;
            }
            apply_str!(wh.path, "path", req);
            apply_str!(wh.secret, "secret", req);
        }
        "mattermost" => {
            let mm = &mut config.gateways.mattermost;
            apply_bool!(mm.enabled, "enabled", req);
            apply_str!(mm.bot_id, "bot_id", req);
            apply_str!(mm.base_url, "base_url", req);
            apply_str!(mm.token, "token", req);
        }
        "signal" => {
            let sg = &mut config.gateways.signal;
            apply_bool!(sg.enabled, "enabled", req);
            apply_str!(sg.bot_id, "bot_id", req);
        }
        "email" => {
            let em = &mut config.gateways.email;
            apply_bool!(em.enabled, "enabled", req);
            apply_str!(em.bot_id, "bot_id", req);
        }
        "whatsapp" => {
            let wa = &mut config.gateways.whatsapp;
            apply_bool!(wa.enabled, "enabled", req);
            apply_str!(wa.bot_id, "bot_id", req);
        }
        "matrix" => {
            let mx = &mut config.gateways.matrix;
            apply_bool!(mx.enabled, "enabled", req);
            apply_str!(mx.bot_id, "bot_id", req);
        }
        "homeassistant" => {
            let ha = &mut config.gateways.homeassistant;
            apply_bool!(ha.enabled, "enabled", req);
            apply_str!(ha.bot_id, "bot_id", req);
        }
        "bluebubbles" => {
            let bb = &mut config.gateways.bluebubbles;
            apply_bool!(bb.enabled, "enabled", req);
            apply_str!(bb.bot_id, "bot_id", req);
        }
        "sms" => {
            let sm = &mut config.gateways.sms;
            apply_bool!(sm.enabled, "enabled", req);
            apply_str!(sm.bot_id, "bot_id", req);
        }
        "msgraph" => {
            let mg = &mut config.gateways.msgraph_webhook;
            apply_bool!(mg.enabled, "enabled", req);
            apply_str!(mg.bot_id, "bot_id", req);
        }
        _ => {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                format!("Unknown gateway platform: {platform}"),
            ));
        }
    }

    save_config_to_disk(&config)?;

    Ok(Json(json!({
        "success": true,
        "message": format!("Gateway platform '{platform}' updated"),
        "restart_required": true
    })))
}

/// Build the axum Router with all API routes.
pub fn build_router(state: AppState) -> Router {
    // API routes that need authentication
    let mut api_routes = Router::new()
        .route("/chat", post(chat))
        .route("/chat/stream", post(chat_stream))
        .route("/sessions", get(list_sessions))
        .route("/sessions", post(create_session))
        .route("/sessions/search", get(search_sessions))
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}", patch(update_session))
        .route("/sessions/{id}", delete(delete_session))
        .route("/sessions/{id}/messages", get(get_session_messages))
        .route("/sessions/{id}/messages", delete(clear_session_messages))
        .route(
            "/sessions/{id}/messages/{message_id}",
            delete(delete_session_message),
        )
        .route("/sessions/{id}/fork", post(fork_session))
        .route("/tools", get(list_tools))
        .route("/config", get(get_config))
        .route("/config", post(update_config))
        .route("/status", get(dashboard_status))
        .route("/mcp/servers", get(list_mcp_servers))
        .route("/mcp/servers", post(add_mcp_server))
        .route("/mcp/servers/{name}", delete(delete_mcp_server))
        .route("/credentials/pool", get(list_credential_pools))
        .route("/credentials/pool", post(add_credential_pool_entry))
        .route(
            "/credentials/pool/{provider}/{index}",
            delete(delete_credential_pool_entry),
        )
        .route("/webhooks", get(list_webhooks))
        .route("/webhooks", post(update_webhook))
        .route("/kanban", get(kanban_dashboard))
        .route("/kanban/boards", get(kanban_boards))
        .route("/kanban/tasks", post(kanban_task_create))
        .route("/kanban/tasks/{id}", get(kanban_task_detail))
        .route("/kanban/tasks/{id}", patch(kanban_task_update))
        .route("/kanban/tasks/{id}/comments", post(kanban_task_comment))
        .route("/workspace/list", get(workspace_list))
        .route("/workspace/read", get(workspace_read))
        .route("/workspace/create", post(workspace_create))
        .route("/workspace/rename", post(workspace_rename))
        .route("/workspace/delete", post(workspace_delete))
        .route("/cron/jobs", get(cron_jobs))
        .route("/cron/jobs", post(cron_create_job))
        .route("/cron/jobs/{id}", delete(cron_delete_job))
        .route("/cron/jobs/{id}/pause", post(cron_pause_job))
        .route("/cron/jobs/{id}/resume", post(cron_resume_job))
        .route("/cron/jobs/{id}/run", post(cron_run_job_now))
        // Knowledge / Memory panel
        .route("/memory/stats", get(memory_stats))
        .route("/memory/search", get(memory_search))
        .route("/memory/entities", get(memory_entities))
        .route("/memory/entities/{id}", get(memory_entity_detail))
        // Gateway control panel
        .route("/gateway/status", get(gateway_status))
        .route("/gateway/config", get(gateway_config))
        .route("/gateway/config", patch(gateway_update_config))
        .route("/gateway/restart", post(gateway_restart))
        .route("/gateway/shutdown", post(gateway_shutdown))
        .route("/gateways/platforms", get(list_gateway_platforms))
        .route(
            "/gateways/platforms/{platform}",
            patch(update_gateway_platform),
        )
        // Agent-dimension (persona) endpoints
        .route("/agents", get(list_agents))
        .route("/agents", post(create_agent))
        .route("/agents/{id}", get(get_agent))
        .route("/agents/{id}", patch(update_agent))
        .route("/agents/{id}", delete(delete_agent))
        .route("/agents/{id}/chat", post(agent_chat))
        .route("/agents/{id}/chat/stream", post(agent_chat_stream))
        .route("/agents/{id}/skills", get(agent_skills))
        .route("/agents/{id}/memory", get(agent_memory))
        .route("/agents/{id}/sessions", get(agent_sessions))
        .route("/activity/snapshot", get(activity_snapshot))
        .route("/activity/stream", get(activity_stream))
        .route("/bindings", get(list_bindings));

    api_routes = api_routes.route_layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));

    // Health check can be unauthenticated
    let api_routes = api_routes.route("/health", get(health));

    let mut v1_routes = Router::new()
        .route("/models", get(models))
        .route("/capabilities", get(capabilities))
        .route("/skills", get(list_v1_skills))
        .route("/toolsets", get(list_v1_toolsets))
        .route("/chat/completions", post(chat_completions))
        .route("/responses", post(responses))
        .route("/responses/{id}", get(get_response))
        .route("/responses/{id}", delete(delete_response))
        .route("/runs", post(create_run))
        .route("/runs/{id}", get(get_run))
        .route("/runs/{id}/events", get(get_run_events))
        .route("/runs/{id}/stop", post(stop_run));
    v1_routes = v1_routes.route_layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));

    Router::new()
        .nest("/api", api_routes)
        .nest("/v1", v1_routes)
        .route("/", get(webui_index))
        .route("/index.html", get(webui_index))
        .route("/favicon.svg", get(webui_favicon))
        .route("/static/{*path}", get(webui_static_asset))
        .fallback(get(webui_index))
        .with_state(state)
}

// The WebUI is the React app in `hakimi-webui/`, built (via `npm run build`) into
// `crates/hakimi-webui/static/` with stable filenames (see vite.config.ts). The
// build output is committed so the server embeds it with no node step in CI.
const WEBUI_INDEX_HTML: &str = include_str!("../../hakimi-webui/static/index.html");
const WEBUI_APP_JS: &str = include_str!("../../hakimi-webui/static/app.js");
const WEBUI_APP_CSS: &str = include_str!("../../hakimi-webui/static/app.css");
const WEBUI_FAVICON_SVG: &str = include_str!("../../hakimi-webui/static/favicon.svg");
const WEBUI_ICONS_SVG: &str = include_str!("../../hakimi-webui/static/icons.svg");

fn static_webui_response(content_type: &'static str, body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}

async fn webui_index() -> Response {
    static_webui_response("text/html; charset=utf-8", WEBUI_INDEX_HTML)
}

async fn webui_favicon() -> Response {
    static_webui_response("image/svg+xml; charset=utf-8", WEBUI_FAVICON_SVG)
}

async fn webui_static_asset(Path(path): Path<String>) -> Response {
    match path.as_str() {
        "app.js" => static_webui_response("text/javascript; charset=utf-8", WEBUI_APP_JS),
        "app.css" => static_webui_response("text/css; charset=utf-8", WEBUI_APP_CSS),
        "favicon.svg" => static_webui_response("image/svg+xml; charset=utf-8", WEBUI_FAVICON_SVG),
        "icons.svg" => static_webui_response("image/svg+xml; charset=utf-8", WEBUI_ICONS_SVG),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
// Handlers
// ---------------------------------------------------------------------------

/// GET /health — simple health check.
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// GET /v1/models — OpenAI-compatible model discovery.
async fn models(State(state): State<AppState>) -> Json<ModelsResponse> {
    let agent = state.agent.lock().await;
    let model = agent.model().trim();
    let model = if model.is_empty() {
        "hakimi-agent"
    } else {
        model
    };

    Json(ModelsResponse {
        object: "list".to_string(),
        data: vec![ModelInfo {
            id: model.to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "hakimi".to_string(),
            permission: Vec::new(),
            root: model.to_string(),
            parent: None,
        }],
    })
}

/// GET /v1/capabilities — advertise the stable HTTP API surface.
async fn capabilities(State(state): State<AppState>) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let model = agent.model().trim();
    let model = if model.is_empty() {
        "hakimi-agent"
    } else {
        model
    };

    let features = json!({
        "chat": true,
        "chat_completions": true,
        "chat_completions_streaming": true,
        "chat_completions_streaming_mode": "completed_sse_snapshot",
        "responses_api": true,
        "responses_streaming": true,
        "responses_streaming_mode": "completed_sse_snapshot",
        "responses_persistence": "sqlite_lru",
        "skills_api": true,
        "toolsets_api": true,
        "session_resources": true,
        "session_create": true,
        "session_update": true,
        "session_delete": true,
        "session_fork": true,
        "session_chat": false,
        "session_chat_streaming": false,
        "session_messages": true,
        "session_search": true,
        "tools_api": true,
        "config_read": true,
        "config_write": true,
        "run_submission": true,
        "run_status": true,
        "run_events_sse": true,
        "run_events_streaming_mode": "live_lifecycle_sse",
        "run_stop": true,
        "websocket_streaming": false,
        "media_api": false
    });
    let dashboard_admin = json!({
        "status": true,
        "mcp_servers_read": true,
        "mcp_servers_write": true,
        "credential_pools_read": true,
        "credential_pools_write": true,
        "webhooks_read": true,
        "webhooks_write": true,
        "kanban_read": true,
        "kanban_write": true,
        "write_operations": true,
        "persistence": "runtime"
    });

    Json(json!({
        "object": "hakimi.api_server.capabilities",
        "platform": "hakimi-agent",
        "model": model,
        "auth": {
            "type": "bearer",
            "required": auth_required()
        },
        "runtime": {
            "mode": "server_agent",
            "tool_execution": "server",
            "split_runtime": false,
            "description": "The HTTP API server runs a server-side Hakimi AIAgent; tools execute on the API-server host."
        },
        "features": features,
        "dashboard_admin": dashboard_admin,
        "endpoints": capability_endpoints()
    }))
}

fn capability_endpoints() -> BTreeMap<&'static str, JsonValue> {
    [
        ("health", "GET", "/api/health"),
        ("models", "GET", "/v1/models"),
        ("capabilities", "GET", "/v1/capabilities"),
        ("skills", "GET", "/v1/skills"),
        ("toolsets", "GET", "/v1/toolsets"),
        ("chat_completions", "POST", "/v1/chat/completions"),
        ("responses", "POST", "/v1/responses"),
        ("response", "GET", "/v1/responses/{id}"),
        ("response_delete", "DELETE", "/v1/responses/{id}"),
        ("run", "POST", "/v1/runs"),
        ("run_status", "GET", "/v1/runs/{id}"),
        ("run_events", "GET", "/v1/runs/{id}/events"),
        ("run_stop", "POST", "/v1/runs/{id}/stop"),
        ("chat", "POST", "/api/chat"),
        ("sessions", "GET", "/api/sessions"),
        ("session_create", "POST", "/api/sessions"),
        ("session", "GET", "/api/sessions/{id}"),
        ("session_update", "PATCH", "/api/sessions/{id}"),
        ("session_delete", "DELETE", "/api/sessions/{id}"),
        ("session_messages", "GET", "/api/sessions/{id}/messages"),
        ("session_fork", "POST", "/api/sessions/{id}/fork"),
        ("session_search", "GET", "/api/sessions/search?q=<query>"),
        ("tools", "GET", "/api/tools"),
        ("config", "GET", "/api/config"),
        ("config_update", "POST", "/api/config"),
        ("dashboard_status", "GET", "/api/status"),
        ("mcp_servers", "GET", "/api/mcp/servers"),
        ("mcp_server_add", "POST", "/api/mcp/servers"),
        ("mcp_server_delete", "DELETE", "/api/mcp/servers/{name}"),
        ("credential_pool", "GET", "/api/credentials/pool"),
        ("credential_pool_add", "POST", "/api/credentials/pool"),
        (
            "credential_pool_delete",
            "DELETE",
            "/api/credentials/pool/{provider}/{index}",
        ),
        ("webhooks", "GET", "/api/webhooks"),
        ("webhook_update", "POST", "/api/webhooks"),
        ("kanban", "GET", "/api/kanban"),
        ("kanban_boards", "GET", "/api/kanban/boards"),
        ("kanban_task", "GET", "/api/kanban/tasks/{id}"),
        ("kanban_task_create", "POST", "/api/kanban/tasks"),
        ("kanban_task_update", "PATCH", "/api/kanban/tasks/{id}"),
        (
            "kanban_task_comment",
            "POST",
            "/api/kanban/tasks/{id}/comments",
        ),
    ]
    .into_iter()
    .map(|(name, method, path)| (name, api_endpoint(method, path)))
    .collect()
}

fn api_endpoint(method: &'static str, path: &'static str) -> JsonValue {
    json!({
        "method": method,
        "path": path
    })
}

fn auth_required() -> bool {
    !std::env::var("HAKIMI_WEBUI_PASSWORD")
        .unwrap_or_default()
        .is_empty()
}

fn bounded_limit(limit: Option<usize>, default: usize, max: usize) -> usize {
    limit.unwrap_or(default).clamp(1, max)
}

fn generated_api_session_id() -> String {
    format!("api_{}", run_id().trim_start_matches("run_"))
}

fn validate_api_session_id(id: &str) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let id = id.trim();
    if id.is_empty() || id.chars().any(|ch| matches!(ch, '\r' | '\n' | '\0')) {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid session id"));
    }
    if id.chars().count() > 256 {
        return Err(api_error(StatusCode::BAD_REQUEST, "session id too long"));
    }
    Ok(())
}

fn requested_session_id(primary: Option<&str>, fallback: Option<&str>) -> String {
    primary
        .or(fallback)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(generated_api_session_id)
}

fn request_bool(value: Option<&JsonValue>, default: bool) -> bool {
    match value {
        Some(JsonValue::Bool(value)) => *value,
        Some(JsonValue::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Some(JsonValue::Number(value)) => value.as_i64().is_some_and(|n| n != 0),
        Some(JsonValue::Null) | None => default,
        Some(_) => default,
    }
}

fn chat_completions_prompt(
    messages: &[ChatCompletionsMessage],
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    conversation_prompt(messages, "OpenAI Chat Completions")
}

fn responses_prompt(
    messages: &[ChatCompletionsMessage],
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    conversation_prompt(messages, "OpenAI Responses API")
}

fn run_prompt(
    messages: &[ChatCompletionsMessage],
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    conversation_prompt(messages, "Hakimi Runs API")
}

fn conversation_prompt(
    messages: &[ChatCompletionsMessage],
    surface: &str,
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    if messages.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "messages must contain at least one message",
        ));
    }

    let mut rendered = Vec::new();
    let mut saw_user = false;
    for message in messages {
        let role = normalized_chat_role(&message.role)?;
        let content = chat_content_text(&message.content)?;
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        if role == "user" {
            saw_user = true;
        }
        rendered.push(format!("{}: {}", chat_role_label(role), content));
    }

    if !saw_user {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "messages must include at least one user message",
        ));
    }
    if rendered.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "messages must include non-empty text content",
        ));
    }

    Ok(truncate_chars(
        &format!(
            "Conversation supplied through {surface}:\n\n{}\n\nRespond to the final user message.",
            rendered.join("\n\n")
        ),
        65_536,
    ))
}

fn responses_input_messages(
    input: &JsonValue,
) -> Result<Vec<ChatCompletionsMessage>, (StatusCode, Json<ErrorResponse>)> {
    let messages = match input {
        JsonValue::Null => Vec::new(),
        JsonValue::String(text) => vec![ChatCompletionsMessage {
            role: "user".to_string(),
            content: JsonValue::String(text.clone()),
        }],
        JsonValue::Array(items) => {
            let mut messages = Vec::new();
            for item in items {
                messages.push(response_input_item_message(item)?);
            }
            messages
        }
        JsonValue::Object(_) => vec![response_input_item_message(input)?],
        other => vec![ChatCompletionsMessage {
            role: "user".to_string(),
            content: JsonValue::String(other.to_string()),
        }],
    };

    if messages.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "input must contain at least one message",
        ));
    }

    if !messages
        .iter()
        .any(|message| normalized_chat_role(&message.role).is_ok_and(|role| role == "user"))
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "input must include at least one user message",
        ));
    }

    Ok(messages)
}

fn run_input_messages(
    req: &RunCreateRequest,
) -> Result<Vec<ChatCompletionsMessage>, (StatusCode, Json<ErrorResponse>)> {
    let mut messages = Vec::new();
    if let Some(instructions) = req
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        messages.push(ChatCompletionsMessage {
            role: "system".to_string(),
            content: JsonValue::String(instructions.to_string()),
        });
    }

    messages.extend(req.messages.clone());
    if let Some(input) = &req.input {
        messages.extend(responses_input_messages(input)?);
    }

    if messages.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "runs require input or messages",
        ));
    }

    if !messages
        .iter()
        .any(|message| normalized_chat_role(&message.role).is_ok_and(|role| role == "user"))
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "runs require at least one user message",
        ));
    }

    Ok(messages)
}

fn response_input_item_message(
    item: &JsonValue,
) -> Result<ChatCompletionsMessage, (StatusCode, Json<ErrorResponse>)> {
    match item {
        JsonValue::String(text) => Ok(ChatCompletionsMessage {
            role: "user".to_string(),
            content: JsonValue::String(text.clone()),
        }),
        JsonValue::Object(map) => {
            if let Some(role) = map.get("role").and_then(JsonValue::as_str) {
                normalized_chat_role(role)?;
                Ok(ChatCompletionsMessage {
                    role: role.to_string(),
                    content: map.get("content").cloned().unwrap_or(JsonValue::Null),
                })
            } else {
                Ok(ChatCompletionsMessage {
                    role: "user".to_string(),
                    content: item.clone(),
                })
            }
        }
        other => Ok(ChatCompletionsMessage {
            role: "user".to_string(),
            content: JsonValue::String(other.to_string()),
        }),
    }
}

fn response_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("resp_{nanos}")
}

fn response_message_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("msg_{nanos}")
}

fn response_text_chunks(value: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut count = 0;

    for ch in value.chars() {
        current.push(ch);
        count += 1;
        if count >= max_chars {
            chunks.push(std::mem::take(&mut current));
            count = 0;
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn sse_event(event: &str, data: JsonValue) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

fn sse_data(data: JsonValue) -> String {
    format!("data: {data}\n\n")
}

fn chat_completion_sse_body(completion: &JsonValue) -> String {
    let completion_id = completion
        .get("id")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let created = completion
        .get("created")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let model = completion
        .get("model")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let content = completion
        .get("choices")
        .and_then(JsonValue::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();

    let mut body = String::new();
    body.push_str(&sse_data(json!({
        "id": completion_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant"},
            "finish_reason": null
        }]
    })));

    for chunk in response_text_chunks(content, 2048) {
        body.push_str(&sse_data(json!({
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"content": chunk},
                "finish_reason": null
            }]
        })));
    }

    body.push_str(&sse_data(json!({
        "id": completion_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    })));
    body.push_str("data: [DONE]\n\n");
    body
}

fn chat_completion_sse_response(completion: &JsonValue) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        chat_completion_sse_body(completion),
    )
        .into_response()
}

fn responses_sse_body(response: &JsonValue) -> String {
    let response_id = response
        .get("id")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let created_at = response
        .get("created_at")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let model = response
        .get("model")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let output_text = response
        .get("output_text")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let item_id = response
        .get("output")
        .and_then(JsonValue::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("id"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();

    let mut body = String::new();
    body.push_str(&sse_event(
        "response.created",
        json!({
            "type": "response.created",
            "response": {
                "id": response_id,
                "object": "response",
                "created_at": created_at,
                "status": "in_progress",
                "model": model
            }
        }),
    ));

    for chunk in response_text_chunks(output_text, 2048) {
        body.push_str(&sse_event(
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "response_id": response_id,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "delta": chunk
            }),
        ));
    }

    body.push_str(&sse_event(
        "response.completed",
        json!({
            "type": "response.completed",
            "response": response
        }),
    ));
    body.push_str("data: [DONE]\n\n");
    body
}

fn responses_sse_response(response: &JsonValue) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        responses_sse_body(response),
    )
        .into_response()
}

fn run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("run_{nanos}")
}

fn normalized_chat_role(role: &str) -> Result<&'static str, (StatusCode, Json<ErrorResponse>)> {
    match role.trim().to_ascii_lowercase().as_str() {
        "system" | "developer" => Ok("system"),
        "user" => Ok("user"),
        "assistant" => Ok("assistant"),
        "tool" | "function" => Ok("tool"),
        other => Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("unsupported chat message role: {other}"),
        )),
    }
}

fn chat_role_label(role: &str) -> &'static str {
    match role {
        "system" => "System",
        "assistant" => "Assistant",
        "tool" => "Tool",
        _ => "User",
    }
}

fn chat_content_text(value: &JsonValue) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    match value {
        JsonValue::Null => Ok(String::new()),
        JsonValue::String(text) => Ok(truncate_chars(text, 65_536)),
        JsonValue::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                let text = chat_content_part_text(part)?;
                if !text.trim().is_empty() {
                    out.push(text);
                }
            }
            Ok(truncate_chars(&out.join("\n"), 65_536))
        }
        JsonValue::Object(_) => chat_content_part_text(value),
        other => Ok(truncate_chars(&other.to_string(), 65_536)),
    }
}

fn chat_content_part_text(value: &JsonValue) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    match value {
        JsonValue::String(text) => Ok(text.clone()),
        JsonValue::Object(map) => {
            let part_type = map
                .get("type")
                .and_then(JsonValue::as_str)
                .unwrap_or("text")
                .trim()
                .to_ascii_lowercase();
            match part_type.as_str() {
                "text" | "input_text" | "output_text" => Ok(map
                    .get("text")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_string()),
                "image_url" | "input_image" | "file" | "input_file" => Err(api_error(
                    StatusCode::BAD_REQUEST,
                    format!(
                        "unsupported chat content part type: {part_type}; this endpoint currently accepts text-only chat completions"
                    ),
                )),
                other => Err(api_error(
                    StatusCode::BAD_REQUEST,
                    format!("unsupported chat content part type: {other}"),
                )),
            }
        }
        JsonValue::Null => Ok(String::new()),
        other => Ok(other.to_string()),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn unix_timestamp_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn chat_completion_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("chatcmpl-{nanos}")
}

fn redacted_env(env: &std::collections::HashMap<String, String>) -> BTreeMap<String, String> {
    env.iter()
        .map(|(key, value)| {
            let rendered = if value.trim().is_empty() {
                String::new()
            } else {
                "<redacted>".to_string()
            };
            (key.clone(), rendered)
        })
        .collect()
}

fn configured(value: &str) -> bool {
    !value.trim().is_empty()
}

fn non_empty(
    value: impl Into<String>,
    field: &str,
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    let value = value.into().trim().to_string();
    if value.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("{field} must not be empty"),
        ));
    }
    Ok(value)
}

fn valid_name(
    value: impl Into<String>,
    field: &str,
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    let value = non_empty(value, field)?;
    let ok = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    if !ok {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("{field} may only contain ASCII letters, numbers, '.', '_' and '-'"),
        ));
    }
    Ok(value)
}

fn api_error(status: StatusCode, error: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
        }),
    )
}

fn kanban_dashboard_error(err: hakimi_common::HakimiError) -> (StatusCode, Json<ErrorResponse>) {
    let message = err.to_string();
    let status = if message.contains("not found") {
        StatusCode::NOT_FOUND
    } else if message.contains("invalid")
        || message.contains("required")
        || message.contains("status")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    api_error(status, message)
}

fn mcp_server_summary(name: &str, server: &hakimi_config::McpServerConfig) -> serde_json::Value {
    json!({
        "name": name,
        "transport": "stdio",
        "command": server.command.clone(),
        "args_count": server.args.len(),
        "env_count": server.env.len(),
        "env": redacted_env(&server.env)
    })
}

fn credential_pool_summary(
    provider: &str,
    pool: &hakimi_config::CredentialPoolConfig,
) -> serde_json::Value {
    let entries: Vec<_> = pool
        .credentials
        .iter()
        .enumerate()
        .map(|(idx, cred)| {
            json!({
                "index": idx + 1,
                "id": cred
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("{provider}-cred-{idx}")),
                "has_api_key": configured(&cred.api_key),
                "base_url_configured": cred.base_url.as_deref().map(configured).unwrap_or(false),
                "org_id_configured": cred.org_id.as_deref().map(configured).unwrap_or(false),
                "source": cred.source.clone(),
                "priority": cred.priority.unwrap_or(0),
                "max_concurrent": cred.max_concurrent.unwrap_or(10)
            })
        })
        .collect();

    json!({
        "provider": provider,
        "strategy": pool.strategy.as_deref().unwrap_or("round_robin"),
        "count": entries.len(),
        "entries": entries
    })
}

fn webhook_summary(webhook: &hakimi_config::WebhookGatewayConfig) -> serde_json::Value {
    json!({
        "object": "hakimi.dashboard.webhooks",
        "enabled": webhook.enabled,
        "bot_id": webhook.bot_id.clone(),
        "port": webhook.port,
        "path": webhook.path.clone(),
        "secret_configured": configured(&webhook.secret),
        "routes": [],
        "secrets_redacted": true,
        "write_operations": true,
        "persistence": "runtime"
    })
}

/// Response body for GET /api/activity/snapshot.
#[derive(Debug, Serialize)]
struct ActivitySnapshotResponse {
    personas: Vec<hakimi_common::PersonaActivity>,
}

/// GET /api/activity/snapshot — current activity row per registered persona
/// (registry identity joined with the live activity overlay).
async fn activity_snapshot(State(state): State<AppState>) -> Json<ActivitySnapshotResponse> {
    let states = hakimi_common::all_live_states();
    let reg = state.persona_registry.read().await;
    let personas = reg
        .list()
        .into_iter()
        .map(|cfg| {
            hakimi_common::PersonaActivity::from_parts(
                &cfg.id,
                &cfg.name,
                &cfg.avatar,
                states.get(&cfg.id),
            )
        })
        .collect();
    Json(ActivitySnapshotResponse { personas })
}

/// GET /api/activity/stream — SSE of live ActivityEvents.
async fn activity_stream() -> Response {
    use tokio::sync::broadcast::error::RecvError;
    let rx = hakimi_common::subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
                    return Some((
                        Ok::<Event, Infallible>(Event::default().event("activity").data(data)),
                        rx,
                    ));
                }
                Err(RecvError::Lagged(_)) => continue, // slow consumer dropped events; resync on reconnect
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// GET /status — dashboard runtime status without secrets.
async fn dashboard_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let model = agent.model().trim().to_string();
    let model = if model.is_empty() {
        "hakimi-agent".to_string()
    } else {
        model
    };
    let tool_count = agent.tool_registry().get_definitions().await.len();
    drop(agent);

    let session_count = {
        use hakimi_session::SessionOps;
        let db = state.session_db.lock().await;
        db.get_recent_sessions(None, 50)
            .map(|sessions| sessions.len())
            .unwrap_or_default()
    };

    let config = state.config.lock().await;
    Json(json!({
        "object": "hakimi.dashboard.status",
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "model": model,
        "auth": {
            "type": "bearer",
            "required": auth_required()
        },
        "runtime": {
            "mode": "server_agent",
            "tool_execution": "server"
        },
        "resources": {
            "sessions_sampled": session_count,
            "tools": tool_count,
            "mcp_servers": config.mcp_servers.len(),
            "credential_providers": config.credential_pools.len(),
            "webhook_enabled": config.gateways.webhook.enabled
        },
        "dashboard_admin": {
            "readonly": false,
            "write_operations": true,
            "persistence": "runtime",
            "mcp_servers": "/api/mcp/servers",
            "credential_pool": "/api/credentials/pool",
            "webhooks": "/api/webhooks",
            "kanban": "/api/kanban"
        }
    }))
}

/// GET /mcp/servers — dashboard-safe MCP server summaries.
async fn list_mcp_servers(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.lock().await;
    let mut servers: Vec<_> = config
        .mcp_servers
        .iter()
        .map(|(name, server)| mcp_server_summary(name, server))
        .collect();
    servers.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["name"].as_str().unwrap_or_default())
    });
    let count = servers.len();

    Json(json!({
        "object": "hakimi.dashboard.mcp_servers",
        "servers": servers,
        "count": count,
        "secrets_redacted": true,
        "write_operations": true,
        "persistence": "runtime"
    }))
}

/// POST /mcp/servers — add an in-memory stdio MCP server config.
async fn add_mcp_server(
    State(state): State<AppState>,
    Json(req): Json<McpServerCreate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if req.url.as_deref().map(configured).unwrap_or(false) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "dashboard MCP writes currently support stdio command/args/env only",
        ));
    }

    let name = valid_name(req.name, "name")?;
    let command = non_empty(req.command.unwrap_or_default(), "command")?;
    let server = hakimi_config::McpServerConfig {
        command,
        args: req.args,
        env: req.env,
    };

    let mut config = state.config.lock().await;
    let summary = match config.mcp_servers.entry(name.clone()) {
        std::collections::hash_map::Entry::Occupied(_) => {
            return Err(api_error(
                StatusCode::CONFLICT,
                format!("MCP server already exists: {name}"),
            ));
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            mcp_server_summary(&name, entry.insert(server))
        }
    };

    Ok(Json(json!({
        "object": "hakimi.dashboard.mcp_server",
        "server": summary,
        "secrets_redacted": true,
        "persistence": "runtime"
    })))
}

/// DELETE /mcp/servers/:name — remove an in-memory MCP server config.
async fn delete_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let name = valid_name(name, "name")?;
    let mut config = state.config.lock().await;
    if config.mcp_servers.remove(&name).is_none() {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("MCP server not found: {name}"),
        ));
    }

    Ok(Json(json!({
        "ok": true,
        "removed": name,
        "persistence": "runtime"
    })))
}

/// GET /credentials/pool — dashboard-safe credential pool summaries.
async fn list_credential_pools(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.lock().await;
    let mut providers: Vec<_> = config
        .credential_pools
        .iter()
        .map(|(provider, pool)| credential_pool_summary(provider, pool))
        .collect();
    providers.sort_by(|a, b| {
        a["provider"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["provider"].as_str().unwrap_or_default())
    });
    let count = providers.len();

    Json(json!({
        "object": "hakimi.dashboard.credential_pool",
        "providers": providers,
        "count": count,
        "secrets_redacted": true,
        "write_operations": true,
        "persistence": "runtime"
    }))
}

/// POST /credentials/pool — add an in-memory credential-pool entry.
async fn add_credential_pool_entry(
    State(state): State<AppState>,
    Json(req): Json<CredentialPoolEntryCreate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let provider = valid_name(req.provider, "provider")?;
    let api_key = non_empty(req.api_key, "api_key")?;
    let id = req
        .id
        .or(req.label)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let mut config = state.config.lock().await;
    let pool = config.credential_pools.entry(provider.clone()).or_default();

    if let Some(strategy) = req.strategy {
        let strategy = strategy.trim();
        if !strategy.is_empty() {
            pool.strategy = Some(strategy.to_string());
        }
    }

    pool.credentials.push(hakimi_config::CredentialConfig {
        id,
        api_key,
        base_url: req.base_url.filter(|value| configured(value)),
        org_id: req.org_id.filter(|value| configured(value)),
        source: Some(
            req.source
                .filter(|value| configured(value))
                .unwrap_or_else(|| "dashboard:runtime".to_string()),
        ),
        priority: req.priority,
        max_concurrent: req.max_concurrent,
    });
    let summary = credential_pool_summary(&provider, pool);

    Ok(Json(json!({
        "object": "hakimi.dashboard.credential_pool.provider",
        "provider": summary,
        "secrets_redacted": true,
        "persistence": "runtime"
    })))
}

/// DELETE /credentials/pool/:provider/:index — remove an in-memory credential.
async fn delete_credential_pool_entry(
    State(state): State<AppState>,
    Path((provider, index)): Path<(String, usize)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let provider = valid_name(provider, "provider")?;
    if index == 0 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "index is 1-based and must be greater than zero",
        ));
    }

    let mut config = state.config.lock().await;
    let Some(pool) = config.credential_pools.get_mut(&provider) else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("credential provider not found: {provider}"),
        ));
    };
    if index > pool.credentials.len() {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("credential index not found: {provider}/{index}"),
        ));
    }
    pool.credentials.remove(index - 1);
    let remaining = pool.credentials.len();
    if remaining == 0 {
        config.credential_pools.remove(&provider);
    }

    Ok(Json(json!({
        "ok": true,
        "provider": provider,
        "removed_index": index,
        "remaining": remaining,
        "persistence": "runtime"
    })))
}

/// GET /webhooks — dashboard-safe webhook gateway summary.
async fn list_webhooks(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.lock().await;
    Json(webhook_summary(&config.gateways.webhook))
}

/// POST /webhooks — update the in-memory webhook gateway config.
async fn update_webhook(
    State(state): State<AppState>,
    Json(req): Json<WebhookUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let mut config = state.config.lock().await;
    let webhook = &mut config.gateways.webhook;

    if let Some(enabled) = req.enabled {
        webhook.enabled = enabled;
    }
    if let Some(bot_id) = req.bot_id {
        webhook.bot_id = valid_name(bot_id, "bot_id")?;
    }
    if let Some(port) = req.port {
        if port == 0 {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "port must be greater than zero",
            ));
        }
        webhook.port = port;
    }
    if let Some(path) = req.path {
        let path = non_empty(path, "path")?;
        if !path.starts_with('/') {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "path must start with '/'",
            ));
        }
        webhook.path = path;
    }
    if let Some(secret) = req.secret {
        webhook.secret = secret;
    }

    Ok(Json(webhook_summary(webhook)))
}

/// GET /kanban — dashboard-safe Kanban task snapshot.
async fn kanban_dashboard(
    Query(query): Query<KanbanDashboardQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    hakimi_tools::kanban_dashboard_snapshot(
        query.board.as_deref(),
        query.status.as_deref(),
        query.assignee.as_deref(),
        bounded_limit(query.limit, 50, 200),
    )
    .map(Json)
    .map_err(kanban_dashboard_error)
}

/// GET /kanban/boards — dashboard-safe Kanban board inventory.
async fn kanban_boards() -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    hakimi_tools::kanban_dashboard_boards()
        .map(Json)
        .map_err(kanban_dashboard_error)
}

/// GET /kanban/tasks/:id — dashboard-safe Kanban task detail.
async fn kanban_task_detail(
    Path(id): Path<String>,
    Query(query): Query<KanbanTaskDashboardQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    hakimi_tools::kanban_dashboard_task(
        query.board.as_deref(),
        id.trim(),
        bounded_limit(query.event_limit, 50, 200),
    )
    .map(Json)
    .map_err(kanban_dashboard_error)
}

/// POST /kanban/tasks — dashboard-safe Kanban task creation.
async fn kanban_task_create(
    Query(query): Query<KanbanTaskDashboardQuery>,
    Json(req): Json<hakimi_tools::KanbanDashboardTaskCreate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    hakimi_tools::kanban_dashboard_create_task(
        query.board.as_deref(),
        req,
        bounded_limit(query.event_limit, 50, 200),
    )
    .map(Json)
    .map_err(kanban_dashboard_error)
}

/// PATCH /kanban/tasks/:id — dashboard-safe Kanban task status/assignee update.
async fn kanban_task_update(
    Path(id): Path<String>,
    Query(query): Query<KanbanTaskDashboardQuery>,
    Json(req): Json<hakimi_tools::KanbanDashboardTaskUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    hakimi_tools::kanban_dashboard_update_task(
        query.board.as_deref(),
        id.trim(),
        req,
        bounded_limit(query.event_limit, 50, 200),
    )
    .map(Json)
    .map_err(kanban_dashboard_error)
}

/// POST /kanban/tasks/:id/comments — dashboard-safe Kanban task comment append.
async fn kanban_task_comment(
    Path(id): Path<String>,
    Query(query): Query<KanbanTaskDashboardQuery>,
    Json(req): Json<hakimi_tools::KanbanDashboardCommentCreate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    hakimi_tools::kanban_dashboard_add_comment(
        query.board.as_deref(),
        id.trim(),
        req,
        bounded_limit(query.event_limit, 50, 200),
    )
    .map(Json)
    .map_err(kanban_dashboard_error)
}

/// POST /chat — send a message to the agent and get a response.
async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!(message_len = req.message.len(), "POST /chat");

    let mut agent = state.agent.lock().await;
    let session_id = agent.session_id().to_string();

    match agent.chat(&req.message).await {
        Ok(response) => Ok(Json(ChatResponse {
            response,
            session_id,
        })),
        Err(e) => {
            let msg = format!("Agent error: {e}");
            tracing::error!("{msg}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: msg }),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Agent-dimension (persona) endpoints
// ---------------------------------------------------------------------------

/// Build an isolated agent for a named persona from the shared template
/// (`AppState::agent`), reading its skills from `skills_dir`. Used by the chat
/// endpoint and by the CRUD handlers to keep `AppState::persona_agents` in sync.
async fn build_persona_agent_for(
    state: &AppState,
    cfg: &hakimi_core::PersonaConfig,
    skills_dir: &std::path::Path,
) -> hakimi_core::AIAgent {
    let template = state.agent.lock().await.clone();
    let context_length = {
        let config = state.config.lock().await;
        let model = if cfg.model.trim().is_empty() {
            template.model()
        } else {
            cfg.model.as_str()
        };
        hakimi_common::resolve_model_context_length(
            model,
            Some(config.model.context_length).filter(|length| *length > 0),
            config.compression.context_length,
        )
        .context_length
    };
    let base_agent = hakimi_core::build_persona_agent(&template, cfg, skills_dir, context_length);

    // Wrap with dispatch (inherit dispatch config from template)
    let _model_config = {
        let config = state.config.lock().await;
        config.model.clone()
    };

    // TODO: Wrap with DispatchedAgent when ModelDispatcher is implemented.
    // For now, return base agent directly.
    base_agent
}

/// Insert/replace a named persona's gateway agent so routing reflects CRUD
/// without a restart. The default persona is never stored (it uses the legacy
/// shared agent).
async fn sync_gateway_persona_agent(
    state: &AppState,
    cfg: &hakimi_core::PersonaConfig,
    skills_dir: &std::path::Path,
) {
    if cfg.id == hakimi_core::DEFAULT_PERSONA_ID {
        return;
    }
    let agent = build_persona_agent_for(state, cfg, skills_dir).await;
    state.persona_agents.write().await.insert(
        cfg.id.clone(),
        std::sync::Arc::new(tokio::sync::Mutex::new(agent)),
    );
}

/// GET /api/agents — list personas with the default persona id.
async fn list_agents(State(state): State<AppState>) -> Json<AgentsListResponse> {
    let reg = state.persona_registry.read().await;
    let agents = reg.list().into_iter().cloned().collect();
    Json(AgentsListResponse {
        agents,
        default: reg.default_id().to_string(),
    })
}

/// POST /api/agents — create a persona from the posted config.
async fn create_agent(
    State(state): State<AppState>,
    Json(cfg): Json<hakimi_core::PersonaConfig>,
) -> Result<Json<hakimi_core::PersonaConfig>, (StatusCode, Json<ErrorResponse>)> {
    let id = cfg.id.clone();
    let (created, skills_dir) = {
        let mut reg = state.persona_registry.write().await;
        reg.create(cfg).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
        let created = reg.get(&id).cloned().expect("persona present after create");
        let skills_dir = reg.agents_dir().join(&id).join("skills");
        (created, skills_dir)
    };
    sync_gateway_persona_agent(&state, &created, &skills_dir).await;
    hakimi_common::publish(hakimi_common::ActivityEvent::PersonaCreated {
        id: created.id.clone(),
        name: created.name.clone(),
        avatar: created.avatar.clone(),
    });
    Ok(Json(created))
}

/// GET /api/agents/{id} — fetch a persona config.
async fn get_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<hakimi_core::PersonaConfig>, (StatusCode, Json<ErrorResponse>)> {
    let reg = state.persona_registry.read().await;
    match reg.get(&id) {
        Some(cfg) => Ok(Json(cfg.clone())),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("persona '{id}' not found"),
            }),
        )),
    }
}

/// PATCH /api/agents/{id} — merge provided fields and persist.
async fn update_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AgentUpdateRequest>,
) -> Result<Json<hakimi_core::PersonaConfig>, (StatusCode, Json<ErrorResponse>)> {
    let (updated, skills_dir) = {
        let mut reg = state.persona_registry.write().await;
        let mut cfg = match reg.get(&id) {
            Some(cfg) => cfg.clone(),
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("persona '{id}' not found"),
                    }),
                ));
            }
        };

        if let Some(name) = req.name {
            cfg.name = name;
        }
        if let Some(avatar) = req.avatar {
            cfg.avatar = avatar;
        }
        if let Some(description) = req.description {
            cfg.description = description;
        }
        if let Some(model) = req.model {
            cfg.model = model;
        }
        if let Some(effort) = req.reasoning_effort {
            cfg.reasoning_effort = if effort.trim().is_empty() {
                None
            } else {
                Some(effort)
            };
        }
        if let Some(system_prompt) = req.system_prompt {
            cfg.system_prompt = system_prompt;
        }
        if let Some(enabled_skills) = req.enabled_skills {
            cfg.enabled_skills = enabled_skills;
        }
        if let Some(bindings) = req.bindings {
            cfg.bindings = bindings;
        }
        if let Some(is_default) = req.is_default {
            cfg.is_default = is_default;
        }
        if let Some(addressable) = req.addressable {
            cfg.addressable = addressable;
        }

        reg.update(cfg).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
        let updated = reg.get(&id).cloned().expect("persona present after update");
        let skills_dir = reg.agents_dir().join(&id).join("skills");
        (updated, skills_dir)
    };
    // Rebuild the persona's gateway agent so model/prompt/skills changes take
    // effect without a restart (in unified mode the loop shares this map).
    sync_gateway_persona_agent(&state, &updated, &skills_dir).await;
    hakimi_common::publish(hakimi_common::ActivityEvent::PersonaUpdated {
        id: updated.id.clone(),
        name: updated.name.clone(),
        avatar: updated.avatar.clone(),
    });
    Ok(Json(updated))
}

/// DELETE /api/agents/{id} — remove a persona (default persona cannot be removed).
async fn delete_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AgentDeleteResponse>, (StatusCode, Json<ErrorResponse>)> {
    {
        let mut reg = state.persona_registry.write().await;
        reg.delete(&id).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    }
    state.persona_agents.write().await.remove(&id);
    hakimi_common::publish(hakimi_common::ActivityEvent::PersonaDeleted { id: id.clone() });
    Ok(Json(AgentDeleteResponse { id, deleted: true }))
}

/// POST /api/agents/{id}/chat — chat with a specific persona (non-streaming).
async fn agent_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Fetch the persona config + the registry's backing dir under a read lock.
    let (cfg, agents_dir) = {
        let reg = state.persona_registry.read().await;
        match reg.get(&id) {
            Some(cfg) => (cfg.clone(), reg.agents_dir().to_path_buf()),
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("persona '{id}' not found"),
                    }),
                ));
            }
        }
    };

    // Build the agent for this persona. The default persona reuses the shared
    // template directly (full default behavior); named personas get an isolated
    // agent (own model/prompt/context/skills), mirroring the gateway split.
    let mut persona_agent = if id == hakimi_core::DEFAULT_PERSONA_ID {
        state.agent.lock().await.clone()
    } else {
        let skills_dir = agents_dir.join(&id).join("skills");
        build_persona_agent_for(&state, &cfg, &skills_dir).await
    };

    let session_id = persona_agent.session_id().to_string();
    match persona_agent.chat(&req.message).await {
        Ok(response) => Ok(Json(ChatResponse {
            response,
            session_id,
        })),
        Err(e) => {
            let msg = format!("Agent error: {e}");
            tracing::error!("{msg}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: msg }),
            ))
        }
    }
}

/// GET /api/bindings — channel-binding overview plus the default persona.
async fn list_bindings(State(state): State<AppState>) -> Json<BindingsResponse> {
    let reg = state.persona_registry.read().await;
    let bindings = reg
        .bindings()
        .iter()
        .map(|(channel, persona)| (channel.clone(), persona.clone()))
        .collect();
    Json(BindingsResponse {
        bindings,
        default: reg.default_id().to_string(),
    })
}

/// GET /api/agents/{id}/skills — skills available to a persona plus its enabled set.
/// The default persona reads the instance skill store; named personas read
/// `agents/{id}/skills`.
async fn agent_skills(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AgentSkillsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let (cfg, skills_dir) = {
        let reg = state.persona_registry.read().await;
        match reg.get(&id) {
            Some(cfg) => (cfg.clone(), reg.agents_dir().join(&id).join("skills")),
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("persona '{id}' not found"),
                    }),
                ));
            }
        }
    };

    let enabled: std::collections::HashSet<&str> =
        cfg.enabled_skills.iter().map(String::as_str).collect();
    let to_info = |skill: &hakimi_skills::Skill| AgentSkillInfo {
        name: skill.name.clone(),
        description: skill.description.clone(),
        tags: skill.tags.clone(),
        enabled: enabled.contains(skill.name.as_str()),
    };

    let available = if id == hakimi_core::DEFAULT_PERSONA_ID {
        let agent = state.agent.lock().await;
        agent
            .skill_store()
            .map(|store| store.skills().iter().map(to_info).collect())
            .unwrap_or_default()
    } else {
        let store = hakimi_skills::SkillStore::load(&skills_dir)
            .unwrap_or_else(|_| hakimi_skills::SkillStore::empty());
        store.skills().iter().map(to_info).collect()
    };

    Ok(Json(AgentSkillsResponse {
        available,
        enabled: cfg.enabled_skills.clone(),
    }))
}

/// GET /api/agents/{id}/memory — list the persona's memory dir and `MEMORY.md`.
/// The default persona reads the instance memory dir (`<home>/memory`); named
/// personas read `agents/{id}/memory`.
async fn agent_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AgentMemoryResponse>, (StatusCode, Json<ErrorResponse>)> {
    let memory_dir = {
        let reg = state.persona_registry.read().await;
        if reg.get(&id).is_none() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("persona '{id}' not found"),
                }),
            ));
        }
        if id == hakimi_core::DEFAULT_PERSONA_ID {
            reg.agents_dir()
                .parent()
                .map(|root| root.join("memory"))
                .unwrap_or_else(|| reg.agents_dir().join("memory"))
        } else {
            reg.agents_dir().join(&id).join("memory")
        }
    };

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&memory_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                files.push(name.to_string());
            }
        }
    }
    files.sort();
    let memory_md = std::fs::read_to_string(memory_dir.join("MEMORY.md")).ok();

    Ok(Json(AgentMemoryResponse {
        dir: memory_dir.to_string_lossy().into_owned(),
        files,
        memory_md,
    }))
}

/// Build an SSE `Response` from an mpsc receiver carrying `__DONE__`/`__ERROR__`/
/// token-prefixed messages.
fn sse_response_from_rx(rx: tokio::sync::mpsc::Receiver<String>) -> Response {
    let stream = futures::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|msg| {
            if let Some(payload) = msg.strip_prefix("__DONE__") {
                (
                    Ok::<Event, Infallible>(Event::default().event("done").data(payload)),
                    rx,
                )
            } else if let Some(payload) = msg.strip_prefix("__ERROR__") {
                (
                    Ok::<Event, Infallible>(Event::default().event("error").data(payload)),
                    rx,
                )
            } else if let Some(payload) = msg.strip_prefix("__SESSION__") {
                (
                    Ok::<Event, Infallible>(Event::default().event("session").data(payload)),
                    rx,
                )
            } else {
                (
                    Ok::<Event, Infallible>(Event::default().event("token").data(msg)),
                    rx,
                )
            }
        })
    });
    Sse::new(stream).into_response()
}

/// Resolve a persona's session database. The default persona uses the instance
/// DB; named personas open + cache `agents/<id>/sessions.db` on first access.
async fn resolve_persona_session_db(
    state: &AppState,
    id: &str,
) -> Result<
    std::sync::Arc<tokio::sync::Mutex<hakimi_session::SessionDB>>,
    (StatusCode, Json<ErrorResponse>),
> {
    if id == hakimi_core::DEFAULT_PERSONA_ID {
        return Ok(state.session_db.clone());
    }
    if let Some(db) = state.persona_session_dbs.read().await.get(id) {
        return Ok(db.clone());
    }
    let path = {
        let reg = state.persona_registry.read().await;
        reg.agents_dir().join(id).join("sessions.db")
    };
    let db = tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = hakimi_session::SessionDB::new(&path)?;
        db.initialize()?;
        Ok::<_, anyhow::Error>(db)
    })
    .await
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("join error: {e}"),
        )
    })?
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("open session db: {e}"),
        )
    })?;
    let arc = std::sync::Arc::new(tokio::sync::Mutex::new(db));
    state
        .persona_session_dbs
        .write()
        .await
        .insert(id.to_string(), arc.clone());
    Ok(arc)
}

/// GET /api/agents/{id}/sessions — recent sessions from the persona's database.
async fn agent_sessions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SessionInfo>>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::SessionOps;
    {
        let reg = state.persona_registry.read().await;
        if reg.get(&id).is_none() {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                format!("persona '{id}' not found"),
            ));
        }
    }
    let db_arc = resolve_persona_session_db(&state, &id).await?;
    let db = db_arc.lock().await;
    match db.get_recent_sessions(None, 50) {
        Ok(metas) => Ok(Json(metas.into_iter().map(SessionInfo::from).collect())),
        Err(e) => Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list sessions: {e}"),
        )),
    }
}

/// POST /api/agents/{id}/chat/stream — streaming persona chat (SSE). Persists to
/// the persona's session DB when a `session_id` is supplied. The default persona
/// reuses the shared template agent + instance DB; named personas get their own.
async fn agent_chat_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Response {
    let (cfg, skills_dir, is_default) = {
        let reg = state.persona_registry.read().await;
        match reg.get(&id) {
            Some(c) => (
                c.clone(),
                reg.agents_dir().join(&id).join("skills"),
                id == hakimi_core::DEFAULT_PERSONA_ID,
            ),
            None => {
                return api_error(StatusCode::NOT_FOUND, format!("persona '{id}' not found"))
                    .into_response();
            }
        }
    };

    let (tx_for_handler, rx) = tokio::sync::mpsc::channel::<String>(512);
    let user_message = req.message.clone();
    let requested_session_id = req
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let session_db = match resolve_persona_session_db(&state, &id).await {
        Ok(db) => db,
        Err((_, Json(err))) => {
            let _ = tx_for_handler.send(format!("__ERROR__{}", err.error)).await;
            drop(tx_for_handler);
            return sse_response_from_rx(rx);
        }
    };

    let mut cloned_agent = if is_default {
        state.agent.lock().await.clone()
    } else {
        build_persona_agent_for(&state, &cfg, &skills_dir).await
    };

    {
        // Team executor needs AIAgent directly
        let agent_guard = state.agent.lock().await;
        let base_agent = agent_guard.clone(); // AIAgent already, no unwrap needed
        drop(agent_guard);

        let model_config = {
            let config = state.config.lock().await;
            config.model.clone()
        };

        let template = std::sync::Arc::new(base_agent);
        let team_base = hakimi_core::PersonaTeamExecutor::new(
            state.persona_registry.clone(),
            template,
            model_config,
            128_000,
        );
        let lead_id = if is_default {
            hakimi_core::DEFAULT_PERSONA_ID.to_string()
        } else {
            cfg.id.clone()
        };
        cloned_agent.set_team_executor(Some(Arc::new(team_base.for_lead(&lead_id))));
    }

    if let Some(session_id) = requested_session_id.as_deref() {
        let restored = {
            let db = session_db.lock().await;
            match db.get_session(session_id) {
                Ok(Some(_)) => db.restore_session(session_id, None),
                Ok(None) => Err(anyhow::anyhow!("session not found: {session_id}")),
                Err(e) => Err(e),
            }
        };
        match restored {
            Ok(messages) => {
                cloned_agent.set_session_id(session_id.to_string());
                cloned_agent.clear_messages();
                for message in messages {
                    cloned_agent.add_message(message);
                }
            }
            Err(e) => {
                let _ = tx_for_handler.send(format!("__ERROR__{e}")).await;
                drop(tx_for_handler);
                return sse_response_from_rx(rx);
            }
        }
    }

    cloned_agent.set_streaming(true);
    cloned_agent.set_streaming_callback(Some(Arc::new({
        let tx = tx_for_handler.clone();
        move |token: String| {
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(token).await;
            });
        }
    })));

    let session_id = cloned_agent.session_id().to_string();
    let tx = tx_for_handler.clone();
    let id = id.clone();

    if requested_session_id.is_none() {
        use hakimi_session::SessionOps;
        let db = session_db.lock().await;
        if let Err(e) = db.create_session_with_id(
            &session_id,
            "webui",
            None,
            Some(cloned_agent.model()),
            None,
            None,
        ) {
            let _ = tx_for_handler
                .send(format!("__ERROR__Failed to create session: {e}"))
                .await;
            drop(tx_for_handler);
            return sse_response_from_rx(rx);
        }
        let _ = tx_for_handler
            .send(format!("__SESSION__{session_id}"))
            .await;
    }

    tokio::spawn(async move {
        use hakimi_session::MessageOps;
        hakimi_common::publish(hakimi_common::ActivityEvent::TurnStarted {
            persona_id: id.clone(),
            task_hint: None,
            model: Some(cloned_agent.model().to_string()),
        });
        match cloned_agent.chat(&user_message).await {
            Ok(response) => {
                {
                    let persist_result = {
                        let db = session_db.lock().await;
                        db.save_message(&session_id, &CoreMessage::user(user_message.clone()))
                            .and_then(|_| {
                                db.save_message(
                                    &session_id,
                                    &CoreMessage::assistant(response.clone()),
                                )
                            })
                    };
                    if let Err(e) = persist_result {
                        hakimi_common::publish(hakimi_common::ActivityEvent::TurnEnded {
                            persona_id: id.clone(),
                        });
                        let _ = tx.send(format!("__ERROR__{e}")).await;
                        return;
                    }
                }
                let done = serde_json::json!({
                    "response": response,
                    "session_id": session_id,
                });
                hakimi_common::publish(hakimi_common::ActivityEvent::TurnEnded {
                    persona_id: id.clone(),
                });
                let _ = tx.send(format!("__DONE__{done}")).await;
            }
            Err(e) => {
                hakimi_common::publish(hakimi_common::ActivityEvent::TurnEnded {
                    persona_id: id.clone(),
                });
                let _ = tx.send(format!("__ERROR__{}", e)).await;
            }
        }
    });
    drop(tx_for_handler);
    sse_response_from_rx(rx)
}

/// POST /chat/stream — send a message, stream the response via SSE.
///
/// Events:
///   - `event: token\n data: <text_chunk>\n\n`
///   - `event: done\n  data: {"response":"<full_text>","session_id":"<id>"}\n\n`
///   - `event: error\n data: <error_message>\n\n`
async fn chat_stream(State(state): State<AppState>, Json(req): Json<ChatRequest>) -> Response {
    info!(message_len = req.message.len(), "POST /chat/stream");

    let (tx_for_handler, rx) = tokio::sync::mpsc::channel::<String>(512);
    let user_message = req.message.clone();
    let requested_session_id = req
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    // Clone the agent, then attach this request's streaming callback only to the clone.
    // Do not store the request-local `tx` callback on the shared AppState agent:
    // that keeps the sender alive after the `done` event and the browser waits forever.
    let mut cloned_agent = {
        let agent = state.agent.lock().await;
        agent.clone()
    };

    if let Some(session_id) = requested_session_id.as_deref() {
        let restored = {
            let db = state.session_db.lock().await;
            match db.get_session(session_id) {
                Ok(Some(_)) => db.restore_session(session_id, None),
                Ok(None) => Err(anyhow::anyhow!("session not found: {session_id}")),
                Err(e) => Err(e),
            }
        };

        match restored {
            Ok(messages) => {
                cloned_agent.set_session_id(session_id.to_string());
                cloned_agent.clear_messages();
                for message in messages {
                    cloned_agent.add_message(message);
                }
            }
            Err(e) => {
                let _ = tx_for_handler.send(format!("__ERROR__{e}")).await;
                drop(tx_for_handler);
                let stream = futures::stream::unfold(rx, |mut rx| async {
                    rx.recv().await.map(|msg| {
                        let payload = msg.strip_prefix("__ERROR__").unwrap_or(&msg).to_string();
                        (
                            Ok::<Event, Infallible>(Event::default().event("error").data(payload)),
                            rx,
                        )
                    })
                });
                return Sse::new(stream).into_response();
            }
        }
    }

    cloned_agent.set_streaming(true);
    cloned_agent.set_streaming_callback(Some(Arc::new({
        let tx = tx_for_handler.clone();
        move |token: String| {
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(token).await;
            });
        }
    })));

    let session_id = cloned_agent.session_id().to_string();
    let should_persist = requested_session_id.is_some();
    let tx = tx_for_handler.clone();
    let session_db = state.session_db.clone();

    // Run the chat in a background task.
    tokio::spawn(async move {
        match cloned_agent.chat(&user_message).await {
            Ok(response) => {
                if should_persist {
                    let persist_result = {
                        let db = session_db.lock().await;
                        db.save_message(&session_id, &CoreMessage::user(user_message.clone()))
                            .and_then(|_| {
                                db.save_message(
                                    &session_id,
                                    &CoreMessage::assistant(response.clone()),
                                )
                            })
                    };

                    if let Err(e) = persist_result {
                        let _ = tx.send(format!("__ERROR__{e}")).await;
                        return;
                    }
                }

                let done = serde_json::json!({
                    "response": response,
                    "session_id": session_id,
                });
                let _ = tx.send(format!("__DONE__{done}")).await;
            }
            Err(e) => {
                let _ = tx.send(format!("__ERROR__{}", e)).await;
            }
        }
    });
    // Drop the handler's original sender after moving a clone into the worker.
    // Otherwise the SSE receiver never observes channel closure after `done/error`.
    drop(tx_for_handler);

    // Convert mpsc receiver into an SSE stream.
    let stream = futures::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|msg| {
            if let Some(payload) = msg.strip_prefix("__DONE__") {
                (
                    Ok::<Event, Infallible>(Event::default().event("done").data(payload)),
                    rx,
                )
            } else if let Some(payload) = msg.strip_prefix("__ERROR__") {
                (
                    Ok::<Event, Infallible>(Event::default().event("error").data(payload)),
                    rx,
                )
            } else {
                (
                    Ok::<Event, Infallible>(Event::default().event("token").data(msg)),
                    rx,
                )
            }
        })
    });

    Sse::new(stream).into_response()
}

/// POST /v1/chat/completions — OpenAI-compatible chat with optional SSE snapshot output.
async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionsRequest>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let stream = request_bool(req.stream.as_ref(), false);
    let prompt = chat_completions_prompt(&req.messages)?;
    info!(
        message_count = req.messages.len(),
        prompt_len = prompt.len(),
        "POST /v1/chat/completions"
    );

    let mut agent = {
        let agent = state.agent.lock().await;
        agent.clone()
    };
    agent.clear_messages();
    agent.set_streaming(false);
    agent.set_streaming_callback(None);

    if let Some(model) = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        agent.set_model(model.to_string());
    }
    let model = agent.model().to_string();

    match agent.run_conversation(&prompt).await {
        Ok(result) => {
            let created = unix_timestamp_secs();
            let completion = json!({
                "id": chat_completion_id(),
                "object": "chat.completion",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": result.final_response
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": result.usage.prompt_tokens,
                    "completion_tokens": result.usage.completion_tokens,
                    "total_tokens": result.usage.total_tokens
                }
            });
            if stream {
                Ok(chat_completion_sse_response(&completion))
            } else {
                Ok(Json(completion).into_response())
            }
        }
        Err(e) => {
            let msg = format!("Agent error: {e}");
            tracing::error!("{msg}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: msg }),
            ))
        }
    }
}

async fn execute_responses_request(
    state: &AppState,
    req: ResponsesRequest,
) -> Result<ResponsesExecution, (StatusCode, Json<ErrorResponse>)> {
    let mut new_messages = Vec::new();
    if let Some(instructions) = req
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        new_messages.push(ChatCompletionsMessage {
            role: "system".to_string(),
            content: JsonValue::String(instructions.to_string()),
        });
    }
    new_messages.extend(responses_input_messages(&req.input)?);

    let mut messages = if let Some(previous_response_id) = req
        .previous_response_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let store = state.response_store.lock().await;
        match store.messages(previous_response_id) {
            Some(messages) => messages,
            None => {
                return Err(api_error(
                    StatusCode::NOT_FOUND,
                    format!("previous response not found: {previous_response_id}"),
                ));
            }
        }
    } else {
        Vec::new()
    };
    messages.extend(new_messages);

    let prompt = responses_prompt(&messages)?;
    info!(
        message_count = messages.len(),
        prompt_len = prompt.len(),
        previous_response_id = req.previous_response_id.as_deref().unwrap_or(""),
        "POST /v1/responses"
    );

    let mut agent = {
        let agent = state.agent.lock().await;
        agent.clone()
    };
    agent.clear_messages();
    agent.set_streaming(false);
    agent.set_streaming_callback(None);

    if let Some(model) = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        agent.set_model(model.to_string());
    }
    let model = agent.model().to_string();

    match agent.run_conversation(&prompt).await {
        Ok(result) => {
            let created = unix_timestamp_secs();
            let id = response_id();
            let message_id = response_message_id();
            let output_text = result.final_response.clone();
            let response = json!({
                "id": id.clone(),
                "object": "response",
                "created_at": created,
                "status": "completed",
                "model": model,
                "previous_response_id": req.previous_response_id,
                "output": [{
                    "id": message_id,
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": output_text.clone(),
                        "annotations": []
                    }]
                }],
                "output_text": output_text,
                "usage": {
                    "input_tokens": result.usage.prompt_tokens,
                    "output_tokens": result.usage.completion_tokens,
                    "total_tokens": result.usage.total_tokens
                }
            });

            let mut stored_messages = messages;
            stored_messages.push(ChatCompletionsMessage {
                role: "assistant".to_string(),
                content: JsonValue::String(result.final_response),
            });
            state
                .response_store
                .lock()
                .await
                .insert(id, response.clone(), stored_messages);

            Ok(ResponsesExecution { response })
        }
        Err(e) => {
            let msg = format!("Agent error: {e}");
            tracing::error!("{msg}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: msg }),
            ))
        }
    }
}

/// POST /v1/responses — OpenAI Responses-compatible chat with optional SSE snapshot output.
async fn responses(
    State(state): State<AppState>,
    Json(req): Json<ResponsesRequest>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let stream = request_bool(req.stream.as_ref(), false);
    let result = execute_responses_request(&state, req).await?;

    if stream {
        Ok(responses_sse_response(&result.response))
    } else {
        Ok(Json(result.response).into_response())
    }
}

/// GET /v1/responses/:id — retrieve a stored Responses API result.
async fn get_response(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let store = state.response_store.lock().await;
    match store.get(id.trim()) {
        Some(response) => Ok(Json(response)),
        None => Err(api_error(
            StatusCode::NOT_FOUND,
            format!("response not found: {id}"),
        )),
    }
}

/// DELETE /v1/responses/:id — remove a stored Responses API result.
async fn delete_response(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let deleted = state.response_store.lock().await.delete(id.trim());
    if deleted {
        Ok(Json(json!({
            "id": id,
            "object": "response.deleted",
            "deleted": true
        })))
    } else {
        Err(api_error(
            StatusCode::NOT_FOUND,
            format!("response not found: {id}"),
        ))
    }
}

/// POST /v1/runs — submit a text-only agent run and return a pollable run id.
async fn create_run(
    State(state): State<AppState>,
    Json(req): Json<RunCreateRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    if request_bool(req.stream.as_ref(), false) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "stream=true is not yet supported on /v1/runs; poll /v1/runs/{id} for status",
        ));
    }

    let messages = run_input_messages(&req)?;
    let prompt = run_prompt(&messages)?;
    let id = run_id();
    let session_id = req
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&id)
        .to_string();

    let mut agent = {
        let agent = state.agent.lock().await;
        agent.clone()
    };
    agent.clear_messages();
    agent.set_streaming(false);
    agent.set_streaming_callback(None);

    if let Some(model) = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        agent.set_model(model.to_string());
    }
    let model = agent.model().to_string();
    let created_at = unix_timestamp_secs();
    let interrupt = agent.interrupt_handle();

    let initial = {
        let mut store = state.run_store.lock().await;
        store.insert(StoredRun::new(id.clone(), session_id, model, created_at));
        store
            .get(&id)
            .expect("newly inserted run should be present")
    };

    let run_store = state.run_store.clone();
    let run_id = id.clone();
    let handle = tokio::spawn(async move {
        run_store.lock().await.set_status(&run_id, "running");
        match agent.run_conversation(&prompt).await {
            Ok(result) => {
                let usage = json!({
                    "prompt_tokens": result.usage.prompt_tokens,
                    "completion_tokens": result.usage.completion_tokens,
                    "total_tokens": result.usage.total_tokens
                });
                run_store
                    .lock()
                    .await
                    .complete(&run_id, result.final_response, usage);
            }
            Err(e) => {
                let msg = format!("Agent error: {e}");
                tracing::error!("{msg}");
                run_store.lock().await.fail(&run_id, msg);
            }
        }
    });
    state.run_store.lock().await.attach_control(
        &id,
        RunControl {
            interrupt,
            task: handle,
        },
    );

    Ok((StatusCode::ACCEPTED, Json(initial)))
}

/// GET /v1/runs/:id — retrieve the latest status for a submitted run.
async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let store = state.run_store.lock().await;
    match store.get(id.trim()) {
        Some(run) => Ok(Json(run)),
        None => Err(api_error(
            StatusCode::NOT_FOUND,
            format!("run not found: {id}"),
        )),
    }
}

fn run_sse_event(run_id: &str, event: &RunEvent) -> Result<Event, Infallible> {
    Ok(Event::default()
        .event(event.event.clone())
        .json_data(event.to_json(run_id))
        .unwrap_or_else(|_| Event::default().event("run.event").data("{}")))
}

fn live_run_event_stream(
    run_id: String,
    receiver: Option<broadcast::Receiver<RunEvent>>,
    since_sequence: usize,
) -> impl futures::Stream<Item = Result<Event, Infallible>> {
    stream::unfold(
        (run_id, receiver, since_sequence, false),
        |(run_id, receiver, since_sequence, done)| async move {
            if done {
                return None;
            }

            let mut receiver = receiver?;

            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        if event.sequence <= since_sequence {
                            continue;
                        }
                        let done = is_terminal_run_status(&event.status);
                        let next_sequence = event.sequence;
                        let sse_event = run_sse_event(&run_id, &event);
                        return Some((sse_event, (run_id, Some(receiver), next_sequence, done)));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    )
}

/// GET /v1/runs/:id/events — stream stored and live run lifecycle events as SSE.
async fn get_run_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let id = id.trim().to_string();
    let subscription = {
        let store = state.run_store.lock().await;
        store.subscribe_events(&id)
    };

    let Some(subscription) = subscription else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("run not found: {id}"),
        ));
    };

    let snapshot_run_id = id.clone();
    let snapshot = stream::iter(
        subscription
            .snapshot
            .into_iter()
            .map(move |event| run_sse_event(&snapshot_run_id, &event)),
    );
    let live = live_run_event_stream(id, subscription.receiver, subscription.since_sequence);
    let events = snapshot.chain(live);

    Ok(Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(30))
                .text("keepalive"),
        )
        .into_response())
}

/// POST /v1/runs/:id/stop — cancel a submitted run.
async fn stop_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let mut store = state.run_store.lock().await;
    match store.stop(id.trim()) {
        StopRunResult::Cancelled(run) => Ok(Json(run)),
        StopRunResult::AlreadyFinished(status) => Err(api_error(
            StatusCode::CONFLICT,
            format!("run already finished with status {status}: {id}"),
        )),
        StopRunResult::NotFound => Err(api_error(
            StatusCode::NOT_FOUND,
            format!("run not found: {id}"),
        )),
    }
}

/// GET /sessions — list recent sessions from the database.
async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionInfo>>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::SessionOps;

    let db = state.session_db.lock().await;
    match db.get_recent_sessions(None, 50) {
        Ok(metas) => Ok(Json(metas.into_iter().map(SessionInfo::from).collect())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to list sessions: {e}"),
            }),
        )),
    }
}

/// POST /sessions — create an empty API-visible session row.
async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<SessionCreateRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::SessionOps;

    let id = requested_session_id(req.id.as_deref(), req.session_id.as_deref());
    validate_api_session_id(&id)?;

    let default_model = {
        let agent = state.agent.lock().await;
        agent.model().to_string()
    };
    let source = req
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("api_server");
    let model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_model.as_str());

    let db = state.session_db.lock().await;
    if db
        .get_session(&id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get session: {e}"),
            )
        })?
        .is_some()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            format!("Session already exists: {id}"),
        ));
    }

    db.create_session_with_id(
        &id,
        source,
        req.user_id.as_deref(),
        Some(model),
        req.system_prompt.as_deref(),
        None,
    )
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create session: {e}"),
        )
    })?;

    if let Some(title) = req.title.as_deref()
        && let Err(e) = db.set_unique_title(&id, title)
    {
        let _ = db.delete_session(&id);
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("Failed to set session title: {e}"),
        ));
    }

    let session = db
        .get_session(&id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get session: {e}"),
            )
        })?
        .map(SessionInfo::from)
        .ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "created session missing"))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "object": "hakimi.session",
            "session": session
        })),
    ))
}

/// GET /sessions/:id — get details for a specific session.
async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfo>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::SessionOps;

    let db = state.session_db.lock().await;
    match db.get_session(&id) {
        Ok(Some(meta)) => Ok(Json(SessionInfo::from(meta))),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Session not found: {id}"),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to get session: {e}"),
            }),
        )),
    }
}

/// PATCH /sessions/:id — update client-safe session metadata.
async fn update_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<JsonValue>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::SessionOps;

    let id = id.trim().to_string();
    validate_api_session_id(&id)?;
    let JsonValue::Object(fields) = body else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "session update body must be a JSON object",
        ));
    };

    let allowed = ["title", "end_reason"];
    let unknown = fields
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("Unsupported session fields: {}", unknown.join(", ")),
        ));
    }

    let db = state.session_db.lock().await;
    if db
        .get_session(&id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get session: {e}"),
            )
        })?
        .is_none()
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("Session not found: {id}"),
        ));
    }

    if let Some(title) = fields.get("title") {
        match title {
            JsonValue::Null => db.clear_title(&id),
            JsonValue::String(value) => db.set_title(&id, value),
            _ => Err(anyhow::anyhow!("title must be a string or null")),
        }
        .map_err(|e| {
            api_error(
                StatusCode::BAD_REQUEST,
                format!("Failed to update title: {e}"),
            )
        })?;
    }

    if let Some(end_reason) = fields.get("end_reason") {
        match end_reason {
            JsonValue::Null => {}
            JsonValue::String(reason) if reason.trim().is_empty() => {}
            JsonValue::String(reason) => db.end_session(&id, reason.trim()).map_err(|e| {
                api_error(
                    StatusCode::BAD_REQUEST,
                    format!("Failed to end session: {e}"),
                )
            })?,
            _ => {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "end_reason must be a string or null",
                ));
            }
        }
    }

    let session = db
        .get_session(&id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get session: {e}"),
            )
        })?
        .map(SessionInfo::from)
        .ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "updated session missing"))?;

    Ok(Json(json!({
        "object": "hakimi.session",
        "session": session
    })))
}

/// DELETE /sessions/:id — remove a session and its stored messages.
async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::SessionOps;

    let id = id.trim().to_string();
    validate_api_session_id(&id)?;
    let deleted = state
        .session_db
        .lock()
        .await
        .delete_session(&id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete session: {e}"),
            )
        })?;
    if !deleted {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("Session not found: {id}"),
        ));
    }

    Ok(Json(json!({
        "object": "hakimi.session.deleted",
        "id": id,
        "deleted": true
    })))
}

/// DELETE /sessions/:id/messages — clear messages for a session without deleting the session row.
async fn clear_session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::SessionOps;

    let id = id.trim().to_string();
    validate_api_session_id(&id)?;
    let cleared = state
        .session_db
        .lock()
        .await
        .clear_session_messages(&id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to clear session messages: {e}"),
            )
        })?;
    if !cleared {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("Session not found: {id}"),
        ));
    }

    Ok(Json(json!({
        "object": "hakimi.session.messages.clear",
        "id": id,
        "cleared": true
    })))
}

/// DELETE /sessions/:id/messages/:message_id — delete a single message from a session.
async fn delete_session_message(
    State(state): State<AppState>,
    Path((session_id, message_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::MessageOps;

    let session_id = session_id.trim().to_string();
    let message_id = message_id.trim().to_string();
    validate_api_session_id(&session_id)?;

    let deleted = state
        .session_db
        .lock()
        .await
        .delete_message(&session_id, &message_id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete message: {e}"),
            )
        })?;

    if !deleted {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("Message not found: {message_id}"),
        ));
    }

    Ok(Json(json!({
        "object": "hakimi.session.message.delete",
        "session_id": session_id,
        "message_id": message_id,
        "deleted": true
    })))
}

/// POST /sessions/:id/fork — create a child session carrying the transcript forward.
async fn fork_session(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    Json(req): Json<SessionForkRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::{MessageOps, SessionOps};

    let source_id = source_id.trim().to_string();
    validate_api_session_id(&source_id)?;
    let fork_id = requested_session_id(req.id.as_deref(), req.session_id.as_deref());
    validate_api_session_id(&fork_id)?;

    let db = state.session_db.lock().await;
    let source = db
        .get_session(&source_id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get session: {e}"),
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                format!("Session not found: {source_id}"),
            )
        })?;
    if db
        .get_session(&fork_id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get session: {e}"),
            )
        })?
        .is_some()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            format!("Session already exists: {fork_id}"),
        ));
    }

    let messages = db.get_messages(&source_id).map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get session messages: {e}"),
        )
    })?;
    db.end_session(&source_id, "branched").map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to mark source session branched: {e}"),
        )
    })?;
    db.create_session_with_id(
        &fork_id,
        "api_server",
        source.user_id.as_deref(),
        source.model.as_deref(),
        source.system_prompt.as_deref(),
        Some(&source_id),
    )
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fork session: {e}"),
        )
    })?;

    for message in messages {
        db.save_message(&fork_id, &message).map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to copy fork messages: {e}"),
            )
        })?;
    }
    if let Some(title) = req.title.as_deref()
        && let Err(e) = db.set_unique_title(&fork_id, title)
    {
        let _ = db.delete_session(&fork_id);
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("Failed to set fork title: {e}"),
        ));
    }

    let session = db
        .get_session(&fork_id)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get forked session: {e}"),
            )
        })?
        .map(SessionInfo::from)
        .ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "forked session missing"))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "object": "hakimi.session",
            "session": session
        })),
    ))
}

/// GET /sessions/search — search saved message content across sessions.
async fn search_sessions(
    State(state): State<AppState>,
    Query(params): Query<SessionSearchQuery>,
) -> Result<Json<SessionSearchResponse>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::{MessageOps, SessionOps};

    let query = params.q.unwrap_or_default().trim().to_string();
    let limit = bounded_limit(params.limit, 20, 100);
    if query.is_empty() {
        return Ok(Json(SessionSearchResponse {
            object: "list".to_string(),
            query,
            count: 0,
            data: Vec::new(),
        }));
    }

    let db = state.session_db.lock().await;
    let results = db.search_messages(&query, limit as i64).map_err(|e| {
        api_error(
            StatusCode::BAD_REQUEST,
            format!("Failed to search sessions: {e}"),
        )
    })?;

    let data = results
        .into_iter()
        .map(|result| {
            let meta = db.get_session(&result.session_id).ok().flatten();
            SessionSearchResultInfo {
                session_id: result.session_id,
                message_id: result.message_id,
                content: result.content,
                rank: result.rank,
                title: meta.as_ref().and_then(|session| session.title.clone()),
                source: meta.as_ref().and_then(|session| session.source.clone()),
                model: meta.as_ref().and_then(|session| session.model.clone()),
                started_at: meta.and_then(|session| session.started_at),
            }
        })
        .collect::<Vec<_>>();

    Ok(Json(SessionSearchResponse {
        object: "list".to_string(),
        query,
        count: data.len(),
        data,
    }))
}

/// GET /sessions/:id/messages — get sanitized messages for a specific session.
async fn get_session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<SessionMessagesQuery>,
) -> Result<Json<SessionMessagesResponse>, (StatusCode, Json<ErrorResponse>)> {
    use hakimi_session::SessionOps;

    let max_messages = params.limit.map(|limit| limit.clamp(1, 500));
    let db = state.session_db.lock().await;
    match db.get_session_with_messages(&id, max_messages) {
        Ok(Some((meta, messages))) => {
            let messages = messages
                .into_iter()
                .map(SessionMessageInfo::from)
                .collect::<Vec<_>>();
            Ok(Json(SessionMessagesResponse {
                object: "hakimi.session.messages".to_string(),
                session: SessionInfo::from(meta),
                count: messages.len(),
                messages,
            }))
        }
        Ok(None) => Err(api_error(
            StatusCode::NOT_FOUND,
            format!("Session not found: {id}"),
        )),
        Err(e) => Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get session messages: {e}"),
        )),
    }
}

/// GET /tools — list all available tools registered in the agent.
async fn list_tools(State(state): State<AppState>) -> Json<Vec<ToolInfo>> {
    let agent = state.agent.lock().await;
    let defs = agent.tool_registry().get_definitions().await;

    Json(
        defs.into_iter()
            .map(|d| ToolInfo {
                name: d.name,
                description: d.description,
                parameters: d.parameters,
            })
            .collect(),
    )
}

/// GET /v1/skills — list loaded runtime skill metadata without prompt bodies.
async fn list_v1_skills(State(state): State<AppState>) -> Json<SkillsResponse> {
    let agent = state.agent.lock().await;
    let Some(store) = agent.skill_store() else {
        return Json(SkillsResponse {
            object: "list".to_string(),
            total: 0,
            active: Vec::new(),
            data: Vec::new(),
        });
    };

    let active = store.working_set().active_skill_names();
    let data = store
        .skills()
        .iter()
        .map(|skill| SkillInfo {
            name: skill.name.clone(),
            description: skill.description.clone(),
            trigger: skill.trigger.clone(),
            tags: skill.tags.clone(),
            phases: skill
                .phases
                .iter()
                .map(|phase| phase.as_str().to_string())
                .collect(),
            platforms: skill.platforms.clone(),
            provenance: skill.provenance_label(),
            active: active.contains(&skill.name),
        })
        .collect::<Vec<_>>();

    Json(SkillsResponse {
        object: "list".to_string(),
        total: data.len(),
        active,
        data,
    })
}

/// GET /v1/toolsets — group currently registered tools by toolset.
async fn list_v1_toolsets(State(state): State<AppState>) -> Json<ToolsetsResponse> {
    let agent = state.agent.lock().await;
    let mut defs = agent.tool_registry().get_definitions().await;
    defs.sort_by(|a, b| a.toolset.cmp(&b.toolset).then(a.name.cmp(&b.name)));

    let total_tools = defs.len();
    let mut grouped: BTreeMap<String, Vec<ToolsetToolInfo>> = BTreeMap::new();
    let mut deferrable_by_toolset: BTreeMap<String, bool> = BTreeMap::new();

    for def in defs {
        let toolset = if def.toolset.trim().is_empty() {
            "default".to_string()
        } else {
            def.toolset
        };
        let deferrable = hakimi_tools::tool_search::is_deferrable_tool(&def.name, &toolset);
        deferrable_by_toolset
            .entry(toolset.clone())
            .and_modify(|value| *value |= deferrable)
            .or_insert(deferrable);
        grouped.entry(toolset).or_default().push(ToolsetToolInfo {
            name: def.name,
            description: def.description,
            parameters: def.parameters,
        });
    }

    let data = grouped
        .into_iter()
        .map(|(name, tools)| {
            let deferrable = deferrable_by_toolset.get(&name).copied().unwrap_or(false);
            ToolsetInfo {
                source: toolset_source(&name).to_string(),
                deferrable,
                tool_count: tools.len(),
                name,
                tools,
            }
        })
        .collect::<Vec<_>>();

    Json(ToolsetsResponse {
        object: "list".to_string(),
        total_toolsets: data.len(),
        total_tools,
        data,
    })
}

fn toolset_source(toolset: &str) -> &'static str {
    if toolset == "mcp" || toolset.starts_with("mcp-") {
        "mcp"
    } else if matches!(toolset, "http" | "plugin") {
        "plugin"
    } else {
        "core"
    }
}

/// GET /config — return the current configuration (no secrets).
async fn get_config(State(state): State<AppState>) -> Json<SanitizedConfig> {
    let config = state.config.lock().await;

    // Convert ModelTiers to DTO (with API keys masked)
    let model_tiers = config.model.tiers.as_ref().map(|tiers| ModelTiersDto {
        primary: TierConfigDto {
            provider: tiers.primary.provider.clone(),
            model: tiers.primary.model.clone(),
            api_key: if tiers.primary.api_key.is_empty() {
                None
            } else {
                Some("••••••••".to_string()) // Mask for security
            },
            base_url: tiers.primary.base_url.clone(),
        },
        light: tiers.light.as_ref().map(|tier| TierConfigDto {
            provider: tier.provider.clone(),
            model: tier.model.clone(),
            api_key: if tier.api_key.is_empty() {
                None
            } else {
                Some("••••••••".to_string())
            },
            base_url: tier.base_url.clone(),
        }),
        reasoning: tiers.reasoning.as_ref().map(|tier| TierConfigDto {
            provider: tier.provider.clone(),
            model: tier.model.clone(),
            api_key: if tier.api_key.is_empty() {
                None
            } else {
                Some("••••••••".to_string())
            },
            base_url: tier.base_url.clone(),
        }),
    });

    Json(SanitizedConfig {
        model_default: config.model.default.clone(),
        model_provider: config.model.provider.clone(),
        model_tiers,
        auto_dispatch_enabled: config.model.auto_dispatch.enabled,
        auto_dispatch_show_decision: config.model.auto_dispatch.show_dispatch_decision,
        auto_dispatch_two_stage_enabled: config.model.auto_dispatch.two_stage.enabled,
        agent_max_turns: config.agent.max_turns,
        agent_verbose: config.agent.verbose,
        agent_system_prompt: config.agent.system_prompt.clone(),
        agent_reasoning_effort: config.agent.reasoning_effort.clone(),
        agent_save_trajectories: config.agent.save_trajectories,
        agent_trajectory_dir: config.agent.trajectory_dir.clone(),
        terminal_env_type: config.terminal.env_type.clone(),
        terminal_cwd: config.terminal.cwd.clone(),
        terminal_timeout: config.terminal.timeout,
        terminal_docker_image: config.terminal.docker_image.clone(),
        compression_enabled: config.compression.enabled,
        compression_engine: config.compression.engine.clone(),
        compression_model: config.compression.model.clone(),
        compression_context_length: config.compression.context_length,
        display_streaming: config.display.streaming,
        display_skin: config.display.skin.clone(),
        embedding_enabled: config.embedding.enabled,
        embedding_provider: config.embedding.provider.clone(),
        embedding_model: config.embedding.model.clone(),
        embedding_dimension: config.embedding.dimension,
        embedding_batch_size: config.embedding.batch_size,
        embedding_normalize: config.embedding.normalize,
        mcp_server_count: config.mcp_servers.len(),
    })
}

/// POST /config — update runtime configuration fields.
async fn update_config(
    State(state): State<AppState>,
    Json(update): Json<ConfigUpdate>,
) -> Result<Json<SanitizedConfig>, (StatusCode, Json<ErrorResponse>)> {
    let mut config = state.config.lock().await;

    if let Some(v) = update.model_default {
        config.model.default = v;
    }
    if let Some(v) = update.model_provider {
        config.model.provider = v;
    }
    if let Some(v) = update.agent_max_turns {
        config.agent.max_turns = v;
    }
    if let Some(v) = update.agent_verbose {
        config.agent.verbose = v;
    }
    if let Some(v) = update.agent_system_prompt {
        config.agent.system_prompt = v;
    }
    if let Some(v) = update.agent_save_trajectories {
        config.agent.save_trajectories = v;
    }
    if let Some(v) = update.agent_trajectory_dir {
        config.agent.trajectory_dir = v;
    }
    if let Some(v) = update.terminal_cwd {
        config.terminal.cwd = v;
    }
    if let Some(v) = update.terminal_timeout {
        config.terminal.timeout = v;
    }
    if let Some(v) = update.compression_engine {
        config.compression.engine = v;
    }
    if let Some(v) = update.compression_model {
        config.compression.model = v;
    }
    if let Some(v) = update.compression_context_length {
        config.compression.context_length = v;
    }
    if let Some(v) = update.agent_reasoning_effort {
        config.agent.reasoning_effort = v;
    }
    if let Some(v) = update.terminal_env_type {
        config.terminal.env_type = v;
    }
    if let Some(v) = update.terminal_docker_image {
        config.terminal.docker_image = v;
    }
    if let Some(v) = update.compression_enabled {
        config.compression.enabled = v;
    }
    if let Some(v) = update.display_streaming {
        config.display.streaming = v;
    }
    if let Some(v) = update.display_skin {
        config.display.skin = v;
    }
    if let Some(v) = update.embedding_enabled {
        config.embedding.enabled = v;
    }
    if let Some(v) = update.embedding_provider {
        config.embedding.provider = v;
    }
    if let Some(v) = update.embedding_model {
        config.embedding.model = v;
    }
    if let Some(v) = update.embedding_dimension {
        config.embedding.dimension = v;
    }
    if let Some(v) = update.embedding_batch_size {
        config.embedding.batch_size = v;
    }
    if let Some(v) = update.embedding_normalize {
        config.embedding.normalize = v;
    }
    drop(config);

    if let Some(password) = update.password {
        *state.webui_password.lock().await = password;
    }

    let config = state.config.lock().await;

    // Convert ModelTiers to DTO (without API keys)
    let model_tiers = config.model.tiers.as_ref().map(|tiers| ModelTiersDto {
        primary: TierConfigDto {
            provider: tiers.primary.provider.clone(),
            model: tiers.primary.model.clone(),
            api_key: None, // Redacted for security
            base_url: tiers.primary.base_url.clone(),
        },
        light: tiers.light.as_ref().map(|tier| TierConfigDto {
            provider: tier.provider.clone(),
            model: tier.model.clone(),
            api_key: None, // Redacted for security
            base_url: tier.base_url.clone(),
        }),
        reasoning: tiers.reasoning.as_ref().map(|tier| TierConfigDto {
            provider: tier.provider.clone(),
            model: tier.model.clone(),
            api_key: None, // Redacted for security
            base_url: tier.base_url.clone(),
        }),
    });

    // Return the updated config (sanitized).
    let response = SanitizedConfig {
        model_default: config.model.default.clone(),
        model_provider: config.model.provider.clone(),
        model_tiers,
        auto_dispatch_enabled: config.model.auto_dispatch.enabled,
        auto_dispatch_show_decision: config.model.auto_dispatch.show_dispatch_decision,
        auto_dispatch_two_stage_enabled: config.model.auto_dispatch.two_stage.enabled,
        agent_max_turns: config.agent.max_turns,
        agent_verbose: config.agent.verbose,
        agent_system_prompt: config.agent.system_prompt.clone(),
        agent_reasoning_effort: config.agent.reasoning_effort.clone(),
        agent_save_trajectories: config.agent.save_trajectories,
        agent_trajectory_dir: config.agent.trajectory_dir.clone(),
        terminal_env_type: config.terminal.env_type.clone(),
        terminal_cwd: config.terminal.cwd.clone(),
        terminal_timeout: config.terminal.timeout,
        terminal_docker_image: config.terminal.docker_image.clone(),
        compression_enabled: config.compression.enabled,
        compression_engine: config.compression.engine.clone(),
        compression_model: config.compression.model.clone(),
        compression_context_length: config.compression.context_length,
        display_streaming: config.display.streaming,
        display_skin: config.display.skin.clone(),
        embedding_enabled: config.embedding.enabled,
        embedding_provider: config.embedding.provider.clone(),
        embedding_model: config.embedding.model.clone(),
        embedding_dimension: config.embedding.dimension,
        embedding_batch_size: config.embedding.batch_size,
        embedding_normalize: config.embedding.normalize,
        mcp_server_count: config.mcp_servers.len(),
    };

    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::AppState;
    use axum::body::Body;
    use axum::http::{self, Request};
    use hakimi_common::ToolContext;
    use hakimi_session::{MessageOps, SessionDB, SessionOps};
    use serde_json::json;
    use std::ffi::OsString;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    // ---------- helpers ----------

    static KANBAN_ENV_LOCK: Mutex<()> = Mutex::const_new(());
    static WORKSPACE_CWD_LOCK: Mutex<()> = Mutex::const_new(());

    struct EnvVarGuard {
        key: &'static str,
        old: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let old = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, old }
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, old }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(old) = &self.old {
                    std::env::set_var(self.key, old);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    /// A minimal mock tool for testing /tools endpoint.
    struct MockTool;
    #[async_trait::async_trait]
    impl hakimi_tools::Tool for MockTool {
        fn name(&self) -> &str {
            "mock_tool"
        }
        fn toolset(&self) -> &str {
            "test"
        }
        fn description(&self) -> &str {
            "A mock tool for testing"
        }
        fn schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            _args: &serde_json::Value,
            _ctx: &ToolContext,
        ) -> hakimi_common::Result<String> {
            Ok("mock result".into())
        }
    }

    struct StaticTransport;

    #[async_trait::async_trait]
    impl hakimi_transports::ProviderTransport for StaticTransport {
        fn api_mode(&self) -> hakimi_common::ApiMode {
            hakimi_common::ApiMode::ChatCompletions
        }

        fn provider_name(&self) -> &str {
            "static"
        }

        async fn execute(
            &self,
            _model: &str,
            messages: &[hakimi_common::Message],
            _tools: &[hakimi_common::ToolDefinition],
            _params: &hakimi_transports::RequestParams,
        ) -> hakimi_common::Result<hakimi_common::NormalizedResponse> {
            let prompt = messages
                .iter()
                .rev()
                .find_map(|message| message.content.as_deref())
                .unwrap_or_default();

            Ok(hakimi_common::NormalizedResponse {
                content: Some(format!("stub response to: {prompt}")),
                tool_calls: None,
                finish_reason: Some(hakimi_common::FinishReason::Stop),
                usage: Some(hakimi_common::Usage {
                    prompt_tokens: 7,
                    completion_tokens: 3,
                    total_tokens: 10,
                    cached_tokens: 0,
                    reasoning_tokens: 0,
                }),
                reasoning: None,
            })
        }

        async fn execute_streaming(
            &self,
            _model: &str,
            messages: &[hakimi_common::Message],
            _tools: &[hakimi_common::ToolDefinition],
            _params: &hakimi_transports::RequestParams,
        ) -> hakimi_common::Result<
            Pin<
                Box<
                    dyn futures::stream::Stream<
                            Item = std::result::Result<hakimi_transports::StreamEvent, String>,
                        > + Send,
                >,
            >,
        > {
            let prompt = messages
                .iter()
                .rev()
                .find_map(|message| message.content.as_deref())
                .unwrap_or_default()
                .to_string();
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(hakimi_transports::StreamEvent::ContentDelta(format!(
                    "stub response to: {prompt}"
                ))),
                Ok(hakimi_transports::StreamEvent::Finished("stop".to_string())),
                Ok(hakimi_transports::StreamEvent::Done),
            ])))
        }
    }

    struct SlowTransport;

    #[async_trait::async_trait]
    impl hakimi_transports::ProviderTransport for SlowTransport {
        fn api_mode(&self) -> hakimi_common::ApiMode {
            hakimi_common::ApiMode::ChatCompletions
        }

        fn provider_name(&self) -> &str {
            "slow"
        }

        async fn execute(
            &self,
            _model: &str,
            _messages: &[hakimi_common::Message],
            _tools: &[hakimi_common::ToolDefinition],
            _params: &hakimi_transports::RequestParams,
        ) -> hakimi_common::Result<hakimi_common::NormalizedResponse> {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok(hakimi_common::NormalizedResponse {
                content: Some("slow response".to_string()),
                tool_calls: None,
                finish_reason: Some(hakimi_common::FinishReason::Stop),
                usage: Some(hakimi_common::Usage::default()),
                reasoning: None,
            })
        }

        async fn execute_streaming(
            &self,
            _model: &str,
            _messages: &[hakimi_common::Message],
            _tools: &[hakimi_common::ToolDefinition],
            _params: &hakimi_transports::RequestParams,
        ) -> hakimi_common::Result<
            Pin<
                Box<
                    dyn futures::stream::Stream<
                            Item = std::result::Result<hakimi_transports::StreamEvent, String>,
                        > + Send,
                >,
            >,
        > {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    /// Build a minimal AppState for testing (no real agent).
    /// Uses a stub transport so we don't need a real LLM.
    fn test_state() -> AppState {
        test_state_with_transport(Arc::new(StaticTransport))
    }

    fn test_state_with_transport(
        transport: Arc<dyn hakimi_transports::ProviderTransport>,
    ) -> AppState {
        use hakimi_context::SimpleContextEngine;

        let context_engine: Arc<tokio::sync::RwLock<dyn hakimi_context::ContextEngine>> =
            Arc::new(tokio::sync::RwLock::new(SimpleContextEngine::new(128_000)));

        let tool_registry = hakimi_tools::ToolRegistry::new();

        let agent = hakimi_core::AIAgent::builder()
            .model("test-model")
            .transport(transport)
            .context_engine(context_engine)
            .tool_registry(tool_registry)
            .build()
            .unwrap();

        let db = SessionDB::new(std::path::Path::new(":memory:")).unwrap();
        db.initialize().unwrap();

        AppState {
            agent: Arc::new(Mutex::new(agent)),
            config: Arc::new(Mutex::new(hakimi_config::HakimiConfig::default())),
            session_db: Arc::new(Mutex::new(db)),
            response_store: Arc::new(Mutex::new(ResponsesStore::new(100))),
            run_store: Arc::new(Mutex::new(RunsStore::default())),
            webui_password: Arc::new(Mutex::new(String::new())),
            knowledge_provider: Arc::new(Mutex::new(hakimi_knowledge::KnowledgeProvider::new(
                std::env::temp_dir().join(format!(
                    "hakimi-test-knowledge-{}-{}.json",
                    std::process::id(),
                    unix_timestamp_millis()
                )),
            ))),
            gateway: None,
            persona_registry: Arc::new(tokio::sync::RwLock::new(
                hakimi_core::PersonaRegistry::load({
                    // Unique per `test_state()` call so parallel tests that create
                    // the same persona id don't collide on a shared on-disk registry.
                    use std::sync::atomic::{AtomicU64, Ordering};
                    static SEQ: AtomicU64 = AtomicU64::new(0);
                    std::env::temp_dir().join(format!(
                        "hakimi-test-agents-{}-{}",
                        std::process::id(),
                        SEQ.fetch_add(1, Ordering::Relaxed)
                    ))
                })
                .unwrap(),
            )),
            persona_agents: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            persona_session_dbs: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            shutdown_tx: None,
        }
    }

    // ---------- tests ----------

    #[test]
    fn test_responses_store_persists_messages_and_evicts_old_entries() {
        let path = std::env::temp_dir().join(format!(
            "hakimi-response-store-{}-{}.db",
            std::process::id(),
            unix_timestamp_millis()
        ));
        let _ = std::fs::remove_file(&path);

        let first_messages = vec![ChatCompletionsMessage {
            role: "user".to_string(),
            content: json!("first turn"),
        }];
        let second_messages = vec![ChatCompletionsMessage {
            role: "user".to_string(),
            content: json!("second turn"),
        }];

        {
            let mut store = ResponsesStore::with_path(&path, 1).unwrap();
            store.insert(
                "resp_1".to_string(),
                json!({"id": "resp_1", "status": "completed"}),
                first_messages.clone(),
            );
        }

        {
            let mut reopened = ResponsesStore::with_path(&path, 1).unwrap();
            assert_eq!(reopened.get("resp_1").unwrap()["id"], "resp_1");
            let messages = reopened.messages("resp_1").unwrap();
            assert_eq!(messages[0].role, "user");
            assert_eq!(messages[0].content, json!("first turn"));

            reopened.insert(
                "resp_2".to_string(),
                json!({"id": "resp_2", "status": "completed"}),
                second_messages,
            );
            assert!(reopened.get("resp_1").is_none());
            assert_eq!(reopened.get("resp_2").unwrap()["id"], "resp_2");
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(std::path::PathBuf::from(format!("{}-wal", path.display())));
        let _ = std::fs::remove_file(std::path::PathBuf::from(format!("{}-shm", path.display())));
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.status, "ok");
    }

    async fn read_json(resp: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn json_post(uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    fn json_patch(uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("PATCH")
            .uri(uri)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    #[tokio::test]
    async fn test_agents_list_includes_default() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/agents")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["default"], "default");
        let ids: Vec<&str> = json["agents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"default"));
    }

    #[tokio::test]
    async fn test_update_agent_toggles_addressable() {
        let app = build_router(test_state());

        // New personas are addressable by default (auto-exposed in the response).
        let resp = app
            .clone()
            .oneshot(json_post(
                "/api/agents",
                json!({"id": "coder", "name": "Coder"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["addressable"], true);

        // PATCH can turn it off.
        let resp = app
            .clone()
            .oneshot(json_patch(
                "/api/agents/coder",
                json!({"addressable": false}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["addressable"], false);
    }

    #[tokio::test]
    async fn test_agents_create_get_update_delete() {
        let app = build_router(test_state());

        // Create
        let resp = app
            .clone()
            .oneshot(json_post(
                "/api/agents",
                json!({"id": "coder", "name": "Coder"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["id"], "coder");
        assert_eq!(json["name"], "Coder");

        // Duplicate create rejected
        let resp = app
            .clone()
            .oneshot(json_post("/api/agents", json!({"id": "coder"})))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);

        // Get
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/agents/coder")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        // Update
        let resp = app
            .clone()
            .oneshot(json_patch(
                "/api/agents/coder",
                json!({"model": "claude-opus-4-8", "bindings": ["telegram:devbot"]}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["model"], "claude-opus-4-8");

        // Bindings overview reflects the update
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/bindings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = read_json(resp).await;
        assert_eq!(json["bindings"]["telegram:devbot"], "coder");

        // Delete
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/agents/coder")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        // Default persona cannot be deleted
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/agents/default")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_default_agent_chat_uses_stub_transport() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(json_post(
                "/api/agents/default/chat",
                json!({"message": "hello"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert!(
            json["response"].as_str().unwrap().contains("hello"),
            "stub transport echoes the prompt: {json:?}"
        );
    }

    #[tokio::test]
    async fn test_agent_chat_unknown_persona_is_404() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(json_post(
                "/api/agents/ghost/chat",
                json!({"message": "hi"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_delete_agent_syncs_gateway_map() {
        let state = test_state();
        let agents = state.persona_agents.clone();
        let app = build_router(state);

        // Creating a named persona registers its gateway agent.
        let resp = app
            .clone()
            .oneshot(json_post(
                "/api/agents",
                json!({"id": "coder", "name": "Coder"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        assert!(agents.read().await.contains_key("coder"));

        // Deleting it drops the gateway agent.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/agents/coder")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        assert!(!agents.read().await.contains_key("coder"));
    }

    #[tokio::test]
    async fn test_agent_skills_reflects_enabled() {
        let app = build_router(test_state());
        // Create a persona with an enabled skill.
        let resp = app
            .clone()
            .oneshot(json_post(
                "/api/agents",
                json!({"id": "coder", "enabled_skills": ["tdd"]}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents/coder/skills")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["enabled"][0], "tdd");
    }

    #[tokio::test]
    async fn test_agent_memory_ok_and_unknown_404() {
        let app = build_router(test_state());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/agents/default/memory")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert!(json["dir"].as_str().is_some());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents/ghost/memory")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_agent_sessions_ok_and_unknown_404() {
        let app = build_router(test_state());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/agents/default/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert!(json.is_array());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents/ghost/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_agent_chat_stream_default_emits_sse() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(json_post(
                "/api/agents/default/chat/stream",
                json!({"message": "hello"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let content_type = resp
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(
            content_type.starts_with("text/event-stream"),
            "content-type: {content_type}"
        );
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            body.contains("done"),
            "SSE body should end with a done event: {body}"
        );
    }

    #[tokio::test]
    async fn test_embedded_webui_assets_are_served_without_filesystem_cwd() {
        let state = test_state();
        let app = build_router(state);

        for (uri, expected_type, expected_body) in [
            ("/", "text/html", "/static/app.js"),
            ("/index.html", "text/html", "id=\"root\""),
            ("/static/app.js", "text/javascript", "persona"),
            ("/static/app.css", "text/css", "persona-rail"),
            ("/static/favicon.svg", "image/svg+xml", "<svg"),
            ("/static/icons.svg", "image/svg+xml", "<svg"),
            ("/favicon.svg", "image/svg+xml", "<svg"),
        ] {
            let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), http::StatusCode::OK, "{uri}");
            let content_type = resp
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            assert!(
                content_type.starts_with(expected_type),
                "{uri}: {content_type}"
            );

            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let body = String::from_utf8(body.to_vec()).unwrap();
            assert!(body.contains(expected_body), "{uri}");
        }
    }

    #[tokio::test]
    async fn test_workspace_root_path_slash_lists_workdir() {
        let _guard = WORKSPACE_CWD_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();
        std::fs::write(temp.path().join("hakimi-workspace-smoke.txt"), "ok").unwrap();

        let state = test_state();
        let app = build_router(state);
        let req = Request::builder()
            .uri("/api/workspace/list?path=%2F")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        std::env::set_current_dir(original).unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let names: Vec<&str> = json["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["name"].as_str())
            .collect();
        assert!(names.contains(&"hakimi-workspace-smoke.txt"));
    }

    #[tokio::test]
    async fn test_workspace_path_escape_still_forbidden() {
        let _guard = WORKSPACE_CWD_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();

        let state = test_state();
        let app = build_router(state);
        let req = Request::builder()
            .uri("/api/workspace/list?path=..")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        std::env::set_current_dir(original).unwrap();

        assert_eq!(resp.status(), http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_unknown_static_asset_returns_404() {
        let state = test_state();
        let app = build_router(state);
        let req = Request::builder()
            .uri("/static/missing.js")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_v1_models_endpoint() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/v1/models")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let models: ModelsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(models.object, "list");
        assert_eq!(models.data.len(), 1);
        assert_eq!(models.data[0].id, "test-model");
        assert_eq!(models.data[0].object, "model");
        assert_eq!(models.data[0].owned_by, "hakimi");
        assert_eq!(models.data[0].root, "test-model");
        assert!(models.data[0].parent.is_none());
    }

    #[tokio::test]
    async fn test_v1_capabilities_endpoint() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/v1/capabilities")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let capabilities: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(capabilities["object"], "hakimi.api_server.capabilities");
        assert_eq!(capabilities["platform"], "hakimi-agent");
        assert_eq!(capabilities["model"], "test-model");
        assert_eq!(capabilities["auth"]["type"], "bearer");
        assert_eq!(capabilities["runtime"]["mode"], "server_agent");
        assert_eq!(capabilities["runtime"]["tool_execution"], "server");
        assert_eq!(capabilities["runtime"]["split_runtime"], false);
        assert_eq!(capabilities["features"]["chat"], true);
        assert_eq!(capabilities["features"]["session_resources"], true);
        assert_eq!(capabilities["features"]["session_create"], true);
        assert_eq!(capabilities["features"]["session_update"], true);
        assert_eq!(capabilities["features"]["session_delete"], true);
        assert_eq!(capabilities["features"]["session_fork"], true);
        assert_eq!(capabilities["features"]["session_chat"], false);
        assert_eq!(capabilities["features"]["session_chat_streaming"], false);
        assert_eq!(capabilities["features"]["session_messages"], true);
        assert_eq!(capabilities["features"]["session_search"], true);
        assert_eq!(capabilities["features"]["tools_api"], true);
        assert_eq!(capabilities["features"]["chat_completions"], true);
        assert_eq!(capabilities["features"]["chat_completions_streaming"], true);
        assert_eq!(
            capabilities["features"]["chat_completions_streaming_mode"],
            "completed_sse_snapshot"
        );
        assert_eq!(capabilities["features"]["responses_api"], true);
        assert_eq!(capabilities["features"]["responses_streaming"], true);
        assert_eq!(
            capabilities["features"]["responses_streaming_mode"],
            "completed_sse_snapshot"
        );
        assert_eq!(
            capabilities["features"]["responses_persistence"],
            "sqlite_lru"
        );
        assert_eq!(capabilities["features"]["skills_api"], true);
        assert_eq!(capabilities["features"]["toolsets_api"], true);
        assert_eq!(capabilities["features"]["run_submission"], true);
        assert_eq!(capabilities["features"]["run_status"], true);
        assert_eq!(capabilities["features"]["run_events_sse"], true);
        assert_eq!(
            capabilities["features"]["run_events_streaming_mode"],
            "live_lifecycle_sse"
        );
        assert_eq!(capabilities["features"]["run_stop"], true);
        assert_eq!(
            capabilities["endpoints"]["models"],
            json!({"method": "GET", "path": "/v1/models"})
        );
        assert_eq!(
            capabilities["endpoints"]["skills"],
            json!({"method": "GET", "path": "/v1/skills"})
        );
        assert_eq!(
            capabilities["endpoints"]["toolsets"],
            json!({"method": "GET", "path": "/v1/toolsets"})
        );
        assert_eq!(
            capabilities["endpoints"]["chat_completions"],
            json!({"method": "POST", "path": "/v1/chat/completions"})
        );
        assert_eq!(
            capabilities["endpoints"]["responses"],
            json!({"method": "POST", "path": "/v1/responses"})
        );
        assert_eq!(
            capabilities["endpoints"]["run"],
            json!({"method": "POST", "path": "/v1/runs"})
        );
        assert_eq!(
            capabilities["endpoints"]["run_status"],
            json!({"method": "GET", "path": "/v1/runs/{id}"})
        );
        assert_eq!(
            capabilities["endpoints"]["run_events"],
            json!({"method": "GET", "path": "/v1/runs/{id}/events"})
        );
        assert_eq!(
            capabilities["endpoints"]["run_stop"],
            json!({"method": "POST", "path": "/v1/runs/{id}/stop"})
        );
        assert_eq!(
            capabilities["endpoints"]["chat"],
            json!({"method": "POST", "path": "/api/chat"})
        );
        assert_eq!(
            capabilities["endpoints"]["session_create"],
            json!({"method": "POST", "path": "/api/sessions"})
        );
        assert_eq!(
            capabilities["endpoints"]["session_update"],
            json!({"method": "PATCH", "path": "/api/sessions/{id}"})
        );
        assert_eq!(
            capabilities["endpoints"]["session_delete"],
            json!({"method": "DELETE", "path": "/api/sessions/{id}"})
        );
        assert_eq!(
            capabilities["endpoints"]["session_fork"],
            json!({"method": "POST", "path": "/api/sessions/{id}/fork"})
        );
        assert_eq!(
            capabilities["endpoints"]["session_messages"],
            json!({"method": "GET", "path": "/api/sessions/{id}/messages"})
        );
        assert_eq!(
            capabilities["endpoints"]["session_search"],
            json!({"method": "GET", "path": "/api/sessions/search?q=<query>"})
        );
        assert_eq!(capabilities["dashboard_admin"]["status"], true);
        assert_eq!(capabilities["dashboard_admin"]["mcp_servers_read"], true);
        assert_eq!(capabilities["dashboard_admin"]["kanban_read"], true);
        assert_eq!(capabilities["dashboard_admin"]["kanban_write"], true);
        assert_eq!(
            capabilities["endpoints"]["dashboard_status"],
            json!({"method": "GET", "path": "/api/status"})
        );
        assert_eq!(
            capabilities["endpoints"]["mcp_servers"],
            json!({"method": "GET", "path": "/api/mcp/servers"})
        );
        assert_eq!(capabilities["dashboard_admin"]["write_operations"], true);
        assert_eq!(
            capabilities["endpoints"]["mcp_server_add"],
            json!({"method": "POST", "path": "/api/mcp/servers"})
        );
        assert_eq!(
            capabilities["endpoints"]["kanban"],
            json!({"method": "GET", "path": "/api/kanban"})
        );
        assert_eq!(
            capabilities["endpoints"]["kanban_boards"],
            json!({"method": "GET", "path": "/api/kanban/boards"})
        );
        assert_eq!(
            capabilities["endpoints"]["kanban_task"],
            json!({"method": "GET", "path": "/api/kanban/tasks/{id}"})
        );
        assert_eq!(
            capabilities["endpoints"]["kanban_task_create"],
            json!({"method": "POST", "path": "/api/kanban/tasks"})
        );
        assert_eq!(
            capabilities["endpoints"]["kanban_task_update"],
            json!({"method": "PATCH", "path": "/api/kanban/tasks/{id}"})
        );
        assert_eq!(
            capabilities["endpoints"]["kanban_task_comment"],
            json!({"method": "POST", "path": "/api/kanban/tasks/{id}/comments"})
        );
    }

    #[tokio::test]
    async fn test_v1_skills_endpoint_lists_metadata_without_content() {
        let state = test_state();
        {
            let _agent = state.agent.lock().await;
            let mut skill = hakimi_skills::Skill::new(
                "release-check",
                "# Release checklist\n- Do not expose this body",
            );
            skill.description = "Checks release readiness".to_string();
            skill.trigger = Some("when preparing a release".to_string());
            skill.tags = vec!["release".to_string(), "ci".to_string()];
            skill.phases = vec![hakimi_skills::HarnessPhase::Validate];
            skill.platforms = vec!["linux".to_string(), "windows".to_string()];

            let mut store = hakimi_skills::SkillStore::from_skills(vec![skill]);
            store.observe("release validation failed");
            // TODO: restore when DispatchedAgent is implemented
            // let base = agent.base_agent_mut();
            // *base = base.clone().with_skill_store(Some(store));
        }
        let app = build_router(state);

        let req = Request::builder()
            .uri("/v1/skills")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let skills: SkillsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(skills.object, "list");
        assert_eq!(skills.total, 1);
        assert_eq!(skills.active, vec!["release-check"]);
        assert_eq!(skills.data[0].name, "release-check");
        assert_eq!(skills.data[0].description, "Checks release readiness");
        assert_eq!(skills.data[0].phases, vec!["validate"]);
        assert_eq!(skills.data[0].provenance, "local/local");
        assert!(skills.data[0].active);

        let raw = String::from_utf8(body.to_vec()).unwrap();
        assert!(!raw.contains("Do not expose this body"));
    }

    #[tokio::test]
    async fn test_v1_toolsets_endpoint_groups_registered_tools() {
        let state = test_state();
        state
            .agent
            .lock()
            .await
            .tool_registry()
            .register(Arc::new(MockTool))
            .await;
        let app = build_router(state);

        let req = Request::builder()
            .uri("/v1/toolsets")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let toolsets: ToolsetsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(toolsets.object, "list");
        assert_eq!(toolsets.total_toolsets, 1);
        assert_eq!(toolsets.total_tools, 1);
        assert_eq!(toolsets.data[0].name, "test");
        assert_eq!(toolsets.data[0].source, "core");
        assert!(!toolsets.data[0].deferrable);
        assert_eq!(toolsets.data[0].tools[0].name, "mock_tool");
    }

    #[tokio::test]
    async fn test_v1_chat_completions_non_streaming_endpoint() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "model": "custom-test-model",
                    "messages": [
                        {"role": "system", "content": "Answer tersely."},
                        {"role": "user", "content": [
                            {"type": "text", "text": "Say hello"},
                            {"type": "input_text", "text": "and mention Hakimi"}
                        ]}
                    ],
                    "stream": "false"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let completion: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(completion["object"], "chat.completion");
        assert_eq!(completion["model"], "custom-test-model");
        assert_eq!(completion["choices"][0]["message"]["role"], "assistant");
        let content = completion["choices"][0]["message"]["content"]
            .as_str()
            .unwrap();
        assert!(content.contains("Conversation supplied through OpenAI Chat Completions"));
        assert!(content.contains("Say hello"));
        assert!(content.contains("and mention Hakimi"));
        assert_eq!(completion["usage"]["total_tokens"], 10);

        let agent = state.agent.lock().await;
        assert!(
            agent.messages().is_empty(),
            "OpenAI-compatible chat completions should not mutate the shared /api/chat history"
        );
    }

    #[tokio::test]
    async fn test_v1_chat_completions_streaming_returns_sse_snapshot() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "messages": [{"role": "user", "content": "hello"}],
                    "stream": true
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let content_type = resp
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(content_type.starts_with("text/event-stream"));

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let events = String::from_utf8(body.to_vec()).unwrap();
        assert!(events.contains("\"object\":\"chat.completion.chunk\""));
        assert!(events.contains("\"delta\":{\"role\":\"assistant\"}"));
        assert!(events.contains("\"delta\":{\"content\":\""));
        assert!(events.contains("Conversation supplied through OpenAI Chat Completions"));
        assert!(events.contains("\"finish_reason\":\"stop\""));
        assert!(events.ends_with("data: [DONE]\n\n"));

        let agent = state.agent.lock().await;
        assert!(
            agent.messages().is_empty(),
            "streaming chat completions should not mutate the shared /api/chat history"
        );
    }

    #[tokio::test]
    async fn test_api_chat_stream_closes_after_done_event() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/chat/stream")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "message": "hello" }).to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let mut stream = resp.into_body().into_data_stream();
        let mut events = String::new();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.unwrap();
                events.push_str(&String::from_utf8_lossy(&chunk));
                if events.contains("event: done") {
                    return;
                }
            }
            panic!("/api/chat/stream ended before sending done event: {events:?}");
        })
        .await
        .expect("/api/chat/stream should emit done promptly");
        assert!(events.contains("stub response to: hello"));
    }

    #[tokio::test]
    async fn test_api_chat_stream_persists_messages_to_requested_session() {
        let state = test_state();
        let app = build_router(state.clone());
        {
            let db = state.session_db.lock().await;
            db.create_session_with_id(
                "persist-session",
                "webui",
                None,
                Some("test-model"),
                None,
                None,
            )
            .unwrap();
        }

        for message in ["first", "second"] {
            let req = Request::builder()
                .method("POST")
                .uri("/api/chat/stream")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "message": message, "session_id": "persist-session" }).to_string(),
                ))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), http::StatusCode::OK);

            let mut stream = resp.into_body().into_data_stream();
            let mut events = String::new();
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.unwrap();
                    events.push_str(&String::from_utf8_lossy(&chunk));
                    if events.contains("event: done") {
                        return;
                    }
                }
                panic!("/api/chat/stream ended before sending done event: {events:?}");
            })
            .await
            .expect("/api/chat/stream should emit done promptly");
        }

        let db = state.session_db.lock().await;
        let messages = db.get_messages("persist-session").unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].content.as_deref(), Some("first"));
        assert_eq!(
            messages[1].content.as_deref(),
            Some("stub response to: first")
        );
        assert_eq!(messages[2].content.as_deref(), Some("second"));
        assert_eq!(
            messages[3].content.as_deref(),
            Some("stub response to: second")
        );
    }

    #[tokio::test]
    async fn test_v1_responses_non_streaming_endpoint_and_store() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "model": "responses-test-model",
                    "instructions": "Answer as a test harness.",
                    "input": [
                        {"role": "user", "content": [
                            {"type": "input_text", "text": "Summarize Hakimi"},
                            {"type": "text", "text": "in one sentence"}
                        ]}
                    ],
                    "stream": "false"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(response["object"], "response");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["model"], "responses-test-model");
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][0]["role"], "assistant");
        assert_eq!(response["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(response["usage"]["total_tokens"], 10);
        let output_text = response["output_text"].as_str().unwrap();
        assert!(output_text.contains("Conversation supplied through OpenAI Responses API"));
        assert!(output_text.contains("Summarize Hakimi"));
        assert!(output_text.contains("in one sentence"));

        let response_id = response["id"].as_str().unwrap();
        let get_req = Request::builder()
            .uri(format!("/v1/responses/{response_id}"))
            .body(Body::empty())
            .unwrap();
        let get_resp = app.clone().oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), http::StatusCode::OK);
        let get_body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
        assert_eq!(stored["id"], response["id"]);

        let agent = state.agent.lock().await;
        assert!(
            agent.messages().is_empty(),
            "Responses API should not mutate the shared /api/chat history"
        );
    }

    #[tokio::test]
    async fn test_v1_responses_previous_response_id_chains_history() {
        let state = test_state();
        let app = build_router(state);

        let first_req = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .body(Body::from(json!({"input": "First turn"}).to_string()))
            .unwrap();
        let first_resp = app.clone().oneshot(first_req).await.unwrap();
        assert_eq!(first_resp.status(), http::StatusCode::OK);
        let first_body = axum::body::to_bytes(first_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let first: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
        let first_id = first["id"].as_str().unwrap();

        let second_req = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "previous_response_id": first_id,
                    "input": "Second turn"
                })
                .to_string(),
            ))
            .unwrap();
        let second_resp = app.oneshot(second_req).await.unwrap();
        assert_eq!(second_resp.status(), http::StatusCode::OK);
        let second_body = axum::body::to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let second: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
        let output = second["output_text"].as_str().unwrap();
        assert_eq!(second["previous_response_id"], first["id"]);
        assert!(output.contains("First turn"));
        assert!(output.contains("Second turn"));
    }

    #[tokio::test]
    async fn test_v1_responses_delete_and_missing_previous_response() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"input": "temporary response"}).to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let response_id = response["id"].as_str().unwrap();

        let delete_req = Request::builder()
            .method("DELETE")
            .uri(format!("/v1/responses/{response_id}"))
            .body(Body::empty())
            .unwrap();
        let delete_resp = app.clone().oneshot(delete_req).await.unwrap();
        assert_eq!(delete_resp.status(), http::StatusCode::OK);

        let get_req = Request::builder()
            .uri(format!("/v1/responses/{response_id}"))
            .body(Body::empty())
            .unwrap();
        let get_resp = app.clone().oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), http::StatusCode::NOT_FOUND);

        let missing_chain_req = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "previous_response_id": "resp_missing",
                    "input": "continue"
                })
                .to_string(),
            ))
            .unwrap();
        let missing_chain_resp = app.oneshot(missing_chain_req).await.unwrap();
        assert_eq!(missing_chain_resp.status(), http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_v1_responses_streaming_returns_sse_snapshot_and_store() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "input": "hello",
                    "stream": true
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let content_type = resp
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(content_type.starts_with("text/event-stream"));

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let events = String::from_utf8(body.to_vec()).unwrap();
        assert!(events.contains("event: response.created"));
        assert!(events.contains("event: response.output_text.delta"));
        assert!(events.contains("event: response.completed"));
        assert!(events.contains("data: [DONE]"));
        assert!(
            events.contains("stub response to: Conversation supplied through OpenAI Responses API")
        );

        let completed_line = events
            .lines()
            .find(|line| {
                line.starts_with("data: {") && line.contains("\"type\":\"response.completed\"")
            })
            .expect("completed response event should include the full response");
        let completed: serde_json::Value =
            serde_json::from_str(completed_line.trim_start_matches("data: ")).unwrap();
        let response_id = completed["response"]["id"].as_str().unwrap();

        let get_req = Request::builder()
            .uri(format!("/v1/responses/{response_id}"))
            .body(Body::empty())
            .unwrap();
        let get_resp = app.oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_v1_runs_submit_and_poll_status() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/v1/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "model": "runs-test-model",
                    "session_id": "external-session-1",
                    "instructions": "Answer as a background worker.",
                    "input": "Summarize the run API"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::ACCEPTED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let submitted: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let run_id = submitted["id"].as_str().unwrap().to_string();
        assert_eq!(submitted["object"], "hakimi.run");
        assert_eq!(submitted["session_id"], "external-session-1");
        assert_eq!(submitted["model"], "runs-test-model");
        assert!(matches!(
            submitted["status"].as_str().unwrap(),
            "queued" | "running" | "completed"
        ));

        let mut polled = submitted;
        for _ in 0..20 {
            let req = Request::builder()
                .uri(format!("/v1/runs/{run_id}"))
                .body(Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), http::StatusCode::OK);
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            polled = serde_json::from_slice(&body).unwrap();
            if polled["status"] == "completed" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        assert_eq!(polled["status"], "completed");
        let output_text = polled["output_text"].as_str().unwrap();
        assert!(output_text.contains("Conversation supplied through Hakimi Runs API"));
        assert!(output_text.contains("Summarize the run API"));
        assert_eq!(polled["usage"]["total_tokens"], 10);

        let req = Request::builder()
            .uri(format!("/v1/runs/{run_id}/events"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let content_type = resp
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(content_type.starts_with("text/event-stream"));
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let events = String::from_utf8(body.to_vec()).unwrap();
        assert!(events.contains("event: run.queued"));
        assert!(events.contains("event: run.running"));
        assert!(events.contains("event: run.completed"));

        let agent = state.agent.lock().await;
        assert!(
            agent.messages().is_empty(),
            "Runs API should not mutate the shared /api/chat history"
        );
    }

    #[tokio::test]
    async fn test_v1_runs_rejects_streaming_for_now() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "input": "hello",
                    "stream": true
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_v1_runs_stop_cancels_running_run() {
        let state = test_state_with_transport(Arc::new(SlowTransport));
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/runs")
            .header("content-type", "application/json")
            .body(Body::from(json!({"input": "wait for stop"}).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::ACCEPTED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let submitted: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let run_id = submitted["id"].as_str().unwrap().to_string();

        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/runs/{run_id}/stop"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let stopped: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(stopped["status"], "cancelled");
        assert_eq!(stopped["error"], "Stop requested via API");

        let req = Request::builder()
            .uri(format!("/v1/runs/{run_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let polled: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(polled["status"], "cancelled");

        let req = Request::builder()
            .uri(format!("/v1/runs/{run_id}/events"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let events = String::from_utf8(body.to_vec()).unwrap();
        assert!(events.contains("event: run.queued"));
        assert!(events.contains("event: run.cancelled"));
    }

    #[tokio::test]
    async fn test_v1_runs_events_stream_waits_for_live_terminal_event() {
        let state = test_state_with_transport(Arc::new(SlowTransport));
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"input": "stream until stop"}).to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::ACCEPTED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let submitted: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let run_id = submitted["id"].as_str().unwrap().to_string();

        let events_req = Request::builder()
            .uri(format!("/v1/runs/{run_id}/events"))
            .body(Body::empty())
            .unwrap();
        let events_app = app.clone();
        let events_task = tokio::spawn(async move {
            let resp = events_app.oneshot(events_req).await.unwrap();
            assert_eq!(resp.status(), http::StatusCode::OK);
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
        });

        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert!(
            !events_task.is_finished(),
            "live run event stream should remain open before a terminal event"
        );

        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/runs/{run_id}/stop"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = tokio::time::timeout(std::time::Duration::from_secs(2), events_task)
            .await
            .expect("events stream should close after cancellation")
            .unwrap();
        let events = String::from_utf8(body.to_vec()).unwrap();
        assert!(events.contains("event: run.queued"));
        assert!(events.contains("event: run.running"));
        assert!(events.contains("event: run.cancelled"));
        assert!(events.contains("\"sequence\":0"));
    }

    #[tokio::test]
    async fn test_v1_runs_stop_finished_run_returns_conflict() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/runs")
            .header("content-type", "application/json")
            .body(Body::from(json!({"input": "finish first"}).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let submitted: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let run_id = submitted["id"].as_str().unwrap().to_string();

        let mut completed = false;
        for _ in 0..20 {
            let req = Request::builder()
                .uri(format!("/v1/runs/{run_id}"))
                .body(Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let polled: serde_json::Value = serde_json::from_slice(&body).unwrap();
            if polled["status"] == "completed" {
                completed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            completed,
            "run should complete before stop conflict assertion"
        );

        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/runs/{run_id}/stop"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_v1_runs_missing_run_returns_404() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/v1/runs/run_missing")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn chat_completions_prompt_rejects_image_parts() {
        let messages = vec![ChatCompletionsMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "describe this"},
                {"type": "image_url", "image_url": {"url": "https://example.com/a.png"}}
            ]),
        }];
        let err = chat_completions_prompt(&messages).unwrap_err();
        assert_eq!(err.0, http::StatusCode::BAD_REQUEST);
        assert!(err.1.0.error.contains("text-only chat completions"));
    }

    #[tokio::test]
    async fn test_dashboard_status_endpoint() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(status["object"], "hakimi.dashboard.status");
        assert_eq!(status["status"], "ok");
        assert_eq!(status["model"], "test-model");
        assert_eq!(status["dashboard_admin"]["readonly"], false);
        assert_eq!(status["resources"]["mcp_servers"], 0);
    }

    #[tokio::test]
    async fn test_dashboard_mcp_servers_redacts_env_values() {
        let state = test_state();
        {
            let mut config = state.config.lock().await;
            config.mcp_servers.insert(
                "demo".to_string(),
                hakimi_config::McpServerConfig {
                    command: "npx".to_string(),
                    args: vec!["-y".to_string(), "demo-mcp".to_string()],
                    env: std::collections::HashMap::from([(
                        "API_KEY".to_string(),
                        "test-mcp-secret-value".to_string(),
                    )]),
                },
            );
        }
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/mcp/servers")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_text.contains("test-mcp-secret-value"));
        let mcp: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(mcp["object"], "hakimi.dashboard.mcp_servers");
        assert_eq!(mcp["count"], 1);
        assert_eq!(mcp["servers"][0]["name"], "demo");
        assert_eq!(mcp["servers"][0]["transport"], "stdio");
        assert_eq!(mcp["servers"][0]["args_count"], 2);
        assert_eq!(mcp["servers"][0]["env"]["API_KEY"], "<redacted>");
    }

    #[tokio::test]
    async fn test_dashboard_mcp_server_add_delete_runtime_config() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/api/mcp/servers")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "name": "demo",
                    "command": "npx",
                    "args": ["-y", "demo-mcp"],
                    "env": {"API_KEY": "test-mcp-secret-value"}
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_text.contains("test-mcp-secret-value"));
        let created: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(created["server"]["name"], "demo");
        assert_eq!(created["server"]["env"]["API_KEY"], "<redacted>");

        {
            let config = state.config.lock().await;
            let server = config.mcp_servers.get("demo").unwrap();
            assert_eq!(server.command, "npx");
            assert_eq!(server.env.get("API_KEY").unwrap(), "test-mcp-secret-value");
        }

        let req = Request::builder()
            .method("DELETE")
            .uri("/api/mcp/servers/demo")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let config = state.config.lock().await;
        assert!(!config.mcp_servers.contains_key("demo"));
    }

    #[tokio::test]
    async fn test_dashboard_mcp_server_add_rejects_url_transport_for_now() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/mcp/servers")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"name": "remote", "url": "https://example.com/mcp"}).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_dashboard_credential_pool_redacts_api_keys() {
        let state = test_state();
        {
            let mut config = state.config.lock().await;
            config.credential_pools.insert(
                "openrouter".to_string(),
                hakimi_config::CredentialPoolConfig {
                    strategy: Some("fill_first".to_string()),
                    credentials: vec![hakimi_config::CredentialConfig {
                        id: Some("primary".to_string()),
                        api_key: "test-openrouter-secret-value".to_string(),
                        base_url: Some("https://openrouter.ai/api/v1".to_string()),
                        org_id: None,
                        source: Some("manual:test".to_string()),
                        priority: Some(10),
                        max_concurrent: Some(3),
                    }],
                },
            );
        }
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/credentials/pool")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_text.contains("test-openrouter-secret-value"));
        let pool: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(pool["object"], "hakimi.dashboard.credential_pool");
        assert_eq!(pool["providers"][0]["provider"], "openrouter");
        assert_eq!(pool["providers"][0]["strategy"], "fill_first");
        assert_eq!(pool["providers"][0]["entries"][0]["id"], "primary");
        assert_eq!(pool["providers"][0]["entries"][0]["has_api_key"], true);
        assert_eq!(
            pool["providers"][0]["entries"][0]["base_url_configured"],
            true
        );
    }

    #[tokio::test]
    async fn test_dashboard_credential_pool_add_delete_runtime_entry() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/api/credentials/pool")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "provider": "openrouter",
                    "api_key": "test-openrouter-secret-value",
                    "label": "primary",
                    "base_url": "https://openrouter.ai/api/v1",
                    "strategy": "fill_first"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_text.contains("test-openrouter-secret-value"));
        let pool: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(pool["provider"]["provider"], "openrouter");
        assert_eq!(pool["provider"]["entries"][0]["id"], "primary");
        assert_eq!(pool["provider"]["entries"][0]["has_api_key"], true);

        {
            let config = state.config.lock().await;
            let entry = &config.credential_pools["openrouter"].credentials[0];
            assert_eq!(entry.api_key, "test-openrouter-secret-value");
            assert_eq!(entry.source.as_deref(), Some("dashboard:runtime"));
        }

        let req = Request::builder()
            .method("DELETE")
            .uri("/api/credentials/pool/openrouter/1")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let config = state.config.lock().await;
        assert!(!config.credential_pools.contains_key("openrouter"));
    }

    #[tokio::test]
    async fn test_dashboard_webhooks_redacts_secret() {
        let state = test_state();
        {
            let mut config = state.config.lock().await;
            config.gateways.webhook.enabled = true;
            config.gateways.webhook.bot_id = "webhook-main".to_string();
            config.gateways.webhook.port = 9100;
            config.gateways.webhook.path = "/hooks".to_string();
            config.gateways.webhook.secret = "test-webhook-secret-value".to_string();
        }
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/webhooks")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_text.contains("test-webhook-secret-value"));
        let webhooks: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(webhooks["object"], "hakimi.dashboard.webhooks");
        assert_eq!(webhooks["enabled"], true);
        assert_eq!(webhooks["bot_id"], "webhook-main");
        assert_eq!(webhooks["port"], 9100);
        assert_eq!(webhooks["path"], "/hooks");
        assert_eq!(webhooks["secret_configured"], true);
    }

    #[tokio::test]
    async fn test_dashboard_webhook_update_redacts_runtime_secret() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/api/webhooks")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "enabled": true,
                    "bot_id": "webhook-admin",
                    "port": 9091,
                    "path": "/events",
                    "secret": "test-webhook-secret-value"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_text.contains("test-webhook-secret-value"));
        let webhooks: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(webhooks["enabled"], true);
        assert_eq!(webhooks["bot_id"], "webhook-admin");
        assert_eq!(webhooks["port"], 9091);
        assert_eq!(webhooks["path"], "/events");
        assert_eq!(webhooks["secret_configured"], true);

        let config = state.config.lock().await;
        assert_eq!(config.gateways.webhook.secret, "test-webhook-secret-value");
    }

    #[tokio::test]
    async fn test_dashboard_webhook_update_rejects_relative_path() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/webhooks")
            .header("content-type", "application/json")
            .body(Body::from(json!({"path": "events"}).to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_dashboard_kanban_snapshot_boards_and_task_detail() {
        let _lock = KANBAN_ENV_LOCK.lock().await;
        let db_path = std::env::temp_dir().join(format!(
            "hakimi-kanban-dashboard-{}-{}.db",
            std::process::id(),
            unix_timestamp_millis()
        ));
        let home_path = std::env::temp_dir().join(format!(
            "hakimi-kanban-home-{}-{}",
            std::process::id(),
            unix_timestamp_millis()
        ));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&home_path);

        let _db = EnvVarGuard::set("HAKIMI_KANBAN_DB", db_path.as_os_str());
        let _home = EnvVarGuard::set("HAKIMI_KANBAN_HOME", home_path.as_os_str());
        let _hermes_db = EnvVarGuard::remove("HERMES_KANBAN_DB");
        let _hermes_home = EnvVarGuard::remove("HERMES_KANBAN_HOME");
        let _board = EnvVarGuard::remove("HAKIMI_KANBAN_BOARD");
        let _hermes_board = EnvVarGuard::remove("HERMES_KANBAN_BOARD");

        let created = hakimi_tools::kanban_response(Some("create Dashboard task"));
        let created: serde_json::Value = serde_json::from_str(&created).unwrap();
        let task_id = created["id"].as_str().unwrap().to_string();
        let comment_command = format!("comment {task_id} Reviewed from dashboard test");
        let comment = hakimi_tools::kanban_response(Some(&comment_command));
        let comment: serde_json::Value = serde_json::from_str(&comment).unwrap();
        assert_eq!(comment["body"], "Reviewed from dashboard test");

        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/kanban?status=todo&limit=5")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(snapshot["object"], "hakimi.dashboard.kanban");
        assert_eq!(snapshot["board"]["slug"], "default");
        assert_eq!(snapshot["filters"]["status"], "todo");
        assert_eq!(snapshot["count"], 1);
        assert_eq!(snapshot["tasks"][0]["id"], task_id);
        assert_eq!(snapshot["tasks"][0]["title"], "Dashboard task");
        assert_eq!(snapshot["write_operations"], true);

        let req = Request::builder()
            .uri("/api/kanban/boards")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let boards: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(boards["current"], "default");
        assert_eq!(boards["boards"][0]["slug"], "default");

        let req = Request::builder()
            .uri(format!("/api/kanban/tasks/{task_id}?event_limit=5"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(detail["object"], "hakimi.dashboard.kanban.task");
        assert_eq!(detail["task"]["id"], task_id);
        assert_eq!(
            detail["comments"][0]["body"],
            "Reviewed from dashboard test"
        );
        assert!(detail["events"].as_array().unwrap().len() >= 2);
        assert_eq!(detail["write_operations"], true);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(std::path::PathBuf::from(format!(
            "{}-wal",
            db_path.display()
        )));
        let _ = std::fs::remove_file(std::path::PathBuf::from(format!(
            "{}-shm",
            db_path.display()
        )));
        let _ = std::fs::remove_dir_all(&home_path);
    }

    #[tokio::test]
    async fn test_dashboard_kanban_write_api_creates_updates_and_comments() {
        let _lock = KANBAN_ENV_LOCK.lock().await;
        let db_path = std::env::temp_dir().join(format!(
            "hakimi-kanban-dashboard-write-{}-{}.db",
            std::process::id(),
            unix_timestamp_millis()
        ));
        let home_path = std::env::temp_dir().join(format!(
            "hakimi-kanban-home-write-{}-{}",
            std::process::id(),
            unix_timestamp_millis()
        ));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&home_path);

        let _db = EnvVarGuard::set("HAKIMI_KANBAN_DB", db_path.as_os_str());
        let _home = EnvVarGuard::set("HAKIMI_KANBAN_HOME", home_path.as_os_str());
        let _hermes_db = EnvVarGuard::remove("HERMES_KANBAN_DB");
        let _hermes_home = EnvVarGuard::remove("HERMES_KANBAN_HOME");
        let _board = EnvVarGuard::remove("HAKIMI_KANBAN_BOARD");
        let _hermes_board = EnvVarGuard::remove("HERMES_KANBAN_BOARD");

        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/kanban/tasks?event_limit=10")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Dashboard write task",
                    "body": "Created from the dashboard write API",
                    "assignee": "operator",
                    "priority": 3
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(created["object"], "hakimi.dashboard.kanban.task");
        assert_eq!(created["task"]["title"], "Dashboard write task");
        assert_eq!(created["task"]["assignee"], "operator");
        assert_eq!(created["task"]["priority"], 3);
        assert_eq!(created["write_operations"], true);
        let task_id = created["task"]["id"].as_str().unwrap().to_string();

        let req = Request::builder()
            .method("PATCH")
            .uri(format!("/api/kanban/tasks/{task_id}?event_limit=10"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "status": "blocked",
                    "blocked_reason": "Need operator review",
                    "comment": "Blocked from dashboard",
                    "author": "dashboard-reviewer"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let blocked: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(blocked["task"]["status"], "blocked");
        assert_eq!(blocked["task"]["blocked_reason"], "Need operator review");
        assert!(
            blocked["comments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|comment| comment["body"] == "Blocked from dashboard")
        );
        assert!(
            blocked["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"] == "blocked")
        );

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/kanban/tasks/{task_id}/comments"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "body": "Follow-up from dashboard",
                    "author": "ops"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let commented: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            commented["comments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|comment| comment["body"] == "Follow-up from dashboard"
                    && comment["author"] == "ops")
        );

        let req = Request::builder()
            .method("POST")
            .uri("/api/kanban/tasks")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Invalid blocked task",
                    "status": "blocked"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(std::path::PathBuf::from(format!(
            "{}-wal",
            db_path.display()
        )));
        let _ = std::fs::remove_file(std::path::PathBuf::from(format!(
            "{}-shm",
            db_path.display()
        )));
        let _ = std::fs::remove_dir_all(&home_path);
    }

    #[tokio::test]
    async fn test_dashboard_kanban_unknown_board_returns_404() {
        let _lock = KANBAN_ENV_LOCK.lock().await;
        let db_path = std::env::temp_dir().join(format!(
            "hakimi-kanban-dashboard-missing-{}-{}.db",
            std::process::id(),
            unix_timestamp_millis()
        ));
        let home_path = std::env::temp_dir().join(format!(
            "hakimi-kanban-home-missing-{}-{}",
            std::process::id(),
            unix_timestamp_millis()
        ));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&home_path);

        let _db = EnvVarGuard::set("HAKIMI_KANBAN_DB", db_path.as_os_str());
        let _home = EnvVarGuard::set("HAKIMI_KANBAN_HOME", home_path.as_os_str());
        let _hermes_db = EnvVarGuard::remove("HERMES_KANBAN_DB");
        let _hermes_home = EnvVarGuard::remove("HERMES_KANBAN_HOME");
        let _board = EnvVarGuard::remove("HAKIMI_KANBAN_BOARD");
        let _hermes_board = EnvVarGuard::remove("HERMES_KANBAN_BOARD");

        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/kanban?board=missing-board")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&home_path);
    }

    #[tokio::test]
    async fn test_list_tools_endpoint() {
        let state = test_state();
        // Register a mock tool
        state
            .agent
            .lock()
            .await
            .tool_registry()
            .register(Arc::new(MockTool))
            .await;

        let app = build_router(state);
        let req = Request::builder()
            .uri("/api/tools")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let tools: Vec<ToolInfo> = serde_json::from_slice(&body).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "mock_tool");
    }

    #[tokio::test]
    async fn test_get_config_endpoint() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/config")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let config: SanitizedConfig = serde_json::from_slice(&body).unwrap();
        assert_eq!(config.agent_max_turns, 90);
        assert!(!config.agent_save_trajectories);
        assert_eq!(config.agent_trajectory_dir, "");
        assert_eq!(config.compression_engine, "smart");
        assert_eq!(config.compression_model, "");
        assert!(config.embedding_enabled);
        assert_eq!(config.embedding_model, "BAAI/bge-m3");
    }

    #[tokio::test]
    async fn test_update_config_endpoint() {
        let state = test_state();
        let app = build_router(state);

        let update = json!({
            "agent_max_turns": 42,
            "agent_verbose": true,
            "agent_save_trajectories": true,
            "agent_trajectory_dir": "./trajectories",
            "compression_engine": "llm",
            "compression_model": "claude-3-5-haiku-latest",
            "embedding_enabled": false
        });
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/api/config")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&update).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let config: SanitizedConfig = serde_json::from_slice(&body).unwrap();
        assert_eq!(config.agent_max_turns, 42);
        assert!(config.agent_verbose);
        assert!(config.agent_save_trajectories);
        assert_eq!(config.agent_trajectory_dir, "./trajectories");
        assert_eq!(config.compression_engine, "llm");
        assert_eq!(config.compression_model, "claude-3-5-haiku-latest");
        assert!(!config.embedding_enabled);
    }

    #[tokio::test]
    async fn test_list_sessions_empty() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/sessions")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let sessions: Vec<SessionInfo> = serde_json::from_slice(&body).unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/sessions/nonexistent-id")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_session_endpoint_accepts_client_session_id() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/api/sessions")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "id": "api-session-1",
                    "title": "API session",
                    "model": "session-model",
                    "system_prompt": "Use session controls."
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(created["object"], "hakimi.session");
        assert_eq!(created["session"]["id"], "api-session-1");
        assert_eq!(created["session"]["source"], "api_server");
        assert_eq!(created["session"]["title"], "API session");
        assert_eq!(created["session"]["model"], "session-model");

        let db = state.session_db.lock().await;
        let meta = db
            .get_session("api-session-1")
            .unwrap()
            .expect("created session should persist");
        assert_eq!(meta.system_prompt.as_deref(), Some("Use session controls."));
    }

    #[tokio::test]
    async fn test_update_session_endpoint_sets_title_and_end_reason() {
        let state = test_state();
        let session_id = {
            let db = state.session_db.lock().await;
            db.create_session("api-test", Some("user1"), Some("test-model"), None)
                .unwrap()
        };
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("PATCH")
            .uri(format!("/api/sessions/{session_id}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Reviewed session",
                    "end_reason": "archived"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated["object"], "hakimi.session");
        assert_eq!(updated["session"]["title"], "Reviewed session");

        let db = state.session_db.lock().await;
        let meta = db.get_session(&session_id).unwrap().unwrap();
        assert_eq!(meta.title.as_deref(), Some("Reviewed session"));
        assert_eq!(meta.end_reason.as_deref(), Some("archived"));
        assert!(meta.ended_at.is_some());
    }

    #[tokio::test]
    async fn test_delete_session_endpoint_removes_session_and_messages() {
        let state = test_state();
        let session_id = {
            let db = state.session_db.lock().await;
            let session_id = db
                .create_session("api-test", Some("user1"), Some("test-model"), None)
                .unwrap();
            db.save_message(&session_id, &hakimi_common::Message::user("delete me"))
                .unwrap();
            session_id
        };
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/api/sessions/{session_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let deleted: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(deleted["object"], "hakimi.session.deleted");
        assert_eq!(deleted["deleted"], true);

        let db = state.session_db.lock().await;
        assert!(db.get_session(&session_id).unwrap().is_none());
        assert!(db.get_messages(&session_id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_fork_session_endpoint_copies_transcript_and_sets_parent() {
        let state = test_state();
        let source_id = {
            let db = state.session_db.lock().await;
            let session_id = db
                .create_session("api-test", Some("user1"), Some("test-model"), None)
                .unwrap();
            db.set_title(&session_id, "Source session").unwrap();
            db.save_message(&session_id, &hakimi_common::Message::user("branch point"))
                .unwrap();
            db.save_message(
                &session_id,
                &hakimi_common::Message::assistant("branch reply"),
            )
            .unwrap();
            session_id
        };
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/sessions/{source_id}/fork"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "id": "api-fork-1",
                    "title": "Forked session"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let forked: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(forked["object"], "hakimi.session");
        assert_eq!(forked["session"]["id"], "api-fork-1");
        assert_eq!(forked["session"]["message_count"], 2);

        let db = state.session_db.lock().await;
        let source = db.get_session(&source_id).unwrap().unwrap();
        assert_eq!(source.end_reason.as_deref(), Some("branched"));
        let fork = db.get_session("api-fork-1").unwrap().unwrap();
        assert_eq!(fork.parent_session_id.as_deref(), Some(source_id.as_str()));
        assert_eq!(fork.title.as_deref(), Some("Forked session"));
        let fork_messages = db.get_messages("api-fork-1").unwrap();
        assert_eq!(fork_messages.len(), 2);
        assert_eq!(fork_messages[0].content.as_deref(), Some("branch point"));
    }

    #[tokio::test]
    async fn test_list_sessions_with_data() {
        let state = test_state();

        // Insert a session directly via the DB.
        {
            let db = state.session_db.lock().await;
            db.create_session("api-test", Some("user1"), Some("test-model"), None)
                .unwrap();
        }

        let app = build_router(state);
        let req = Request::builder()
            .uri("/api/sessions")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let sessions: Vec<SessionInfo> = serde_json::from_slice(&body).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].source.as_deref(), Some("api-test"));
    }

    #[tokio::test]
    async fn test_get_session_messages_endpoint_returns_sanitized_messages() {
        let state = test_state();
        let session_id = {
            let db = state.session_db.lock().await;
            let session_id = db
                .create_session("api-test", Some("user1"), Some("test-model"), None)
                .unwrap();
            db.save_message(
                &session_id,
                &hakimi_common::Message::user("Find release notes"),
            )
            .unwrap();
            let mut assistant = hakimi_common::Message::assistant("Use the release checklist");
            assistant.reasoning = Some("internal reasoning should not be serialized".to_string());
            db.save_message(&session_id, &assistant).unwrap();
            session_id
        };

        let app = build_router(state);
        let req = Request::builder()
            .uri(format!("/api/sessions/{session_id}/messages?limit=1"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_text.contains("internal reasoning should not be serialized"));
        let messages: SessionMessagesResponse = serde_json::from_str(&body_text).unwrap();
        assert_eq!(messages.object, "hakimi.session.messages");
        assert_eq!(messages.session.id, session_id);
        assert_eq!(messages.count, 1);
        assert_eq!(
            messages.messages[0].content.as_deref(),
            Some("Use the release checklist")
        );
        assert!(messages.messages[0].has_reasoning);
    }

    #[tokio::test]
    async fn test_clear_session_messages_endpoint_persists_empty_transcript() {
        let state = test_state();
        let session_id = {
            let db = state.session_db.lock().await;
            let session_id = db
                .create_session("api-test", Some("user1"), Some("test-model"), None)
                .unwrap();
            db.save_message(&session_id, &hakimi_common::Message::user("clear me"))
                .unwrap();
            db.save_message(
                &session_id,
                &hakimi_common::Message::assistant("cleared reply"),
            )
            .unwrap();
            session_id
        };

        let app = build_router(state.clone());
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/api/sessions/{session_id}/messages"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let db = state.session_db.lock().await;
        let meta = db
            .get_session(&session_id)
            .unwrap()
            .expect("session remains");
        assert_eq!(meta.message_count, 0);
        assert!(db.get_messages(&session_id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_search_sessions_endpoint_uses_fts_results() {
        let state = test_state();
        let session_id = {
            let db = state.session_db.lock().await;
            let session_id = db
                .create_session("api-test", Some("user1"), Some("test-model"), None)
                .unwrap();
            db.save_message(
                &session_id,
                &hakimi_common::Message::user("Hermes dashboard session search"),
            )
            .unwrap();
            db.save_message(
                &session_id,
                &hakimi_common::Message::assistant("Other text"),
            )
            .unwrap();
            session_id
        };

        let app = build_router(state);
        let req = Request::builder()
            .uri("/api/sessions/search?q=dashboard&limit=5")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let search: SessionSearchResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(search.object, "list");
        assert_eq!(search.query, "dashboard");
        assert_eq!(search.count, 1);
        assert_eq!(search.data[0].session_id, session_id);
        assert_eq!(
            search.data[0].content.as_deref(),
            Some("Hermes dashboard session search")
        );
        assert_eq!(search.data[0].source.as_deref(), Some("api-test"));
    }

    #[tokio::test]
    async fn test_activity_snapshot_includes_personas_as_idle() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/activity/snapshot")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        let arr = json["personas"].as_array().unwrap();
        // default persona present, idle by default
        let def = arr.iter().find(|p| p["id"] == "default").unwrap();
        assert_eq!(def["state"], "idle");
    }

    #[tokio::test]
    async fn test_create_agent_publishes_activity_event() {
        let mut rx = hakimi_common::subscribe();
        let app = build_router(test_state());
        let resp = app
            .oneshot(json_post(
                "/api/agents",
                json!({"id": "evt_coder", "name": "Coder"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        // drain until we see our PersonaCreated (other tests may share the global bus)
        let mut found = false;
        for _ in 0..50 {
            match rx.try_recv() {
                Ok(hakimi_common::ActivityEvent::PersonaCreated { id, .. })
                    if id == "evt_coder" =>
                {
                    found = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        assert!(found, "expected PersonaCreated for evt_coder");
    }
}
