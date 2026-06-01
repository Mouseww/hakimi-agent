//! Home Assistant platform adapter.
//!
//! Sends outbound messages as Home Assistant persistent notifications through
//! the REST API. Inbound WebSocket event monitoring remains outside this first
//! Rust-native slice so the adapter stays dependency-light and matches
//! Hakimi's current outbound gateway model.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

const DEFAULT_HOME_ASSISTANT_BASE_URL: &str = "http://homeassistant.local:8123";
const MAX_NOTIFICATION_CHARS: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeAssistantAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_homeassistant_bot_id")]
    pub bot_id: String,
    /// Home Assistant base URL, for example `http://homeassistant.local:8123`.
    pub base_url: String,
    /// Long-lived access token.
    pub token: String,
    /// Optional default notification title for bare `homeassistant` sends.
    #[serde(default = "default_notification_title")]
    pub default_title: String,
}

fn default_homeassistant_bot_id() -> String {
    "homeassistant".to_string()
}

fn default_notification_title() -> String {
    "Hakimi".to_string()
}

pub struct HomeAssistantAdapter {
    config: HomeAssistantAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl HomeAssistantAdapter {
    pub fn new(config: HomeAssistantAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }

    fn base_url(&self) -> &str {
        let configured = self.config.base_url.trim();
        if configured.is_empty() {
            DEFAULT_HOME_ASSISTANT_BASE_URL
        } else {
            configured
        }
    }

    fn notification_url(&self) -> String {
        format!(
            "{}/api/services/persistent_notification/create",
            self.base_url().trim_end_matches('/')
        )
    }

    fn title<'a>(&'a self, chat_id: &'a str) -> &'a str {
        let chat_id = chat_id.trim();
        if chat_id.is_empty() {
            self.config.default_title.trim()
        } else {
            chat_id
        }
    }
}

#[async_trait]
impl PlatformAdapter for HomeAssistantAdapter {
    fn name(&self) -> &str {
        "homeassistant"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.config.token.trim().is_empty() {
            anyhow::bail!("Home Assistant gateway requires a long-lived access token");
        }
        info!(base_url = %self.base_url(), "Home Assistant adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        if self.config.token.trim().is_empty() {
            anyhow::bail!("Home Assistant send_message requires a long-lived access token");
        }

        let title = self.title(chat_id);
        let body = notification_message(text);
        let payload = serde_json::json!({
            "title": title,
            "message": body,
        });

        let resp = self
            .client
            .post(self.notification_url())
            .bearer_auth(self.config.token.trim())
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Home Assistant notification failed: status={}, body={}",
                status,
                body_text
            );
        }

        info!(
            title = title,
            text_len = text.len(),
            "Home Assistant: notification sent"
        );
        Ok(())
    }

    fn max_message_chars(&self) -> Option<usize> {
        Some(MAX_NOTIFICATION_CHARS)
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("Home Assistant adapter disconnected");
        Ok(())
    }
}

fn notification_message(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.chars().take(MAX_NOTIFICATION_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> HomeAssistantAdapterConfig {
        HomeAssistantAdapterConfig {
            bot_id: "homeassistant".into(),
            base_url: "http://ha.local:8123/".into(),
            token: "ha_token".into(),
            default_title: "Hakimi Alert".into(),
        }
    }

    #[test]
    fn test_construction() {
        let adapter = HomeAssistantAdapter::new(make_config());
        assert_eq!(adapter.name(), "homeassistant");
        assert_eq!(adapter.bot_id(), "homeassistant");
    }

    #[test]
    fn test_notification_url_trims_base_slash() {
        let adapter = HomeAssistantAdapter::new(make_config());
        assert_eq!(
            adapter.notification_url(),
            "http://ha.local:8123/api/services/persistent_notification/create"
        );
    }

    #[test]
    fn test_default_base_url_when_empty() {
        let mut config = make_config();
        config.base_url.clear();
        let adapter = HomeAssistantAdapter::new(config);
        assert_eq!(
            adapter.notification_url(),
            "http://homeassistant.local:8123/api/services/persistent_notification/create"
        );
    }

    #[test]
    fn test_title_fallback_and_override() {
        let adapter = HomeAssistantAdapter::new(make_config());
        assert_eq!(adapter.title(""), "Hakimi Alert");
        assert_eq!(adapter.title("Kitchen"), "Kitchen");
    }

    #[test]
    fn test_notification_message_is_char_safe() {
        let input = "好".repeat(MAX_NOTIFICATION_CHARS + 1);
        let message = notification_message(&input);
        assert_eq!(message.chars().count(), MAX_NOTIFICATION_CHARS);
        assert!(message.ends_with('好'));
    }

    #[test]
    fn test_take_receiver_once() {
        let mut adapter = HomeAssistantAdapter::new(make_config());
        assert!(adapter.take_receiver().is_some());
        assert!(adapter.take_receiver().is_none());
    }
}
