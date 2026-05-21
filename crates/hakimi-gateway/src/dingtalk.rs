//! DingTalk (钉钉) platform adapter.
//!
//! Sends messages via DingTalk's custom robot webhook API.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_dingtalk_bot_id")]
    pub bot_id: String,
    pub webhook_url: String,
    pub secret: Option<String>,
}

fn default_dingtalk_bot_id() -> String {
    "default".to_string()
}

pub struct DingTalkAdapter {
    config: DingTalkAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl DingTalkAdapter {
    pub fn new(config: DingTalkAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }
}

#[async_trait]
impl PlatformAdapter for DingTalkAdapter {
    fn name(&self) -> &str {
        "dingtalk"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!("DingTalk adapter connected");
        Ok(())
    }

    async fn send_message(&self, _chat_id: &str, text: &str) -> anyhow::Result<()> {
        let url = &self.config.webhook_url;

        let body = serde_json::json!({
            "msgtype": "text",
            "text": {
                "content": text
            }
        });

        let resp = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "DingTalk send_message failed: status={}, body={}",
                status,
                body_text
            );
        }

        info!(chat_id = _chat_id, text_len = text.len(), "DingTalk: message sent");
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("DingTalk adapter disconnected");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    #[test]
    fn test_construction() {
        let config = DingTalkAdapterConfig {
            bot_id: "default".into(),
            webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=***".into(),
            secret: None,
        };
        let adapter = DingTalkAdapter::new(config);
        assert_eq!(
            adapter.config.webhook_url,
            "https://oapi.dingtalk.com/robot/send?access_token=***"
        );
        assert!(adapter.config.secret.is_none());
    }

    #[test]
    fn test_name() {
        let config = DingTalkAdapterConfig {
            bot_id: "default".into(),
            webhook_url: "https://example.com".into(),
            secret: None,
        };
        let adapter = DingTalkAdapter::new(config);
        assert_eq!(adapter.name(), "dingtalk");
    }

    #[test]
    fn test_config_with_secret() {
        let config = DingTalkAdapterConfig {
            bot_id: "default".into(),
            webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=***".into(),
            secret: Some("SECtest".into()),
        };
        let adapter = DingTalkAdapter::new(config);
        assert_eq!(adapter.config.secret.as_deref(), Some("SECtest"));
    }

    #[test]
    fn test_take_receiver() {
        let config = DingTalkAdapterConfig {
            bot_id: "default".into(),
            webhook_url: "https://example.com".into(),
            secret: None,
        };
        let mut adapter = DingTalkAdapter::new(config);
        assert!(adapter.take_receiver().is_some());
        assert!(adapter.take_receiver().is_none());
    }
}
