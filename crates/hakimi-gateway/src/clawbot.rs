//! WeChat ClawBot gateway adapter.
//!
//! Supports three modes:
//! - `http_bridge`: backward-compatible generic bridge from v0.3.63.
//! - `weclawbot_api`: Cp0204/WeClawBot-API outbound send API.
//! - `ilink_native`: official WeChat ClawBot/iLink HTTP protocol with QR
//!   login, getupdates long polling, and sendmessage replies with context_token.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::{GatewayMessage, PlatformAdapter};

const HTTP_BRIDGE_BASE_URL: &str = "http://127.0.0.1:5700";
const ILINK_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const DEFAULT_POLL_PATH: &str = "/messages";
const DEFAULT_SEND_PATH: &str = "/send_message";
const DEFAULT_EDIT_PATH: &str = "/edit_message";
const DEFAULT_POLL_INTERVAL_MS: u64 = 1_000;
const DEFAULT_POLL_LIMIT: usize = 50;
const DEFAULT_CHANNEL_VERSION: &str = "1.0.2";
const DEFAULT_APP_CLIENT_VERSION: &str = "2.4.3";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClawBotMode {
    #[default]
    HttpBridge,
    WeClawBotApi,
    IlinkNative,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawBotAdapterConfig {
    /// Gateway platform name exposed to routing, usually `clawbot`.
    #[serde(default = "default_clawbot_platform_name")]
    pub platform_name: String,
    /// Adapter mode: http_bridge | weclawbot_api | ilink_native.
    #[serde(default)]
    pub mode: ClawBotMode,
    /// Bot / role identifier for this instance.
    #[serde(default = "default_clawbot_bot_id")]
    pub bot_id: String,
    /// Base URL. For ilink_native this defaults to https://ilinkai.weixin.qq.com.
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// Optional bearer token. For ilink_native this can seed an existing bot_token.
    #[serde(default)]
    pub token: String,
    /// Generic bridge polling endpoint path.
    #[serde(default = "default_poll_path")]
    pub poll_path: String,
    /// Generic bridge send endpoint path.
    #[serde(default = "default_send_path")]
    pub send_path: String,
    /// Generic bridge edit endpoint path.
    #[serde(default = "default_edit_path")]
    pub edit_path: String,
    /// Polling interval in milliseconds for http_bridge retry loops.
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Maximum messages requested per generic bridge poll.
    #[serde(default = "default_poll_limit")]
    pub poll_limit: usize,
    /// iLink token/cursor/context store directory.
    #[serde(default = "default_token_store")]
    pub token_store: String,
    /// iLink channel_version in base_info.
    #[serde(default = "default_channel_version")]
    pub channel_version: String,
    /// iLink client version header.
    #[serde(default = "default_app_client_version")]
    pub app_client_version: String,
    /// Optional platform that receives iLink login QR notifications.
    #[serde(default)]
    pub login_notify_platform: String,
    /// Optional bot id for login QR notifications.
    #[serde(default)]
    pub login_notify_bot_id: String,
    /// Optional chat id for login QR notifications.
    #[serde(default)]
    pub login_notify_chat_id: String,
    /// Allowed inbound sender IDs. Gateway ingress policy enforces this list.
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

fn default_clawbot_bot_id() -> String {
    "clawbot".to_string()
}

fn default_clawbot_platform_name() -> String {
    "clawbot".to_string()
}

fn default_base_url() -> String {
    HTTP_BRIDGE_BASE_URL.to_string()
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

fn default_token_store() -> String {
    "~/.hakimi/clawbot".to_string()
}

fn default_channel_version() -> String {
    DEFAULT_CHANNEL_VERSION.to_string()
}

fn default_app_client_version() -> String {
    DEFAULT_APP_CLIENT_VERSION.to_string()
}

impl Default for ClawBotAdapterConfig {
    fn default() -> Self {
        Self {
            platform_name: default_clawbot_platform_name(),
            mode: ClawBotMode::HttpBridge,
            bot_id: default_clawbot_bot_id(),
            base_url: default_base_url(),
            token: String::new(),
            poll_path: default_poll_path(),
            send_path: default_send_path(),
            edit_path: default_edit_path(),
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            poll_limit: DEFAULT_POLL_LIMIT,
            token_store: default_token_store(),
            channel_version: default_channel_version(),
            app_client_version: default_app_client_version(),
            login_notify_platform: String::new(),
            login_notify_bot_id: String::new(),
            login_notify_chat_id: String::new(),
            allowed_users: Vec::new(),
        }
    }
}

pub struct ClawBotAdapter {
    config: ClawBotAdapterConfig,
    platform_name: String,
    client: Client,
    msg_tx: mpsc::UnboundedSender<GatewayMessage>,
    msg_rx: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    poll_handle: Option<JoinHandle<()>>,
    ilink_state: Arc<Mutex<IlinkStoredState>>,
}

impl ClawBotAdapter {
    pub fn new(mut config: ClawBotAdapterConfig) -> Self {
        if matches!(config.mode, ClawBotMode::IlinkNative)
            && config.base_url == HTTP_BRIDGE_BASE_URL
        {
            config.base_url = ILINK_BASE_URL.to_string();
        }
        let platform_name = normalize_platform_name(&config.platform_name);
        let state = load_ilink_state(&config).unwrap_or_else(|err| {
            warn!(error = %err, "failed to load iLink state; starting fresh");
            IlinkStoredState::default()
        });
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        Self {
            config,
            platform_name,
            client: Client::builder()
                .timeout(Duration::from_secs(45))
                .build()
                .unwrap_or_else(|_| Client::new()),
            msg_tx,
            msg_rx: Some(msg_rx),
            poll_handle: None,
            ilink_state: Arc::new(Mutex::new(state)),
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

    fn spawn_http_bridge_poll_loop(&self) -> JoinHandle<()> {
        let client = self.client.clone();
        let config = self.config.clone();
        let msg_tx = self.msg_tx.clone();
        tokio::spawn(async move {
            let mut offset: Option<String> = None;
            loop {
                match poll_http_bridge_once(&client, &config, offset.as_deref()).await {
                    Ok(batch) => {
                        for envelope in batch.messages {
                            if let Some(next_offset) = envelope.next_offset.clone() {
                                offset = Some(next_offset);
                            }
                            if let Some(msg) = convert_bridge_message(
                                &config.platform_name,
                                &config.bot_id,
                                &envelope.value,
                            ) && msg_tx.send(msg).is_err()
                            {
                                error!("ClawBot receiver dropped; stopping poll loop");
                                return;
                            }
                        }
                        if let Some(next_offset) = batch.next_offset {
                            offset = Some(next_offset);
                        }
                    }
                    Err(err) => warn!(error = %err, "ClawBot bridge poll failed, retrying"),
                }
                tokio::time::sleep(Duration::from_millis(config.poll_interval_ms)).await;
            }
        })
    }

    async fn ensure_ilink_login(
        client: &Client,
        config: &ClawBotAdapterConfig,
        ilink_state: &Arc<Mutex<IlinkStoredState>>,
        msg_tx: &mpsc::UnboundedSender<GatewayMessage>,
    ) -> Result<()> {
        if !config.token.trim().is_empty() {
            let mut state = ilink_state
                .lock()
                .map_err(|_| anyhow::anyhow!("iLink state lock poisoned"))?;
            if state.bot_token.is_empty() {
                state.bot_token = config.token.clone();
            }
            if state.base_url.is_empty() {
                state.base_url = config.base_url.clone();
            }
            save_ilink_state(config, &state)?;
            return Ok(());
        }
        {
            let state = ilink_state
                .lock()
                .map_err(|_| anyhow::anyhow!("iLink state lock poisoned"))?;
            if !state.bot_token.is_empty() {
                return Ok(());
            }
        }

        let qr = ilink_get_qrcode(client, config).await?;
        info!(
            qrcode_url = %qr.qrcode_img_content,
            "WeChat ClawBot login required: scan this QR URL with WeChat"
        );
        println!(
            "\n=== WeChat ClawBot / iLink login ===\nScan with WeChat: {}\n",
            qr.qrcode_img_content
        );
        notify_ilink_login_qr(msg_tx, config, &qr.qrcode_img_content);

        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let status = ilink_get_qrcode_status(client, config, &qr.qrcode).await?;
            if let Some(token) = status.bot_token {
                let mut state = ilink_state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("iLink state lock poisoned"))?;
                state.bot_token = token;
                state.base_url = status.baseurl.unwrap_or_else(|| config.base_url.clone());
                save_ilink_state(config, &state)?;
                notify_ilink_login_complete(msg_tx, config);
                info!("WeChat ClawBot iLink login completed");
                return Ok(());
            }
        }
    }
}

#[async_trait]
impl PlatformAdapter for ClawBotAdapter {
    fn name(&self) -> &str {
        &self.platform_name
    }

    fn bot_id(&self) -> &str {
        &self.config.bot_id
    }

    async fn connect(&mut self) -> Result<()> {
        info!(mode = ?self.config.mode, base_url = %self.config.base_url, "connecting ClawBot adapter");
        let handle = match self.config.mode {
            ClawBotMode::HttpBridge => self.spawn_http_bridge_poll_loop(),
            ClawBotMode::WeClawBotApi => {
                info!("WeClawBot-API mode is outbound-only; no inbound receiver is started");
                tokio::spawn(async { std::future::pending::<()>().await })
            }
            ClawBotMode::IlinkNative => {
                let client = self.client.clone();
                let config = self.config.clone();
                let state = self.ilink_state.clone();
                let msg_tx = self.msg_tx.clone();
                tokio::spawn(async move {
                    loop {
                        match Self::ensure_ilink_login(&client, &config, &state, &msg_tx).await {
                            Ok(()) => break,
                            Err(err) => {
                                warn!(error = %err, "iLink login failed, retrying in background");
                                tokio::time::sleep(Duration::from_secs(15)).await;
                            }
                        }
                    }
                    loop {
                        match ilink_poll_once(&client, &config, &state).await {
                            Ok(messages) => {
                                for msg in messages {
                                    if msg_tx.send(msg).is_err() {
                                        error!(
                                            "ClawBot receiver dropped; stopping iLink poll loop"
                                        );
                                        return;
                                    }
                                }
                            }
                            Err(err) => warn!(error = %err, "iLink getupdates failed, retrying"),
                        }
                        tokio::time::sleep(Duration::from_millis(config.poll_interval_ms)).await;
                    }
                })
            }
        };
        self.poll_handle = Some(handle);
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<()> {
        match self.config.mode {
            ClawBotMode::HttpBridge => self.send_http_bridge_message(chat_id, text).await,
            ClawBotMode::WeClawBotApi => self.send_weclawbot_api_message(text).await,
            ClawBotMode::IlinkNative => self.send_ilink_message(chat_id, text).await,
        }
    }

    async fn send_message_get_id(&self, chat_id: &str, text: &str) -> Result<Option<i64>> {
        match self.config.mode {
            ClawBotMode::HttpBridge => self.send_http_bridge_message_get_id(chat_id, text).await,
            _ => {
                self.send_message(chat_id, text).await?;
                Ok(None)
            }
        }
    }

    async fn edit_message(&self, chat_id: &str, message_id: i64, text: &str) -> Result<()> {
        match self.config.mode {
            ClawBotMode::HttpBridge => {
                self.edit_http_bridge_message(chat_id, message_id, text)
                    .await
            }
            // WeChat does not support editing normal messages; treat progressive edit as no-op.
            _ => Ok(()),
        }
    }

    async fn send_chat_action(&self, chat_id: &str, action: &str) -> Result<()> {
        match self.config.mode {
            ClawBotMode::WeClawBotApi => {
                let status = if action == "typing" { 1 } else { 2 };
                self.send_weclawbot_typing(status).await
            }
            ClawBotMode::IlinkNative => {
                let status = if action == "typing" { 1 } else { 2 };
                self.send_ilink_typing(chat_id, status).await
            }
            ClawBotMode::HttpBridge => Ok(()),
        }
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

impl ClawBotAdapter {
    async fn send_http_bridge_message(&self, chat_id: &str, text: &str) -> Result<()> {
        let payload = bridge_send_payload(chat_id, text);
        let resp = self
            .auth_request(self.client.post(self.endpoint(&self.config.send_path)))
            .json(&payload)
            .send()
            .await
            .context("failed to send ClawBot bridge message")?;
        ensure_success(resp, "ClawBot bridge send_message").await
    }

    async fn send_http_bridge_message_get_id(
        &self,
        chat_id: &str,
        text: &str,
    ) -> Result<Option<i64>> {
        let payload = bridge_send_payload(chat_id, text);
        let resp = self
            .auth_request(self.client.post(self.endpoint(&self.config.send_path)))
            .json(&payload)
            .send()
            .await
            .context("failed to send ClawBot bridge message")?;
        let body = ensure_success_json(resp, "ClawBot bridge send_message").await?;
        Ok(extract_i64(&body, &["message_id", "msg_id", "id"]))
    }

    async fn edit_http_bridge_message(
        &self,
        chat_id: &str,
        message_id: i64,
        text: &str,
    ) -> Result<()> {
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
            .context("failed to edit ClawBot bridge message")?;
        ensure_success(resp, "ClawBot bridge edit_message").await
    }

    async fn send_weclawbot_api_message(&self, text: &str) -> Result<()> {
        let path = format!("/bots/{}/messages", self.config.bot_id);
        let payload = serde_json::json!({ "text": text });
        let resp = self
            .auth_request(self.client.post(join_url(&self.config.base_url, &path)))
            .json(&payload)
            .send()
            .await
            .context("failed to send WeClawBot-API message")?;
        ensure_success(resp, "WeClawBot-API messages").await
    }

    async fn send_weclawbot_typing(&self, status: i32) -> Result<()> {
        let path = format!("/bots/{}/typing", self.config.bot_id);
        let payload = serde_json::json!({ "status": status });
        let resp = self
            .auth_request(self.client.post(join_url(&self.config.base_url, &path)))
            .json(&payload)
            .send()
            .await
            .context("failed to send WeClawBot-API typing")?;
        ensure_success(resp, "WeClawBot-API typing").await
    }

    async fn send_ilink_message(&self, chat_id: &str, text: &str) -> Result<()> {
        let (base_url, token, context_token) = {
            let state = self
                .ilink_state
                .lock()
                .map_err(|_| anyhow::anyhow!("iLink state lock poisoned"))?;
            let context_token = state
                .context_tokens
                .get(chat_id)
                .cloned()
                .with_context(|| {
                    format!(
                        "missing iLink context_token for chat {chat_id}; user must message first"
                    )
                })?;
            (
                state.base_url_or(&self.config.base_url),
                state.bot_token.clone(),
                context_token,
            )
        };
        let payload = build_ilink_sendmessage_payload(
            chat_id,
            text,
            &context_token,
            &self.config.channel_version,
        );
        let resp = ilink_auth_headers(
            self.client
                .post(join_url(&base_url, "/ilink/bot/sendmessage")),
            &self.config,
            Some(&token),
        )
        .json(&payload)
        .send()
        .await
        .context("failed to send iLink message")?;
        ensure_success(resp, "iLink sendmessage").await
    }

    async fn send_ilink_typing(&self, chat_id: &str, status: i32) -> Result<()> {
        let (base_url, token, typing_ticket) = {
            let state = self
                .ilink_state
                .lock()
                .map_err(|_| anyhow::anyhow!("iLink state lock poisoned"))?;
            (
                state.base_url_or(&self.config.base_url),
                state.bot_token.clone(),
                state.typing_tickets.get(chat_id).cloned(),
            )
        };
        let ticket = match typing_ticket {
            Some(ticket) => ticket,
            None => return Ok(()),
        };
        let payload = serde_json::json!({
            "to_user_id": chat_id,
            "typing_ticket": ticket,
            "status": status,
            "base_info": base_info(&self.config.channel_version),
        });
        let resp = ilink_auth_headers(
            self.client
                .post(join_url(&base_url, "/ilink/bot/sendtyping")),
            &self.config,
            Some(&token),
        )
        .json(&payload)
        .send()
        .await
        .context("failed to send iLink typing")?;
        ensure_success(resp, "iLink sendtyping").await
    }
}

#[derive(Debug, Default)]
struct PollBatch {
    messages: Vec<PollMessageEnvelope>,
    next_offset: Option<String>,
}

#[derive(Debug)]
struct PollMessageEnvelope {
    value: Value,
    next_offset: Option<String>,
}

async fn poll_http_bridge_once(
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
        .context("failed to poll ClawBot bridge messages")?;
    let body = ensure_success_json(resp, "ClawBot bridge poll").await?;
    Ok(parse_bridge_poll_batch(body))
}

fn notify_ilink_login_qr(
    msg_tx: &mpsc::UnboundedSender<GatewayMessage>,
    config: &ClawBotAdapterConfig,
    qrcode_img_url: &str,
) {
    if config.login_notify_chat_id.trim().is_empty() {
        return;
    }
    let platform = if config.login_notify_platform.trim().is_empty() {
        "telegram"
    } else {
        config.login_notify_platform.trim()
    };
    let bot_id = if config.login_notify_bot_id.trim().is_empty() {
        "telegram_bot"
    } else {
        config.login_notify_bot_id.trim()
    };
    let msg = GatewayMessage {
        platform: "__hakimi_system__".to_string(),
        bot_id: bot_id.to_string(),
        chat_id: config.login_notify_chat_id.clone(),
        user_id: "clawbot-login".to_string(),
        text: format!(
            "请用微信扫描二维码登录 ClawBot。登录完成后会自动保存状态；Telegram gateway 不会被阻塞。\n\nHAKIMI_ROUTE_PLATFORM={platform}"
        ),
        media: Some(qrcode_img_url.to_string()),
        callback_data: None,
                reply_to_message_id: None,
                reply_to_text: None,
            };
    let _ = msg_tx.send(msg);
}

fn notify_ilink_login_complete(
    msg_tx: &mpsc::UnboundedSender<GatewayMessage>,
    config: &ClawBotAdapterConfig,
) {
    if config.login_notify_chat_id.trim().is_empty() {
        return;
    }
    let platform = if config.login_notify_platform.trim().is_empty() {
        "telegram"
    } else {
        config.login_notify_platform.trim()
    };
    let bot_id = if config.login_notify_bot_id.trim().is_empty() {
        "telegram_bot"
    } else {
        config.login_notify_bot_id.trim()
    };
    let msg = GatewayMessage {
        platform: "__hakimi_system__".to_string(),
        bot_id: bot_id.to_string(),
        chat_id: config.login_notify_chat_id.clone(),
        user_id: "clawbot-login".to_string(),
        text: format!(
            "✅ ClawBot 微信登录完成，登录态已保存。\n\nHAKIMI_ROUTE_PLATFORM={platform}"
        ),
        media: None,
        callback_data: None,
                reply_to_message_id: None,
                reply_to_text: None,
            };
    let _ = msg_tx.send(msg);
}

fn parse_bridge_poll_batch(body: Value) -> PollBatch {
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

fn convert_bridge_message(
    platform_name: &str,
    bot_id: &str,
    value: &Value,
) -> Option<GatewayMessage> {
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
        platform: normalize_platform_name(platform_name),
        bot_id: bot_id.to_string(),
        chat_id,
        user_id,
        text,
        media,
        callback_data: None,
                reply_to_message_id: None,
                reply_to_text: None,
            })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IlinkStoredState {
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    get_updates_buf: String,
    #[serde(default)]
    context_tokens: HashMap<String, String>,
    #[serde(default)]
    typing_tickets: HashMap<String, String>,
}

impl IlinkStoredState {
    fn base_url_or(&self, fallback: &str) -> String {
        if self.base_url.is_empty() {
            fallback.to_string()
        } else {
            self.base_url.clone()
        }
    }
}

#[derive(Debug)]
struct IlinkQrCode {
    qrcode: String,
    qrcode_img_content: String,
}

#[derive(Debug, Deserialize)]
struct IlinkQrStatus {
    #[serde(default)]
    bot_token: Option<String>,
    #[serde(default)]
    baseurl: Option<String>,
}

async fn ilink_get_qrcode(client: &Client, config: &ClawBotAdapterConfig) -> Result<IlinkQrCode> {
    let resp = ilink_auth_headers(
        client
            .get(join_url(&config.base_url, "/ilink/bot/get_bot_qrcode"))
            .query(&[("bot_type", "3")]),
        config,
        None,
    )
    .send()
    .await
    .context("failed to request iLink QR code")?;
    let body = ensure_success_json(resp, "iLink get_bot_qrcode").await?;
    parse_ilink_qrcode(&body)
}

fn parse_ilink_qrcode(body: &Value) -> Result<IlinkQrCode> {
    let qrcode = extract_string(body, &["qrcode", "qr_code", "qrcode_key"])
        .context("iLink QR response missing qrcode")?;
    let qrcode_img_content = extract_string(body, &["qrcode_img_content", "qrcode_url", "url"])
        .context("iLink QR response missing qrcode_img_content")?;
    Ok(IlinkQrCode {
        qrcode,
        qrcode_img_content,
    })
}

async fn ilink_get_qrcode_status(
    client: &Client,
    config: &ClawBotAdapterConfig,
    qrcode: &str,
) -> Result<IlinkQrStatus> {
    let resp = ilink_auth_headers(
        client
            .get(join_url(&config.base_url, "/ilink/bot/get_qrcode_status"))
            .query(&[("qrcode", qrcode)]),
        config,
        None,
    )
    .send()
    .await
    .context("failed to request iLink QR status")?;
    let body = ensure_success_json(resp, "iLink get_qrcode_status").await?;
    serde_json::from_value(body).context("failed to parse iLink QR status")
}

async fn ilink_poll_once(
    client: &Client,
    config: &ClawBotAdapterConfig,
    state: &Arc<Mutex<IlinkStoredState>>,
) -> Result<Vec<GatewayMessage>> {
    let (base_url, token, cursor) = {
        let guard = state
            .lock()
            .map_err(|_| anyhow::anyhow!("iLink state lock poisoned"))?;
        (
            guard.base_url_or(&config.base_url),
            guard.bot_token.clone(),
            guard.get_updates_buf.clone(),
        )
    };
    if token.is_empty() {
        anyhow::bail!("iLink bot_token is empty; login required");
    }
    let payload = serde_json::json!({
        "get_updates_buf": cursor,
        "base_info": base_info(&config.channel_version),
    });
    let resp = ilink_auth_headers(
        client.post(join_url(&base_url, "/ilink/bot/getupdates")),
        config,
        Some(&token),
    )
    .json(&payload)
    .send()
    .await
    .context("failed to poll iLink getupdates")?;
    let body = ensure_success_json(resp, "iLink getupdates").await?;
    let (next_cursor, messages, context_updates, typing_updates) =
        parse_ilink_updates(&config.platform_name, &config.bot_id, &body);
    {
        let mut guard = state
            .lock()
            .map_err(|_| anyhow::anyhow!("iLink state lock poisoned"))?;
        if let Some(next) = next_cursor {
            guard.get_updates_buf = next;
        }
        for (chat_id, context_token) in context_updates {
            guard.context_tokens.insert(chat_id, context_token);
        }
        for (chat_id, typing_ticket) in typing_updates {
            guard.typing_tickets.insert(chat_id, typing_ticket);
        }
        save_ilink_state(config, &guard)?;
    }
    Ok(messages)
}

type IlinkParseResult = (
    Option<String>,
    Vec<GatewayMessage>,
    Vec<(String, String)>,
    Vec<(String, String)>,
);

fn parse_ilink_updates(platform_name: &str, bot_id: &str, body: &Value) -> IlinkParseResult {
    let next_cursor = extract_string(body, &["get_updates_buf", "next_buf", "cursor"]);
    let msgs = body.get("msgs").or_else(|| body.get("messages"));
    let values = match msgs {
        Some(Value::Array(items)) => items.clone(),
        Some(other) => vec![other.clone()],
        None => Vec::new(),
    };
    let mut out = Vec::new();
    let mut contexts = Vec::new();
    let mut typing_tickets = Vec::new();
    for msg in values {
        if extract_i64(&msg, &["message_type"]) == Some(2) {
            continue;
        }
        let Some(chat_id) = extract_string(&msg, &["from_user_id", "from", "sender", "user_id"])
        else {
            continue;
        };
        let text = extract_ilink_text(&msg);
        if text.trim().is_empty() {
            continue;
        }
        if let Some(context_token) = extract_string(&msg, &["context_token", "contextToken"]) {
            contexts.push((chat_id.clone(), context_token));
        }
        if let Some(typing_ticket) = extract_string(&msg, &["typing_ticket", "typingTicket"]) {
            typing_tickets.push((chat_id.clone(), typing_ticket));
        }
        out.push(GatewayMessage {
            platform: normalize_platform_name(platform_name),
            bot_id: bot_id.to_string(),
            chat_id: chat_id.clone(),
            user_id: chat_id,
            text,
            media: None,
            callback_data: None,
                reply_to_message_id: None,
                reply_to_text: None,
            });
    }
    (next_cursor, out, contexts, typing_tickets)
}

fn extract_ilink_text(msg: &Value) -> String {
    let Some(Value::Array(items)) = msg.get("item_list") else {
        return extract_string(msg, &["text", "content", "message"]).unwrap_or_default();
    };
    let mut text = String::new();
    for item in items {
        let is_text = extract_i64(item, &["type"]) == Some(1);
        if is_text
            && let Some(value) = item
                .get("text_item")
                .and_then(|v| extract_string(v, &["text", "content"]))
        {
            text.push_str(&value);
        }
    }
    text
}

fn build_ilink_sendmessage_payload(
    chat_id: &str,
    text: &str,
    context_token: &str,
    channel_version: &str,
) -> Value {
    serde_json::json!({
        "msg": {
            "from_user_id": "",
            "to_user_id": chat_id,
            "client_id": format!("hakimi-{}", uuid::Uuid::new_v4().simple()),
            "message_type": 2,
            "message_state": 2,
            "context_token": context_token,
            "item_list": [
                { "type": 1, "text_item": { "text": text } }
            ]
        },
        "base_info": base_info(channel_version),
    })
}

fn base_info(channel_version: &str) -> Value {
    serde_json::json!({ "channel_version": channel_version })
}

fn normalize_platform_name(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "clawbot".to_string()
    } else {
        value.to_ascii_lowercase()
    }
}

fn ilink_auth_headers(
    builder: reqwest::RequestBuilder,
    config: &ClawBotAdapterConfig,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut builder = builder
        .header("Content-Type", "application/json")
        .header("AuthorizationType", "ilink_bot_token")
        .header("X-WECHAT-UIN", random_wechat_uin())
        .header("iLink-App-Id", "bot")
        .header("iLink-App-ClientVersion", &config.app_client_version);
    if let Some(token) = token
        && !token.is_empty()
    {
        builder = builder.bearer_auth(token);
    }
    builder
}

fn random_wechat_uin() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0);
    base64::engine::general_purpose::STANDARD.encode(nanos.to_be_bytes())
}

fn bridge_send_payload(chat_id: &str, text: &str) -> Value {
    serde_json::json!({
        "chat_id": chat_id,
        "conversation_id": chat_id,
        "to": chat_id,
        "text": text,
        "content": text,
        "msgtype": "text",
    })
}

fn state_path(config: &ClawBotAdapterConfig) -> PathBuf {
    expand_home(&config.token_store).join(format!("{}.json", sanitize_filename(&config.bot_id)))
}

fn load_ilink_state(config: &ClawBotAdapterConfig) -> Result<IlinkStoredState> {
    let path = state_path(config);
    if !path.exists() {
        return Ok(IlinkStoredState::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read iLink state from {}", path.display()))?;
    serde_json::from_str(&text).context("failed to parse iLink state")
}

fn save_ilink_state(config: &ClawBotAdapterConfig, state: &IlinkStoredState) -> Result<()> {
    let path = state_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create iLink state dir {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(state).context("failed to serialize iLink state")?;
    std::fs::write(&path, text)
        .with_context(|| format!("failed to write iLink state to {}", path.display()))
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    Path::new(path).to_path_buf()
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
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
        assert_eq!(cfg.platform_name, "clawbot");
        assert_eq!(cfg.mode, ClawBotMode::HttpBridge);
        assert_eq!(cfg.bot_id, "clawbot");
        assert_eq!(cfg.base_url, HTTP_BRIDGE_BASE_URL);
        assert_eq!(cfg.poll_path, DEFAULT_POLL_PATH);
        assert_eq!(cfg.send_path, DEFAULT_SEND_PATH);
    }

    #[test]
    fn ilink_native_rewrites_default_base_url() {
        let cfg = ClawBotAdapterConfig {
            mode: ClawBotMode::IlinkNative,
            ..Default::default()
        };
        let adapter = ClawBotAdapter::new(cfg);
        assert_eq!(adapter.config.base_url, ILINK_BASE_URL);
    }

    #[test]
    fn adapter_name_is_clawbot() {
        let adapter = ClawBotAdapter::new(ClawBotAdapterConfig::default());
        assert_eq!(adapter.name(), "clawbot");
        assert_eq!(adapter.bot_id(), "clawbot");
    }

    #[test]
    fn adapter_name_can_be_weixin_alias() {
        let adapter = ClawBotAdapter::new(ClawBotAdapterConfig {
            platform_name: "weixin".to_string(),
            bot_id: "weixin-main".to_string(),
            mode: ClawBotMode::IlinkNative,
            ..Default::default()
        });
        assert_eq!(adapter.name(), "weixin");
        assert_eq!(adapter.bot_id(), "weixin-main");
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
        let batch = parse_bridge_poll_batch(body);
        assert_eq!(batch.next_offset.as_deref(), Some("42"));
        assert_eq!(batch.messages.len(), 2);
        let first = convert_bridge_message("clawbot", "bot", &batch.messages[0].value).unwrap();
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
        let msg = convert_bridge_message("clawbot", "wx", &value).unwrap();
        assert_eq!(msg.chat_id, "group@chatroom");
        assert_eq!(msg.user_id, "wxid_abc");
        assert_eq!(msg.text, "ping");
        assert_eq!(msg.media.as_deref(), Some("file-1"));
    }

    #[test]
    fn converts_bridge_messages_with_weixin_platform_alias() {
        let value = serde_json::json!({
            "chat_id": "wxid_home",
            "user_id": "wxid_sender",
            "text": "ping"
        });
        let msg = convert_bridge_message("weixin", "weixin-main", &value).unwrap();
        assert_eq!(msg.platform, "weixin");
        assert_eq!(msg.bot_id, "weixin-main");
        assert_eq!(msg.chat_id, "wxid_home");
        assert_eq!(msg.user_id, "wxid_sender");
    }

    #[test]
    fn parses_ilink_qrcode_response() {
        let body = serde_json::json!({
            "qrcode": "qr-key",
            "qrcode_img_content": "https://example.com/qr"
        });
        let qr = parse_ilink_qrcode(&body).unwrap();
        assert_eq!(qr.qrcode, "qr-key");
        assert_eq!(qr.qrcode_img_content, "https://example.com/qr");
    }

    #[test]
    fn parses_ilink_updates_and_context_tokens() {
        let body = serde_json::json!({
            "get_updates_buf": "next-cursor",
            "msgs": [
                {
                    "from_user_id": "user@im.wechat",
                    "context_token": "ctx-1",
                    "typing_ticket": "typing-1",
                    "message_type": 1,
                    "item_list": [
                        {"type": 1, "text_item": {"text": "你"}},
                        {"type": 1, "text_item": {"text": "好"}}
                    ]
                },
                {
                    "from_user_id": "self@im.wechat",
                    "context_token": "ctx-self",
                    "message_type": 2,
                    "item_list": [{"type": 1, "text_item": {"text": "skip"}}]
                }
            ]
        });
        let (cursor, messages, contexts, typing_tickets) =
            parse_ilink_updates("clawbot", "bot", &body);
        assert_eq!(cursor.as_deref(), Some("next-cursor"));
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].chat_id, "user@im.wechat");
        assert_eq!(messages[0].text, "你好");
        assert_eq!(
            contexts,
            vec![("user@im.wechat".to_string(), "ctx-1".to_string())]
        );
        assert_eq!(
            typing_tickets,
            vec![("user@im.wechat".to_string(), "typing-1".to_string())]
        );
    }

    #[test]
    fn parses_ilink_updates_with_weixin_platform_alias() {
        let body = serde_json::json!({
            "msgs": [{
                "from_user_id": "wxid_abc",
                "context_token": "ctx",
                "message_type": 1,
                "item_list": [{"type": 1, "text_item": {"text": "ping"}}]
            }]
        });
        let (_, messages, contexts, _) = parse_ilink_updates("weixin", "weixin-main", &body);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].platform, "weixin");
        assert_eq!(messages[0].bot_id, "weixin-main");
        assert_eq!(messages[0].chat_id, "wxid_abc");
        assert_eq!(contexts, vec![("wxid_abc".to_string(), "ctx".to_string())]);
    }

    #[test]
    fn builds_ilink_sendmessage_payload_with_context_token() {
        let payload = build_ilink_sendmessage_payload("user@im.wechat", "回复", "ctx", "1.0.2");
        assert_eq!(payload["msg"]["to_user_id"], "user@im.wechat");
        assert_eq!(payload["msg"]["message_type"], 2);
        assert_eq!(payload["msg"]["message_state"], 2);
        assert_eq!(payload["msg"]["context_token"], "ctx");
        assert_eq!(payload["msg"]["item_list"][0]["text_item"]["text"], "回复");
        assert_eq!(payload["base_info"]["channel_version"], "1.0.2");
    }

    #[test]
    fn state_path_sanitizes_bot_id() {
        let cfg = ClawBotAdapterConfig {
            bot_id: "abc@im.bot".to_string(),
            token_store: "/tmp/hakimi-clawbot-test".to_string(),
            ..Default::default()
        };
        assert_eq!(
            state_path(&cfg),
            PathBuf::from("/tmp/hakimi-clawbot-test/abc_im_bot.json")
        );
    }
}
