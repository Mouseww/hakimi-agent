//! DingTalk (钉钉) platform adapter stub.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkAdapterConfig {
    pub webhook_url: String,
    pub secret: Option<String>,
}

pub struct DingTalkAdapter {
    config: DingTalkAdapterConfig,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl DingTalkAdapter {
    pub fn new(config: DingTalkAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        Self { config, receiver: Some(receiver) }
    }
}

#[async_trait]
impl PlatformAdapter for DingTalkAdapter {
    fn name(&self) -> &str { "dingtalk" }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!("DingTalk adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        info!(chat_id, text_len = text.len(), "DingTalk: sending message");
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("DingTalk adapter disconnected");
        Ok(())
    }
}
