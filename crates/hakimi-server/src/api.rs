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

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
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
    pub terminal_env_type: String,
    pub terminal_cwd: String,
    pub terminal_timeout: u64,
    pub compression_engine: String,
    pub compression_context_length: usize,
    pub display_streaming: bool,
    pub display_skin: String,
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
}

/// Generic error response.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the axum Router with all API routes.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/chat", post(chat))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", get(get_session))
        .route("/tools", get(list_tools))
        .route("/config", get(get_config))
        .route("/config", post(update_config))
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
        terminal_env_type: config.terminal.env_type.clone(),
        terminal_cwd: config.terminal.cwd.clone(),
        terminal_timeout: config.terminal.timeout,
        compression_engine: config.compression.engine.clone(),
        compression_context_length: config.compression.context_length,
        display_streaming: config.display.streaming,
        display_skin: config.display.skin.clone(),
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

    // Return the updated config (sanitized).
    let response = SanitizedConfig {
        model_default: config.model.default.clone(),
        model_provider: config.model.provider.clone(),
        agent_max_turns: config.agent.max_turns,
        agent_verbose: config.agent.verbose,
        agent_system_prompt: config.agent.system_prompt.clone(),
        terminal_env_type: config.terminal.env_type.clone(),
        terminal_cwd: config.terminal.cwd.clone(),
        terminal_timeout: config.terminal.timeout,
        compression_engine: config.compression.engine.clone(),
        compression_context_length: config.compression.context_length,
        display_streaming: config.display.streaming,
        display_skin: config.display.skin.clone(),
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
            .uri("/health")
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
            .uri("/tools")
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
            .uri("/config")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let config: SanitizedConfig = serde_json::from_slice(&body).unwrap();
        assert_eq!(config.agent_max_turns, 90);
        assert_eq!(config.compression_engine, "smart");
    }

    #[tokio::test]
    async fn test_update_config_endpoint() {
        let state = test_state();
        let app = build_router(state);

        let update = json!({"agent_max_turns": 42, "agent_verbose": true});
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/config")
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
    }

    #[tokio::test]
    async fn test_list_sessions_empty() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/sessions")
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
            .uri("/sessions/nonexistent-id")
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
            .uri("/sessions")
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
