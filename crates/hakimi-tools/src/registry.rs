use std::collections::HashMap;
use std::sync::Arc;

use hakimi_common::{HakimiError, Result, ToolDefinition};
use tokio::sync::RwLock;

use crate::trait_def::Tool;

#[derive(Default)]
struct ToolRegistryInner {
    tools: HashMap<String, Arc<dyn Tool>>,
    generation: u64,
}

/// Registry for managing and dispatching tools.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    inner: Arc<RwLock<ToolRegistryInner>>,
}

impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool.
    pub async fn register(&self, tool: Arc<dyn Tool>) {
        let mut inner = self.inner.write().await;
        inner.tools.insert(tool.name().to_string(), tool);
        inner.generation += 1;
    }

    /// Get a tool by name.
    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let inner = self.inner.read().await;
        inner.tools.get(name).cloned()
    }

    /// Get all tool definitions.
    pub async fn get_definitions(&self) -> Vec<ToolDefinition> {
        let inner = self.inner.read().await;
        inner
            .tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.schema(),
            })
            .collect()
    }

    /// List all tool names.
    pub async fn list(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.tools.keys().cloned().collect()
    }

    /// Dispatch a tool call.
    pub async fn dispatch(
        &self,
        name: &str,
        args: &serde_json::Value,
        ctx: &hakimi_common::ToolContext,
    ) -> Result<String> {
        let tool = {
            let inner = self.inner.read().await;
            inner.tools.get(name).cloned()
        };

        if let Some(tool) = tool {
            tool.execute(args, ctx).await
        } else {
            Err(HakimiError::Tool(format!("Tool not found: {}", name)))
        }
    }

    /// Get the current registry generation (incremented on each registration).
    pub async fn generation(&self) -> u64 {
        self.inner.read().await.generation
    }
}
