# Hakimi Plugin System

A flexible plugin system for Hakimi Agent that allows third-party developers to extend functionality without modifying core code.

## Features

- **Async Plugin Interface**: All plugin hooks are async for non-blocking operation
- **Lifecycle Management**: Initialize and shutdown hooks for plugins
- **Message Hooks**: Intercept and modify messages before/after sending
- **Tool Call Hooks**: Intercept and modify tool calls and their results
- **Session Hooks**: React to session start and end events
- **Dependency Management**: Automatic dependency checking between plugins
- **Metadata Support**: Version, author, description, and minimum Hakimi version

## Plugin Trait

```rust
use hakimi_plugin::{HakimiPlugin, PluginMetadata};

#[async_trait]
impl HakimiPlugin for MyPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    
    // Optional: Initialize plugin
    async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }
    
    // Optional: Session hooks
    async fn on_session_start(&self, ctx: &PluginContext, session: &Session) -> Result<()> {
        Ok(())
    }
    
    // Optional: Message hooks
    async fn on_message_before_send(&self, ctx: &PluginContext, message: Message) -> Result<MessageAction> {
        Ok(MessageAction::Continue(message))
    }
    
    // Optional: Tool call hooks
    async fn on_tool_call_before(&self, ctx: &PluginContext, tool_name: &str, params: serde_json::Value) -> Result<ToolCallAction> {
        Ok(ToolCallAction::Continue(params))
    }
}
```

## Example Plugins

### Logger Plugin

```rust
use async_trait::async_trait;
use hakimi_plugin::*;

struct LoggerPlugin {
    metadata: PluginMetadata,
}

impl LoggerPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "com.hakimi.logger".to_string(),
                name: "Logger Plugin".to_string(),
                version: "1.0.0".to_string(),
                author: "Hakimi Team".to_string(),
                description: "Logs all session activities".to_string(),
                dependencies: vec![],
                min_hakimi_version: Some("0.5.0".to_string()),
            },
        }
    }
}

#[async_trait]
impl HakimiPlugin for LoggerPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    
    async fn on_message_after_send(&self, ctx: &PluginContext, message: &Message) -> Result<()> {
        tracing::info!(
            "[LoggerPlugin] Message sent in session {}: {} chars",
            ctx.session_id,
            message.content.len()
        );
        Ok(())
    }
}
```

### Message Filter Plugin

```rust
struct FilterPlugin {
    metadata: PluginMetadata,
    blocked_words: Vec<String>,
}

#[async_trait]
impl HakimiPlugin for FilterPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    
    async fn on_message_before_send(&self, _ctx: &PluginContext, message: Message) -> Result<MessageAction> {
        for word in &self.blocked_words {
            if message.content.contains(word) {
                return Ok(MessageAction::Reject(format!("Contains blocked word: {}", word)));
            }
        }
        Ok(MessageAction::Continue(message))
    }
}
```

## Plugin Registry

```rust
use hakimi_plugin::{PluginRegistry, HakimiPlugin};
use std::sync::Arc;

// Create registry
let registry = Arc::new(PluginRegistry::new());

// Register plugin
let plugin = Arc::new(MyPlugin::new());
registry.register(plugin).await?;

// List plugins
let plugins = registry.list().await;

// Unregister plugin
registry.unregister("com.example.myplugin").await?;
```

## Plugin Manager

```rust
use hakimi_plugin::{PluginManager, PluginContext};

// Create manager
let manager = PluginManager::new(registry);

// Trigger hooks
let ctx = PluginContext {
    session_id: "session-123".to_string(),
    user_id: Some("user-456".to_string()),
    config: serde_json::json!({}),
};

// Message hook example
let message = Message { /* ... */ };
match manager.trigger_message_before_send(&ctx, message).await? {
    MessageAction::Continue(msg) => { /* send message */ }
    MessageAction::Reject(reason) => { /* handle rejection */ }
    MessageAction::Replace(msg) => { /* send replacement */ }
}

// Tool call hook example
let params = serde_json::json!({"query": "test"});
match manager.trigger_tool_call_before(&ctx, "search", params).await? {
    ToolCallAction::Continue(p) => { /* execute tool */ }
    ToolCallAction::Cancel(reason) => { /* handle cancellation */ }
}
```

## Hook Types

### Message Hooks

- **on_message_before_send**: Called before sending a message (can modify/reject)
- **on_message_after_send**: Called after sending a message (read-only)
- **on_message_received**: Called when receiving a message (can modify/reject)

### Tool Call Hooks

- **on_tool_call_before**: Called before executing a tool (can modify/cancel)
- **on_tool_call_after**: Called after tool execution (can modify/replace/error)

### Session Hooks

- **on_session_start**: Called when a session starts
- **on_session_end**: Called when a session ends

## Dependency Management

Plugins can declare dependencies on other plugins:

```rust
PluginMetadata {
    id: "com.example.advanced".to_string(),
    dependencies: vec!["com.example.base".to_string()],
    // ...
}
```

The registry ensures dependencies are loaded before dependent plugins and prevents unloading plugins that are depended upon.

## Testing

Run tests:

```bash
cargo test --package hakimi-plugin
```

14 tests covering:
- Plugin registration and unregistration
- Dependency checking
- Message hook actions (continue, reject, replace)
- Tool call hook actions (continue, cancel)
- Plugin metadata serialization

## License

MIT
