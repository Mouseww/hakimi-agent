//! Microsoft Teams Webhook integration (Outgoing Webhook + Power Automate Workflows).
//!
//! Implements bidirectional Teams integration without Azure Bot registration:
//! - Inbound: Teams Outgoing Webhook POSTs to our HTTP endpoint
//! - Outbound: We POST Adaptive Cards to Power Automate Workflows webhook URLs
//!
//! Reference: https://docs.microsoft.com/en-us/microsoftteams/platform/webhooks-and-connectors/

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, info, warn};

use crate::{GatewayMessage, PlatformAdapter};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Configuration
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Configuration for Teams Webhook adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsWebhookConfig {
    /// HMAC secret token from Teams Outgoing Webhook (base64 string).
    pub hmac_secret: String,

    /// Default Power Automate Workflows webhook URL (fallback).
    #[serde(default)]
    pub default_workflow_url: String,

    /// Channel ID -> Workflows URL mapping.
    /// Key: Teams channel ID (e.g., "19:abc...@thread.tacv2")
    /// Value: Power Automate Workflows webhook URL
    #[serde(default)]
    pub channel_workflows: HashMap<String, String>,

    /// Bot identifier (for internal routing).
    #[serde(default = "default_bot_id")]
    pub bot_id: String,
}

fn default_bot_id() -> String {
    "teams-agent".to_string()
}

