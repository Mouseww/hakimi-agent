//! Platform gateway adapters for the Hakimi Agent.
//!
//! Provides an async trait for chat-platform integrations (Slack, Discord,
//! Telegram, etc.) and a central [`Gateway`] that routes inbound messages
//! to the agent runtime.

mod clawbot;
mod dingtalk;
mod discord;
mod matrix;
mod signal;
mod slack;
mod telegram;
mod webhook;
mod wecom;

pub use clawbot::{ClawBotAdapter, ClawBotAdapterConfig, ClawBotMode};
pub use dingtalk::{DingTalkAdapter, DingTalkAdapterConfig};
pub use discord::{DiscordAdapter, DiscordAdapterConfig, DiscordEmbed};
pub use matrix::{MatrixAdapter, MatrixAdapterConfig};
pub use signal::{SignalAdapter, SignalAdapterConfig};
pub use slack::{SlackAdapter, SlackAdapterConfig, SlackBlock, SlackTextObject};
pub use telegram::TelegramAdapter;
pub use webhook::{WebhookAdapter, WebhookAdapterConfig};
pub use wecom::{WeComAdapter, WeComAdapterConfig};

// TelegramAdapterConfig is not re-exported above, add it
pub use telegram::TelegramAdapterConfig;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Gateway message types
// ---------------------------------------------------------------------------

/// An inbound or outbound message flowing through the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    /// Platform identifier (e.g. "telegram", "slack", "discord").
    pub platform: String,
    /// Bot / role identifier — distinguishes multiple bots on the same platform.
    pub bot_id: String,
    /// Chat / channel identifier on the platform.
    pub chat_id: String,
    /// User identifier on the platform.
    pub user_id: String,
    /// Text body of the message.
    pub text: String,
    /// Optional media attachment path or URL.
    pub media: Option<String>,
}

// ---------------------------------------------------------------------------
// PlatformAdapter trait
// ---------------------------------------------------------------------------

