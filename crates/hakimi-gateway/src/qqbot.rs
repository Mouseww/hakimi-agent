//! QQBot platform adapter.
//!
//! This Rust-native slice covers outbound text delivery through the official
//! QQ Bot v2 REST API. Hermes' Python adapter also implements WebSocket
//! ingress, media upload, keyboards, and QR onboarding; those remain separate
//! parity slices.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

const QQ_API_BASE: &str = "https://api.sgroup.qq.com";
const QQ_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const MAX_QQBOT_CHARS: usize = 4000;
const MSG_TYPE_TEXT: u8 = 0;
const MSG_TYPE_MARKDOWN: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QQBotAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_qqbot_bot_id")]
    pub bot_id: String,
    /// QQ Bot app id.
    pub app_id: String,
    /// QQ Bot client secret.
    pub client_secret: String,
    /// Optional default target for bare `qqbot` sends and cron delivery.
    #[serde(default)]
    pub home_channel: String,
    /// Default chat kind when the target is not prefixed.
    #[serde(default = "default_chat_type")]
    pub default_chat_type: String,
    /// Send Hermes-style markdown payloads for C2C/group messages.
    #[serde(default = "default_markdown_support")]
    pub markdown_support: bool,
    /// Optional API base URL override for tests or proxies.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Optional token endpoint override for tests or proxies.
    #[serde(default)]
    pub token_url: Option<String>,
}

fn default_qqbot_bot_id() -> String {
    "qqbot".to_string()
}

fn default_chat_type() -> String {
    "c2c".to_string()
}

fn default_markdown_support() -> bool {
    true
}

struct QQBotToken {
    value: String,
    expires_at: Instant,
}

pub struct QQBotAdapter {
    config: QQBotAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    token: Mutex<Option<QQBotToken>>,
    msg_seq: AtomicU64,
}

impl QQBotAdapter {
    pub fn new(config: QQBotAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
            token: Mutex::new(None),
            msg_seq: AtomicU64::new(1),
        }
    }

    fn api_base(&self) -> String {
        self.config
            .base_url
            .as_deref()
            .unwrap_or(QQ_API_BASE)
            .trim_end_matches('/')
            .to_string()
    }

    fn token_url(&self) -> String {
        self.config
            .token_url
            .as_deref()
            .unwrap_or(QQ_TOKEN_URL)
            .trim()
            .to_string()
    }

    fn next_msg_seq(&self) -> u64 {
        self.msg_seq.fetch_add(1, Ordering::Relaxed)
    }

    fn recipient<'a>(&'a self, chat_id: &'a str) -> &'a str {
        let chat_id = chat_id.trim();
        if chat_id.is_empty() {
            self.config.home_channel.trim()
        } else {
            chat_id
        }
    }

    fn resolve_chat(&self, chat_id: &str) -> anyhow::Result<QQChat> {
        let target = self.recipient(chat_id);
        if target.is_empty() {
            anyhow::bail!("QQBot send_message requires a user, group, or channel target");
        }

        let (kind, id) = parse_chat_target(target, self.config.default_chat_type.trim())?;
        match kind {
            "c2c" | "user" | "dm" => Ok(QQChat::C2c(id.to_string())),
            "group" => Ok(QQChat::Group(id.to_string())),
            "guild" | "channel" => Ok(QQChat::Guild(id.to_string())),
            other => anyhow::bail!("unsupported QQBot chat type '{}'", other),
        }
    }

    fn text_body(&self, content: &str) -> Value {
        if self.config.markdown_support {
            serde_json::json!({
                "markdown": {
                    "content": content,
                },
                "msg_type": MSG_TYPE_MARKDOWN,
                "msg_seq": self.next_msg_seq(),
            })
        } else {
            serde_json::json!({
                "content": strip_light_markdown(content),
                "msg_type": MSG_TYPE_TEXT,
                "msg_seq": self.next_msg_seq(),
            })
        }
    }

    fn message_url(&self, chat: &QQChat) -> String {
        match chat {
            QQChat::C2c(openid) => format!("{}/v2/users/{}/messages", self.api_base(), openid),
            QQChat::Group(openid) => format!("{}/v2/groups/{}/messages", self.api_base(), openid),
            QQChat::Guild(channel_id) => {
                format!("{}/channels/{}/messages", self.api_base(), channel_id)
            }
        }
    }

    async fn access_token(&self) -> anyhow::Result<String> {
        let now = Instant::now();
        {
            let guard = self.token.lock().await;
            if let Some(token) = guard.as_ref()
                && token.expires_at > now
            {
                return Ok(token.value.clone());
            }
        }

        let mut guard = self.token.lock().await;
        if let Some(token) = guard.as_ref()
            && token.expires_at > Instant::now()
        {
            return Ok(token.value.clone());
        }

        let payload = serde_json::json!({
            "appId": self.config.app_id.trim(),
            "clientSecret": self.config.client_secret.trim(),
        });
        let response = self
            .client
            .post(self.token_url())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "QQBot token request failed: status={}, body={}",
                status,
                body
            );
        }

        let body: Value = response.json().await?;
        let token = body
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("QQBot token response missing access_token"))?
            .to_string();
        let expires_in = parse_expires_in(&body).unwrap_or(7_200);
        let refresh_after = expires_in.saturating_sub(60).max(60);
        *guard = Some(QQBotToken {
            value: token.clone(),
            expires_at: Instant::now() + Duration::from_secs(refresh_after),
        });
        Ok(token)
    }

    async fn send_one(&self, chat: &QQChat, content: &str) -> anyhow::Result<()> {
        let token = self.access_token().await?;
        let body = match chat {
            QQChat::Guild(_) => serde_json::json!({ "content": content }),
            QQChat::C2c(_) | QQChat::Group(_) => self.text_body(content),
        };

        let response = self
            .client
            .post(self.message_url(chat))
            .header("Authorization", format!("QQBot {token}"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", qqbot_user_agent())
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "QQBot send_message failed: status={}, body={}",
                status,
                body
            );
        }
        Ok(())
    }
}

