//! WeCom (企业微信) platform adapter.
//!
//! Sends messages via the WeCom application message API. Requires fetching
//! an access token from the WeCom API first.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeComAdapterConfig {
    pub corp_id: String,
    pub agent_id: String,
    pub secret: String,
}

pub struct WeComAdapter {
    config: WeComAdapterConfig,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl WeComAdapter {
    pub fn new(config: WeComAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        Self {
            config,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }

    /// Build the URL for fetching an access token.
    fn token_url(&self) -> String {
        format!(
            "https://qyapi.weixin.qq.com/cgi-bin/gettoken?corpid={}&corpsecret={}",
            self.config.corp_id, self.config.secret
        )
    }

    /// Build the URL for sending an application message.
    fn send_url(&self, access_token: &str) -> String {
        format!(
            "https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={}",
            access_token
        )
    }

    /// Fetch a fresh access token from the WeCom API.
    async fn get_access_token(&self) -> anyhow::Result<String> {
        let url = self.token_url();
        let resp = self.client.get(&url).send().await?;
        let body: serde_json::Value = resp.json().await?;

        if let Some(errcode) = body.get("errcode") {
            if errcode.as_i64().unwrap_or(0) != 0 {
                let errmsg = body
                    .get("errmsg")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                anyhow::bail!("WeCom gettoken error: errcode={}, errmsg={}", errcode, errmsg);
            }
        }

        body.get("access_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("WeCom gettoken: missing access_token in response"))
    }
}

#[async_trait]
impl PlatformAdapter for WeComAdapter {
    fn name(&self) -> &str {
        "wecom"
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(corp_id = %self.config.corp_id, "WeCom adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let access_token = self.get_access_token().await?;
        let url = self.send_url(&access_token);

        let body = serde_json::json!({
            "touser": chat_id,
            "msgtype": "text",
            "agentid": self.config.agent_id,
            "text": {
                "content": text
            }
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "WeCom send_message failed: status={}, body={}",
                status,
                body_text
            );
        }

        info!(chat_id, text_len = text.len(), "WeCom: message sent");
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("WeCom adapter disconnected");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> WeComAdapterConfig {
        WeComAdapterConfig {
            corp_id: "ww_corp123".into(),
            agent_id: "1000002".into(),
            secret: "test_secret".into(),
        }
    }

    #[test]
    fn test_construction() {
        let adapter = WeComAdapter::new(make_config());
        assert_eq!(adapter.config.corp_id, "ww_corp123");
        assert_eq!(adapter.config.agent_id, "1000002");
        assert_eq!(adapter.config.secret, "test_secret");
    }

    #[test]
    fn test_name() {
        let adapter = WeComAdapter::new(make_config());
        assert_eq!(adapter.name(), "wecom");
    }

    #[test]
    fn test_token_url() {
        let adapter = WeComAdapter::new(make_config());
        let url = adapter.token_url();
        assert!(url.contains("corpid=ww_corp123"));
        assert!(url.contains("corpsecret=test_secret"));
        assert!(url.starts_with("https://qyapi.weixin.qq.com/cgi-bin/gettoken"));
    }

    #[test]
    fn test_send_url() {
        let adapter = WeComAdapter::new(make_config());
        let url = adapter.send_url("my_access_token");
        assert_eq!(
            url,
            "https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token=my_access_token"
        );
    }
}