impl Default for TeamsWebhookConfig {
    fn default() -> Self {
        Self {
            hmac_secret: String::new(),
            default_workflow_url: String::new(),
            channel_workflows: HashMap::new(),
            bot_id: default_bot_id(),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Inbound message structures (from Teams)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct TeamsInboundActivity {
    #[serde(rename = "type")]
    pub activity_type: String,
    pub id: Option<String>,
    pub timestamp: Option<String>,
    pub from: Option<TeamsFrom>,
    pub text: Option<String>,
    #[serde(rename = "channelData")]
    pub channel_data: Option<TeamsChannelData>,
}

#[derive(Debug, Deserialize)]
pub struct TeamsFrom {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "aadObjectId")]
    pub aad_object_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TeamsChannelData {
    #[serde(rename = "teamsChannelId")]
    pub teams_channel_id: Option<String>,
    #[serde(rename = "teamsTeamId")]
    pub teams_team_id: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Outbound message structures (Adaptive Cards)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Helper to build Adaptive Card payloads for Teams.
pub struct AdaptiveCardBuilder {
    title: String,
    body_parts: Vec<serde_json::Value>,
    actions: Vec<serde_json::Value>,
}

impl AdaptiveCardBuilder {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body_parts: Vec::new(),
            actions: Vec::new(),
        }
    }

    pub fn add_text(&mut self, text: impl Into<String>) -> &mut Self {
        self.body_parts.push(json!({
            "type": "TextBlock",
            "text": text.into(),
            "wrap": true
        }));
        self
    }

    pub fn add_fact(&mut self, title: impl Into<String>, value: impl Into<String>) -> &mut Self {
        // Will be wrapped in FactSet when building
        self.body_parts.push(json!({
            "_type": "fact",
            "title": title.into(),
            "value": value.into()
        }));
        self
    }

    pub fn add_button(&mut self, title: impl Into<String>, url: impl Into<String>) -> &mut Self {
        self.actions.push(json!({
            "type": "Action.OpenUrl",
            "title": title.into(),
            "url": url.into()
        }));
        self
    }

    pub fn build(&self) -> serde_json::Value {
        let mut body = vec![json!({
            "type": "TextBlock",
            "size": "Medium",
            "weight": "Bolder",
            "text": self.title
        })];

        // Group facts into FactSet
        let facts: Vec<_> = self
            .body_parts
            .iter()
            .filter(|p| p.get("_type").and_then(|t| t.as_str()) == Some("fact"))
            .map(|f| {
                json!({
                    "title": f["title"],
                    "value": f["value"]
                })
            })
            .collect();

        if !facts.is_empty() {
            body.push(json!({
                "type": "FactSet",
                "facts": facts
            }));
        }

        // Add regular text blocks
        for part in &self.body_parts {
            if part.get("_type").is_none() {
                body.push(part.clone());
            }
        }

        let mut card = json!({
            "type": "message",
            "attachments": [{
                "contentType": "application/vnd.microsoft.card.adaptive",
                "content": {
                    "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                    "type": "AdaptiveCard",
                    "version": "1.4",
                    "body": body
                }
            }]
        });

        if !self.actions.is_empty() {
            card["attachments"][0]["content"]["actions"] = json!(self.actions);
        }

        card
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Adapter implementation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct TeamsWebhookAdapter {
    config: TeamsWebhookConfig,
    bot_id: String,
    http_client: reqwest::Client,
    sender: Option<mpsc::UnboundedSender<GatewayMessage>>,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    /// Track channel ID for each chat (for outbound routing).
    channel_mapping: Arc<RwLock<HashMap<String, String>>>,
}

impl TeamsWebhookAdapter {
    pub fn new(config: TeamsWebhookConfig) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();

        Self {
            config,
            bot_id,
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            sender: Some(sender),
            receiver: Some(receiver),
            channel_mapping: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Verify HMAC signature from Teams Outgoing Webhook.
    pub fn verify_hmac(&self, raw_body: &[u8], auth_header: &str) -> bool {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        if !auth_header.starts_with("HMAC ") {
            return false;
        }

        let provided = &auth_header[5..]; // Strip "HMAC " prefix

        // Decode the secret from base64
        let key = match BASE64.decode(&self.config.hmac_secret) {
            Ok(k) => k,
            Err(e) => {
                warn!(error = %e, "Failed to decode HMAC secret");
                return false;
            }
        };

        // Compute HMAC-SHA256
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC key size should be valid");
        mac.update(raw_body);
        let result = mac.finalize();
        let expected = BASE64.encode(result.into_bytes());

        // Constant-time comparison
        provided == expected
    }

    /// Process inbound Teams activity and convert to GatewayMessage.
    pub fn process_inbound(&self, activity: TeamsInboundActivity) -> Option<GatewayMessage> {
        let text = activity.text.as_ref()?.clone();

        // Strip HTML tags and @mentions from text
        let clean_text = strip_html_tags(&text).trim().to_string();

        if clean_text.is_empty() {
            return None;
        }

        let user_id = activity
            .from
            .as_ref()
            .and_then(|f| f.aad_object_id.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let channel_id = activity
            .channel_data
            .as_ref()
            .and_then(|cd| cd.teams_channel_id.clone())
            .unwrap_or_else(|| "default".to_string());

        // Store channel mapping for later outbound routing
        let chat_id = format!("teams_{}", channel_id);
        let mapping = self.channel_mapping.clone();
        let ch_id = channel_id.clone();
        let chat_id_clone = chat_id.clone();
        tokio::spawn(async move {
            mapping.write().await.insert(chat_id_clone, ch_id);
        });

        Some(GatewayMessage {
            platform: "teams_webhook".to_string(),
            bot_id: self.bot_id.clone(),
            chat_id,
            user_id,
            text: clean_text,
            media: None,
            callback_data: None,
        })
    }

    /// Inject a message from HTTP handler.
    pub fn inject_message(&self, msg: GatewayMessage) {
        if let Some(ref sender) = self.sender {
            let _ = sender.send(msg);
        }
    }

    /// Get the Workflows URL for a channel.
    async fn get_workflow_url(&self, chat_id: &str) -> Option<String> {
        // First try to get channel ID from mapping
        let channel_id = {
            let mapping = self.channel_mapping.read().await;
            mapping.get(chat_id).cloned()
        };

        if let Some(channel_id) = channel_id {
            // Check if we have a specific workflow for this channel
            if let Some(url) = self.config.channel_workflows.get(&channel_id) {
                return Some(url.clone());
            }
        }

        // Fall back to default
        if !self.config.default_workflow_url.is_empty() {
            Some(self.config.default_workflow_url.clone())
        } else {
            None
        }
    }
}

#[async_trait]
impl PlatformAdapter for TeamsWebhookAdapter {
    fn name(&self) -> &str {
        "teams_webhook"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> Result<()> {
        info!(
            bot_id = %self.bot_id,
            channels = self.config.channel_workflows.len(),
            "Teams Webhook adapter ready"
        );
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<()> {
        let url = self
            .get_workflow_url(chat_id)
            .await
            .context("No Workflows URL configured for this channel")?;

        // Build simple text card
        let mut builder = AdaptiveCardBuilder::new("Agent Response");
        builder.add_text(text);
        let card = builder.build();

        debug!(
            chat_id,
            url_len = url.len(),
            text_len = text.len(),
            "Sending card to Teams Workflows"
        );

        let response = self
            .http_client
            .post(&url)
            .json(&card)
            .send()
            .await
            .context("Failed to POST to Workflows webhook")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "Workflows webhook returned {}: {}",
                status,
                &body[..body.len().min(200)]
            );
        }

        Ok(())
    }

    async fn send_media(&self, chat_id: &str, _media: &str, caption: &str) -> Result<()> {
        // Teams doesn't support direct media via Workflows, fall back to text
        self.send_message(chat_id, caption).await
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("Teams Webhook adapter disconnected");
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Utilities
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Strip HTML tags from Teams message text (removes @mentions).
fn strip_html_tags(text: &str) -> String {
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    re.replace_all(text, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        let input = "<at>AgentBot</at> help me with this task";
        let output = strip_html_tags(input);
        assert_eq!(output, "AgentBot help me with this task");
    }

    #[test]
    fn test_adaptive_card_builder() {
        let mut builder = AdaptiveCardBuilder::new("Test Card");
        builder.add_text("This is a test");
        builder.add_button("Click me", "https://example.com");

        let card = builder.build();
        assert_eq!(card["type"], "message");
        assert!(card["attachments"][0]["content"]["body"].is_array());
    }

    #[tokio::test]
    async fn test_adapter_creation() {
        let config = TeamsWebhookConfig {
            hmac_secret: BASE64.encode(b"test-secret"),
            default_workflow_url: "https://example.com/webhook".to_string(),
            ..Default::default()
        };

        let adapter = TeamsWebhookAdapter::new(config);
        assert_eq!(adapter.name(), "teams_webhook");
    }
}
