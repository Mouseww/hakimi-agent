//! BlueBubbles / iMessage platform adapter.
//!
//! Hermes' BlueBubbles adapter supports inbound webhooks, attachments, and
//! tapbacks. This first Rust-native slice covers dependency-light outbound
//! text delivery through a local BlueBubbles server so gateway routing, cron,
//! and `send_message` can target iMessage without a Python bridge.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

const MAX_BLUEBUBBLES_CHARS: usize = 4000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueBubblesAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_bluebubbles_bot_id")]
    pub bot_id: String,
    /// BlueBubbles server URL, for example `http://127.0.0.1:1234`.
    pub server_url: String,
    /// BlueBubbles server password / GUID query token.
    pub password: String,
    /// Optional default chat GUID, phone number, or email for bare sends.
    #[serde(default)]
    pub home_channel: String,
    /// Allow `/api/v1/chat/new` when the target is an address and no chat
    /// GUID can be resolved from the recent chat list.
    #[serde(default)]
    pub allow_new_chat: bool,
}

fn default_bluebubbles_bot_id() -> String {
    "bluebubbles".to_string()
}

pub struct BlueBubblesAdapter {
    config: BlueBubblesAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl BlueBubblesAdapter {
    pub fn new(config: BlueBubblesAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }

    fn server_url(&self) -> String {
        normalize_server_url(&self.config.server_url)
    }

    fn api_url(&self, path: &str) -> String {
        let base = self.server_url();
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!("{base}{path}")
    }

    fn recipient<'a>(&'a self, chat_id: &'a str) -> &'a str {
        let chat_id = chat_id.trim();
        if chat_id.is_empty() {
            self.config.home_channel.trim()
        } else {
            chat_id
        }
    }

    async fn post_json(&self, path: &str, payload: Value) -> anyhow::Result<Value> {
        let response = self
            .client
            .post(self.api_url(path))
            .query(&[("password", self.config.password.trim())])
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "BlueBubbles request failed: status={}, body={}",
                status,
                body
            );
        }

        Ok(response.json().await.unwrap_or(Value::Null))
    }

    async fn resolve_chat_guid(&self, target: &str) -> anyhow::Result<Option<String>> {
        let target = target.trim();
        if target.is_empty() {
            return Ok(None);
        }
        if target.contains(';') {
            return Ok(Some(target.to_string()));
        }

        let payload = serde_json::json!({
            "limit": 100,
            "offset": 0,
            "with": ["participants"],
        });
        let response = self.post_json("/api/v1/chat/query", payload).await?;
        let Some(chats) = response.get("data").and_then(Value::as_array) else {
            return Ok(None);
        };

        for chat in chats {
            let guid = string_field(chat, &["guid", "chatGuid"]);
            let identifier = string_field(chat, &["chatIdentifier", "identifier"]);
            if identifier.as_deref() == Some(target) {
                return Ok(guid);
            }

            let Some(participants) = chat.get("participants").and_then(Value::as_array) else {
                continue;
            };
            for participant in participants {
                if string_field(participant, &["address"]).as_deref() == Some(target) {
                    return Ok(guid);
                }
            }
        }

        Ok(None)
    }

    async fn send_new_chat(&self, target: &str, message: &str) -> anyhow::Result<()> {
        let payload = serde_json::json!({
            "addresses": [target],
            "message": message,
            "tempGuid": temp_guid(),
        });
        self.post_json("/api/v1/chat/new", payload).await?;
        Ok(())
    }
}

#[async_trait]
impl PlatformAdapter for BlueBubblesAdapter {
    fn name(&self) -> &str {
        "bluebubbles"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.server_url().is_empty() || self.config.password.trim().is_empty() {
            anyhow::bail!("BlueBubbles gateway requires server_url and password");
        }
        info!(server_url = %self.server_url(), "BlueBubbles adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        if self.config.password.trim().is_empty() {
            anyhow::bail!("BlueBubbles send_message requires password");
        }

        let recipient = self.recipient(chat_id);
        if recipient.is_empty() {
            anyhow::bail!("BlueBubbles send_message requires a chat GUID or address");
        }

        let chunks = bluebubbles_chunks(&strip_bluebubbles_markdown(text));
        match self.resolve_chat_guid(recipient).await? {
            Some(guid) => {
                for chunk in chunks {
                    let payload = serde_json::json!({
                        "chatGuid": guid.as_str(),
                        "tempGuid": temp_guid(),
                        "message": chunk,
                    });
                    self.post_json("/api/v1/message/text", payload).await?;
                }
            }
            None if self.config.allow_new_chat && looks_like_address(recipient) => {
                if chunks.len() != 1 {
                    anyhow::bail!(
                        "BlueBubbles new-chat fallback requires a single message chunk for target '{}'",
                        redact_target(recipient)
                    );
                }
                self.send_new_chat(recipient, &chunks[0]).await?;
            }
            None => {
                anyhow::bail!(
                    "BlueBubbles chat not found for target '{}'",
                    redact_target(recipient)
                );
            }
        }

        info!(
            target = %redact_target(recipient),
            text_len = text.len(),
            "BlueBubbles: message sent"
        );
        Ok(())
    }

