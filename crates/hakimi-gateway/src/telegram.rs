//! Telegram Bot API gateway adapter.
//!
//! Uses long-polling (`getUpdates`) to receive messages and the Bot API's
//! `sendMessage` / `sendPhoto` endpoints to deliver outbound content.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::path::Path;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::{GatewayMessage, PlatformAdapter};

// ---------------------------------------------------------------------------
// Telegram API types
// ---------------------------------------------------------------------------

/// Top-level response from the Telegram Bot API.
#[derive(Debug, Deserialize)]
struct TgResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

/// A single Telegram update (from `getUpdates`).
#[derive(Debug, Deserialize)]
struct TgUpdate {
    update_id: i64,
    message: Option<TgMessage>,
    callback_query: Option<TgCallbackQuery>,
}

/// A callback query from an inline button.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TgCallbackQuery {
    id: String,
    from: TgUser,
    message: Option<TgMessage>,
    data: Option<String>,
}

/// An incoming Telegram message.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TgMessage {
    message_id: i64,
    from: Option<TgUser>,
    chat: TgChat,
    text: Option<String>,
    photo: Option<Vec<TgPhotoSize>>,
}

/// The sender of a Telegram message.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TgUser {
    id: i64,
    first_name: String,
}

/// The chat a Telegram message belongs to.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TgChat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
}

/// A photo size variant (we receive multiple sizes).
#[derive(Debug, Deserialize)]
struct TgPhotoSize {
    file_id: String,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum Telegram message length (characters).
const MAX_MESSAGE_LENGTH: usize = 4096;

/// Long-polling timeout in seconds.
const POLL_TIMEOUT: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelegramMediaKind {
    Photo,
    Voice,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TelegramBotCommand {
    command: &'static str,
    description: &'static str,
}

fn telegram_bot_commands() -> &'static [TelegramBotCommand] {
    &[
        TelegramBotCommand {
            command: "start",
            description: "Start a conversation",
        },
        TelegramBotCommand {
            command: "help",
            description: "Show the command reference",
        },
        TelegramBotCommand {
            command: "stop",
            description: "Cancel the active task or stream",
        },
        TelegramBotCommand {
            command: "clear",
            description: "Clear this chat's conversation state",
        },
        TelegramBotCommand {
            command: "status",
            description: "Show gateway, platform, and model status",
        },
        TelegramBotCommand {
            command: "usage",
            description: "Show last-turn tokens, cost, and rate limits",
        },
        TelegramBotCommand {
            command: "model",
            description: "Show or switch the active model",
        },
        TelegramBotCommand {
            command: "tools",
            description: "List available tools",
        },
        TelegramBotCommand {
            command: "skills",
            description: "List loaded skills",
        },
        TelegramBotCommand {
            command: "cron",
            description: "Manage scheduled jobs",
        },
        TelegramBotCommand {
            command: "doctor",
            description: "Run setup and runtime diagnostics",
        },
        TelegramBotCommand {
            command: "logs",
            description: "Show recent gateway logs",
        },
        TelegramBotCommand {
            command: "memory",
            description: "View or clear persistent memory",
        },
        TelegramBotCommand {
            command: "checkpoints",
            description: "Manage file checkpoints",
        },
        TelegramBotCommand {
            command: "update",
            description: "Update Hakimi and restart gateway",
        },
        TelegramBotCommand {
            command: "restart",
            description: "Restart the managed gateway service",
        },
        TelegramBotCommand {
            command: "providers",
            description: "List supported LLM providers",
        },
        TelegramBotCommand {
            command: "platforms",
            description: "List connected gateway platforms",
        },
        TelegramBotCommand {
            command: "mcp",
            description: "Manage MCP servers",
        },
        TelegramBotCommand {
            command: "browser",
            description: "Control browser sessions",
        },
        TelegramBotCommand {
            command: "backup",
            description: "Back up Hakimi state",
        },
        TelegramBotCommand {
            command: "dump",
            description: "Export a session database dump",
        },
        TelegramBotCommand {
            command: "goals",
            description: "Manage active goals",
        },
        TelegramBotCommand {
            command: "hooks",
            description: "Inspect configured hooks",
        },
        TelegramBotCommand {
            command: "kanban",
            description: "Open Kanban workflow controls",
        },
        TelegramBotCommand {
            command: "pairing",
            description: "Start gateway pairing",
        },
        TelegramBotCommand {
            command: "tips",
            description: "Show usage tips",
        },
        TelegramBotCommand {
            command: "voice",
            description: "Control voice output",
        },
        TelegramBotCommand {
            command: "webhook",
            description: "Show webhook status",
        },
        TelegramBotCommand {
            command: "gateway",
            description: "Show gateway controls",
        },
        TelegramBotCommand {
            command: "auth",
            description: "Show authentication status",
        },
    ]
}

fn telegram_help_text() -> String {
    let mut help = "🤖 *Hakimi Agent*\n\n\
Simply type a message and I will respond.\n\n\
*Commands:*\n"
        .to_string();

    for command in telegram_bot_commands() {
        help.push_str(&format!("/{} – {}\n", command.command, command.description));
    }

    help
}

// ---------------------------------------------------------------------------
// TelegramAdapter
// ---------------------------------------------------------------------------

/// Configuration for the Telegram adapter.
pub struct TelegramAdapterConfig {
    /// Bot token obtained from BotFather.
    pub token: String,
    /// Bot / role identifier for this instance.
    pub bot_id: String,
    /// Optional base URL override (useful for testing with a local Bot API server).
    /// Defaults to `https://api.telegram.org`.
    pub base_url: Option<String>,
}

/// Telegram Bot API adapter using long-polling.
pub struct TelegramAdapter {
    /// Bot token.
    token: String,
    /// Bot / role identifier.
    bot_id: String,
    /// Base URL for the Bot API (default: `https://api.telegram.org`).
    base_url: String,
    /// Shared HTTP client.
    client: reqwest::Client,
    /// Sender half for pushing received messages upstream.
    msg_tx: mpsc::UnboundedSender<GatewayMessage>,
    /// Receiver half – exposed via [`PlatformAdapter::take_receiver`].
    msg_rx: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    /// Handle to the long-polling background task.
    poll_handle: Option<JoinHandle<()>>,
}

impl TelegramAdapter {
    /// Create a new Telegram adapter from a config.
    pub fn new(config: TelegramAdapterConfig) -> Self {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let base_url = config
            .base_url
            .unwrap_or_else(|| "https://api.telegram.org".to_owned());
        Self {
            token: config.token,
            bot_id: config.bot_id,
            base_url,
            client: reqwest::Client::new(),
            msg_tx,
            msg_rx: Some(msg_rx),
            poll_handle: None,
        }
    }

