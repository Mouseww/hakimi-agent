//! REST API route definitions.
//!
//! Endpoints:
//! - `GET  /health`          — Health check
//! - `POST /chat`            — Send a message, get a response
//! - `GET  /sessions`        — List recent sessions
//! - `GET  /sessions/:id`    — Get session details
//! - `GET  /sessions/search` — Search saved session messages
//! - `GET  /sessions/:id/messages` — Get sanitized session messages
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
//! - `GET  /v1/models`       — OpenAI-compatible model discovery
//! - `GET  /v1/capabilities` — Machine-readable API capability discovery
//! - `GET  /v1/skills`       — List loaded runtime skills without skill bodies
//! - `GET  /v1/toolsets`     — List registered toolsets and their tool schemas
//! - `POST /v1/chat/completions` — OpenAI-compatible non-streaming chat
//! - `POST /v1/responses`    — OpenAI Responses-compatible non-streaming chat
//! - `GET  /v1/responses/:id` — Retrieve a stored Responses API result
//! - `DELETE /v1/responses/:id` — Delete a stored Responses API result
//! - `POST /v1/runs`         — Submit an asynchronous text run
//! - `GET  /v1/runs/:id`     — Poll an asynchronous run status/result
//! - `GET  /v1/runs/:id/events` — Read run lifecycle events as SSE
//! - `POST /v1/runs/:id/stop` — Cancel an asynchronous run

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tokio::task::JoinHandle;
use tracing::info;

use crate::server::AppState;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Request body for POST /chat.
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
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
#[derive(Debug, Clone, Deserialize)]
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

/// Response body for POST /chat.
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub response: String,
    pub session_id: String,
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

#[derive(Debug, Clone)]
struct RunEvent {
    event: String,
    status: String,
    created_at: u64,
    message: Option<String>,
}

impl RunEvent {
    fn at(
        event: impl Into<String>,
        status: impl Into<String>,
        created_at: u64,
        message: Option<String>,
    ) -> Self {
        Self {
            event: event.into(),
            status: status.into(),
            created_at,
            message,
        }
    }

    fn new(event: impl Into<String>, status: impl Into<String>, message: Option<String>) -> Self {
        Self::at(event, status, unix_timestamp_secs(), message)
    }

    fn to_json(&self, run_id: &str) -> JsonValue {
        json!({
            "object": "hakimi.run.event",
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
            events: vec![RunEvent::at("run.queued", "queued", created_at, None)],
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

    fn push_event(&mut self, event: impl Into<String>, message: Option<String>) {
        let event = RunEvent::new(event, self.status.clone(), message);
        self.updated_at = event.created_at;
        self.events.push(event);
    }

    fn events_json(&self) -> Vec<JsonValue> {
        self.events
            .iter()
            .map(|event| event.to_json(&self.id))
            .collect()
    }
}

/// In-memory store for OpenAI Responses-compatible chaining.
#[derive(Debug)]
pub struct ResponsesStore {
    max_entries: usize,
    entries: HashMap<String, StoredResponse>,
    order: VecDeque<String>,
}

impl Default for ResponsesStore {
    fn default() -> Self {
        Self::new(100)
    }
}

impl ResponsesStore {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert(
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
        self.entries
            .get(response_id)
            .map(|stored| stored.response.clone())
    }

    fn messages(&self, response_id: &str) -> Option<Vec<ChatCompletionsMessage>> {
        self.entries
            .get(response_id)
            .map(|stored| stored.messages.clone())
    }

    fn delete(&mut self, response_id: &str) -> bool {
        let removed = self.entries.remove(response_id).is_some();
        if removed {
            self.order.retain(|id| id != response_id);
        }
        removed
    }
}

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

/// In-memory store for asynchronous API runs.
#[derive(Debug)]
pub struct RunsStore {
    max_entries: usize,
    entries: HashMap<String, StoredRun>,
    order: VecDeque<String>,
    controls: HashMap<String, RunControl>,
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
        }
    }

    fn insert(&mut self, run: StoredRun) {
        if !self.entries.contains_key(&run.id) {
            self.order.push_back(run.id.clone());
        }
        self.entries.insert(run.id.clone(), run);

        while self.entries.len() > self.max_entries {
            let Some(evicted) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&evicted);
            if let Some(control) = self.controls.remove(&evicted) {
                control.interrupt.store(true, Ordering::Relaxed);
                control.task.abort();
            }
        }
    }

    fn get(&self, run_id: &str) -> Option<JsonValue> {
        self.entries.get(run_id).map(StoredRun::to_json)
    }

    fn events(&self, run_id: &str) -> Option<Vec<JsonValue>> {
        self.entries.get(run_id).map(StoredRun::events_json)
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
        if let Some(run) = self.entries.get_mut(run_id) {
            if is_terminal_run_status(&run.status) {
                return;
            }
            run.status = status.to_string();
            run.push_event(format!("run.{status}"), None);
        }
    }

