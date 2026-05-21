//! WeCom (企业微信) platform adapter stub.

use async_trait::async_trait;
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
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl WeComAdapter {
    pub fn new(config: WeComAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        Self { config, receiver: Some(receiver) }
    }
}

#[async_trait]
impl PlatformAdapter for WeComAdapter {
    fn name(&self) -> &str { "wecom" }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(corp_id = %self.config.corp_id, "WeCom adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        info!(chat_id, text_len = text.len(), "WeCom: sending message");
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
