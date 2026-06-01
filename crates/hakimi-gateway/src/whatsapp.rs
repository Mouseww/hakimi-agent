//! WhatsApp Business Cloud API platform adapter.
//!
//! Hermes supports WhatsApp through a bridge-oriented adapter. This Rust-native
//! slice covers the dependency-light Cloud API outbound path so gateway
//! delivery and cron/send_message can target WhatsApp without a Node bridge.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

const WHATSAPP_GRAPH_BASE: &str = "https://graph.facebook.com";
const DEFAULT_API_VERSION: &str = "v20.0";
const MAX_WHATSAPP_CHARS: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_whatsapp_bot_id")]
    pub bot_id: String,
    /// WhatsApp Business Cloud API access token.
    pub access_token: String,
    /// Meta phone number ID used in `/{phone_number_id}/messages`.
    pub phone_number_id: String,
    /// Optional default recipient for bare `whatsapp` sends and cron delivery.
    #[serde(default)]
    pub home_channel: String,
    /// Graph API version, for example `v20.0`.
    #[serde(default = "default_api_version")]
    pub api_version: String,
    /// Optional Graph API base URL override for tests or proxies.
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_whatsapp_bot_id() -> String {
    "whatsapp".to_string()
}

fn default_api_version() -> String {
    DEFAULT_API_VERSION.to_string()
}

pub struct WhatsAppAdapter {
    config: WhatsAppAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl WhatsAppAdapter {
    pub fn new(config: WhatsAppAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }

    fn api_base(&self) -> String {
        self.config
            .base_url
            .as_deref()
            .unwrap_or(WHATSAPP_GRAPH_BASE)
            .trim_end_matches('/')
            .to_string()
    }

    fn api_version(&self) -> &str {
        let configured = self.config.api_version.trim().trim_matches('/');
        if configured.is_empty() {
            DEFAULT_API_VERSION
        } else {
            configured
        }
    }

    fn messages_url(&self) -> String {
        format!(
            "{}/{}/{}/messages",
            self.api_base(),
            self.api_version(),
            self.config.phone_number_id.trim()
        )
    }

    fn recipient<'a>(&'a self, chat_id: &'a str) -> &'a str {
        let chat_id = chat_id.trim();
        if chat_id.is_empty() {
            self.config.home_channel.trim()
        } else {
            chat_id
        }
    }
}

#[async_trait]
impl PlatformAdapter for WhatsAppAdapter {
    fn name(&self) -> &str {
        "whatsapp"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.config.access_token.trim().is_empty()
            || self.config.phone_number_id.trim().is_empty()
        {
            anyhow::bail!("WhatsApp gateway requires access_token and phone_number_id");
        }
        info!(
            phone_number_id = %redact_recipient(&self.config.phone_number_id),
            "WhatsApp adapter connected"
        );
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let recipient = self.recipient(chat_id);
        if recipient.is_empty() {
            anyhow::bail!("WhatsApp send_message requires a recipient phone number");
        }
        if self.config.access_token.trim().is_empty()
            || self.config.phone_number_id.trim().is_empty()
        {
            anyhow::bail!("WhatsApp send_message requires access_token and phone_number_id");
        }

        for chunk in whatsapp_chunks(text) {
            let payload = serde_json::json!({
                "messaging_product": "whatsapp",
                "to": recipient,
                "type": "text",
                "text": {
                    "preview_url": false,
                    "body": chunk,
                },
            });

            let resp = self
                .client
                .post(self.messages_url())
                .bearer_auth(self.config.access_token.trim())
                .json(&payload)
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                anyhow::bail!(
                    "WhatsApp send_message failed: status={}, body={}",
                    status,
                    body_text
                );
            }
        }

        info!(
            to = %redact_recipient(recipient),
            text_len = text.len(),
            "WhatsApp: message sent"
        );
        Ok(())
    }

    fn max_message_chars(&self) -> Option<usize> {
        Some(MAX_WHATSAPP_CHARS)
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("WhatsApp adapter disconnected");
        Ok(())
    }
}

fn whatsapp_chunks(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() >= MAX_WHATSAPP_CHARS {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn redact_recipient(value: &str) -> String {
    let visible: String = value
        .chars()
        .rev()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if visible.is_empty() {
        "***".to_string()
    } else {
        format!("***{visible}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> WhatsAppAdapterConfig {
        WhatsAppAdapterConfig {
            bot_id: "whatsapp".into(),
            access_token: "wa_token".into(),
            phone_number_id: "1234567890".into(),
            home_channel: "15552223333".into(),
            api_version: "v20.0".into(),
            base_url: None,
        }
    }

    #[test]
    fn test_construction() {
        let adapter = WhatsAppAdapter::new(make_config());
        assert_eq!(adapter.name(), "whatsapp");
        assert_eq!(adapter.bot_id(), "whatsapp");
    }

    #[test]
    fn test_messages_url_uses_phone_number_id() {
        let adapter = WhatsAppAdapter::new(make_config());
        assert_eq!(
            adapter.messages_url(),
            "https://graph.facebook.com/v20.0/1234567890/messages"
        );
    }

    #[test]
    fn test_base_url_override() {
        let mut config = make_config();
        config.base_url = Some("https://graph.test/".into());
        config.api_version = "/v19.0/".into();
        let adapter = WhatsAppAdapter::new(config);
        assert_eq!(
            adapter.messages_url(),
            "https://graph.test/v19.0/1234567890/messages"
        );
    }

    #[test]
    fn test_empty_api_version_uses_default() {
        let mut config = make_config();
        config.api_version.clear();
        let adapter = WhatsAppAdapter::new(config);
        assert!(adapter.messages_url().contains("/v20.0/"));
    }

    #[test]
    fn test_home_channel_fallback() {
        let adapter = WhatsAppAdapter::new(make_config());
        assert_eq!(adapter.recipient(""), "15552223333");
        assert_eq!(adapter.recipient("15554445555"), "15554445555");
    }

    #[test]
    fn test_whatsapp_chunks_are_char_safe() {
        let input = "好".repeat(MAX_WHATSAPP_CHARS + 1);
        let chunks = whatsapp_chunks(&input);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), MAX_WHATSAPP_CHARS);
        assert_eq!(chunks[1], "好");
    }

    #[test]
    fn test_empty_message_chunk() {
        assert_eq!(whatsapp_chunks("  "), vec![String::new()]);
    }

    #[test]
    fn test_redact_recipient_keeps_only_tail() {
        assert_eq!(redact_recipient("+1 (555) 222-3333"), "***3333");
        assert_eq!(redact_recipient(""), "***");
    }
}