    fn complete(&mut self, run_id: &str, output_text: String, usage: JsonValue) {
        if let Some(run) = self.entries.get_mut(run_id) {
            if is_terminal_run_status(&run.status) {
                self.controls.remove(run_id);
                return;
            }
            run.status = "completed".to_string();
            run.output_text = Some(output_text);
            run.usage = Some(usage);
            run.error = None;
            run.push_event("run.completed", None);
        }
        self.controls.remove(run_id);
    }

    fn fail(&mut self, run_id: &str, error: String) {
        if let Some(run) = self.entries.get_mut(run_id) {
            if is_terminal_run_status(&run.status) {
                self.controls.remove(run_id);
                return;
            }
            run.status = "failed".to_string();
            run.error = Some(error.clone());
            run.push_event("run.failed", Some(error));
        }
        self.controls.remove(run_id);
    }

    fn stop(&mut self, run_id: &str) -> StopRunResult {
        let Some(run) = self.entries.get_mut(run_id) else {
            return StopRunResult::NotFound;
        };
        if is_terminal_run_status(&run.status) {
            return StopRunResult::AlreadyFinished(run.status.clone());
        }

        run.status = "cancelled".to_string();
        let message = "Stop requested via API".to_string();
        run.error = Some(message.clone());
        run.push_event("run.cancelled", Some(message));
        let body = run.to_json();

        if let Some(control) = self.controls.remove(run_id) {
            control.interrupt.store(true, Ordering::Relaxed);
            control.task.abort();
        }

        StopRunResult::Cancelled(body)
    }
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

async fn auth_middleware(req: Request, next: Next) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());
    let password = std::env::var("HAKIMI_WEBUI_PASSWORD").unwrap_or_default();

    if let Some(auth) = auth_header
        && auth == format!("Bearer {}", password)
    {
        return Ok(next.run(req).await);
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// Build the axum Router with all API routes.
pub fn build_router(state: AppState) -> Router {
    // API routes that need authentication
    let mut api_routes = Router::new()
        .route("/chat", post(chat))
        .route("/sessions", get(list_sessions))
        .route("/sessions/search", get(search_sessions))
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}/messages", get(get_session_messages))
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
        .route("/webhooks", post(update_webhook));

    let password = std::env::var("HAKIMI_WEBUI_PASSWORD").unwrap_or_default();
    if !password.is_empty() {
        api_routes = api_routes.route_layer(middleware::from_fn(auth_middleware));
    }

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
    let password = std::env::var("HAKIMI_WEBUI_PASSWORD").unwrap_or_default();
    if !password.is_empty() {
        v1_routes = v1_routes.route_layer(middleware::from_fn(auth_middleware));
    }

    Router::new()
        .nest("/api", api_routes)
        .nest("/v1", v1_routes)
        .fallback_service(tower_http::services::ServeDir::new("../hakimi-webui/dist"))
        .with_state(state)
}

// ---------------------------------------------------------------------------
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
        "features": {
            "chat": true,
            "chat_completions": true,
            "chat_completions_streaming": false,
            "responses_api": true,
            "responses_streaming": false,
            "skills_api": true,
            "toolsets_api": true,
            "session_resources": true,
            "session_messages": true,
            "session_search": true,
            "tools_api": true,
            "config_read": true,
            "config_write": true,
            "run_submission": true,
            "run_status": true,
            "run_events_sse": true,
            "run_stop": true,
            "websocket_streaming": false,
            "media_api": false
        },
        "dashboard_admin": {
            "status": true,
            "mcp_servers_read": true,
            "mcp_servers_write": true,
            "credential_pools_read": true,
            "credential_pools_write": true,
            "webhooks_read": true,
            "webhooks_write": true,
            "write_operations": true,
            "persistence": "runtime"
        },
        "endpoints": {
            "health": {"method": "GET", "path": "/api/health"},
            "models": {"method": "GET", "path": "/v1/models"},
            "capabilities": {"method": "GET", "path": "/v1/capabilities"},
            "skills": {"method": "GET", "path": "/v1/skills"},
            "toolsets": {"method": "GET", "path": "/v1/toolsets"},
            "chat_completions": {"method": "POST", "path": "/v1/chat/completions"},
            "responses": {"method": "POST", "path": "/v1/responses"},
            "response": {"method": "GET", "path": "/v1/responses/{id}"},
            "response_delete": {"method": "DELETE", "path": "/v1/responses/{id}"},
            "run": {"method": "POST", "path": "/v1/runs"},
            "run_status": {"method": "GET", "path": "/v1/runs/{id}"},
            "run_events": {"method": "GET", "path": "/v1/runs/{id}/events"},
            "run_stop": {"method": "POST", "path": "/v1/runs/{id}/stop"},
            "chat": {"method": "POST", "path": "/api/chat"},
            "sessions": {"method": "GET", "path": "/api/sessions"},
            "session": {"method": "GET", "path": "/api/sessions/{id}"},
            "session_messages": {"method": "GET", "path": "/api/sessions/{id}/messages"},
            "session_search": {"method": "GET", "path": "/api/sessions/search?q=<query>"},
            "tools": {"method": "GET", "path": "/api/tools"},
            "config": {"method": "GET", "path": "/api/config"},
            "config_update": {"method": "POST", "path": "/api/config"},
            "dashboard_status": {"method": "GET", "path": "/api/status"},
            "mcp_servers": {"method": "GET", "path": "/api/mcp/servers"},
            "mcp_server_add": {"method": "POST", "path": "/api/mcp/servers"},
            "mcp_server_delete": {"method": "DELETE", "path": "/api/mcp/servers/{name}"},
            "credential_pool": {"method": "GET", "path": "/api/credentials/pool"},
            "credential_pool_add": {"method": "POST", "path": "/api/credentials/pool"},
            "credential_pool_delete": {"method": "DELETE", "path": "/api/credentials/pool/{provider}/{index}"},
            "webhooks": {"method": "GET", "path": "/api/webhooks"},
            "webhook_update": {"method": "POST", "path": "/api/webhooks"}
        }
    }))
}

