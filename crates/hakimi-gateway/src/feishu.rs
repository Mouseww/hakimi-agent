//! Feishu / Lark platform adapter.
//!
//! This first Rust-native gateway surface sends outbound text messages through
//! Feishu's tenant access token and IM message APIs.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_feishu_bot_id")]
    pub bot_id: String,
    pub app_id: String,
    pub app_secret: String,
    #[serde(default)]
    pub default_chat_id: String,
    #[serde(default = "default_feishu_receive_id_type")]
    pub receive_id_type: String,
    #[serde(default = "default_feishu_domain")]
    pub domain: String,
    #[serde(default)]
    pub base_url: String,
}

fn default_feishu_bot_id() -> String {
    "default".to_string()
}

fn default_feishu_receive_id_type() -> String {
    "chat_id".to_string()
}

fn default_feishu_domain() -> String {
    "feishu".to_string()
}

pub struct FeishuAdapter {
    config: FeishuAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl FeishuAdapter {
    pub fn new(config: FeishuAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }

    fn api_base_url(&self) -> String {
        let configured = self.config.base_url.trim();
        if !configured.is_empty() {
            return configured.trim_end_matches('/').to_string();
        }

        match self.config.domain.trim().to_ascii_lowercase().as_str() {
            "lark" | "larksuite" | "larksuite.com" => "https://open.larksuite.com".to_string(),
            _ => "https://open.feishu.cn".to_string(),
        }
    }

    fn token_url(&self) -> String {
        format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            self.api_base_url()
        )
    }

    fn send_url(&self) -> String {
        format!(
            "{}/open-apis/im/v1/messages?receive_id_type={}",
            self.api_base_url(),
            self.receive_id_type()
        )
    }

    fn receive_id_type(&self) -> &str {
        match self.config.receive_id_type.trim() {
            "chat_id" | "open_id" | "user_id" | "union_id" | "email" => {
                self.config.receive_id_type.trim()
            }
            _ => "chat_id",
        }
    }

    async fn get_tenant_access_token(&self) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "app_id": self.config.app_id,
            "app_secret": self.config.app_secret,
        });

        let resp = self
            .client
            .post(self.token_url())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Feishu tenant_access_token request failed: status={}, body={}",
                status,
                body_text
            );
        }

        let body: serde_json::Value = resp.json().await?;
        if body.get("code").and_then(|v| v.as_i64()).unwrap_or(0) != 0 {
            let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
            let msg = body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!(
                "Feishu tenant_access_token error: code={}, msg={}",
                code,
                msg
            );
        }

        body.get("tenant_access_token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Feishu tenant_access_token missing in response"))
    }
}

#[async_trait]
impl PlatformAdapter for FeishuAdapter {
    fn name(&self) -> &str {
        "feishu"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(domain = %self.config.domain, "Feishu adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let receive_id = if chat_id.trim().is_empty() {
            self.config.default_chat_id.trim()
        } else {
            chat_id.trim()
        };
        if receive_id.is_empty() {
            anyhow::bail!("Feishu send_message requires a receive_id/chat_id");
        }

        let access_token = self.get_tenant_access_token().await?;
        let content = serde_json::json!({ "text": text }).to_string();
        let body = serde_json::json!({
            "receive_id": receive_id,
            "msg_type": "text",
            "content": content,
            "uuid": uuid::Uuid::new_v4().to_string(),
        });

        let resp = self
            .client
            .post(self.send_url())
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Feishu send_message failed: status={}, body={}",
                status,
                body_text
            );
        }

        let body: serde_json::Value = resp.json().await?;
        if body.get("code").and_then(|v| v.as_i64()).unwrap_or(0) != 0 {
            let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
            let msg = body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Feishu send_message error: code={}, msg={}", code, msg);
        }

        info!(
            receive_id,
            text_len = text.len(),
            receive_id_type = self.receive_id_type(),
            "Feishu: message sent"
        );
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("Feishu adapter disconnected");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> FeishuAdapterConfig {
        FeishuAdapterConfig {
            bot_id: "default".into(),
            app_id: "cli_test".into(),
            app_secret: "secret".into(),
            default_chat_id: "oc_chat".into(),
            receive_id_type: "chat_id".into(),
            domain: "feishu".into(),
            base_url: String::new(),
        }
    }

    #[test]
    fn test_construction() {
        let adapter = FeishuAdapter::new(make_config());
        assert_eq!(adapter.config.app_id, "cli_test");
        assert_eq!(adapter.config.default_chat_id, "oc_chat");
    }

    #[test]
    fn test_name() {
        let adapter = FeishuAdapter::new(make_config());
        assert_eq!(adapter.name(), "feishu");
    }

    #[test]
    fn test_domain_base_url() {
        let mut config = make_config();
        config.domain = "lark".into();
        let adapter = FeishuAdapter::new(config);
        assert_eq!(adapter.api_base_url(), "https://open.larksuite.com");
    }

    #[test]
    fn test_explicit_base_url_wins() {
        let mut config = make_config();
        config.base_url = "https://example.test/".into();
        let adapter = FeishuAdapter::new(config);
        assert_eq!(adapter.api_base_url(), "https://example.test");
    }

    #[test]
    fn test_token_url() {
        let adapter = FeishuAdapter::new(make_config());
        assert_eq!(
            adapter.token_url(),
            "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal"
        );
    }

    #[test]
    fn test_receive_id_type_falls_back_to_chat_id() {
        let mut config = make_config();
        config.receive_id_type = "bad value".into();
        let adapter = FeishuAdapter::new(config);
        assert_eq!(adapter.receive_id_type(), "chat_id");
        assert!(adapter.send_url().ends_with("receive_id_type=chat_id"));
    }

    #[test]
    fn test_take_receiver() {
        let mut adapter = FeishuAdapter::new(make_config());
        assert!(adapter.take_receiver().is_some());
        assert!(adapter.take_receiver().is_none());
    }
}
