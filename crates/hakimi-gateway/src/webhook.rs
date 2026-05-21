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
    /// Bot / role identifier for this instance.
    #[serde(default = "default_webhook_bot_id")]
    pub bot_id: String,
    /// Path to listen on (e.g. "/webhook").
    pub path: String,
    /// Optional secret for HMAC verification.
    pub secret: Option<String>,
}

fn default_webhook_bot_id() -> String {
    "default".to_string()
}

impl Default for WebhookAdapterConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            bot_id: "default".to_string(),
            path: "/webhook".to_string(),
            secret: None,
        }
    }
}

/// Generic webhook platform adapter.
pub struct WebhookAdapter {
    config: WebhookAdapterConfig,
    bot_id: String,
    sender: Option<mpsc::UnboundedSender<GatewayMessage>>,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl WebhookAdapter {
    pub fn new(config: WebhookAdapterConfig) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
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

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(port = self.config.port, path = %self.config.path, "Webhook adapter ready");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        debug!(
            chat_id,
            text_len = text.len(),
            "Webhook: would send message"
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> WebhookAdapterConfig {
        WebhookAdapterConfig {
            port: 9090,
            bot_id: "default".to_string(),
            path: "/test-hook".to_string(),
            secret: Some("s3cret".to_string()),
        }
    }

    #[test]
    fn test_default_config() {
        let cfg = WebhookAdapterConfig::default();
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.path, "/webhook");
        assert!(cfg.secret.is_none());
    }

    #[tokio::test]
    async fn test_adapter_name_and_connect() {
        let mut adapter = WebhookAdapter::new(make_config());
        assert_eq!(adapter.name(), "webhook");
        assert!(adapter.connect().await.is_ok());
    }

    #[tokio::test]
    async fn test_send_message_succeeds() {
        let adapter = WebhookAdapter::new(make_config());
        assert!(adapter.send_message("chat1", "hello").await.is_ok());
    }

    #[tokio::test]
    async fn test_inject_and_receive_message() {
        let mut adapter = WebhookAdapter::new(make_config());
        let mut rx = adapter.take_receiver().expect("should have receiver");

        let msg = GatewayMessage {
            platform: "webhook".to_string(),
            bot_id: "default".to_string(),
            chat_id: "c1".to_string(),
            user_id: "u1".to_string(),
            text: "ping".to_string(),
            media: None,
        };
        adapter.inject_message(msg);

        let received = rx.recv().await.expect("should receive message");
        assert_eq!(received.chat_id, "c1");
        assert_eq!(received.user_id, "u1");
        assert_eq!(received.text, "ping");
    }

    #[test]
    fn test_take_receiver_returns_none_on_second_call() {
        let mut adapter = WebhookAdapter::new(make_config());
        assert!(adapter.take_receiver().is_some());
        assert!(adapter.take_receiver().is_none());
    }

    #[tokio::test]
    async fn test_disconnect_succeeds() {
        let mut adapter = WebhookAdapter::new(make_config());
        assert!(adapter.disconnect().await.is_ok());
    }

    #[test]
    fn test_inject_without_receiver_is_noop() {
        // If sender is taken (simulated by dropping), inject_message should not panic.
        let mut adapter = WebhookAdapter::new(make_config());
        let _ = adapter.take_receiver(); // drain receiver
        // sender still exists internally; inject should not panic
        let msg = GatewayMessage {
            platform: "webhook".to_string(),
            bot_id: "default".to_string(),
            chat_id: "c2".to_string(),
            user_id: "u2".to_string(),
            text: "test".to_string(),
            media: Some("https://example.com/img.png".to_string()),
        };
        adapter.inject_message(msg);
    }
}