    /// Convenience constructor – create an adapter with just a token.
    pub fn from_token(bot_id: impl Into<String>, token: impl Into<String>) -> Self {
        Self::new(TelegramAdapterConfig {
            token: token.into(),
            bot_id: bot_id.into(),
            base_url: None,
        })
    }

    /// Full URL for a given Bot API method.
    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.base_url, self.token, method)
    }

    // -----------------------------------------------------------------------
    // Bot commands
    // -----------------------------------------------------------------------

    /// Return the appropriate auto-reply text for a bot command, or `None`
    /// if the text is not a known command.
    fn handle_command(text: &str) -> Option<String> {
        match text.trim() {
            "/start" => Some(
                "👋 Welcome! I'm Hakimi, your AI assistant.\n\n\
                 Send me any message and I'll do my best to help."
                    .to_string(),
            ),
            "/help" => Some(telegram_help_text()),
            "/update" => Some(
                "🔄 *Hakimi Updater*\n\n\
                 Starting update and restart sequence...\n\
                 Please wait a moment while the binary is downloaded and the gateway is restarted."
                    .to_string(),
            ),
            "/shutdown" => Some(
                "🛑 *Hakimi Shutdown*\n\n\
                 Initiating graceful shutdown...\n\
                 All running tasks will be completed before exit."
                    .to_string(),
            ),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // Long-polling loop
    // -----------------------------------------------------------------------

    /// Spawn the long-polling background task.
    fn spawn_poll_loop(&self) -> JoinHandle<()> {
        let client = self.client.clone();
        let api_url = self.api_url("getUpdates");
        let msg_tx = self.msg_tx.clone();
        let bot_id = self.bot_id.clone();

        tokio::spawn(async move {
            let mut offset: i64 = 0;

            loop {
                match poll_once(&client, &api_url, offset).await {
                    Ok(updates) => {
                        for update in updates {
                            let update_id = update.update_id;

                            // Handle regular messages
                            if let Some(message) = update.message
                                && let Some(gw_msg) = convert_message(&bot_id, &message)
                            {
                                // Handle bot commands: reply directly via
                                // the sender's channel instead of forwarding
                                // upstream (commands are not agent queries).
                                if let Some(_reply_text) = Self::handle_command(&gw_msg.text) {
                                    // We send the *command* GatewayMessage
                                    // through as well so the upstream can
                                    // decide what to do, but we also mark
                                    // it so the caller knows this was a
                                    // command. For simplicity we just push
                                    // it through – the agent may choose to
                                    // ignore commands.
                                    debug!(
                                        chat_id = %gw_msg.chat_id,
                                        command = %gw_msg.text,
                                        "bot command received"
                                    );
                                }

                                if msg_tx.send(gw_msg).is_err() {
                                    error!("message receiver dropped – stopping poll loop");
                                    return;
                                }
                            }

                            // Handle callback queries (inline button presses)
                            if let Some(callback_query) = update.callback_query
                                && let Some(data) = callback_query.data
                            {
                                debug!(
                                    callback_id = %callback_query.id,
                                    data = %data,
                                    "callback query received"
                                );

                                // Extract chat_id and user_id from the callback
                                let (chat_id, user_id) = if let Some(msg) = callback_query.message {
                                    (msg.chat.id.to_string(), callback_query.from.id.to_string())
                                } else {
                                    // Fallback: use user_id as chat_id for inline queries
                                    let uid = callback_query.from.id.to_string();
                                    (uid.clone(), uid)
                                };

                                // Create a GatewayMessage with callback_data
                                let gw_msg = GatewayMessage {
                                    platform: "telegram".to_owned(),
                                    bot_id: bot_id.clone(),
                                    chat_id,
                                    user_id,
                                    text: String::new(), // Callbacks have no text body
                                    media: None,
                                    callback_data: Some(data.clone()),
                                };

                                if msg_tx.send(gw_msg).is_err() {
                                    error!("message receiver dropped – stopping poll loop");
                                    return;
                                }

                                // Send acknowledgment to Telegram (answerCallbackQuery)
                                // The api_url is like "https://api.telegram.org/bot<token>/getUpdates"
                                // We need "https://api.telegram.org/bot<token>/answerCallbackQuery"
                                let base_url = api_url.trim_end_matches("/getUpdates");
                                let _ = client
                                    .post(format!("{}/answerCallbackQuery", base_url))
                                    .json(&serde_json::json!({
                                        "callback_query_id": callback_query.id
                                    }))
                                    .send()
                                    .await;
                            }

                            // Advance offset so we don't see this update again.
                            offset = update_id + 1;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "getUpdates failed, retrying in 5s");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        })
    }

    /// Send a message with an inline keyboard attached.
    ///
    /// This is a specialized method for sending dispatch feedback buttons.
    /// The keyboard has three buttons: "💡 太复杂", "✅ 正好", "🚀 太简单"
    ///
    /// # Arguments
    /// - `chat_id`: Target chat ID
    /// - `text`: Message text (Markdown supported)
    /// - `dispatch_id`: Unique identifier for this dispatch decision (embedded in callback data)
    pub async fn send_message_with_dispatch_feedback(
        &self,
        chat_id: &str,
        text: &str,
        dispatch_id: &str,
    ) -> Result<()> {
        let text = normalize_outbound_text(text);
        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown",
            "reply_markup": {
                "inline_keyboard": [[
                    {
                        "text": "💡 太复杂",
                        "callback_data": format!("dispatch_lighter:{}", dispatch_id)
                    },
                    {
                        "text": "✅ 正好",
                        "callback_data": format!("dispatch_justright:{}", dispatch_id)
                    },
                    {
                        "text": "🚀 太简单",
                        "callback_data": format!("dispatch_stronger:{}", dispatch_id)
                    }
                ]]
            }
        });

        let resp: TgResponse<serde_json::Value> = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await
            .context("failed to send Telegram message with InlineKeyboard")?
            .json()
            .await
            .context("failed to parse sendMessage response")?;

        if !resp.ok {
            anyhow::bail!(
                "Telegram sendMessage with InlineKeyboard failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PlatformAdapter implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl PlatformAdapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> Result<()> {
        info!("connecting Telegram adapter (starting long-poll loop)");

        // Quick connectivity check.
        let resp: TgResponse<serde_json::Value> = self
            .client
            .get(self.api_url("getMe"))
            .send()
            .await
            .context("failed to reach Telegram API")?
            .json()
            .await
            .context("failed to parse getMe response")?;

        if !resp.ok {
            anyhow::bail!(
                "Telegram getMe failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        info!("Telegram bot identity verified via getMe");

        // Set commands menu in Telegram
        let commands_body = serde_json::json!({
            "commands": telegram_bot_commands()
                .iter()
                .map(|command| {
                    serde_json::json!({
                        "command": command.command,
                        "description": command.description,
                    })
                })
                .collect::<Vec<_>>()
        });

        let _ = self
            .client
            .post(self.api_url("setMyCommands"))
            .json(&commands_body)
            .send()
            .await;

        let handle = self.spawn_poll_loop();
        self.poll_handle = Some(handle);

        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<()> {
        let text = escape_markdown(&normalize_outbound_text(text));
        // Split messages longer than 4096 characters into multiple sends.
        let chunks = split_message(&text, MAX_MESSAGE_LENGTH);

        for chunk in chunks {
            let body = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
                "parse_mode": "Markdown",
            });

            let resp: TgResponse<serde_json::Value> = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&body)
                .send()
                .await
                .context("failed to send Telegram message")?
                .json()
                .await
                .context("failed to parse sendMessage response")?;

            if !resp.ok {
                anyhow::bail!(
                    "Telegram sendMessage failed: {}",
                    resp.description.unwrap_or_else(|| "unknown error".into())
                );
            }
        }

        Ok(())
    }

    fn max_message_chars(&self) -> Option<usize> {
        Some(MAX_MESSAGE_LENGTH)
    }

    async fn send_media(&self, chat_id: &str, media: &str, caption: &str) -> Result<()> {
        let caption = normalize_outbound_text(caption);
        let media_kind = classify_media_kind(media);
        if is_existing_local_file(media) {
            send_local_media(
                &self.client,
                &self.api_url(method_name(media_kind)),
                chat_id,
                media,
                &caption,
                media_kind,
            )
            .await
        } else {
            send_remote_media(
                &self.client,
                &self.api_url(method_name(media_kind)),
                chat_id,
                media,
                &caption,
                media_kind,
            )
            .await
        }
    }

    async fn send_chat_action(&self, chat_id: &str, action: &str) -> Result<()> {
        let body = serde_json::json!({
            "chat_id": chat_id,
            "action": action,
        });
        let _ = self
            .client
            .post(self.api_url("sendChatAction"))
            .json(&body)
            .send()
            .await;
        Ok(())
    }

    fn supports_draft_streaming(&self, chat_id: &str, chat_type: Option<&str>) -> bool {
        if matches!(
            chat_type.map(|kind| kind.to_ascii_lowercase()),
            Some(kind) if kind == "dm" || kind == "private"
        ) {
            return true;
        }

        // Telegram private chat IDs are positive user IDs. Groups/channels are
        // negative IDs, so an unknown chat type can still safely enable drafts
        // for positive numeric IDs and fall back on API rejection otherwise.
        chat_type.is_none()
            && chat_id
                .parse::<i64>()
                .is_ok_and(|parsed_chat_id| parsed_chat_id > 0)
    }

    async fn send_draft(&self, chat_id: &str, draft_id: i64, text: &str) -> Result<()> {
        let text = truncate_draft_text(&normalize_outbound_text(text));
        let body = serde_json::json!({
            "chat_id": chat_id,
            "draft_id": draft_id,
            "text": text,
        });

        let resp: TgResponse<serde_json::Value> = self
            .client
            .post(self.api_url("sendMessageDraft"))
            .json(&body)
            .send()
            .await
            .context("failed to send Telegram draft")?
            .json()
            .await
            .context("failed to parse sendMessageDraft response")?;
        if !resp.ok {
            anyhow::bail!(
                "Telegram sendMessageDraft failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        Ok(())
    }

    async fn send_message_get_id(&self, chat_id: &str, text: &str) -> Result<Option<i64>> {
        let text = escape_markdown(&normalize_outbound_text(text));
        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown",
        });
        let resp: TgResponse<serde_json::Value> = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await
            .context("failed to send Telegram message")?
            .json()
            .await
            .context("failed to parse sendMessage response")?;
        
        if resp.ok {
            if let Some(result) = &resp.result {
                return Ok(result.get("message_id").and_then(|v| v.as_i64()));
            }
        }
        Ok(None)
    }

    async fn edit_message(&self, chat_id: &str, message_id: i64, text: &str) -> Result<()> {
        let text = escape_markdown(&normalize_outbound_text(text));
        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": "Markdown",
        });
        let resp: TgResponse<serde_json::Value> = self
            .client
            .post(self.api_url("editMessageText"))
            .json(&body)
            .send()
            .await
            .context("failed to edit Telegram message")?
            .json()
            .await
            .context("failed to parse editMessageText response")?;
        
        if !resp.ok {
            // Silent handling for "message is not modified"
            if let Some(desc) = &resp.description {
                if !desc.contains("message is not modified") {
                    warn!(error = %desc, "editMessageText failed");
                }
            }
        }
        Ok(())
    }

    async fn delete_message(&self, chat_id: &str, message_id: i64) -> Result<()> {
        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
        });
        let resp: TgResponse<serde_json::Value> = self
            .client
            .post(self.api_url("deleteMessage"))
            .json(&body)
            .send()
            .await
            .context("failed to delete Telegram message")?
            .json()
            .await
            .context("failed to parse deleteMessage response")?;
        if !resp.ok {
            anyhow::bail!(
                "deleteMessage failed: {}",
                resp.description.unwrap_or_else(|| "unknown".to_string())
            );
        }
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.msg_rx.take()
    }

    async fn download_media(&self, media_id: &str) -> Result<(Vec<u8>, String)> {
        let file_resp: TgResponse<serde_json::Value> = self
            .client
            .get(self.api_url("getFile"))
            .query(&[("file_id", media_id)])
            .send()
            .await
            .context("getFile request failed")?
            .json()
            .await
            .context("failed to parse getFile response")?;

        if !file_resp.ok {
            anyhow::bail!(
                "getFile error: {}",
                file_resp.description.unwrap_or_else(|| "unknown".into())
            );
        }

        let file_path = file_resp
            .result
            .and_then(|v| {
                v.get("file_path")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_owned())
            })
            .context("file_path missing in getFile response")?;

        let download_url = format!("{}/file/bot{}/{}", self.base_url, self.token, file_path);
        let resp = self.client.get(&download_url).send().await?;
        let bytes = resp.bytes().await?.to_vec();

        let lower_path = file_path.to_lowercase();
        let mime_type = if lower_path.ends_with(".jpg") || lower_path.ends_with(".jpeg") {
            "image/jpeg"
        } else if lower_path.ends_with(".png") {
            "image/png"
        } else if lower_path.ends_with(".webp") {
            "image/webp"
        } else {
            "application/octet-stream"
        };

        Ok((bytes, mime_type.to_owned()))
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("disconnecting Telegram adapter");
        if let Some(handle) = self.poll_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Perform a single `getUpdates` long-poll request.
async fn poll_once(client: &reqwest::Client, api_url: &str, offset: i64) -> Result<Vec<TgUpdate>> {
    let resp: TgResponse<Vec<TgUpdate>> = client
        .get(api_url)
        .query(&[
            ("offset", offset.to_string()),
            ("timeout", POLL_TIMEOUT.to_string()),
        ])
        .send()
        .await
        .context("getUpdates request failed")?
        .json()
        .await
        .context("failed to parse getUpdates response")?;

    if !resp.ok {
        anyhow::bail!(
            "getUpdates error: {}",
            resp.description.unwrap_or_else(|| "unknown".into())
        );
    }

    Ok(resp.result.unwrap_or_default())
}

fn normalize_outbound_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Escape special characters for Telegram Markdown (v1).
/// Escapes: _ * [ ] ( ) `
fn escape_markdown(text: &str) -> String {
    text.replace('_', "\\_")
        .replace('*', "\\*")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace('`', "\\`")
}

fn truncate_draft_text(text: &str) -> String {
    text.chars().take(MAX_MESSAGE_LENGTH).collect()
}

fn method_name(kind: TelegramMediaKind) -> &'static str {
    match kind {
        TelegramMediaKind::Photo => "sendPhoto",
        TelegramMediaKind::Voice => "sendVoice",
        TelegramMediaKind::Audio => "sendAudio",
    }
}

