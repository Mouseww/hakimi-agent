//! Microsoft Teams Webhook integration.
//!
//! Provides bidirectional Teams integration without Azure Bot registration:
//! - **Inbound**: Teams Outgoing Webhook POSTs to HTTP endpoint
//! - **Outbound**: POST Adaptive Cards to Power Automate Workflows webhook URLs

mod adapter;
mod server;

pub use adapter::{
    AdaptiveCardBuilder, TeamsChannelData, TeamsFrom, TeamsInboundActivity, TeamsWebhookAdapter,
    TeamsWebhookConfig,
};
pub use server::TeamsWebhookServer;