fn auth_required() -> bool {
    !std::env::var("HAKIMI_WEBUI_PASSWORD")
        .unwrap_or_default()
        .is_empty()
}

fn bounded_limit(limit: Option<usize>, default: usize, max: usize) -> usize {
    limit.unwrap_or(default).clamp(1, max)
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
            "webhooks": "/api/webhooks"
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

/// POST /v1/chat/completions — OpenAI-compatible non-streaming chat.
async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionsRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if request_bool(req.stream.as_ref(), false) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "stream=true is not yet supported on /v1/chat/completions",
        ));
    }

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
            Ok(Json(json!({
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
            })))
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

/// POST /v1/responses — OpenAI Responses-compatible non-streaming chat.
async fn responses(
    State(state): State<AppState>,
    Json(req): Json<ResponsesRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if request_bool(req.stream.as_ref(), false) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "stream=true is not yet supported on /v1/responses",
        ));
    }

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

            Ok(Json(response))
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

/// GET /v1/responses/:id — retrieve an in-memory Responses API result.
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

/// DELETE /v1/responses/:id — remove an in-memory Responses API result.
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

/// GET /v1/runs/:id/events — retrieve stored run lifecycle events as SSE.
async fn get_run_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let id = id.trim();
    let events = {
        let store = state.run_store.lock().await;
        store.events(id)
    };

    let Some(events) = events else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("run not found: {id}"),
        ));
    };

    let mut body = String::new();
    for event in events {
        let event_name = event
            .get("event")
            .and_then(JsonValue::as_str)
            .unwrap_or("run.event");
        body.push_str("event: ");
        body.push_str(event_name);
        body.push('\n');
        body.push_str("data: ");
        body.push_str(&event.to_string());
        body.push_str("\n\n");
    }

    Ok((
        [
            (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
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
    Json(SanitizedConfig {
        model_default: config.model.default.clone(),
        model_provider: config.model.provider.clone(),
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

    // Return the updated config (sanitized).
    let response = SanitizedConfig {
        model_default: config.model.default.clone(),
        model_provider: config.model.provider.clone(),
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
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    // ---------- helpers ----------

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
            response_store: Arc::new(Mutex::new(ResponsesStore::default())),
            run_store: Arc::new(Mutex::new(RunsStore::default())),
        }
    }

    // ---------- tests ----------

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
        assert_eq!(capabilities["features"]["session_messages"], true);
        assert_eq!(capabilities["features"]["session_search"], true);
        assert_eq!(capabilities["features"]["tools_api"], true);
        assert_eq!(capabilities["features"]["chat_completions"], true);
        assert_eq!(capabilities["features"]["responses_api"], true);
        assert_eq!(capabilities["features"]["skills_api"], true);
        assert_eq!(capabilities["features"]["toolsets_api"], true);
        assert_eq!(capabilities["features"]["run_submission"], true);
        assert_eq!(capabilities["features"]["run_status"], true);
        assert_eq!(capabilities["features"]["run_events_sse"], true);
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
            capabilities["endpoints"]["session_messages"],
            json!({"method": "GET", "path": "/api/sessions/{id}/messages"})
        );
        assert_eq!(
            capabilities["endpoints"]["session_search"],
            json!({"method": "GET", "path": "/api/sessions/search?q=<query>"})
        );
        assert_eq!(capabilities["dashboard_admin"]["status"], true);
        assert_eq!(capabilities["dashboard_admin"]["mcp_servers_read"], true);
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
    }

    #[tokio::test]
    async fn test_v1_skills_endpoint_lists_metadata_without_content() {
        let state = test_state();
        {
            let mut agent = state.agent.lock().await;
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
            *agent = agent.clone().with_skill_store(Some(store));
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
    async fn test_v1_chat_completions_rejects_streaming_for_now() {
        let state = test_state();
        let app = build_router(state);

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
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);
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
    async fn test_v1_responses_rejects_streaming_for_now() {
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
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);
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
}
