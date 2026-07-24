//! Hakimi Studio local WebSocket endpoint (Phase 0/1).
//!
//! - `GET /v1/studio/health` — liveness
//! - `GET /v1/studio` — WebSocket: JSON `StudioCommand` in, `StudioEventEnvelope` out
//!
//! Runtime includes path-jailed workspace ops. Chat uses pluggable `AgentHost`
//! (mock by default; `CoreAgentHost` when AppState agent is injected).

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use hakimi_studio_api::{AgentHost, MockAgentHost, StudioCommand, StudioEvent, StudioRuntime};
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::core_agent_host::CoreAgentHost;

/// Shared Studio runtime for the HTTP process (local runner).
#[derive(Clone)]
pub struct StudioState {
    pub runtime: Arc<StudioRuntime>,
}

impl StudioState {
    /// Mock agent host (tests / fallback).
    pub fn new() -> Self {
        Self::with_host(Arc::new(MockAgentHost))
    }

    /// Wire the process shared `AIAgent` into Studio chat turns.
    pub fn with_shared_agent(agent: Arc<Mutex<hakimi_core::AIAgent>>) -> Self {
        Self::with_host(Arc::new(CoreAgentHost::new(agent)))
    }

    pub fn with_host(host: Arc<dyn AgentHost>) -> Self {
        let st = Self {
            runtime: Arc::new(StudioRuntime::with_agent_host(
                format!("local-{}", uuid_simple()),
                host,
            )),
        };
        // Optional: attach as pure-relay hub worker when HAKIMI_HUB_URL is set.
        if let Some(cfg) = crate::hub_worker::HubWorkerConfig::from_env() {
            tracing::info!(url = %cfg.hub_url, "spawning hub worker client");
            let _handle = crate::hub_worker::spawn_hub_worker(st.runtime.clone(), cfg);
            // JoinHandle intentionally detached — lives for process lifetime.
            std::mem::forget(_handle);
        }
        st
    }
}

impl Default for StudioState {
    fn default() -> Self {
        Self::new()
    }
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{t:x}")
}

/// Routes: `/v1/studio` and `/v1/studio/health` (full paths so merge into AppState works).
pub fn studio_router(state: StudioState) -> Router {
    Router::new()
        .route("/v1/studio/health", get(studio_health))
        .route("/v1/studio", get(studio_ws_upgrade))
        .with_state(state)
}

async fn studio_health(State(st): State<StudioState>) -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "service": "hakimi-studio",
        "protocol_version": hakimi_studio_api::PROTOCOL_VERSION,
        "local_device_id": st.runtime.local_device_id(),
        "prefer_runner": "local",
        "agent": "core",
    }))
}

async fn studio_ws_upgrade(
    ws: WebSocketUpgrade,
    State(st): State<StudioState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| studio_ws_session(socket, st))
}

async fn studio_ws_session(socket: WebSocket, st: StudioState) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Forward bus events to this socket.
    let mut rx = st.runtime.subscribe().await;
    let send_fwd = sender.clone();
    let forward = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(env) => {
                    let text = match serde_json::to_string(&env) {
                        Ok(t) => t,
                        Err(e) => {
                            warn!(error = %e, "studio event serialize failed");
                            continue;
                        }
                    };
                    let mut s = send_fwd.lock().await;
                    if s.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    debug!(lagged = n, "studio ws lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Read commands from client; device identity is connection-local after hello.
    let mut conn_device: Option<String> = None;
    while let Some(Ok(msg)) = receiver.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                Ok(s) => s,
                Err(_) => continue,
            },
            Message::Ping(p) => {
                let mut s = sender.lock().await;
                let _ = s.send(Message::Pong(p)).await;
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };

        let cmd: StudioCommand = match serde_json::from_str(&text) {
            Ok(c) => c,
            Err(e) => {
                let err = hakimi_studio_api::StudioEventEnvelope::new(
                    0,
                    None,
                    StudioEvent::Error {
                        session_id: None,
                        message: format!("invalid command: {e}"),
                        code: Some("bad_command".into()),
                    },
                );
                if let Ok(t) = serde_json::to_string(&err) {
                    let mut s = sender.lock().await;
                    let _ = s.send(Message::Text(t.into())).await;
                }
                continue;
            }
        };

        // Bind device_id from hello for the rest of this connection.
        if let StudioCommand::Hello { ref device_id, .. } = cmd {
            conn_device = Some(device_id.clone());
        }
        let actor = conn_device.as_deref();

        if let Err(e) = st.runtime.handle_command_as(actor, cmd).await {
            warn!(error = %e, "studio handle_command failed");
            let err = hakimi_studio_api::StudioEventEnvelope::new(
                0,
                None,
                StudioEvent::Error {
                    session_id: None,
                    message: e.to_string(),
                    code: Some("runtime_error".into()),
                },
            );
            if let Ok(t) = serde_json::to_string(&err) {
                let mut s = sender.lock().await;
                let _ = s.send(Message::Text(t.into())).await;
            }
        }
    }

    forward.abort();
}