fn media_field_name(kind: TelegramMediaKind) -> &'static str {
    match kind {
        TelegramMediaKind::Photo => "photo",
        TelegramMediaKind::Voice => "voice",
        TelegramMediaKind::Audio => "audio",
    }
}

fn classify_media_kind(media: &str) -> TelegramMediaKind {
    let lower = media
        .split('?')
        .next()
        .unwrap_or(media)
        .replace('\\', "/")
        .to_ascii_lowercase();
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
    {
        TelegramMediaKind::Photo
    } else if lower.ends_with(".ogg") || lower.ends_with(".opus") {
        TelegramMediaKind::Voice
    } else {
        TelegramMediaKind::Audio
    }
}

fn is_existing_local_file(media: &str) -> bool {
    Path::new(media).is_file()
}

fn file_name_or_default(media: &str, kind: TelegramMediaKind) -> String {
    Path::new(media)
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| match kind {
            TelegramMediaKind::Photo => "hakimi-image.png".to_string(),
            TelegramMediaKind::Voice => "hakimi-voice.ogg".to_string(),
            TelegramMediaKind::Audio => "hakimi-audio.mp3".to_string(),
        })
}

fn mime_for_kind(kind: TelegramMediaKind, media: &str) -> &'static str {
    let lower = media.to_ascii_lowercase();
    match kind {
        TelegramMediaKind::Photo if lower.ends_with(".png") => "image/png",
        TelegramMediaKind::Photo if lower.ends_with(".webp") => "image/webp",
        TelegramMediaKind::Photo if lower.ends_with(".gif") => "image/gif",
        TelegramMediaKind::Photo => "image/jpeg",
        TelegramMediaKind::Voice => "audio/ogg",
        TelegramMediaKind::Audio if lower.ends_with(".wav") => "audio/wav",
        TelegramMediaKind::Audio if lower.ends_with(".m4a") => "audio/mp4",
        TelegramMediaKind::Audio => "audio/mpeg",
    }
}

