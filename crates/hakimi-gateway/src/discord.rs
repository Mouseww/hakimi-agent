//! Discord Bot API gateway adapter.
//!
//! Uses the Discord REST API (v10) for sending messages and a polling loop
//! on `GET /channels/{channel_id}/messages` for receiving inbound messages.
//!
//! Rich embeds are supported via [`DiscordEmbed`].

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::{GatewayMessage, PlatformAdapter};

// ---------------------------------------------------------------------------
// Discord API types
// ---------------------------------------------------------------------------

/// A Discord message returned by the API.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DiscordMessage {
    id: String,
    #[serde(default)]
    content: String,
    author: Option<DiscordUser>,
    channel_id: String,
    #[serde(default)]
    embeds: Vec<serde_json::Value>,
    /// ISO 8601 timestamp.
    timestamp: Option<String>,
}

/// A Discord user.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DiscordUser {
    id: String,
    username: String,
    #[serde(default)]
    bot: bool,
}

/// A Discord embed object for rich message formatting.
#[derive(Debug, Serialize, Default)]
pub struct DiscordEmbed {
    /// Title of the embed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Description / body text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// URL of the embed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Decimal colour value (e.g. `0x00ff00`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    /// Footer text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<EmbedFooter>,
}

/// Footer inside a Discord embed.
#[derive(Debug, Serialize, Default)]
pub struct EmbedFooter {
    pub text: String,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const MAX_CONTENT_LENGTH: usize = 2000;
const POLL_INTERVAL_SECS: u64 = 10;
const RATE_LIMIT_MAX_RETRIES: u32 = 3;

// ---------------------------------------------------------------------------
// DiscordAdapter
// ---------------------------------------------------------------------------

/// Configuration for the Discord adapter.
pub struct DiscordAdapterConfig {
    /// Bot token (without the `Bot ` prefix — the adapter adds it).
    pub token: String,
    /// Bot / role identifier for this instance.
    pub bot_id: String,
    /// Channel ID to poll for messages (optional — if unset, polling is skipped).
    pub channel_id: Option<String>,
    /// Optional API base URL override (useful for testing).
    pub base_url: Option<String>,
}

/// Discord Bot API adapter with REST-based polling for inbound messages.
pub struct DiscordAdapter {
    token: String,
    bot_id: String,
    channel_id: Option<String>,
    base_url: String,
    client: reqwest::Client,
    msg_tx: mpsc::UnboundedSender<GatewayMessage>,
    msg_rx: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    poll_handle: Option<JoinHandle<()>>,
}

impl DiscordAdapter {
    /// Create a new Discord adapter from a config.
    pub fn new(config: DiscordAdapterConfig) -> Self {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let base_url = config
            .base_url
            .unwrap_or_else(|| DISCORD_API_BASE.to_owned());
        Self {
            token: config.token,
            bot_id: config.bot_id,
            channel_id: config.channel_id,
            base_url,
            client: reqwest::Client::new(),
            msg_tx,
            msg_rx: Some(msg_rx),
            poll_handle: None,
        }
    }

    /// Convenience constructor — create an adapter with just a token and channel.
    pub fn from_token_and_channel(bot_id: impl Into<String>, token: impl Into<String>, channel_id: impl Into<String>) -> Self {
        Self::new(DiscordAdapterConfig {
            token: token.into(),
            bot_id: bot_id.into(),
            channel_id: Some(channel_id.into()),
            base_url: None,
        })
    }

