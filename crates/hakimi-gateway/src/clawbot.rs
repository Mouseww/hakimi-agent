//! WeChat ClawBot bridge adapter.
//!
//! ClawBot deployments differ in their exact HTTP schema, so this adapter uses
//! a small configurable HTTP bridge contract:
//! - `GET {base_url}{poll_path}?offset=...&limit=...` receives messages.
//! - `POST {base_url}{send_path}` sends messages.
//! - `POST {base_url}{edit_path}` edits messages when the bridge supports it.
//!
//! The JSON parser intentionally accepts several common field aliases so the
//! bridge can sit in front of ClawBot without Hakimi-specific code changes.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::{GatewayMessage, PlatformAdapter};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:5700";
const DEFAULT_POLL_PATH: &str = "/messages";
const DEFAULT_SEND_PATH: &str = "/send_message";
const DEFAULT_EDIT_PATH: &str = "/edit_message";
const DEFAULT_POLL_INTERVAL_MS: u64 = 1_000;
const DEFAULT_POLL_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawBotAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_clawbot_bot_id")]
    pub bot_id: String,
    /// ClawBot bridge base URL, e.g. `http://127.0.0.1:5700`.
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// Optional bearer token sent as `Authorization: Bearer ...`.
    #[serde(default)]
    pub token: String,
    /// Polling endpoint path.
    #[serde(default = "default_poll_path")]
    pub poll_path: String,
    /// Send endpoint path.
    #[serde(default = "default_send_path")]
    pub send_path: String,
    /// Edit endpoint path.
    #[serde(default = "default_edit_path")]
    pub edit_path: String,
    /// Polling interval in milliseconds.
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Maximum messages requested per poll.
    #[serde(default = "default_poll_limit")]
    pub poll_limit: usize,
}

fn default_clawbot_bot_id() -> String {
    "clawbot".to_string()
}

fn default_base_url() -> String {
    DEFAULT_BASE_URL.to_string()
}

fn default_poll_path() -> String {
    DEFAULT_POLL_PATH.to_string()
}

fn default_send_path() -> String {
    DEFAULT_SEND_PATH.to_string()
}

fn default_edit_path() -> String {
    DEFAULT_EDIT_PATH.to_string()
}

fn default_poll_interval_ms() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}

fn default_poll_limit() -> usize {
    DEFAULT_POLL_LIMIT
}

impl Default for ClawBotAdapterConfig {
    fn default() -> Self {
        Self {
            bot_id: default_clawbot_bot_id(),
            base_url: default_base_url(),
            token: String::new(),
            poll_path: default_poll_path(),
            send_path: default_send_path(),
            edit_path: default_edit_path(),
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            poll_limit: DEFAULT_POLL_LIMIT,
        }
    }
}

pub struct ClawBotAdapter {
    config: ClawBotAdapterConfig,
    client: Client,
    msg_tx: mpsc::UnboundedSender<GatewayMessage>,
    msg_rx: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    poll_handle: Option<JoinHandle<()>>,
}

impl ClawBotAdapter {
    pub fn new(config: ClawBotAdapterConfig) -> Self {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        Self {
            config,
            client: Client::new(),
            msg_tx,
            msg_rx: Some(msg_rx),
            poll_handle: None,
        }
    }

    fn endpoint(&self, path: &str) -> String {
        join_url(&self.config.base_url, path)
    }

    fn auth_request(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.config.token.is_empty() {
            builder
        } else {
            builder.bearer_auth(&self.config.token)
        }
    }

    fn spawn_poll_loop(&self) -> JoinHandle<()> {
        let client = self.client.clone();
        let config = self.config.clone();
        let msg_tx = self.msg_tx.clone();
        tokio::spawn(async move {
            let mut offset: Option<String> = None;
            loop {
                match poll_once(&client, &config, offset.as_deref()).await {
                    Ok(batch) => {
                        for envelope in batch.messages {
                            if let Some(next_offset) = envelope.next_offset.clone() {
                                offset = Some(next_offset);
                            }
                            if let Some(msg) =
                                convert_clawbot_message(&config.bot_id, &envelope.value)
                                && msg_tx.send(msg).is_err()
                            {
                                error!("ClawBot receiver dropped; stopping poll loop");
                                return;
                            }
                        }
                        if let Some(next_offset) = batch.next_offset {
                            offset = Some(next_offset);
                        }
                    }
                    Err(err) => {
                        warn!(error = %err, "ClawBot poll failed, retrying");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(config.poll_interval_ms)).await;
            }
        })
    }
}

#[async_trait]
impl PlatformAdapter for ClawBotAdapter {
    fn name(&self) -> &str {
        "clawbot"
    }

