//! HTTP server implementation for Teams Webhook inbound messages.
//!
//! This module provides an Axum-based HTTP server that handles incoming webhook
//! requests from Microsoft Teams Outgoing Webhooks.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use hakimi_gateway::teams_webhook::{TeamsWebhookAdapter, TeamsWebhookConfig, TeamsWebhookServer};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = TeamsWebhookConfig {
//!         hmac_secret: "your_base64_secret".to_string(),
//!         default_workflow_url: "https://prod-xx.logic.azure.com/...".to_string(),
//!         ..Default::default()
//!     };
//!     
//!     let adapter = Arc::new(TeamsWebhookAdapter::new(config));
//!     let server = TeamsWebhookServer::new(adapter.clone(), "0.0.0.0:3000".parse().unwrap());
//!     
//!     // Start the server in the background
//!     tokio::spawn(async move {
//!         server.serve().await.expect("Server failed");
//!     });
//!     
//!     // Your gateway logic here...
//! }
//! ```

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info, warn};

use super::{TeamsInboundActivity, TeamsWebhookAdapter};

/// HTTP server for Teams Webhook inbound messages.
pub struct TeamsWebhookServer {
    adapter: Arc<TeamsWebhookAdapter>,
    addr: SocketAddr,
}

impl TeamsWebhookServer {
    pub fn new(adapter: Arc<TeamsWebhookAdapter>, addr: SocketAddr) -> Self {
        Self { adapter, addr }
    }

    /// Start the HTTP server.
    pub async fn serve(self) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/teams/inbound", post(teams_inbound_handler))
            .route("/healthz", axum::routing::get(healthz_handler))
            .with_state(self.adapter);

        info!(addr = %self.addr, "Teams Webhook server listening");

        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

/// Health check endpoint.
async fn healthz_handler() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

/// Teams Outgoing Webhook inbound handler.
async fn teams_inbound_handler(
    State(adapter): State<Arc<TeamsWebhookAdapter>>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    // Extract raw body
    let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(error = %e, "Failed to read request body");
            return (StatusCode::BAD_REQUEST, "Invalid body").into_response();
        }
    };

    // Verify HMAC signature
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if !adapter.verify_hmac(&body_bytes, auth_header) {
        warn!("HMAC verification failed");
        return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
    }

    // Parse activity
    let activity: TeamsInboundActivity = match serde_json::from_slice(&body_bytes) {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, "Failed to parse Teams activity");
            return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response();
        }
    };

    // Convert to GatewayMessage
    let gateway_msg = match adapter.process_inbound(activity) {
        Some(msg) => msg,
        None => {
            // Empty message, still return success
            return Json(json!({
                "type": "message",
                "text": "Message received but appears to be empty."
            }))
            .into_response();
        }
    };

    // Inject message into the adapter
    adapter.inject_message(gateway_msg.clone());

    // Return immediate receipt (must be within 10 seconds)
    Json(json!({
        "type": "message",
        "text": format!("Received your request: {}. Processing in the background...", 
                        &gateway_msg.text[..gateway_msg.text.len().min(40)])
    }))
    .into_response()
}