#[async_trait]
impl PlatformAdapter for QQBotAdapter {
    fn name(&self) -> &str {
        "qqbot"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.config.app_id.trim().is_empty() || self.config.client_secret.trim().is_empty() {
            anyhow::bail!("QQBot gateway requires app_id and client_secret");
        }
        info!(
            app_id = %redact_id(&self.config.app_id),
            default_chat_type = %self.config.default_chat_type,
            "QQBot adapter connected"
        );
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let chat = self.resolve_chat(chat_id)?;
        for chunk in qqbot_chunks(text) {
            self.send_one(&chat, &chunk).await?;
        }

        info!(
            target = %chat.redacted(),
            text_len = text.len(),
            "QQBot: message sent"
        );
        Ok(())
    }

    fn max_message_chars(&self) -> Option<usize> {
        Some(MAX_QQBOT_CHARS)
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("QQBot adapter disconnected");
        Ok(())
    }
}

enum QQChat {
    C2c(String),
    Group(String),
    Guild(String),
}

impl QQChat {
    fn redacted(&self) -> String {
        match self {
            QQChat::C2c(value) => format!("c2c:{}", redact_id(value)),
            QQChat::Group(value) => format!("group:{}", redact_id(value)),
            QQChat::Guild(value) => format!("guild:{}", redact_id(value)),
        }
    }
}

fn parse_chat_target<'a>(
    target: &'a str,
    default_chat_type: &'a str,
) -> anyhow::Result<(&'static str, &'a str)> {
    let target = target.trim();
    if let Some((kind, id)) = target.split_once(':') {
        let kind = kind.trim().to_ascii_lowercase();
        let id = id.trim();
        if id.is_empty() {
            anyhow::bail!("QQBot target '{}' is missing an id", target);
        }
        if matches!(
            kind.as_str(),
            "c2c" | "user" | "dm" | "group" | "guild" | "channel"
        ) {
            let normalized_kind = match kind.as_str() {
                "user" | "dm" => "c2c",
                "channel" => "guild",
                "c2c" => "c2c",
                "group" => "group",
                "guild" => "guild",
                _ => unreachable!(),
            };
            return Ok((normalized_kind, id));
        }
    }

    let default_kind = match default_chat_type.trim().to_ascii_lowercase().as_str() {
        "group" => "group",
        "guild" | "channel" => "guild",
        _ => "c2c",
    };
    Ok((default_kind, target))
}