    /// Full URL for a Discord REST endpoint path (e.g. `/channels/123/messages`).
    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Send a raw POST request with proper auth headers, handling rate limits.
    async fn post_with_retry(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(path);
        let mut retries = 0u32;

        loop {
            let resp = self
                .client
                .post(&url)
                .header("Authorization", format!("Bot {}", self.token))
                .header("Content-Type", "application/json")
                .json(body)
                .send()
                .await
                .context("Discord POST request failed")?;

            let status = resp.status();

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(1.0);

                if retries >= RATE_LIMIT_MAX_RETRIES {
                    anyhow::bail!(
                        "Discord rate limited after {} retries (Retry-After: {:.1}s)",
                        retries,
                        retry_after
                    );
                }

                warn!(
                    retry = retries + 1,
                    retry_after_secs = retry_after,
                    "Discord rate limited, waiting before retry"
                );
                tokio::time::sleep(std::time::Duration::from_secs_f64(retry_after)).await;
                retries += 1;
                continue;
            }

            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("Discord API error {}: {}", status, text);
            }

            return resp.json().await.context("failed to parse Discord response");
        }
    }

    /// Send a raw GET request with proper auth headers, handling rate limits.
    async fn get_with_retry(&self, path: &str) -> Result<serde_json::Value> {
        let url = self.api_url(path);
        let mut retries = 0u32;

        loop {
            let resp = self
                .client
                .get(&url)
                .header("Authorization", format!("Bot {}", self.token))
                .send()
                .await
                .context("Discord GET request failed")?;

            let status = resp.status();

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(1.0);

                if retries >= RATE_LIMIT_MAX_RETRIES {
                    anyhow::bail!(
                        "Discord rate limited after {} retries (Retry-After: {:.1}s)",
                        retries,
                        retry_after
                    );
                }

                warn!(
                    retry = retries + 1,
                    retry_after_secs = retry_after,
                    "Discord rate limited on GET, waiting before retry"
                );
                tokio::time::sleep(std::time::Duration::from_secs_f64(retry_after)).await;
                retries += 1;
                continue;
            }

            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("Discord API error {}: {}", status, text);
            }

            return resp.json().await.context("failed to parse Discord response");
        }
    }

    /// Spawn a background polling loop that fetches recent messages from the
    /// configured channel and forwards new ones to the message channel.
    fn spawn_poll_loop(&self) -> JoinHandle<()> {
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let token = self.token.clone();
        let channel_id = self.channel_id.clone().unwrap_or_default();
        let msg_tx = self.msg_tx.clone();
        let bot_id = self.bot_id.clone();

        tokio::spawn(async move {
            // Track the last message ID we've seen so we only forward new ones.
            let mut last_seen_id: Option<String> = None;

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;

                let url = format!(
                    "{}/channels/{}/messages?limit=50",
                    base_url, channel_id
                );

                let resp = match client
                    .get(&url)
                    .header("Authorization", format!("Bot {}", token))
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(error = %e, "Discord poll request failed, retrying");
                        continue;
                    }
                };

                if !resp.status().is_success() {
                    warn!(status = %resp.status(), "Discord poll returned error status");
                    continue;
                }

                let messages: Vec<DiscordMessage> = match resp.json().await {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(error = %e, "failed to parse Discord poll response");
                        continue;
                    }
                };

                // Messages come newest-first; reverse for chronological order.
                let mut messages = messages;
                messages.reverse();

                for msg in &messages {
                    // Skip messages from bots (including ourselves).
                    if msg.author.as_ref().map_or(false, |a| a.bot) {
                        continue;
                    }

                    // Skip messages we've already seen.
                    if let Some(ref seen) = last_seen_id {
                        if msg.id <= *seen {
                            continue;
                        }
                    }

                    let text = msg.content.clone();
                    if text.is_empty() {
                        continue;
                    }

                    let user_id = msg
                        .author
                        .as_ref()
                        .map(|a| a.id.clone())
                        .unwrap_or_else(|| "unknown".to_owned());

                    let gw_msg = GatewayMessage {
                        platform: "discord".to_owned(),
                        bot_id: bot_id.clone(),
                        chat_id: msg.channel_id.clone(),
                        user_id,
                        text,
                        media: None,
                    };

                    if msg_tx.send(gw_msg).is_err() {
                        error!("message receiver dropped – stopping Discord poll loop");
                        return;
                    }
                }

                if let Some(last) = messages.last() {
                    last_seen_id = Some(last.id.clone());
                }
            }
        })
    }

    /// Send a message with optional rich embeds to a Discord channel.
    ///
    /// This is a higher-level method beyond the trait's `send_message` — it
    /// lets callers attach structured embed objects.
    pub async fn send_message_with_embeds(
        &self,
        channel_id: &str,
        content: &str,
        embeds: &[DiscordEmbed],
    ) -> Result<()> {
        // Split long content into chunks (Discord limit is 2000 chars).
        let chunks = split_message(content, MAX_CONTENT_LENGTH);

        for (i, chunk) in chunks.iter().enumerate() {
            let body = serde_json::json!({
                "content": chunk,
                // Only attach embeds on the first chunk to avoid duplication.
                "embeds": if i == 0 { embeds } else { &[] as &[DiscordEmbed] },
            });

            let path = format!("/channels/{}/messages", channel_id);
            self.post_with_retry(&path, &body).await?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PlatformAdapter implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl PlatformAdapter for DiscordAdapter {
    fn name(&self) -> &str {
        "discord"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> Result<()> {
        info!("connecting Discord adapter");

        // Verify bot identity by fetching the current user.
        let me = self
            .get_with_retry("/users/@me")
            .await
            .context("failed to verify Discord bot identity")?;

        let username = me
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        info!(username = %username, "Discord bot identity verified");

        // Start polling if a channel was configured.
        if self.channel_id.is_some() {
            let handle = self.spawn_poll_loop();
            self.poll_handle = Some(handle);
            info!(
                channel_id = self.channel_id.as_deref().unwrap_or(""),
                "started Discord message polling"
            );
        } else {
            debug!("no channel_id configured, skipping Discord poll loop");
        }

        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<()> {
        let chunks = split_message(text, MAX_CONTENT_LENGTH);

        for chunk in chunks {
            let body = serde_json::json!({ "content": chunk });
            let path = format!("/channels/{}/messages", chat_id);
            self.post_with_retry(&path, &body).await?;
        }

        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.msg_rx.take()
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("disconnecting Discord adapter");
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
        let chunks = split_message("hello", 2000);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_split_long_message_on_newline() {
        let part1 = "a".repeat(1800);
        let part2 = "b".repeat(300);
        let text = format!("{part1}\n{part2}");
        let chunks = split_message(&text, 2000);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].len() <= 2000);
    }

    #[test]
    fn test_split_no_newline_boundary() {
        let text = "x".repeat(5000);
        let chunks = split_message(&text, 2000);
        assert!(chunks.len() >= 3);
        for chunk in &chunks {
            assert!(chunk.len() <= 2000);
        }
    }

    #[test]
    fn test_embed_serialization() {
        let embed = DiscordEmbed {
            title: Some("Test".into()),
            description: Some("A test embed".into()),
            color: Some(0x00_ff_00),
            ..Default::default()
        };
        let json = serde_json::to_value(&embed).unwrap();
        assert_eq!(json["title"], "Test");
        assert_eq!(json["color"], 0x00_ff_00);
        // Fields set to None should not appear.
        assert!(json.get("url").is_none());
    }

    #[test]
    fn test_adapter_construction() {
        let adapter = DiscordAdapter::from_token_and_channel("default", "test-token", "12345");
        assert_eq!(adapter.name(), "discord");
        assert_eq!(adapter.channel_id.as_deref(), Some("12345"));
        assert!(adapter.poll_handle.is_none());
    }

    #[test]
    fn test_take_receiver_once() {
        let mut adapter = DiscordAdapter::from_token_and_channel("default", "tok", "ch");
        let rx = adapter.take_receiver();
        assert!(rx.is_some());
        let rx2 = adapter.take_receiver();
        assert!(rx2.is_none());
    }
}
