//! Matrix platform adapter stub.

use async_trait::async_trait;
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
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl MatrixAdapter {
    pub fn new(config: MatrixAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        Self { config, receiver: Some(receiver) }
    }
}

#[async_trait]
impl PlatformAdapter for MatrixAdapter {
    fn name(&self) -> &str { "matrix" }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(homeserver = %self.config.homeserver_url, "Matrix adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        info!(chat_id, text_len = text.len(), "Matrix: sending message");
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
