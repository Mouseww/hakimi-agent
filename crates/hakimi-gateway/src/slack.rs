//! Slack Web API gateway adapter.
//!
//! Uses `chat.postMessage` for sending outbound messages and
//! `conversations.history` for polling inbound messages.
//!
//! Supports Slack Block Kit for rich formatting via [`SlackBlock`] helpers.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::{GatewayMessage, PlatformAdapter};

// ---------------------------------------------------------------------------
// Slack API types
// ---------------------------------------------------------------------------

/// Response from most Slack Web API methods.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SlackResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    messages: Option<Vec<SlackMessage>>,
    #[serde(default)]
    response_metadata: Option<SlackResponseMetadata>,
}

/// Metadata that Slack attaches to list-style responses.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SlackResponseMetadata {
    #[serde(default)]
    next_cursor: Option<String>,
}

/// A Slack message object.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SlackMessage {
    /// Message timestamp (unique ID).
    ts: String,
    /// Plain-text body.
    #[serde(default)]
    text: String,
    /// User who sent the message.
    #[serde(default)]
    user: Option<String>,
    /// Bot ID if the message was sent by a bot.
    #[serde(default)]
    bot_id: Option<String>,
    /// Block Kit blocks (kept as raw JSON for forwarding).
    #[serde(default)]
    blocks: Option<Vec<serde_json::Value>>,
}

/// A Block Kit block for building rich Slack messages.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum SlackBlock {
    /// A simple section with Markdown text.
    #[serde(rename = "section")]
    Section {
        text: SlackTextObject,
    },
    /// A divider line.
    #[serde(rename = "divider")]
    Divider,
    /// A header block.
    #[serde(rename = "header")]
    Header {
        text: SlackPlainTextObject,
    },
    /// A context block (small muted text / images).
    #[serde(rename = "context")]
    Context {
        elements: Vec<SlackTextObject>,
    },
    /// A markdown-formatted rich text block.
    #[serde(rename = "rich_text")]
    RichText {
        elements: Vec<serde_json::Value>,
    },
}

/// A Slack text object (mrkdwn or plain_text).
#[derive(Debug, Serialize)]
pub struct SlackTextObject {
    #[serde(rename = "type")]
    pub text_type: String,
    pub text: String,
}

/// A Slack plain-text object (used in headers).
#[derive(Debug, Serialize)]
pub struct SlackPlainTextObject {
    #[serde(rename = "type")]
    pub text_type: String,
    pub text: String,
}

impl SlackTextObject {
    /// Create a Markdown text object.
    pub fn mrkdwn(text: impl Into<String>) -> Self {
        Self {
            text_type: "mrkdwn".to_owned(),
            text: text.into(),
        }
    }

    /// Create a plain-text text object.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text_type: "plain_text".to_owned(),
            text: text.into(),
        }
    }
}

