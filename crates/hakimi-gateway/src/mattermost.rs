//! Mattermost gateway adapter.
//!
//! Uses the Mattermost REST API (v4) for outbound posts and optional channel
//! polling for inbound posts. This keeps the first Mattermost surface small and
//! dependency-free while matching Hermes' URL/token/channel configuration model.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::{GatewayMessage, PlatformAdapter};

const MAX_POST_LENGTH: usize = 4000;
const POLL_INTERVAL_SECS: u64 = 10;

/// Mattermost gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MattermostAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_mattermost_bot_id")]
    pub bot_id: String,
    /// Mattermost server URL, for example `https://mattermost.example.com`.
    pub server_url: String,
    /// Bot token or personal access token.
    pub token: String,
    /// Optional channel ID to poll for inbound posts.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// Optional API base URL override for tests.
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_mattermost_bot_id() -> String {
    "mattermost".to_string()
}

#[derive(Debug, Deserialize)]
struct MattermostUser {
    id: String,
    #[serde(default)]
    username: String,
}

#[derive(Debug, Deserialize)]
struct MattermostPost {
    id: String,
    channel_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct MattermostPostsResponse {
    order: Vec<String>,
    posts: std::collections::HashMap<String, MattermostPost>,
}

/// Mattermost REST adapter with optional polling for inbound posts.
pub struct MattermostAdapter {
    config: MattermostAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    sender: mpsc::UnboundedSender<GatewayMessage>,
    poll_handle: Option<JoinHandle<()>>,
}

impl MattermostAdapter {
    /// Create a Mattermost adapter from config.
    pub fn new(config: MattermostAdapterConfig) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
            sender,
            poll_handle: None,
        }
    }

    fn api_base(&self) -> String {
        self.config
            .base_url
            .clone()
            .unwrap_or_else(|| format!("{}/api/v4", self.config.server_url.trim_end_matches('/')))
            .trim_end_matches('/')
            .to_string()
    }

    fn auth_value(&self) -> String {
        format!("Bearer {}", self.config.token)
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/{}", self.api_base(), path.trim_start_matches('/'))
    }

    async fn fetch_self(&self) -> Result<MattermostUser> {
        let resp = self
            .client
            .get(self.api_url("users/me"))
            .header("Authorization", self.auth_value())
            .send()
            .await
            .context("Mattermost users/me request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Mattermost users/me failed: status={}, body={}",
                status,
                body
            );
        }

        resp.json()
            .await
            .context("failed to parse Mattermost users/me response")
    }

    fn spawn_poll_loop(&self, bot_user_id: String) -> Option<JoinHandle<()>> {
        let channel_id = self.config.channel_id.clone()?;
        let client = self.client.clone();
        let api_base = self.api_base();
        let auth = self.auth_value();
        let sender = self.sender.clone();
        let bot_id = self.bot_id.clone();

        Some(tokio::spawn(async move {
            let mut after_post: Option<String> = None;

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;

                let url = format!("{api_base}/channels/{channel_id}/posts");
                let mut request = client.get(&url).header("Authorization", auth.clone());
                if let Some(after) = after_post.as_deref() {
                    request = request.query(&[("after", after)]);
                }

                let resp = match request.send().await {
                    Ok(resp) => resp,
                    Err(err) => {
                        warn!(error = %err, "Mattermost poll request failed");
                        continue;
                    }
                };

                if !resp.status().is_success() {
                    warn!(status = %resp.status(), "Mattermost poll returned error status");
                    continue;
                }

                let posts: MattermostPostsResponse = match resp.json().await {
                    Ok(posts) => posts,
                    Err(err) => {
                        warn!(error = %err, "failed to parse Mattermost poll response");
                        continue;
                    }
                };

                for post_id in posts.order.iter().rev() {
                    let Some(post) = posts.posts.get(post_id) else {
                        continue;
                    };
                    after_post = Some(post.id.clone());

                    if post.user_id == bot_user_id || post.message.trim().is_empty() {
                        continue;
                    }

                    let message = GatewayMessage {
                        platform: "mattermost".to_string(),
                        bot_id: bot_id.clone(),
                        chat_id: post.channel_id.clone(),
                        user_id: post.user_id.clone(),
                        text: post.message.clone(),
                        media: None,
                        callback_data: None,
                        reply_to_message_id: None,
                        reply_to_text: None,
                    };

                    if sender.send(message).is_err() {
                        warn!("Mattermost receiver dropped; stopping poll loop");
                        return;
                    }
                }
            }
        }))
    }
}

#[async_trait]
impl PlatformAdapter for MattermostAdapter {
    fn name(&self) -> &str {
        "mattermost"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        let me = self.fetch_self().await?;
        info!(
            user_id = %me.id,
            username = %me.username,
            "Mattermost adapter connected"
        );
        self.poll_handle = self.spawn_poll_loop(me.id);
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let channel_id = if chat_id.trim().is_empty() {
            self.config.channel_id.as_deref().unwrap_or_default()
        } else {
            chat_id
        };
        if channel_id.trim().is_empty() {
            anyhow::bail!("Mattermost send_message requires a channel_id");
        }

        let body = serde_json::json!({
            "channel_id": channel_id,
            "message": truncate_post(text),
        });

        let resp = self
            .client
            .post(self.api_url("posts"))
            .header("Authorization", self.auth_value())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Mattermost posts request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Mattermost send_message failed: status={}, body={}",
                status,
                body
            );
        }

        info!(
            channel_id,
            text_len = text.len(),
            "Mattermost: message sent"
        );
        Ok(())
    }

    fn max_message_chars(&self) -> Option<usize> {
        Some(MAX_POST_LENGTH)
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if let Some(handle) = self.poll_handle.take() {
            handle.abort();
        }
        info!("Mattermost adapter disconnected");
        Ok(())
    }
}

fn truncate_post(text: &str) -> String {
    if text.chars().count() <= MAX_POST_LENGTH {
        return text.to_string();
    }

    let mut truncated: String = text.chars().take(MAX_POST_LENGTH - 3).collect();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> MattermostAdapterConfig {
        MattermostAdapterConfig {
            bot_id: "ops-mm".into(),
            server_url: "https://mattermost.example.com/".into(),
            token: "mm-redacted".into(),
            channel_id: Some("channel-123".into()),
            base_url: None,
        }
    }

    #[test]
    fn construction_sets_platform_identity() {
        let adapter = MattermostAdapter::new(make_config());
        assert_eq!(adapter.name(), "mattermost");
        assert_eq!(adapter.bot_id(), "ops-mm");
    }

    #[test]
    fn api_base_defaults_to_v4_path() {
        let adapter = MattermostAdapter::new(make_config());
        assert_eq!(adapter.api_base(), "https://mattermost.example.com/api/v4");
    }

    #[test]
    fn api_base_honors_override() {
        let config = MattermostAdapterConfig {
            base_url: Some("http://127.0.0.1:9000/api/v4/".into()),
            ..make_config()
        };
        let adapter = MattermostAdapter::new(config);
        assert_eq!(
            adapter.api_url("/posts"),
            "http://127.0.0.1:9000/api/v4/posts"
        );
    }

    #[test]
    fn truncate_post_caps_by_chars() {
        let long = "a".repeat(MAX_POST_LENGTH + 10);
        let out = truncate_post(&long);
        assert_eq!(out.chars().count(), MAX_POST_LENGTH);
        assert!(out.ends_with("..."));
    }
}
