//! Platform gateway adapters for the Hakimi Agent.
//!
//! Provides an async trait for chat-platform integrations (Slack, Discord,
//! Telegram, etc.) and a central [`Gateway`] that routes inbound messages
//! to the agent runtime.

mod clawbot;
mod dingtalk;
mod discord;
mod feishu;
pub mod lifecycle;
mod matrix;
mod mattermost;
mod signal;
mod slack;
mod telegram;
mod webhook;
mod wecom;

pub use clawbot::{ClawBotAdapter, ClawBotAdapterConfig, ClawBotMode};
pub use dingtalk::{DingTalkAdapter, DingTalkAdapterConfig};
pub use discord::{DiscordAdapter, DiscordAdapterConfig, DiscordEmbed};
pub use feishu::{FeishuAdapter, FeishuAdapterConfig};
pub use lifecycle::{gateway_events_log_path, read_recent_gateway_events, read_recent_lines};
pub use matrix::{MatrixAdapter, MatrixAdapterConfig};
pub use mattermost::{MattermostAdapter, MattermostAdapterConfig};
pub use signal::{SignalAdapter, SignalAdapterConfig};
pub use slack::{SlackAdapter, SlackAdapterConfig, SlackBlock, SlackTextObject};
pub use telegram::TelegramAdapter;
pub use webhook::{WebhookAdapter, WebhookAdapterConfig};
pub use wecom::{WeComAdapter, WeComAdapterConfig};

// TelegramAdapterConfig is not re-exported above, add it
pub use telegram::TelegramAdapterConfig;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

const SILENCE_NARRATION_MAX_CHARS: usize = 64;

/// Return true when text is only an outbound silence narration token.
///
/// This is intentionally narrow: substantive prose that contains "silent" is
/// delivered, while bare loop-prone tokens such as `*(silent)*`, `no reply`,
/// `.`, `...`, `…`, and `🔇` are dropped before reaching chat adapters.
pub fn is_silence_narration(content: &str) -> bool {
    let stripped = content.trim();
    if stripped.is_empty() || stripped.chars().count() > SILENCE_NARRATION_MAX_CHARS {
        return false;
    }

    let marker_trimmed = trim_silence_wrappers(stripped);
    if marker_trimmed.is_empty() {
        return false;
    }
    if marker_trimmed
        .chars()
        .all(|c| matches!(c, '.' | '…' | '🔇'))
    {
        return true;
    }

    let unwrapped = strip_parenthesized_token(marker_trimmed);
    let normalized = unwrapped
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();

    matches!(
        normalized.as_str(),
        "silent" | "silence" | "no response" | "no reply"
    )
}

fn trim_silence_wrappers(mut text: &str) -> &str {
    loop {
        let trimmed = text
            .trim()
            .trim_matches(|c| matches!(c, '*' | '_' | '~' | '`'))
            .trim();
        if trimmed.len() == text.len() {
            return trimmed;
        }
        text = trimmed;
    }
}

fn strip_parenthesized_token(text: &str) -> &str {
    let text = text.trim();
    if let Some(inner) = text
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        inner.trim()
    } else {
        text
    }
}

