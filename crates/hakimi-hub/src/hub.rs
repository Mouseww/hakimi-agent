//! Axum routes for the Studio Hub.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use hakimi_studio_api::{StudioCommand, StudioEvent, StudioEventEnvelope};
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};

use crate::state::{HubState, Outbound};

#[derive(Debug, Clone, Default)]
pub struct HubConfig {
    pub bind: String,
    pub token: Option<String>,
    /// "relay" (default production) or "embedded" (demo with in-process runtime).
    pub mode: String,
}

impl HubConfig {
    pub fn from_env() -> Self {
        Self {
            bind: std::env::var("HAKIMI_HUB_BIND").unwrap_or_else(|_| "0.0.0.0:3010".into()),
            token: std::env::var("HAKIMI_HUB_TOKEN").ok().filter(|s| !s.is_empty()),
            mode: std::env::var("HAKIMI_HUB_MODE").unwrap_or_else(|_| "embedded".into()),
        }
    }
}

pub fn hub_router(state: HubState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/hub/health", get(health))
        .route("/v1/studio/health", get(health))
        .route("/v1/studio", get(ws_upgrade))
        .route("/v1/hub", get(ws_upgrade))
        .with_state(state)
}

async fn health(State(st): State<HubState>) -> impl IntoResponse {
    Json(st.health_json().await)
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(st): State<HubState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_session(socket, st))
}

async fn ws_session(socket: WebSocket, st: HubState) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Per-connection outbound queue (bus fan-out + worker_dispatch).
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Outbound>();

    let send_fwd = sender.clone();
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            match msg {
                Outbound::Text(text) => {
                    let mut s = send_fwd.lock().await;
                    if s.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Fan-out all bus events to this connection.
    let mut rx = st.bus.subscribe().await;
    let out_bus = out_tx.clone();
    let forward = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(env) => {
                    let text = match serde_json::to_string(&env) {
                        Ok(t) => t,
                        Err(e) => {
                            warn!(error = %e, "hub event serialize failed");
                            continue;
                        }
                    };
                    if out_bus.send(Outbound::Text(text)).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    debug!(lagged = n, "hub ws lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    info!("hub ws client connected");
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
                if let Ok(ctrl) = serde_json::from_str::<HubControl>(&text) {
                    match ctrl {
                        HubControl::HubPing { nonce } => {
                            let env =
                                StudioEventEnvelope::new(0, None, StudioEvent::Pong { nonce });
                            if let Ok(t) = serde_json::to_string(&env) {
                                let _ = out_tx.send(Outbound::Text(t));
                            }
                            continue;
                        }
                    }
                }
                let err = StudioEventEnvelope::new(
                    0,
                    None,
                    StudioEvent::Error {
                        session_id: None,
                        message: format!("invalid command: {e}"),
                        code: Some("bad_command".into()),
                    },
                );
                if let Ok(t) = serde_json::to_string(&err) {
                    let _ = out_tx.send(Outbound::Text(t));
                }
                continue;
            }
        };

        if let StudioCommand::Hello { ref device_id, .. } = cmd {
            conn_device = Some(device_id.clone());
            st.register_connection(device_id.clone(), out_tx.clone()).await;
        }

        if let Err(e) = st.handle_command_as(conn_device.as_deref(), cmd).await {
            warn!(error = %e, "hub handle_command failed");
            let err = StudioEventEnvelope::new(
                0,
                None,
                StudioEvent::Error {
                    session_id: None,
                    message: e.to_string(),
                    code: Some("hub_error".into()),
                },
            );
            if let Ok(t) = serde_json::to_string(&err) {
                let _ = out_tx.send(Outbound::Text(t));
            }
        }
    }

    if let Some(ref id) = conn_device {
        st.unregister_connection(id).await;
    }
    forward.abort();
    writer.abort();
    info!("hub ws client disconnected");
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HubControl {
    HubPing {
        #[serde(default)]
        nonce: Option<String>,
    },
}