async fn send_remote_media(
    client: &reqwest::Client,
    api_url: &str,
    chat_id: &str,
    media: &str,
    caption: &str,
    kind: TelegramMediaKind,
) -> Result<()> {
    let escaped_caption = escape_markdown(caption);
    let field_name = media_field_name(kind);
    let body = serde_json::json!({
        "chat_id": chat_id,
        field_name: media,
        "caption": escaped_caption,
        "parse_mode": "Markdown",
    });
    let response: TgResponse<serde_json::Value> = client
        .post(api_url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("failed to send Telegram {}", method_name(kind)))?
        .json()
        .await
        .with_context(|| format!("failed to parse {} response", method_name(kind)))?;
    
    if !response.ok {
        anyhow::bail!(
            "Telegram {} failed: {}",
            method_name(kind),
            response.description.unwrap_or_else(|| "unknown error".into())
        );
    }
    Ok(())
}

async fn send_local_media(
    client: &reqwest::Client,
    api_url: &str,
    chat_id: &str,
    media: &str,
    caption: &str,
    kind: TelegramMediaKind,
) -> Result<()> {
    let escaped_caption = escape_markdown(caption);
    let bytes = std::fs::read(media)
        .with_context(|| format!("failed to read local media file: {media}"))?;
    let field_name = media_field_name(kind);
    let file_name = file_name_or_default(media, kind);
    let mime = mime_for_kind(kind, media);
    let part = Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(mime)
        .context("failed to set multipart MIME type")?;

    let form = Form::new()
        .text("chat_id", chat_id.to_string())
        .text("caption", escaped_caption)
        .text("parse_mode", "Markdown".to_string())
        .part(field_name.to_string(), part);
    let response: TgResponse<serde_json::Value> = client
        .post(api_url)
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("failed to upload Telegram {}", method_name(kind)))?
        .json()
        .await
        .with_context(|| format!("failed to parse {} upload response", method_name(kind)))?;
    
    if !response.ok {
        anyhow::bail!(
            "Telegram {} failed: {}",
            method_name(kind),
            response.description.unwrap_or_else(|| "unknown error".into())
        );
    }
    Ok(())
}

