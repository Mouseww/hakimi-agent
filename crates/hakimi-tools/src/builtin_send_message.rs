use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};
use tracing::debug;

use crate::Tool;

/// A queued outbound message waiting to be picked up by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    /// Target in `platform:chat_id` format (e.g., "telegram:123456789").
    pub target: String,
    /// The message content to send.
    pub message: String,
    /// ID of the session that generated this message.
    pub session_id: String,
    /// ISO 8601 timestamp when the message was queued.
    pub queued_at: String,
}

/// Global outbound message queue shared between tools and the gateway.
pub static MESSAGE_QUEUE: LazyLock<Mutex<VecDeque<QueuedMessage>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// Pop the next message from the outbound queue (non-blocking).
/// Returns `None` if the queue is empty.
pub fn pop_message() -> Option<QueuedMessage> {
    MESSAGE_QUEUE
        .lock()
        .ok()
        .and_then(|mut q| q.pop_front())
}

/// Get the current number of queued messages.
pub fn queue_len() -> usize {
    MESSAGE_QUEUE.lock().map(|q| q.len()).unwrap_or(0)
}

/// Built-in tool for sending messages to external platforms via the gateway queue.
pub struct SendMessageTool;

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn toolset(&self) -> &str {
        "communication"
    }

    fn description(&self) -> &str {
        "Send a message to an external platform. Messages are queued for the gateway to deliver. Target format: 'platform:chat_id' (e.g., 'telegram:123456789')."
    }

    fn emoji(&self) -> &str {
        "\u{1f4e8}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Target destination in 'platform:chat_id' format. Example: 'telegram:123456789'."
                },
                "message": {
                    "type": "string",
                    "description": "The message content to send."
                }
            },
            "required": ["target", "message"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let target = args
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: target".into()))?;

        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: message".into()))?;

        // Validate target format: platform:chat_id
        if !target.contains(':') {
            return Err(HakimiError::Tool(format!(
                "invalid target format '{}'. Expected 'platform:chat_id' (e.g., 'telegram:123456789').",
                target
            )));
        }

        let now = chrono::Utc::now().to_rfc3339();

        let queued = QueuedMessage {
            target: target.to_string(),
            message: message.to_string(),
            session_id: ctx.session_id.clone(),
            queued_at: now,
        };

        debug!(
            target = %target,
            message_len = message.len(),
            session_id = %ctx.session_id,
            "queuing outbound message"
        );

        let mut queue = MESSAGE_QUEUE.lock().map_err(|e| {
            HakimiError::Tool(format!("failed to lock message queue: {e}"))
        })?;

        let queue_size = queue.len();
        queue.push_back(queued);

        Ok(format!(
            "Message queued for delivery to '{target}'. Queue position: {}. Total queued: {}.",
            queue_size + 1,
            queue_size + 1
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            user_id: Some("user-1".to_string()),
            task_id: None,
            workdir: "/tmp".to_string(),
        }
    }

    /// Drain the message queue to avoid cross-test pollution
    fn drain_queue() {
        while pop_message().is_some() {}
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = SendMessageTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "target"));
        assert!(required.iter().any(|v| v == "message"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = SendMessageTool;
        assert_eq!(tool.name(), "send_message");
        assert_eq!(tool.toolset(), "communication");
        assert!(tool.check_available());
        assert_eq!(tool.emoji(), "📨");
    }

    #[tokio::test]
    async fn test_queue_message() {
        drain_queue();

        let ctx = test_ctx();
        let args = json!({
            "target": "telegram:123456789",
            "message": "Hello from the agent!"
        });

        let result = SendMessageTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("queued"));
        assert!(result.contains("telegram:123456789"));

        // Pop the message and verify
        let msg = pop_message().expect("expected a queued message");
        assert_eq!(msg.target, "telegram:123456789");
        assert_eq!(msg.message, "Hello from the agent!");
        assert_eq!(msg.session_id, "test-session");

        drain_queue();
    }

    #[tokio::test]
    async fn test_pop_message() {
        drain_queue();

        let ctx = test_ctx();
        SendMessageTool
            .execute(
                &json!({"target": "discord:abc", "message": "test"}),
                &ctx,
            )
            .await
            .unwrap();

        let msg = pop_message().unwrap();
        assert_eq!(msg.target, "discord:abc");
        assert_eq!(msg.message, "test");

        // Queue should be empty now
        assert!(pop_message().is_none());

        drain_queue();
    }

    #[tokio::test]
    async fn test_queue_multiple_messages() {
        drain_queue();

        let ctx = test_ctx();
        SendMessageTool
            .execute(&json!({"target": "telegram:1", "message": "first"}), &ctx)
            .await
            .unwrap();
        SendMessageTool
            .execute(&json!({"target": "telegram:2", "message": "second"}), &ctx)
            .await
            .unwrap();

        assert_eq!(queue_len(), 2);

        let msg1 = pop_message().unwrap();
        assert_eq!(msg1.message, "first");
        let msg2 = pop_message().unwrap();
        assert_eq!(msg2.message, "second");
        assert!(pop_message().is_none());

        drain_queue();
    }

    #[tokio::test]
    async fn test_invalid_target_format_error() {
        let ctx = test_ctx();
        let args = json!({
            "target": "no-colon-here",
            "message": "hello"
        });
        let err = SendMessageTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("invalid target format"));
    }

    #[tokio::test]
    async fn test_missing_target_error() {
        let ctx = test_ctx();
        let args = json!({"message": "hello"});
        let err = SendMessageTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("target"));
    }

    #[tokio::test]
    async fn test_missing_message_error() {
        let ctx = test_ctx();
        let args = json!({"target": "telegram:123"});
        let err = SendMessageTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("message"));
    }
}