    fn bot_id(&self) -> &str {
        &self.config.bot_id
    }

    async fn connect(&mut self) -> Result<()> {
        info!(base_url = %self.config.base_url, "connecting ClawBot adapter");
        let handle = self.spawn_poll_loop();
        self.poll_handle = Some(handle);
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<()> {
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "conversation_id": chat_id,
            "to": chat_id,
            "text": text,
            "content": text,
            "msgtype": "text",
        });
        let resp = self
            .auth_request(self.client.post(self.endpoint(&self.config.send_path)))
            .json(&payload)
            .send()
            .await
            .context("failed to send ClawBot message")?;
        ensure_success(resp, "ClawBot send_message").await?;
        debug!(chat_id, text_len = text.len(), "ClawBot message sent");
        Ok(())
    }

    async fn send_message_get_id(&self, chat_id: &str, text: &str) -> Result<Option<i64>> {
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "conversation_id": chat_id,
            "to": chat_id,
            "text": text,
            "content": text,
            "msgtype": "text",
        });
        let resp = self
            .auth_request(self.client.post(self.endpoint(&self.config.send_path)))
            .json(&payload)
            .send()
            .await
            .context("failed to send ClawBot message")?;
        let body = ensure_success_json(resp, "ClawBot send_message").await?;
        Ok(extract_i64(&body, &["message_id", "msg_id", "id"]))
    }

    async fn edit_message(&self, chat_id: &str, message_id: i64, text: &str) -> Result<()> {
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "conversation_id": chat_id,
            "message_id": message_id,
            "msg_id": message_id,
            "text": text,
            "content": text,
        });
        let resp = self
            .auth_request(self.client.post(self.endpoint(&self.config.edit_path)))
            .json(&payload)
            .send()
            .await
            .context("failed to edit ClawBot message")?;
        ensure_success(resp, "ClawBot edit_message").await
    }

    async fn send_chat_action(&self, _chat_id: &str, _action: &str) -> Result<()> {
        // WeChat/ClawBot bridges commonly do not expose typing indicators.
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.msg_rx.take()
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(handle) = self.poll_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
        info!("ClawBot adapter disconnected");
        Ok(())
    }
}

#[derive(Debug)]
struct PollBatch {
    messages: Vec<PollMessageEnvelope>,
    next_offset: Option<String>,
}

#[derive(Debug)]
struct PollMessageEnvelope {
    value: Value,
    next_offset: Option<String>,
}

async fn poll_once(
    client: &Client,
    config: &ClawBotAdapterConfig,
    offset: Option<&str>,
) -> Result<PollBatch> {
    let mut request = client
        .get(join_url(&config.base_url, &config.poll_path))
        .query(&[("limit", config.poll_limit.to_string())]);
    if let Some(offset) = offset
        && !offset.is_empty()
    {
        request = request.query(&[("offset", offset.to_string())]);
    }
    if !config.token.is_empty() {
        request = request.bearer_auth(&config.token);
    }
    let resp = request
        .send()
        .await
        .context("failed to poll ClawBot messages")?;
    let body = ensure_success_json(resp, "ClawBot poll").await?;
    Ok(parse_poll_batch(body))
}

fn parse_poll_batch(body: Value) -> PollBatch {
    let next_offset = extract_string(&body, &["next_offset", "nextOffset", "offset", "cursor"]);
    let raw_messages = body
        .get("messages")
        .or_else(|| body.get("data"))
        .or_else(|| body.get("items"))
        .or_else(|| body.get("result"));

    let values = match raw_messages {
        Some(Value::Array(items)) => items.clone(),
        Some(other) => vec![other.clone()],
        None => match body {
            Value::Array(items) => items,
            other => vec![other],
        },
    };

    let messages = values
        .into_iter()
        .map(|value| PollMessageEnvelope {
            next_offset: extract_string(&value, &["next_offset", "nextOffset", "offset", "cursor"]),
            value,
        })
        .collect();

    PollBatch {
        messages,
        next_offset,
    }
}