/// Convert a [`TgMessage`] into a [`GatewayMessage`].
///
/// Returns `None` if the message has no usable content (no text and no photo).
fn convert_message(bot_id: &str, msg: &TgMessage) -> Option<GatewayMessage> {
    let user_id = msg
        .from
        .as_ref()
        .map(|u| u.id.to_string())
        .unwrap_or_else(|| "unknown".to_owned());

    let chat_id = msg.chat.id.to_string();

    // Prefer text; fall back to a photo marker.
    let text = msg
        .text
        .clone()
        .or_else(|| msg.photo.as_ref().map(|_| "[photo]".to_owned()))
        .unwrap_or_default();

    if text.is_empty() {
        return None;
    }

    let media = msg.photo.as_ref().and_then(|photos| {
        // Pick the largest available photo size (last in the array).
        photos.last().map(|p| p.file_id.clone())
    });

    Some(GatewayMessage {
        platform: "telegram".to_owned(),
        bot_id: bot_id.to_owned(),
        chat_id,
        user_id,
        text,
        media,
        callback_data: None,
    })
}

/// Split `text` into chunks of at most `max_len` characters.
///
/// Tries to split on newline boundaries for cleaner output.
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_owned()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_owned());
            break;
        }

        // Try to find a newline boundary within the limit.
        let slice = &remaining[..max_len];
        let split_at = slice
            .rfind('\n')
            .or_else(|| slice.rfind(' '))
            .unwrap_or(max_len);

        chunks.push(remaining[..split_at].to_owned());
        remaining = remaining[split_at..].trim_start_matches('\n');
    }

    chunks
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_short_message() {
        let chunks = split_message("hello", 4096);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_split_long_message_on_newline() {
        let part1 = "a".repeat(4000);
        let part2 = "b".repeat(100);
        let text = format!("{part1}\n{part2}");
        let chunks = split_message(&text, 4096);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].len() <= 4096);
    }

    #[test]
    fn test_split_no_newline_boundary() {
        let text = "x".repeat(10000);
        let chunks = split_message(&text, 4096);
        assert!(chunks.len() >= 3);
        for chunk in &chunks {
            assert!(chunk.len() <= 4096);
        }
    }

    #[test]
    fn test_normalize_outbound_text_preserves_line_breaks() {
        let chunks = split_message(&normalize_outbound_text("line1\r\nline2\rline3"), 4096);
        assert_eq!(chunks, vec!["line1\nline2\nline3"]);
    }

    #[test]
    fn test_convert_text_message() {
        let msg = TgMessage {
            message_id: 1,
            from: Some(TgUser {
                id: 42,
                first_name: "Alice".into(),
            }),
            chat: TgChat {
                id: 100,
                chat_type: "private".into(),
            },
            text: Some("Hello!".into()),
            photo: None,
        };
        let gw = convert_message("default", &msg).unwrap();
        assert_eq!(gw.platform, "telegram");
        assert_eq!(gw.chat_id, "100");
        assert_eq!(gw.user_id, "42");
        assert_eq!(gw.text, "Hello!");
        assert!(gw.media.is_none());
    }

    #[test]
    fn test_convert_photo_message() {
        let msg = TgMessage {
            message_id: 2,
            from: Some(TgUser {
                id: 7,
                first_name: "Bob".into(),
            }),
            chat: TgChat {
                id: 200,
                chat_type: "group".into(),
            },
            text: None,
            photo: Some(vec![
                TgPhotoSize {
                    file_id: "small_id".into(),
                },
                TgPhotoSize {
                    file_id: "large_id".into(),
                },
            ]),
        };
        let gw = convert_message("default", &msg).unwrap();
        assert_eq!(gw.text, "[photo]");
        assert_eq!(gw.media.as_deref(), Some("large_id"));
    }

    #[test]
    fn test_handle_start_command() {
        let reply = TelegramAdapter::handle_command("/start");
        assert!(reply.is_some());
        assert!(reply.unwrap().contains("Welcome"));
    }

    #[test]
    fn test_handle_help_command() {
        let reply = TelegramAdapter::handle_command("/help");
        assert!(reply.is_some());
        let reply = reply.unwrap();
        assert!(reply.contains("Commands"));
        assert!(reply.contains("/usage"));
        assert!(reply.contains("/doctor"));
        assert!(reply.contains("/logs"));
        assert!(reply.contains("/providers"));
    }

    #[test]
    fn telegram_command_menu_includes_gateway_quality_commands() {
        let commands: Vec<&str> = telegram_bot_commands()
            .iter()
            .map(|command| command.command)
            .collect();

        for expected in [
            "start",
            "help",
            "stop",
            "clear",
            "status",
            "usage",
            "cron",
            "doctor",
            "logs",
            "memory",
            "checkpoints",
            "providers",
            "platforms",
            "mcp",
            "browser",
            "update",
            "restart",
        ] {
            assert!(commands.contains(&expected), "missing /{expected}");
        }
    }

    #[test]
    fn test_handle_unknown_command() {
        assert!(TelegramAdapter::handle_command("/unknown").is_none());
        assert!(TelegramAdapter::handle_command("hello").is_none());
    }

    #[test]
    fn test_adapter_name() {
        let adapter = TelegramAdapter::from_token("default", "test:token");
        assert_eq!(adapter.name(), "telegram");
    }

    #[test]
    fn telegram_draft_support_is_private_chat_only() {
        let adapter = TelegramAdapter::from_token("default", "test:token");

        assert!(adapter.supports_draft_streaming("123", Some("private")));
        assert!(adapter.supports_draft_streaming("123", Some("dm")));
        assert!(adapter.supports_draft_streaming("123", None));
        assert!(!adapter.supports_draft_streaming("-100123", None));
        assert!(!adapter.supports_draft_streaming("-100123", Some("group")));
        assert!(!adapter.supports_draft_streaming("channel", None));
    }

    #[test]
    fn telegram_draft_truncation_is_utf8_safe() {
        let text = "好".repeat(MAX_MESSAGE_LENGTH + 2);
        let truncated = truncate_draft_text(&text);

        assert_eq!(truncated.chars().count(), MAX_MESSAGE_LENGTH);
        assert!(truncated.ends_with('好'));
    }

    #[test]
    fn classify_media_kind_routes_images_voice_and_audio() {
        assert_eq!(
            classify_media_kind("C:/tmp/generated.png"),
            TelegramMediaKind::Photo
        );
        assert_eq!(
            classify_media_kind("https://example.com/voice.ogg?download=1"),
            TelegramMediaKind::Voice
        );
        assert_eq!(
            classify_media_kind("C:/tmp/audio.mp3"),
            TelegramMediaKind::Audio
        );
    }

    #[test]
    fn local_file_detection_only_accepts_existing_paths() {
        let path = std::env::temp_dir().join(format!("hakimi-tg-media-{}.mp3", std::process::id()));
        std::fs::write(&path, b"audio").unwrap();
        assert!(is_existing_local_file(path.to_str().unwrap()));
        std::fs::remove_file(&path).unwrap();
        assert!(!is_existing_local_file(path.to_str().unwrap()));
    }
}
