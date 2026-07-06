//! Microsoft Teams Webhook Integration Example (Rust)
//!
//! This example demonstrates how to integrate Hakimi Agent with Microsoft Teams
//! using Outgoing Webhooks and Power Automate Workflows (no Azure Bot registration required).
//!
//! ## Prerequisites
//!
//! 1. Create a Power Automate Workflow in your Teams channel:
//!    - Channel → ... → Workflows → "Post to a channel when a webhook request is received"
//!    - Copy the generated webhook URL
//!
//! 2. Create an Outgoing Webhook in your Team:
//!    - Team → Manage team → Apps → "Create an outgoing webhook"
//!    - Name: AgentBot
//!    - Callback URL: https://your-domain.com/teams/inbound
//!    - Save the security token (HMAC secret, base64-encoded)
//!
//! ## Configuration
//!
//! Set environment variables:
//! ```bash
//! export TEAMS_HMAC_SECRET="<base64_string_from_teams>"
//! export TEAMS_WORKFLOW_URL="https://prod-xx.westus.logic.azure.com/..."
//! ```
//!
//! ## Run
//!
//! ```bash
//! cargo run --example teams_webhook_integration
//! ```

use hakimi_gateway::teams_webhook::{
    AdaptiveCardBuilder, TeamsWebhookAdapter, TeamsWebhookConfig, TeamsWebhookServer,
};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load configuration from environment
    let hmac_secret = env::var("TEAMS_HMAC_SECRET")
        .expect("TEAMS_HMAC_SECRET environment variable required");
    let default_workflow_url = env::var("TEAMS_WORKFLOW_URL")
        .expect("TEAMS_WORKFLOW_URL environment variable required");

    let config = TeamsWebhookConfig {
        hmac_secret,
        default_workflow_url,
        channel_workflows: HashMap::new(),
        bot_id: "hakimi-agent".to_string(),
    };

    // Create and connect the adapter
    let adapter = Arc::new(TeamsWebhookAdapter::new(config));
    let mut adapter_clone = Arc::clone(&adapter);
    adapter_clone
        .connect()
        .await
        .expect("Failed to connect adapter");

    // Start HTTP server in background
    let server_adapter = Arc::clone(&adapter);
    let server = TeamsWebhookServer::new(
        server_adapter,
        "0.0.0.0:8080".parse().expect("Invalid address"),
    );

    tokio::spawn(async move {
        info!("Starting Teams Webhook HTTP server on 0.0.0.0:8080");
        if let Err(e) = server.serve().await {
            error!("Server error: {}", e);
        }
    });

    // Message processing loop
    info!("Teams Webhook integration ready");
    info!("Webhook endpoint: http://localhost:8080/teams/inbound");
    info!("Health check: http://localhost:8080/healthz");

    let mut receiver = adapter
        .take_receiver()
        .expect("Failed to take receiver");

    while let Some(msg) = receiver.recv().await {
        info!(
            "Received message from {} in {}: {}",
            msg.user_id, msg.chat_id, msg.text
        );

        // Spawn async task to process message
        let adapter_clone = Arc::clone(&adapter);
        tokio::spawn(async move {
            if let Err(e) = process_message(adapter_clone, msg).await {
                error!("Failed to process message: {}", e);
            }
        });
    }

    Ok(())
}

/// Process incoming message and send response
async fn process_message(
    adapter: Arc<TeamsWebhookAdapter>,
    msg: hakimi_gateway::GatewayMessage,
) -> anyhow::Result<()> {
    // Simulate agent processing
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Build a rich Adaptive Card response
    let mut builder = AdaptiveCardBuilder::new("Task Completed");
    builder
        .add_text(format!("Your request: \"{}\"", msg.text))
        .add_fact("Status", "✅ Success")
        .add_fact("Processing Time", "2.1s")
        .add_button("View Details", "https://example.com/task/12345");

    let card_json = builder.build();

    // Send the card to Teams via Workflows webhook
    // Note: In real implementation, you'd extract the text from card_json
    // For now, send a simple text response
    adapter
        .send_message(&msg.chat_id, "Task completed! Check the card above.")
        .await?;

    Ok(())
}