fn parse_expires_in(body: &Value) -> Option<u64> {
    body.get("expires_in")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
        })
        .filter(|value| *value > 0)
}

fn qqbot_chunks(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() >= MAX_QQBOT_CHARS {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn strip_light_markdown(text: &str) -> String {
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

fn qqbot_user_agent() -> String {
    format!(
        "QQBotAdapter/1.1.0 (Rust; Hakimi/{})",
        env!("CARGO_PKG_VERSION")
    )
}

fn redact_id(value: &str) -> String {
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

    fn make_config() -> QQBotAdapterConfig {
        QQBotAdapterConfig {
            bot_id: "qqbot".into(),
            app_id: "app_123456".into(),
            client_secret: "client_secret".into(),
            home_channel: "group:home_openid".into(),
            default_chat_type: "c2c".into(),
            markdown_support: true,
            base_url: None,
            token_url: None,
        }
    }

    #[test]
    fn test_construction() {
        let adapter = QQBotAdapter::new(make_config());
        assert_eq!(adapter.name(), "qqbot");
        assert_eq!(adapter.bot_id(), "qqbot");
    }

    #[test]
    fn test_api_urls() {
        let mut config = make_config();
        config.base_url = Some("https://qq.test/api/".into());
        config.token_url = Some("https://qq.test/token".into());
        let adapter = QQBotAdapter::new(config);
        assert_eq!(adapter.token_url(), "https://qq.test/token");
        assert_eq!(
            adapter.message_url(&QQChat::Group("abc".into())),
            "https://qq.test/api/v2/groups/abc/messages"
        );
        assert_eq!(
            adapter.message_url(&QQChat::Guild("chan".into())),
            "https://qq.test/api/channels/chan/messages"
        );
    }

    #[test]
    fn test_resolve_chat_prefixes() {
        let adapter = QQBotAdapter::new(make_config());
        assert!(matches!(
            adapter.resolve_chat("group:group_openid").unwrap(),
            QQChat::Group(_)
        ));
        assert!(matches!(
            adapter.resolve_chat("guild:channel_id").unwrap(),
            QQChat::Guild(_)
        ));
        assert!(matches!(
            adapter.resolve_chat("user:user_openid").unwrap(),
            QQChat::C2c(_)
        ));
    }

    #[test]
    fn test_home_channel_fallback() {
        let adapter = QQBotAdapter::new(make_config());
        assert!(matches!(
            adapter.resolve_chat("").unwrap(),
            QQChat::Group(_)
        ));
    }

    #[test]
    fn test_text_body_supports_markdown_and_plain_text() {
        let adapter = QQBotAdapter::new(make_config());
        let body = adapter.text_body("**hello**");
        assert_eq!(body["msg_type"], MSG_TYPE_MARKDOWN);
        assert_eq!(body["markdown"]["content"], "**hello**");

        let mut config = make_config();
        config.markdown_support = false;
        let adapter = QQBotAdapter::new(config);
        let body = adapter.text_body("**hello**");
        assert_eq!(body["msg_type"], MSG_TYPE_TEXT);
        assert_eq!(body["content"], "hello");
    }

    #[test]
    fn test_qqbot_chunks_are_utf8_safe() {
        let input = "好".repeat(MAX_QQBOT_CHARS + 1);
        let chunks = qqbot_chunks(&input);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), MAX_QQBOT_CHARS);
        assert_eq!(chunks[1], "好");
    }

    #[test]
    fn test_parse_expires_in_accepts_number_and_string() {
        assert_eq!(
            parse_expires_in(&serde_json::json!({"expires_in": 7200})),
            Some(7200)
        );
        assert_eq!(
            parse_expires_in(&serde_json::json!({"expires_in": "3600"})),
            Some(3600)
        );
    }

    #[test]
    fn test_redact_id_keeps_tail() {
        assert_eq!(redact_id("app_123456"), "***3456");
        assert_eq!(redact_id(""), "***");
    }
}