fn silence_filter_env_override() -> Option<bool> {
    for key in [
        "HAKIMI_FILTER_SILENCE_NARRATION",
        "HERMES_FILTER_SILENCE_NARRATION",
    ] {
        if let Ok(value) = std::env::var(key) {
            let normalized = value.trim().to_ascii_lowercase();
            return Some(matches!(normalized.as_str(), "1" | "true" | "yes" | "on"));
        }
    }
    None
}

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
    filter_silence_narration: bool,
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
            filter_silence_narration: silence_filter_env_override().unwrap_or(true),
        }
    }

    /// Enable or disable outbound silence-narration filtering.
    pub fn set_filter_silence_narration(&mut self, enabled: bool) {
        self.filter_silence_narration = silence_filter_env_override().unwrap_or(enabled);
    }

    /// Register a platform adapter.
    pub fn add_adapter(&mut self, adapter: Box<dyn PlatformAdapter>) {
        let platform = adapter.name().to_string();
        let bot_id = adapter.bot_id().to_string();
        tracing::info!("registered platform adapter: {}", platform);
        lifecycle::record_gateway_event(
            "adapter.registered",
            Some(&platform),
            Some(&bot_id),
            None,
            "",
        );
        self.adapters.push(adapter);
    }

    /// Connect all registered adapters.
    pub async fn connect_all(&mut self) -> anyhow::Result<()> {
        for adapter in &mut self.adapters {
            let platform = adapter.name().to_string();
            let bot_id = adapter.bot_id().to_string();
            tracing::info!("connecting adapter: {}", platform);
            lifecycle::record_gateway_event(
                "adapter.connect.start",
                Some(&platform),
                Some(&bot_id),
                None,
                "",
            );
            match adapter.connect().await {
                Ok(()) => lifecycle::record_gateway_event(
                    "adapter.connect.ok",
                    Some(&platform),
                    Some(&bot_id),
                    None,
                    "",
                ),
                Err(err) => {
                    lifecycle::record_gateway_event(
                        "adapter.connect.error",
                        Some(&platform),
                        Some(&bot_id),
                        None,
                        err.to_string(),
                    );
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    /// Disconnect all registered adapters.
    pub async fn disconnect_all(&mut self) -> anyhow::Result<()> {
        for adapter in &mut self.adapters {
            let platform = adapter.name().to_string();
            let bot_id = adapter.bot_id().to_string();
            tracing::info!("disconnecting adapter: {}", platform);
            lifecycle::record_gateway_event(
                "adapter.disconnect.start",
                Some(&platform),
                Some(&bot_id),
                None,
                "",
            );
            match adapter.disconnect().await {
                Ok(()) => lifecycle::record_gateway_event(
                    "adapter.disconnect.ok",
                    Some(&platform),
                    Some(&bot_id),
                    None,
                    "",
                ),
                Err(err) => {
                    lifecycle::record_gateway_event(
                        "adapter.disconnect.error",
                        Some(&platform),
                        Some(&bot_id),
                        None,
                        err.to_string(),
                    );
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    /// Route an outbound message to the correct adapter by platform name.
    pub async fn route_message(&self, msg: &GatewayMessage) -> anyhow::Result<()> {
        let adapter = match self
            .adapters
            .iter()
            .find(|a| a.name() == msg.platform && a.bot_id() == msg.bot_id)
        {
            Some(adapter) => adapter,
            None => {
                let err = anyhow::anyhow!(
                    "no adapter for platform '{}' with bot_id '{}'",
                    msg.platform,
                    msg.bot_id
                );
                lifecycle::record_gateway_event(
                    "route.missing_adapter",
                    Some(&msg.platform),
                    Some(&msg.bot_id),
                    Some(&msg.chat_id),
                    err.to_string(),
                );
                return Err(err);
            }
        };

        if self.should_filter_outbound_text(msg) {
            tracing::warn!(
                platform = %msg.platform,
                chat_id = %msg.chat_id,
                "dropped silence-narration outbound gateway message"
            );
            lifecycle::record_gateway_event(
                "route.filtered_silence",
                Some(&msg.platform),
                Some(&msg.bot_id),
                Some(&msg.chat_id),
                "",
            );
            return Ok(());
        }

        let result = if let Some(media) = msg.media.as_deref()
            && !media.trim().is_empty()
        {
            adapter.send_media(&msg.chat_id, media, &msg.text).await
        } else {
            adapter.send_message(&msg.chat_id, &msg.text).await
        };

        match &result {
            Ok(()) => lifecycle::record_gateway_event(
                "route.ok",
                Some(&msg.platform),
                Some(&msg.bot_id),
                Some(&msg.chat_id),
                if msg
                    .media
                    .as_deref()
                    .is_some_and(|media| !media.trim().is_empty())
                {
                    "media=true"
                } else {
                    ""
                },
            ),
            Err(err) => lifecycle::record_gateway_event(
                "route.error",
                Some(&msg.platform),
                Some(&msg.bot_id),
                Some(&msg.chat_id),
                err.to_string(),
            ),
        }
        result
    }

    /// Route an outbound message to the correct adapter and get its ID.
    pub async fn route_message_get_id(&self, msg: &GatewayMessage) -> anyhow::Result<Option<i64>> {
        let adapter = match self
            .adapters
            .iter()
            .find(|a| a.name() == msg.platform && a.bot_id() == msg.bot_id)
        {
            Some(adapter) => adapter,
            None => {
                let err = anyhow::anyhow!(
                    "no adapter for platform '{}' with bot_id '{}'",
                    msg.platform,
                    msg.bot_id
                );
                lifecycle::record_gateway_event(
                    "route_get_id.missing_adapter",
                    Some(&msg.platform),
                    Some(&msg.bot_id),
                    Some(&msg.chat_id),
                    err.to_string(),
                );
                return Err(err);
            }
        };

        if self.should_filter_outbound_text(msg) {
            tracing::warn!(
                platform = %msg.platform,
                chat_id = %msg.chat_id,
                "dropped silence-narration outbound gateway message"
            );
            lifecycle::record_gateway_event(
                "route_get_id.filtered_silence",
                Some(&msg.platform),
                Some(&msg.bot_id),
                Some(&msg.chat_id),
                "",
            );
            return Ok(None);
        }

        let result = adapter.send_message_get_id(&msg.chat_id, &msg.text).await;
        match &result {
            Ok(message_id) => lifecycle::record_gateway_event(
                "route_get_id.ok",
                Some(&msg.platform),
                Some(&msg.bot_id),
                Some(&msg.chat_id),
                format!("message_id={}", message_id.unwrap_or_default()),
            ),
            Err(err) => lifecycle::record_gateway_event(
                "route_get_id.error",
                Some(&msg.platform),
                Some(&msg.bot_id),
                Some(&msg.chat_id),
                err.to_string(),
            ),
        }
        result
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

        if self.filter_silence_narration && is_silence_narration(text) {
            tracing::warn!(
                platform = %platform,
                chat_id = %chat_id,
                "dropped silence-narration gateway message edit"
            );
            lifecycle::record_gateway_event(
                "edit.filtered_silence",
                Some(platform),
                Some(bot_id),
                Some(chat_id),
                format!("message_id={message_id}"),
            );
            return Ok(());
        }

        let result = adapter.edit_message(chat_id, message_id, text).await;
        match &result {
            Ok(()) => lifecycle::record_gateway_event(
                "edit.ok",
                Some(platform),
                Some(bot_id),
                Some(chat_id),
                format!("message_id={message_id}"),
            ),
            Err(err) => lifecycle::record_gateway_event(
                "edit.error",
                Some(platform),
                Some(bot_id),
                Some(chat_id),
                err.to_string(),
            ),
        }
        result
    }

    fn should_filter_outbound_text(&self, msg: &GatewayMessage) -> bool {
        if !self.filter_silence_narration {
            return false;
        }
        if msg
            .media
            .as_deref()
            .is_some_and(|media| !media.trim().is_empty())
        {
            return false;
        }
        is_silence_narration(&msg.text)
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
                lifecycle::record_gateway_event(
                    "receiver.attached",
                    Some(&name),
                    Some(&bid),
                    None,
                    "",
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn silence_narration_detects_wrapped_tokens() {
        for content in [
            "*(silent)*",
            "*Silence.*",
            "🔇",
            ".",
            "…",
            "...",
            "(silent)",
            "_silent_",
            "`silent`",
            "~silent~",
            "no response",
            "No Reply.",
        ] {
            assert!(is_silence_narration(content), "{content}");
        }
    }

    #[test]
    fn silence_narration_rejects_substantive_messages() {
        for content in [
            "Silence is golden - here is the plan...",
            "Silent install completed",
            "The deployment ran silently in the background",
            "ok",
            "Here is the result:\n\n- item one\n- item two",
            "silent xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "",
            "   ",
        ] {
            assert!(!is_silence_narration(content), "{content}");
        }
    }

    #[derive(Clone, Default)]
    struct RecordingAdapter {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl PlatformAdapter for RecordingAdapter {
        fn name(&self) -> &str {
            "test"
        }

        fn bot_id(&self) -> &str {
            "bot"
        }

        async fn connect(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_message(&self, _chat_id: &str, text: &str) -> anyhow::Result<()> {
            self.calls.lock().unwrap().push(text.to_string());
            Ok(())
        }

        async fn send_media(
            &self,
            _chat_id: &str,
            media: &str,
            caption: &str,
        ) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{media}|{caption}"));
            Ok(())
        }

        async fn send_message_get_id(
            &self,
            _chat_id: &str,
            text: &str,
        ) -> anyhow::Result<Option<i64>> {
            self.calls.lock().unwrap().push(text.to_string());
            Ok(Some(42))
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn gateway_message(text: &str) -> GatewayMessage {
        GatewayMessage {
            platform: "test".to_string(),
            bot_id: "bot".to_string(),
            chat_id: "chat".to_string(),
            user_id: "user".to_string(),
            text: text.to_string(),
            media: None,
        }
    }

    #[tokio::test]
    async fn route_message_drops_silence_narration() {
        let adapter = RecordingAdapter::default();
        let calls = adapter.calls.clone();
        let mut gateway = Gateway::new();
        gateway.add_adapter(Box::new(adapter));

        gateway
            .route_message(&gateway_message("*(silent)*"))
            .await
            .unwrap();

        assert!(calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn route_message_delivers_real_message() {
        let adapter = RecordingAdapter::default();
        let calls = adapter.calls.clone();
        let mut gateway = Gateway::new();
        gateway.add_adapter(Box::new(adapter));

        gateway
            .route_message(&gateway_message("Silence is golden - deploy is green."))
            .await
            .unwrap();

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "Silence is golden - deploy is green.");
    }

    #[tokio::test]
    async fn route_message_get_id_drops_and_returns_none() {
        let adapter = RecordingAdapter::default();
        let calls = adapter.calls.clone();
        let mut gateway = Gateway::new();
        gateway.add_adapter(Box::new(adapter));

        let message_id = gateway
            .route_message_get_id(&gateway_message("..."))
            .await
            .unwrap();

        assert!(message_id.is_none());
        assert!(calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn route_message_delivers_when_filter_disabled() {
        let adapter = RecordingAdapter::default();
        let calls = adapter.calls.clone();
        let mut gateway = Gateway::new();
        gateway.set_filter_silence_narration(false);
        gateway.add_adapter(Box::new(adapter));

        gateway.route_message(&gateway_message("🔇")).await.unwrap();

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "🔇");
    }

    #[tokio::test]
    async fn route_message_keeps_media_with_silent_caption() {
        let adapter = RecordingAdapter::default();
        let calls = adapter.calls.clone();
        let mut gateway = Gateway::new();
        gateway.add_adapter(Box::new(adapter));
        let mut msg = gateway_message("silent");
        msg.media = Some("/tmp/image.png".to_string());

        gateway.route_message(&msg).await.unwrap();

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "/tmp/image.png|silent");
    }
}
