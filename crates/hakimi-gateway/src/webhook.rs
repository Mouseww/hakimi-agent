//! Webhook platform adapter — generic HTTP webhook integration.
//!
//! Receives messages via HTTP POST webhooks and sends responses back
//! to configured callback URLs.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::{GatewayMessage, PlatformAdapter};

/// Configuration for the webhook adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAdapterConfig {
    /// Port to listen on for incoming webhooks.
    pub port: u16,
    /// Path to listen on (e.g. "/webhook").
    pub path: String,
    /// Optional secret for HMAC verification.
    pub secret: Option<String>,
}

impl Default for WebhookAdapterConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            path: "/webhook".to_string(),
            secret: None,
        }
    }
}

/// Generic webhook platform adapter.
pub struct WebhookAdapter {
    config: WebhookAdapterConfig,
    sender: Option<mpsc::UnboundedSender<GatewayMessage>>,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl WebhookAdapter {
    pub fn new(config: WebhookAdapterConfig) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            config,
            sender: Some(sender),
            receiver: Some(receiver),
        }
    }

    /// Inject a message (called by the HTTP handler).
    pub fn inject_message(&self, msg: GatewayMessage) {
        if let Some(ref sender) = self.sender {
            let _ = sender.send(msg);
        }
    }
}

#[async_trait]
impl PlatformAdapter for WebhookAdapter {
    fn name(&self) -> &str {
        "webhook"
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(port = self.config.port, path = %self.config.path, "Webhook adapter ready");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        debug!(chat_id, text_len = text.len(), "Webhook: would send message");
        // In a real implementation, this would POST to a callback URL.
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("Webhook adapter disconnected");
        Ok(())
    }
}