impl SlackPlainTextObject {
    /// Create a plain_text object (for headers).
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text_type: "plain_text".to_owned(),
            text: text.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const SLACK_API_BASE: &str = "https://slack.com/api";
const MAX_TEXT_LENGTH: usize = 40000;
const POLL_INTERVAL_SECS: u64 = 10;

// ---------------------------------------------------------------------------
// SlackAdapter
// ---------------------------------------------------------------------------

/// Configuration for the Slack adapter.
pub struct SlackAdapterConfig {
    /// Bot token (xoxb-…).
    pub token: String,
    /// Bot / role identifier for this instance.
    pub bot_id: String,
    /// Channel ID to poll for messages (optional — if unset, polling is skipped).
    pub channel_id: Option<String>,
    /// Optional API base URL override (useful for testing).
    pub base_url: Option<String>,
}

/// Slack Web API adapter with polling for inbound messages.
pub struct SlackAdapter {
    token: String,
    bot_id: String,
    channel_id: Option<String>,
    base_url: String,
    client: reqwest::Client,
    msg_tx: mpsc::UnboundedSender<GatewayMessage>,
    msg_rx: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    poll_handle: Option<JoinHandle<()>>,
}

impl SlackAdapter {
    /// Create a new Slack adapter from a config.
    pub fn new(config: SlackAdapterConfig) -> Self {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let base_url = config
            .base_url
            .unwrap_or_else(|| SLACK_API_BASE.to_owned());
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
    pub fn from_token_and_channel(
        bot_id: impl Into<String>,
        token: impl Into<String>,
        channel_id: impl Into<String>,
    ) -> Self {
        Self::new(SlackAdapterConfig {
            token: token.into(),
            bot_id: bot_id.into(),
            channel_id: Some(channel_id.into()),
            base_url: None,
        })
    }

    /// Full URL for a Slack Web API method.
    fn api_url(&self, method: &str) -> String {
        format!("{}/{}", self.base_url, method)
    }

    /// Authorization header value.
    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Spawn a background polling loop that fetches recent messages from the
    /// configured Slack channel and forwards new ones to the message channel.
    fn spawn_poll_loop(&self) -> JoinHandle<()> {
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let token = self.token.clone();
        let channel_id = self.channel_id.clone().unwrap_or_default();
        let msg_tx = self.msg_tx.clone();
        let bot_id = self.bot_id.clone();

        tokio::spawn(async move {
            // Track the latest timestamp we've seen so we only forward new messages.
            let mut latest_ts: Option<String> = None;

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;

                let url = format!("{}/conversations.history", base_url);

                let mut params = vec![("channel", channel_id.as_str()), ("limit", "50")];
                if let Some(ref ts) = latest_ts {
                    params.push(("oldest", ts.as_str()));
                }

                let resp = match client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .query(&params)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(error = %e, "Slack poll request failed, retrying");
                        continue;
                    }
                };

                if !resp.status().is_success() {
                    warn!(status = %resp.status(), "Slack poll returned error status");
                    continue;
                }

                let slack_resp: SlackResponse = match resp.json().await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(error = %e, "failed to parse Slack poll response");
                        continue;
                    }
                };

                if !slack_resp.ok {
                    warn!(
                        error = slack_resp.error.as_deref().unwrap_or("unknown"),
                        "Slack API returned not ok"
                    );
                    continue;
                }

                let messages = slack_resp.messages.unwrap_or_default();

                // Messages come newest-first; reverse for chronological order.
                let mut messages = messages;
                messages.reverse();

                for msg in &messages {
                    // Skip bot messages (including our own).
                    if msg.bot_id.is_some() {
                        continue;
                    }

                    let text = msg.text.clone();
                    if text.is_empty() {
                        continue;
                    }

                    let user_id = msg.user.clone().unwrap_or_else(|| "unknown".to_owned());

                    let gw_msg = GatewayMessage {
                        platform: "slack".to_owned(),
                        bot_id: bot_id.clone(),
                        chat_id: channel_id.clone(),
                        user_id,
                        text,
                        media: None,
                    };

                    if msg_tx.send(gw_msg).is_err() {
                        error!("message receiver dropped – stopping Slack poll loop");
                        return;
                    }
                }

