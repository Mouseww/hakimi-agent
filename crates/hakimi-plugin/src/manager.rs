use std::sync::Arc;

use crate::registry::PluginRegistry;
use crate::{Message, MessageAction, PluginContext, Session, ToolCallAction, ToolCallResultAction};
use hakimi_common::error::Result;

/// 插件管理器（协调插件生命周期和钩子调用）
pub struct PluginManager {
    registry: Arc<PluginRegistry>,
}

impl PluginManager {
    pub fn new(registry: Arc<PluginRegistry>) -> Self {
        Self { registry }
    }

    // === 会话钩子 ===

    pub async fn trigger_session_start(
        &self,
        ctx: &PluginContext,
        session: &Session,
    ) -> Result<()> {
        let plugins = self.registry.all().await;

        for plugin in plugins {
            if let Err(e) = plugin.on_session_start(ctx, session).await {
                tracing::error!(
                    "Plugin '{}' failed on_session_start: {}",
                    plugin.metadata().id,
                    e
                );
            }
        }

        Ok(())
    }

    pub async fn trigger_session_end(&self, ctx: &PluginContext, session: &Session) -> Result<()> {
        let plugins = self.registry.all().await;

        for plugin in plugins {
            if let Err(e) = plugin.on_session_end(ctx, session).await {
                tracing::error!(
                    "Plugin '{}' failed on_session_end: {}",
                    plugin.metadata().id,
                    e
                );
            }
        }

        Ok(())
    }

    // === 消息钩子 ===

    pub async fn trigger_message_before_send(
        &self,
        ctx: &PluginContext,
        mut message: Message,
    ) -> Result<MessageAction> {
        let plugins = self.registry.all().await;

        for plugin in plugins {
            match plugin.on_message_before_send(ctx, message.clone()).await {
                Ok(MessageAction::Continue(msg)) => {
                    message = msg;
                }
                Ok(MessageAction::Reject(reason)) => {
                    tracing::warn!(
                        "Plugin '{}' rejected message: {}",
                        plugin.metadata().id,
                        reason
                    );
                    return Ok(MessageAction::Reject(reason));
                }
                Ok(MessageAction::Replace(msg)) => {
                    tracing::info!("Plugin '{}' replaced message", plugin.metadata().id);
                    return Ok(MessageAction::Replace(msg));
                }
                Err(e) => {
                    tracing::error!(
                        "Plugin '{}' failed on_message_before_send: {}",
                        plugin.metadata().id,
                        e
                    );
                }
            }
        }

        Ok(MessageAction::Continue(message))
    }

    pub async fn trigger_message_after_send(
        &self,
        ctx: &PluginContext,
        message: &Message,
    ) -> Result<()> {
        let plugins = self.registry.all().await;

        for plugin in plugins {
            if let Err(e) = plugin.on_message_after_send(ctx, message).await {
                tracing::error!(
                    "Plugin '{}' failed on_message_after_send: {}",
                    plugin.metadata().id,
                    e
                );
            }
        }

        Ok(())
    }

    pub async fn trigger_message_received(
        &self,
        ctx: &PluginContext,
        mut message: Message,
    ) -> Result<MessageAction> {
        let plugins = self.registry.all().await;

        for plugin in plugins {
            match plugin.on_message_received(ctx, message.clone()).await {
                Ok(MessageAction::Continue(msg)) => {
                    message = msg;
                }
                Ok(MessageAction::Reject(reason)) => {
                    tracing::warn!(
                        "Plugin '{}' rejected received message: {}",
                        plugin.metadata().id,
                        reason
                    );
                    return Ok(MessageAction::Reject(reason));
                }
                Ok(MessageAction::Replace(msg)) => {
                    tracing::info!(
                        "Plugin '{}' replaced received message",
                        plugin.metadata().id
                    );
                    return Ok(MessageAction::Replace(msg));
                }
                Err(e) => {
                    tracing::error!(
                        "Plugin '{}' failed on_message_received: {}",
                        plugin.metadata().id,
                        e
                    );
                }
            }
        }

        Ok(MessageAction::Continue(message))
    }

    // === 工具调用钩子 ===

    pub async fn trigger_tool_call_before(
        &self,
        ctx: &PluginContext,
        tool_name: &str,
        mut params: serde_json::Value,
    ) -> Result<ToolCallAction> {
        let plugins = self.registry.all().await;

        for plugin in plugins {
            match plugin
                .on_tool_call_before(ctx, tool_name, params.clone())
                .await
            {
                Ok(ToolCallAction::Continue(p)) => {
                    params = p;
                }
                Ok(ToolCallAction::Cancel(reason)) => {
                    tracing::warn!(
                        "Plugin '{}' cancelled tool call '{}': {}",
                        plugin.metadata().id,
                        tool_name,
                        reason
                    );
                    return Ok(ToolCallAction::Cancel(reason));
                }
                Err(e) => {
                    tracing::error!(
                        "Plugin '{}' failed on_tool_call_before: {}",
                        plugin.metadata().id,
                        e
                    );
                }
            }
        }

        Ok(ToolCallAction::Continue(params))
    }

