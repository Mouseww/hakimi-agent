//! Signal platform adapter stub.
//!
//! Placeholder for Signal messenger integration via signal-cli.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalAdapterConfig {
    pub phone_number: String,
    pub signal_cli_path: String,
}

pub struct SignalAdapter {
    config: SignalAdapterConfig,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl SignalAdapter {
    pub fn new(config: SignalAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        Self { config, receiver: Some(receiver) }
    }
}

#[async_trait]
impl PlatformAdapter for SignalAdapter {
    fn name(&self) -> &str { "signal" }

    async fn connect(&mut self) -> anyhow::Result<()> {
        info!(phone = %self.config.phone_number, "Signal adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        info!(chat_id, text_len = text.len(), "Signal: sending message");
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("Signal adapter disconnected");
        Ok(())
    }
}
