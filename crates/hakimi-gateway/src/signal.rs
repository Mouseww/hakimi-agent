//! Signal platform adapter.
//!
//! Communicates with a signal-cli REST daemon to send and receive messages.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_signal_bot_id")]
    pub bot_id: String,
    pub phone_number: String,
    pub signal_cli_path: String,
}

fn default_signal_bot_id() -> String {
    "default".to_string()
}

pub struct SignalAdapter {
    config: SignalAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl SignalAdapter {
    pub fn new(config: SignalAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }

    /// Return the base URL of the signal-cli REST daemon.
    fn base_url(&self) -> &str {
        &self.config.signal_cli_path
    }

    /// Build the send-message endpoint URL.
    fn send_url(&self) -> String {
        format!("{}/v2/send", self.base_url().trim_end_matches('/'))
    }
}

#[async_trait]
impl PlatformAdapter for SignalAdapter {
    fn name(&self) -> &str {
        "signal"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(phone = %self.config.phone_number, "Signal adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let url = self.send_url();

        let body = serde_json::json!({
            "message": text,
            "number": self.config.phone_number,
            "recipients": [chat_id],
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Signal send_message failed: status={}, body={}",
                status,
                body_text
            );
        }

        info!(chat_id, text_len = text.len(), "Signal: message sent");
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("Signal adapter disconnected");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> SignalAdapterConfig {
        SignalAdapterConfig {
            bot_id: "default".into(),
            phone_number: "+1234567890".into(),
            signal_cli_path: "http://localhost:8080".into(),
        }
    }

    #[test]
    fn test_construction() {
        let adapter = SignalAdapter::new(make_config());
        assert_eq!(adapter.config.phone_number, "+1234567890");
    }

    #[test]
    fn test_name() {
        let adapter = SignalAdapter::new(make_config());
        assert_eq!(adapter.name(), "signal");
    }

    #[test]
    fn test_send_url_construction() {
        let adapter = SignalAdapter::new(make_config());
        assert_eq!(adapter.send_url(), "http://localhost:8080/v2/send");
    }

    #[test]
    fn test_send_url_trailing_slash() {
        let config = SignalAdapterConfig {
            bot_id: "default".into(),
            phone_number: "+1234567890".into(),
            signal_cli_path: "http://localhost:8080/".into(),
        };
        let adapter = SignalAdapter::new(config);
        assert_eq!(adapter.send_url(), "http://localhost:8080/v2/send");
    }
}