                // Advance the latest timestamp cursor.
                if let Some(last) = messages.last() {
                    latest_ts = Some(last.ts.clone());
                }
            }
        })
    }

    /// Send a message with Block Kit blocks to a Slack channel.
    ///
    /// This is a higher-level method beyond the trait's `send_message` — it
    /// lets callers attach structured Block Kit objects.
    pub async fn send_message_with_blocks(
        &self,
        channel: &str,
        text: &str,
        blocks: &[SlackBlock],
    ) -> Result<()> {
        let chunks = split_message(text, MAX_TEXT_LENGTH);

        for (i, chunk) in chunks.iter().enumerate() {
            let blocks_json = if i == 0 && !blocks.is_empty() {
                serde_json::to_value(blocks).unwrap_or_default()
            } else {
                serde_json::Value::Array(vec![])
            };

            let body = serde_json::json!({
                "channel": channel,
                "text": chunk,
                "blocks": blocks_json,
            });

            let resp: SlackResponse = self
                .client
                .post(self.api_url("chat.postMessage"))
                .header("Authorization", self.auth_header())
                .header("Content-Type", "application/json; charset=utf-8")
                .json(&body)
                .send()
                .await
                .context("Slack chat.postMessage request failed")?
                .json()
                .await
                .context("failed to parse chat.postMessage response")?;

            if !resp.ok {
                anyhow::bail!(
                    "Slack chat.postMessage failed: {}",
                    resp.error.unwrap_or_else(|| "unknown error".into())
                );
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PlatformAdapter implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl PlatformAdapter for SlackAdapter {
    fn name(&self) -> &str {
        "slack"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> Result<()> {
        info!("connecting Slack adapter");

        // Verify bot identity with auth.test.
        let resp: SlackResponse = self
            .client
            .post(self.api_url("auth.test"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("Slack auth.test request failed")?
            .json()
            .await
            .context("failed to parse auth.test response")?;

        if !resp.ok {
            anyhow::bail!(
                "Slack auth.test failed: {}",
                resp.error.unwrap_or_else(|| "unknown error".into())
            );
        }
        info!("Slack bot identity verified via auth.test");

        // Start polling if a channel was configured.
        if self.channel_id.is_some() {
            let handle = self.spawn_poll_loop();
            self.poll_handle = Some(handle);
            info!(
                channel_id = self.channel_id.as_deref().unwrap_or(""),
                "started Slack message polling"
            );
        } else {
            debug!("no channel_id configured, skipping Slack poll loop");
        }

        Ok(())
    }

    async fn send_message(&self, channel: &str, text: &str) -> Result<()> {
        let chunks = split_message(text, MAX_TEXT_LENGTH);

        for chunk in chunks {
            let body = serde_json::json!({
                "channel": channel,
                "text": chunk,
            });

            let resp: SlackResponse = self
                .client
                .post(self.api_url("chat.postMessage"))
                .header("Authorization", self.auth_header())
                .header("Content-Type", "application/json; charset=utf-8")
                .json(&body)
                .send()
                .await
                .context("Slack chat.postMessage request failed")?
                .json()
                .await
                .context("failed to parse chat.postMessage response")?;

            if !resp.ok {
                anyhow::bail!(
                    "Slack chat.postMessage failed: {}",
                    resp.error.unwrap_or_else(|| "unknown error".into())
                );
            }
        }

        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.msg_rx.take()
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("disconnecting Slack adapter");
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
        let chunks = split_message("hello", 40000);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_split_long_message_on_newline() {
        let part1 = "a".repeat(39000);
        let part2 = "b".repeat(2000);
        let text = format!("{part1}\n{part2}");
        let chunks = split_message(&text, 40000);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 40000);
        }
    }

    #[test]
    fn test_split_no_newline_boundary() {
        let text = "x".repeat(100000);
        let chunks = split_message(&text, 40000);
        assert!(chunks.len() >= 3);
        for chunk in &chunks {
            assert!(chunk.len() <= 40000);
        }
    }

    #[test]
    fn test_block_section_serialization() {
        let block = SlackBlock::Section {
            text: SlackTextObject::mrkdwn("*Hello* world"),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "section");
        assert_eq!(json["text"]["type"], "mrkdwn");
        assert_eq!(json["text"]["text"], "*Hello* world");
    }

    #[test]
    fn test_block_divider_serialization() {
        let block = SlackBlock::Divider;
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "divider");
    }

    #[test]
    fn test_block_header_serialization() {
        let block = SlackBlock::Header {
            text: SlackPlainTextObject::new("My Header"),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "header");
        assert_eq!(json["text"]["text"], "My Header");
    }

    #[test]
    fn test_text_object_constructors() {
        let md = SlackTextObject::mrkdwn("bold");
        assert_eq!(md.text_type, "mrkdwn");
        assert_eq!(md.text, "bold");

        let plain = SlackTextObject::plain("plain");
        assert_eq!(plain.text_type, "plain_text");
        assert_eq!(plain.text, "plain");
    }

    #[test]
    fn test_adapter_construction() {
        let adapter = SlackAdapter::from_token_and_channel("default", "xoxb-test", "C12345");
        assert_eq!(adapter.name(), "slack");
        assert_eq!(adapter.channel_id.as_deref(), Some("C12345"));
        assert!(adapter.poll_handle.is_none());
    }

    #[test]
    fn test_take_receiver_once() {
        let mut adapter = SlackAdapter::from_token_and_channel("default", "tok", "ch");
        let rx = adapter.take_receiver();
        assert!(rx.is_some());
        let rx2 = adapter.take_receiver();
        assert!(rx2.is_none());
    }
}