    fn max_message_chars(&self) -> Option<usize> {
        Some(MAX_BLUEBUBBLES_CHARS)
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("BlueBubbles adapter disconnected");
        Ok(())
    }
}

fn normalize_server_url(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        return String::new();
    }
    let with_scheme = if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    with_scheme.trim_end_matches('/').to_string()
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn temp_guid() -> String {
    format!(
        "temp-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}

fn looks_like_address(value: &str) -> bool {
    value.contains('@') || value.starts_with('+')
}

fn bluebubbles_chunks(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut chunks = Vec::new();
    for paragraph in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let mut current = String::new();
        for ch in paragraph.chars() {
            if current.chars().count() >= MAX_BLUEBUBBLES_CHARS {
                chunks.push(std::mem::take(&mut current));
            }
            current.push(ch);
        }
        if !current.is_empty() {
            chunks.push(current);
        }
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

fn strip_bluebubbles_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    let mut in_code_block = false;

    while let Some(ch) = chars.next() {
        if ch == '`' && chars.peek() == Some(&'`') {
            chars.next();
            if chars.peek() == Some(&'`') {
                chars.next();
                in_code_block = !in_code_block;
                continue;
            }
            out.push(ch);
            out.push('`');
            continue;
        }
        if matches!(ch, '*' | '_' | '`' | '~') && !in_code_block {
            continue;
        }
        out.push(ch);
    }

    out.trim().to_string()
}

fn redact_target(value: &str) -> String {
    let value = value.trim();
    if value.contains('@') {
        let mut parts = value.splitn(2, '@');
        let local = parts.next().unwrap_or_default();
        let domain = parts.next().unwrap_or_default();
        let visible: String = local.chars().take(2).collect();
        return format!("{visible}***@{domain}");
    }

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

    fn make_config() -> BlueBubblesAdapterConfig {
        BlueBubblesAdapterConfig {
            bot_id: "bluebubbles".into(),
            server_url: "127.0.0.1:1234/".into(),
            password: "bb_password".into(),
            home_channel: "iMessage;-;user@example.com".into(),
            allow_new_chat: true,
        }
    }

    #[test]
    fn construction_sets_platform_identity() {
        let adapter = BlueBubblesAdapter::new(make_config());
        assert_eq!(adapter.name(), "bluebubbles");
        assert_eq!(adapter.bot_id(), "bluebubbles");
    }

    #[test]
    fn normalizes_server_url() {
        assert_eq!(
            normalize_server_url("127.0.0.1:1234/"),
            "http://127.0.0.1:1234"
        );
        assert_eq!(
            normalize_server_url("https://bb.example.com///"),
            "https://bb.example.com"
        );
    }

    #[test]
    fn api_url_joins_without_double_slashes() {
        let adapter = BlueBubblesAdapter::new(make_config());
        assert_eq!(
            adapter.api_url("/api/v1/message/text"),
            "http://127.0.0.1:1234/api/v1/message/text"
        );
    }

    #[test]
    fn recipient_falls_back_to_home_channel() {
        let adapter = BlueBubblesAdapter::new(make_config());
        assert_eq!(adapter.recipient(""), "iMessage;-;user@example.com");
        assert_eq!(adapter.recipient("chat-guid"), "chat-guid");
    }

    #[test]
    fn chunks_are_utf8_safe_and_paragraph_aware() {
        let input = format!("{}\n\nsecond", "好".repeat(MAX_BLUEBUBBLES_CHARS + 1));
        let chunks = bluebubbles_chunks(&input);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chars().count(), MAX_BLUEBUBBLES_CHARS);
        assert_eq!(chunks[1], "好");
        assert_eq!(chunks[2], "second");
    }

    #[test]
    fn strips_chat_markdown_without_destroying_code_blocks() {
        assert_eq!(
            strip_bluebubbles_markdown("**hello** _world_ `x`\n```rust\nlet x = 1;\n```"),
            "hello world x\nrust\nlet x = 1;"
        );
    }

    #[test]
    fn redacts_addresses_and_chat_ids() {
        assert_eq!(redact_target("person@example.com"), "pe***@example.com");
        assert_eq!(redact_target("iMessage;-;+15551234567"), "***4567");
    }
}