fn convert_clawbot_message(bot_id: &str, value: &Value) -> Option<GatewayMessage> {
    let text = extract_string(value, &["text", "content", "message", "msg", "body"])?;
    let chat_id = extract_string(
        value,
        &[
            "chat_id",
            "conversation_id",
            "room_id",
            "group_id",
            "from_group",
            "from",
            "sender",
            "wxid",
        ],
    )?;
    let user_id = extract_string(
        value,
        &[
            "user_id",
            "sender_id",
            "from_user",
            "from",
            "sender",
            "wxid",
        ],
    )
    .unwrap_or_else(|| chat_id.clone());
    let media = extract_string(value, &["media", "media_id", "file_id", "image", "url"]);

    Some(GatewayMessage {
        platform: "clawbot".to_string(),
        bot_id: bot_id.to_string(),
        chat_id,
        user_id,
        text,
        media,
    })
}

fn extract_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(found) = value.get(*key) {
            match found {
                Value::String(s) if !s.is_empty() => return Some(s.clone()),
                Value::Number(n) => return Some(n.to_string()),
                Value::Bool(b) => return Some(b.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn extract_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(found) = value.get(*key) {
            if let Some(n) = found.as_i64() {
                return Some(n);
            }
            if let Some(s) = found.as_str()
                && let Ok(n) = s.parse::<i64>()
            {
                return Some(n);
            }
        }
    }
    None
}

fn join_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

async fn ensure_success(resp: reqwest::Response, label: &str) -> Result<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("{label} failed: status={status}, body={text}")
    }
}

async fn ensure_success_json(resp: reqwest::Response, label: &str) -> Result<Value> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("{label} failed: status={status}, body={text}");
    }
    if text.trim().is_empty() {
        Ok(Value::Null)
    } else {
        serde_json::from_str(&text).with_context(|| format!("failed to parse {label} JSON"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    #[test]
    fn default_config_is_local_bridge() {
        let cfg = ClawBotAdapterConfig::default();
        assert_eq!(cfg.bot_id, "clawbot");
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
        assert_eq!(cfg.poll_path, DEFAULT_POLL_PATH);
        assert_eq!(cfg.send_path, DEFAULT_SEND_PATH);
    }

    #[test]
    fn adapter_name_is_clawbot() {
        let adapter = ClawBotAdapter::new(ClawBotAdapterConfig::default());
        assert_eq!(adapter.name(), "clawbot");
        assert_eq!(adapter.bot_id(), "clawbot");
    }

    #[test]
    fn joins_base_url_and_path() {
        assert_eq!(join_url("http://x/", "/send"), "http://x/send");
        assert_eq!(join_url("http://x", "send"), "http://x/send");
    }

    #[test]
    fn parses_common_poll_shapes() {
        let body = serde_json::json!({
            "next_offset": "42",
            "messages": [
                {"chat_id": "room1", "user_id": "u1", "text": "你好", "id": 7},
                {"conversation_id": "room2", "sender": "u2", "content": "hi"}
            ]
        });
        let batch = parse_poll_batch(body);
        assert_eq!(batch.next_offset.as_deref(), Some("42"));
        assert_eq!(batch.messages.len(), 2);
        let first = convert_clawbot_message("bot", &batch.messages[0].value).unwrap();
        assert_eq!(first.platform, "clawbot");
        assert_eq!(first.bot_id, "bot");
        assert_eq!(first.chat_id, "room1");
        assert_eq!(first.user_id, "u1");
        assert_eq!(first.text, "你好");
    }

    #[test]
    fn converts_clawbot_alias_fields() {
        let value = serde_json::json!({
            "room_id": "group@chatroom",
            "from_user": "wxid_abc",
            "message": "ping",
            "media_id": "file-1"
        });
        let msg = convert_clawbot_message("wx", &value).unwrap();
        assert_eq!(msg.chat_id, "group@chatroom");
        assert_eq!(msg.user_id, "wxid_abc");
        assert_eq!(msg.text, "ping");
        assert_eq!(msg.media.as_deref(), Some("file-1"));
    }
}