/// Abstraction over a chat-platform connection.
///
/// Each platform (Telegram, Slack, Discord, …) implements this trait to
/// integrate with the [`Gateway`].
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// Human-readable platform name (e.g. "telegram").
    fn name(&self) -> &str;

    /// Identifier for this specific bot / role instance.
    fn bot_id(&self) -> &str;

    /// Establish the connection to the platform (login, WebSocket, polling, etc.).
    async fn connect(&mut self) -> anyhow::Result<()>;

    /// Send a message to a specific chat / channel.
    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()>;

    /// Send a media attachment with an optional caption.
    ///
    /// `media` may be a platform file ID, an HTTP(S) URL, or a local path for
    /// adapters that support uploads. The default implementation degrades to a
    /// plain text message so non-media platforms remain compatible.
    async fn send_media(&self, chat_id: &str, media: &str, caption: &str) -> anyhow::Result<()> {
        let mut text = caption.to_string();
        if !media.trim().is_empty() {
            if !text.trim().is_empty() {
                text.push('\n');
            }
            text.push_str(media);
        }
        self.send_message(chat_id, &text).await
    }

    /// Send a chat action (e.g. "typing") to indicate the bot is working.
    /// Default: no-op for platforms that don't support it.
    async fn send_chat_action(&self, _chat_id: &str, _action: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Edit an existing message (for streaming progressive updates).
    /// Returns Ok(message_id) on success, Err if not supported.
    async fn edit_message(
        &self,
        _chat_id: &str,
        _message_id: i64,
        _text: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Send a message and return the platform message ID (for later editing).
    async fn send_message_get_id(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<i64>> {
        self.send_message(chat_id, text).await?;
        Ok(None)
    }

    /// Delete an existing message when the platform supports it.
    async fn delete_message(&self, _chat_id: &str, _message_id: i64) -> anyhow::Result<()> {
        anyhow::bail!("Message deletion not supported on this platform")
    }

    /// Take ownership of the inbound message receiver channel.
    ///
    /// Returns `Some(receiver)` if the adapter supports receiving messages
    /// and the receiver has not already been taken. Returns `None` otherwise.
    fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<GatewayMessage>> {
        None
    }

    /// Download a media file given its platform-specific identifier or URL.
    /// Returns the raw bytes of the file and its MIME type (e.g., "image/jpeg").
    async fn download_media(&self, _media_id: &str) -> anyhow::Result<(Vec<u8>, String)> {
        anyhow::bail!("Media download not supported on this platform")
    }

    /// Gracefully disconnect from the platform.
    async fn disconnect(&mut self) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// Gateway
// ---------------------------------------------------------------------------

/// Central gateway that owns a set of platform adapters and routes messages.
pub struct Gateway {
    adapters: Vec<Box<dyn PlatformAdapter>>,
}

/// A received inbound message paired with its originating platform adapter name.
pub struct InboundMessage {
    /// The gateway message received from a platform.
    pub message: GatewayMessage,
    /// Index of the adapter that produced this message.
    pub adapter_index: usize,
}

impl Gateway {
    /// Create an empty gateway.
    pub fn new() -> Self {
        Self {
            adapters: Vec::new(),
        }
    }

    /// Register a platform adapter.
    pub fn add_adapter(&mut self, adapter: Box<dyn PlatformAdapter>) {
        tracing::info!("registered platform adapter: {}", adapter.name());
        self.adapters.push(adapter);
    }

    /// Connect all registered adapters.
    pub async fn connect_all(&mut self) -> anyhow::Result<()> {
        for adapter in &mut self.adapters {
            tracing::info!("connecting adapter: {}", adapter.name());
            adapter.connect().await?;
        }
        Ok(())
    }

    /// Disconnect all registered adapters.
    pub async fn disconnect_all(&mut self) -> anyhow::Result<()> {
        for adapter in &mut self.adapters {
            tracing::info!("disconnecting adapter: {}", adapter.name());
            adapter.disconnect().await?;
        }
        Ok(())
    }

    /// Route an outbound message to the correct adapter by platform name.
    pub async fn route_message(&self, msg: &GatewayMessage) -> anyhow::Result<()> {
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.name() == msg.platform && a.bot_id() == msg.bot_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no adapter for platform '{}' with bot_id '{}'",
                    msg.platform,
                    msg.bot_id
                )
            })?;

        if let Some(media) = msg.media.as_deref()
            && !media.trim().is_empty()
        {
            adapter.send_media(&msg.chat_id, media, &msg.text).await
        } else {
            adapter.send_message(&msg.chat_id, &msg.text).await
        }
    }

    /// Route an outbound message to the correct adapter and get its ID.
    pub async fn route_message_get_id(&self, msg: &GatewayMessage) -> anyhow::Result<Option<i64>> {
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.name() == msg.platform && a.bot_id() == msg.bot_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no adapter for platform '{}' with bot_id '{}'",
                    msg.platform,
                    msg.bot_id
                )
            })?;

        adapter.send_message_get_id(&msg.chat_id, &msg.text).await
    }

    /// Download media from a platform adapter.
    pub async fn download_media(
        &self,
        platform: &str,
        bot_id: &str,
        media_id: &str,
    ) -> anyhow::Result<(Vec<u8>, String)> {
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.name() == platform && a.bot_id() == bot_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no adapter for platform '{}' with bot_id '{}'",
                    platform,
                    bot_id
                )
            })?;

        adapter.download_media(media_id).await
    }

    /// Edit an existing message by ID.
    pub async fn edit_message(
        &self,
        platform: &str,
        bot_id: &str,
        chat_id: &str,
        message_id: i64,
        text: &str,
    ) -> anyhow::Result<()> {
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.name() == platform && a.bot_id() == bot_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no adapter for platform '{}' with bot_id '{}'",
                    platform,
                    bot_id
                )
            })?;

        adapter.edit_message(chat_id, message_id, text).await
    }

    /// Delete an existing message by ID when supported by the adapter.
    pub async fn delete_message(
        &self,
        platform: &str,
        bot_id: &str,
        chat_id: &str,
        message_id: i64,
    ) -> anyhow::Result<()> {
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.name() == platform && a.bot_id() == bot_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no adapter for platform '{}' with bot_id '{}'",
                    platform,
                    bot_id
                )
            })?;

        adapter.delete_message(chat_id, message_id).await
    }

    /// Send a chat action (e.g. "typing") to the correct adapter by bot_id.
    pub async fn send_chat_action(
        &self,
        bot_id: &str,
        chat_id: &str,
        action: &str,
    ) -> anyhow::Result<()> {
        for adapter in &self.adapters {
            if adapter.bot_id() == bot_id {
                return adapter.send_chat_action(chat_id, action).await;
            }
        }
        Ok(())
    }

    /// Return the list of registered platform names.
    pub fn platforms(&self) -> Vec<&str> {
        self.adapters.iter().map(|a| a.name()).collect()
    }

    /// Drain all inbound message receivers from registered adapters and merge
    /// them into a single [`tokio::sync::mpsc::UnboundedReceiver`].
    ///
    /// This should be called after [`connect_all`](Self::connect_all).
    /// Each adapter that supports receiving messages will have its receiver
    /// taken and merged into a single stream.
    pub fn take_all_receivers(
        &mut self,
    ) -> Vec<(
        String,
        String,
        tokio::sync::mpsc::UnboundedReceiver<GatewayMessage>,
    )> {
        let mut receivers = Vec::new();
        for adapter in &mut self.adapters {
            let name = adapter.name().to_owned();
            let bid = adapter.bot_id().to_owned();
            if let Some(rx) = adapter.take_receiver() {
                tracing::info!("took message receiver for adapter: {}", name);
                receivers.push((name, bid, rx));
            }
        }
        receivers
    }
}

impl Default for Gateway {
    fn default() -> Self {
        Self::new()
    }
}
