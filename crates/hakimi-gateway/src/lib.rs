//! Platform gateway adapters for the Hakimi Agent.
//!
//! Provides an async trait for chat-platform integrations (Slack, Discord,
//! Telegram, etc.) and a central [`Gateway`] that routes inbound messages
//! to the agent runtime.

mod discord;
mod slack;
mod telegram;

pub use discord::{DiscordAdapter, DiscordAdapterConfig, DiscordEmbed};
pub use slack::{SlackAdapter, SlackAdapterConfig, SlackBlock, SlackTextObject};
pub use telegram::TelegramAdapter;

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

    /// Establish the connection to the platform (login, WebSocket, polling, etc.).
    async fn connect(&mut self) -> anyhow::Result<()>;

    /// Send a message to a specific chat / channel.
    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()>;

    /// Take ownership of the inbound message receiver channel.
    ///
    /// Returns `Some(receiver)` if the adapter supports receiving messages
    /// and the receiver has not already been taken. Returns `None` otherwise.
    fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<GatewayMessage>> {
        None
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
            .find(|a| a.name() == msg.platform)
            .ok_or_else(|| anyhow::anyhow!("no adapter for platform: {}", msg.platform))?;

        adapter.send_message(&msg.chat_id, &msg.text).await
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
    ) -> Vec<(String, tokio::sync::mpsc::UnboundedReceiver<GatewayMessage>)> {
        let mut receivers = Vec::new();
        for adapter in &mut self.adapters {
            let name = adapter.name().to_owned();
            if let Some(rx) = adapter.take_receiver() {
                tracing::info!("took message receiver for adapter: {}", name);
                receivers.push((name, rx));
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
