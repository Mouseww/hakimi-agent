//! REST API route definitions.
//!
//! Endpoints:
//! - `GET  /health`          — Health check
//! - `POST /chat`            — Send a message, get a response
//! - `GET  /sessions`        — List recent sessions
//! - `GET  /sessions/:id`    — Get session details
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

use std::collections::{BTreeMap, HashMap};

use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
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
        .route("/sessions/{id}", get(get_session))
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
        .route("/capabilities", get(capabilities));
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
            "chat_completions": false,
            "chat_completions_streaming": false,
            "responses_api": false,
            "responses_streaming": false,
            "session_resources": true,
            "tools_api": true,
            "config_read": true,
            "config_write": true,
            "run_submission": false,
            "run_status": false,
            "run_events_sse": false,
            "run_stop": false,
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
            "chat": {"method": "POST", "path": "/api/chat"},
            "sessions": {"method": "GET", "path": "/api/sessions"},
            "session": {"method": "GET", "path": "/api/sessions/{id}"},
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
    use hakimi_session::{SessionDB, SessionOps};
    use serde_json::json;
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

    /// Build a minimal AppState for testing (no real agent).
    /// Uses a stub transport so we don't need a real LLM.
    fn test_state() -> AppState {
        use hakimi_context::SimpleContextEngine;
        use hakimi_transports::ChatCompletionsTransport;

        let transport = Arc::new(ChatCompletionsTransport::new(
            "http://localhost:0".into(),
            "test-key".into(),
            reqwest::Client::new(),
        ));
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
        let resp = app.oneshot(req).await.unwrap();

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
        assert_eq!(capabilities["features"]["tools_api"], true);
        assert_eq!(capabilities["features"]["chat_completions"], false);
        assert_eq!(capabilities["features"]["run_events_sse"], false);
        assert_eq!(
            capabilities["endpoints"]["models"],
            json!({"method": "GET", "path": "/v1/models"})
        );
        assert_eq!(
            capabilities["endpoints"]["chat"],
            json!({"method": "POST", "path": "/api/chat"})
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
}