    pub async fn trigger_tool_call_after(
        &self,
        ctx: &PluginContext,
        tool_name: &str,
        mut result: serde_json::Value,
    ) -> Result<ToolCallResultAction> {
        let plugins = self.registry.all().await;

        for plugin in plugins {
            match plugin
                .on_tool_call_after(ctx, tool_name, result.clone())
                .await
            {
                Ok(ToolCallResultAction::Continue(r)) => {
                    result = r;
                }
                Ok(ToolCallResultAction::Replace(r)) => {
                    tracing::info!(
                        "Plugin '{}' replaced tool result for '{}'",
                        plugin.metadata().id,
                        tool_name
                    );
                    return Ok(ToolCallResultAction::Replace(r));
                }
                Ok(ToolCallResultAction::Error(err)) => {
                    tracing::error!(
                        "Plugin '{}' marked tool '{}' as failed: {}",
                        plugin.metadata().id,
                        tool_name,
                        err
                    );
                    return Ok(ToolCallResultAction::Error(err));
                }
                Err(e) => {
                    tracing::error!(
                        "Plugin '{}' failed on_tool_call_after: {}",
                        plugin.metadata().id,
                        e
                    );
                }
            }
        }

        Ok(ToolCallResultAction::Continue(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HakimiPlugin, PluginMetadata};
    use async_trait::async_trait;

    struct MockPlugin {
        metadata: PluginMetadata,
        should_reject: bool,
    }

    impl MockPlugin {
        fn new(id: &str, should_reject: bool) -> Self {
            Self {
                metadata: PluginMetadata {
                    id: id.to_string(),
                    name: "Mock Plugin".to_string(),
                    version: "1.0.0".to_string(),
                    author: "Test".to_string(),
                    description: "Mock plugin for testing".to_string(),
                    dependencies: vec![],
                    min_hakimi_version: None,
                },
                should_reject,
            }
        }
    }

    #[async_trait]
    impl HakimiPlugin for MockPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.metadata
        }

        async fn on_message_before_send(
            &self,
            _ctx: &PluginContext,
            message: Message,
        ) -> Result<MessageAction> {
            if self.should_reject {
                Ok(MessageAction::Reject("Rejected by mock plugin".to_string()))
            } else {
                Ok(MessageAction::Continue(message))
            }
        }

        async fn on_tool_call_before(
            &self,
            _ctx: &PluginContext,
            _tool_name: &str,
            params: serde_json::Value,
        ) -> Result<ToolCallAction> {
            if self.should_reject {
                Ok(ToolCallAction::Cancel(
                    "Cancelled by mock plugin".to_string(),
                ))
            } else {
                Ok(ToolCallAction::Continue(params))
            }
        }
    }

    #[tokio::test]
    async fn test_trigger_message_before_send_continue() {
        let registry = Arc::new(PluginRegistry::new());
        let plugin = Arc::new(MockPlugin::new("test.plugin", false));
        registry.register(plugin).await.unwrap();

        let manager = PluginManager::new(registry);
        let ctx = PluginContext {
            session_id: "session1".to_string(),
            user_id: Some("user1".to_string()),
            config: serde_json::json!({}),
        };

        let message = Message {
            id: "msg1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: 0,
        };

        let result = manager
            .trigger_message_before_send(&ctx, message)
            .await
            .unwrap();
        match result {
            MessageAction::Continue(msg) => {
                assert_eq!(msg.content, "Hello");
            }
            _ => panic!("Expected Continue"),
        }
    }

    #[tokio::test]
    async fn test_trigger_message_before_send_reject() {
        let registry = Arc::new(PluginRegistry::new());
        let plugin = Arc::new(MockPlugin::new("test.plugin", true));
        registry.register(plugin).await.unwrap();

        let manager = PluginManager::new(registry);
        let ctx = PluginContext {
            session_id: "session1".to_string(),
            user_id: Some("user1".to_string()),
            config: serde_json::json!({}),
        };

        let message = Message {
            id: "msg1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: 0,
        };

        let result = manager
            .trigger_message_before_send(&ctx, message)
            .await
            .unwrap();
        match result {
            MessageAction::Reject(reason) => {
                assert!(reason.contains("Rejected by mock plugin"));
            }
            _ => panic!("Expected Reject"),
        }
    }

    #[tokio::test]
    async fn test_trigger_tool_call_before_continue() {
        let registry = Arc::new(PluginRegistry::new());
        let plugin = Arc::new(MockPlugin::new("test.plugin", false));
        registry.register(plugin).await.unwrap();

        let manager = PluginManager::new(registry);
        let ctx = PluginContext {
            session_id: "session1".to_string(),
            user_id: Some("user1".to_string()),
            config: serde_json::json!({}),
        };

        let params = serde_json::json!({"key": "value"});
        let result = manager
            .trigger_tool_call_before(&ctx, "test_tool", params.clone())
            .await
            .unwrap();

        match result {
            ToolCallAction::Continue(p) => {
                assert_eq!(p, params);
            }
            _ => panic!("Expected Continue"),
        }
    }

    #[tokio::test]
    async fn test_trigger_tool_call_before_cancel() {
        let registry = Arc::new(PluginRegistry::new());
        let plugin = Arc::new(MockPlugin::new("test.plugin", true));
        registry.register(plugin).await.unwrap();

        let manager = PluginManager::new(registry);
        let ctx = PluginContext {
            session_id: "session1".to_string(),
            user_id: Some("user1".to_string()),
            config: serde_json::json!({}),
        };

        let params = serde_json::json!({"key": "value"});
        let result = manager
            .trigger_tool_call_before(&ctx, "test_tool", params)
            .await
            .unwrap();

        match result {
            ToolCallAction::Cancel(reason) => {
                assert!(reason.contains("Cancelled by mock plugin"));
            }
            _ => panic!("Expected Cancel"),
        }
    }
}
