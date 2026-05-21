use std::collections::HashMap;
use std::sync::Arc;

use hakimi_common::{HakimiError, Result, ToolContext, ToolDefinition};
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::Tool;

/// A thread-safe registry of tools that supports dynamic registration and dispatch.
#[derive(Clone)]
pub struct ToolRegistry {
    inner: Arc<RwLock<ToolRegistryInner>>,
}

struct ToolRegistryInner {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Incremented on every register/deregister operation for cache invalidation.
    generation: u64,
}

impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ToolRegistryInner {
                tools: HashMap::new(),
                generation: 0,
            })),
        }
    }

    /// Register a tool. Overwrites any existing tool with the same name.
    pub async fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        info!(tool = %name, toolset = %tool.toolset(), "registering tool");
        let mut inner = self.inner.write().await;
        inner.tools.insert(name, tool);
        inner.generation += 1;
    }

    /// Remove a tool by name. Returns `true` if the tool was found and removed.
    pub async fn deregister(&self, name: &str) -> bool {
        let mut inner = self.inner.write().await;
        let removed = inner.tools.remove(name).is_some();
        if removed {
            info!(tool = %name, "deregistered tool");
            inner.generation += 1;
        } else {
            warn!(tool = %name, "attempted to deregister unknown tool");
        }
        removed
    }

    /// Get a clone of the Arc to a tool by name.
    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let inner = self.inner.read().await;
        inner.tools.get(name).cloned()
    }

    /// List the names of all registered tools.
    pub async fn list(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.tools.keys().cloned().collect()
    }

    /// Get `ToolDefinition`s for all registered and available tools.
    pub async fn get_definitions(&self) -> Vec<ToolDefinition> {
        let inner = self.inner.read().await;
        inner
            .tools
            .values()
            .filter(|t| t.check_available())
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.schema(),
            })
            .collect()
    }

    /// Dispatch a tool call by name.
    pub async fn dispatch(
        &self,
        name: &str,
        args: &JsonValue,
        ctx: &ToolContext,
    ) -> Result<String> {
        let tool = {
            let inner = self.inner.read().await;
            inner.tools.get(name).cloned()
        };

        let tool = tool.ok_or_else(|| {
            debug!(tool = %name, "tool not found in registry");
            HakimiError::Tool(format!("unknown tool: {name}"))
        })?;

        if !tool.check_available() {
            return Err(HakimiError::Tool(format!(
                "tool '{name}' is currently unavailable"
            )));
        }

        debug!(tool = %name, "dispatching tool execution");
        let result = tool.execute(args, ctx).await?;

        // Truncate if max_result_size is set
        if let Some(max) = tool.max_result_size() {
            if result.len() > max {
                warn!(tool = %name, len = result.len(), max, "truncating tool result");
                let mut truncated = String::with_capacity(max + 20);
                truncated.push_str(&result[..max]);
                truncated.push_str("\n[truncated]");
                return Ok(truncated);
            }
        }

        Ok(result)
    }

    /// Returns the current generation counter value.
    pub async fn generation(&self) -> u64 {
        let inner = self.inner.read().await;
        inner.generation
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    /// A simple mock tool for testing the registry.
    struct MockTool {
        tool_name: String,
    }

    impl MockTool {
        fn new(name: &str) -> Self {
            Self {
                tool_name: name.to_string(),
            }
        }
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.tool_name
        }

        fn toolset(&self) -> &str {
            "test"
        }

        fn description(&self) -> &str {
            "A mock tool for testing"
        }

        fn schema(&self) -> JsonValue {
            json!({"type": "object", "properties": {}})
        }

        async fn execute(
            &self,
            _args: &JsonValue,
            _ctx: &hakimi_common::ToolContext,
        ) -> hakimi_common::Result<String> {
            Ok(format!("{}: executed", self.tool_name))
        }
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(MockTool::new("mock1"));
        registry.register(tool).await;

        let got = registry.get("mock1").await;
        assert!(got.is_some());
        assert_eq!(got.unwrap().name(), "mock1");
    }

    #[tokio::test]
    async fn test_get_nonexistent_returns_none() {
        let registry = ToolRegistry::new();
        assert!(registry.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_deregister() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("mock1"))).await;

        assert!(registry.deregister("mock1").await);
        assert!(registry.get("mock1").await.is_none());
        // Deregistering again returns false
        assert!(!registry.deregister("mock1").await);
    }

    #[tokio::test]
    async fn test_list_tools() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("a"))).await;
        registry.register(Arc::new(MockTool::new("b"))).await;

        let mut names = registry.list().await;
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn test_get_definitions() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("mock1"))).await;

        let defs = registry.get_definitions().await;
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "mock1");
        assert_eq!(defs[0].description, "A mock tool for testing");
    }

    #[tokio::test]
    async fn test_dispatch() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("mock1"))).await;

        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };

        let result = registry.dispatch("mock1", &json!({}), &ctx).await.unwrap();
        assert_eq!(result, "mock1: executed");
    }

    #[tokio::test]
    async fn test_dispatch_unknown_tool_error() {
        let registry = ToolRegistry::new();
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };

        let err = registry
            .dispatch("nonexistent", &json!({}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("unknown tool"));
    }

    #[tokio::test]
    async fn test_overwrite_tool() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("mock1"))).await;
        registry.register(Arc::new(MockTool::new("mock1"))).await;

        let names = registry.list().await;
        assert_eq!(names.len(), 1);
    }

    #[tokio::test]
    async fn test_generation_increments() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.generation().await, 0);

        registry.register(Arc::new(MockTool::new("a"))).await;
        assert_eq!(registry.generation().await, 1);

        registry.register(Arc::new(MockTool::new("b"))).await;
        assert_eq!(registry.generation().await, 2);

        registry.deregister("a").await;
        assert_eq!(registry.generation().await, 3);
    }
}
