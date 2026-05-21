//! Matrix platform adapter.
//!
//! Sends messages to a Matrix room via the Client-Server API.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixAdapterConfig {
    pub homeserver_url: String,
    pub access_token: String,
    pub room_id: String,
}

pub struct MatrixAdapter {
    config: MatrixAdapterConfig,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl MatrixAdapter {
    pub fn new(config: MatrixAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        Self {
            config,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }

    /// Build the URL for sending a room message.
    ///
    /// Uses `PUT /_matrix/client/v3/rooms/{roomId}/send/{txnId}`.
    fn build_send_url(&self, room_id: &str, txn_id: &str) -> String {
        let base = self.config.homeserver_url.trim_end_matches('/');
        // URL-encode the room_id since it contains `:`
        let encoded_room = room_id.replace(':', "%3A");
        format!(
            "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            base, encoded_room, txn_id
        )
    }
}

#[async_trait]
impl PlatformAdapter for MatrixAdapter {
    fn name(&self) -> &str {
        "matrix"
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(homeserver = %self.config.homeserver_url, "Matrix adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        // Use provided chat_id, or fall back to the configured room_id
        let room = if chat_id.is_empty() {
            &self.config.room_id
        } else {
            chat_id
        };

        let txn_id = format!("txn_{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis());
        let url = self.build_send_url(room, &txn_id);

        let body = serde_json::json!({
            "msgtype": "m.text",
            "body": text,
        });

        let resp = self
            .client
            .put(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Matrix send_message failed: status={}, body={}",
                status,
                body_text
            );
        }

        info!(room = room, text_len = text.len(), "Matrix: message sent");
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("Matrix adapter disconnected");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> MatrixAdapterConfig {
        MatrixAdapterConfig {
            homeserver_url: "https://matrix.example.com".into(),
            access_token: "syt_test_token".into(),
            room_id: "!abc:example.com".into(),
        }
    }

    #[test]
    fn test_construction() {
        let adapter = MatrixAdapter::new(make_config());
        assert_eq!(adapter.config.room_id, "!abc:example.com");
        assert_eq!(adapter.config.access_token, "syt_test_token");
    }

    #[test]
    fn test_name() {
        let adapter = MatrixAdapter::new(make_config());
        assert_eq!(adapter.name(), "matrix");
    }

    #[test]
    fn test_build_send_url() {
        let adapter = MatrixAdapter::new(make_config());
        let url = adapter.build_send_url("!abc:example.com", "txn123");
        assert_eq!(
            url,
            "https://matrix.example.com/_matrix/client/v3/rooms/!abc%3Aexample.com/send/m.room.message/txn123"
        );
    }

    #[test]
    fn test_build_send_url_trailing_slash() {
        let config = MatrixAdapterConfig {
            homeserver_url: "https://matrix.example.com/".into(),
            access_token: "token".into(),
            room_id: "!room:server".into(),
        };
        let adapter = MatrixAdapter::new(config);
        let url = adapter.build_send_url("!room:server", "txn1");
        assert!(url.starts_with("https://matrix.example.com/_matrix/client/v3/rooms/"));
        assert!(!url.contains("//_matrix"));
    }
}
