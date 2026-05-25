use crate::provider::KnowledgeProvider;
use async_trait::async_trait;
use hakimi_common::{Result, ToolContext, ToolDefinition};
use hakimi_context::MemoryProvider;
use hakimi_tools::Tool;
use serde_json::Value as JsonValue;
use std::sync::Arc;

/// Adapter exposing a [`KnowledgeProvider`] tool definition as a runtime tool.
pub struct KnowledgeTool {
    provider: Arc<KnowledgeProvider>,
    definition: ToolDefinition,
}

impl KnowledgeTool {
    pub fn new(provider: Arc<KnowledgeProvider>, definition: ToolDefinition) -> Self {
        Self {
            provider,
            definition,
        }
    }
}

#[async_trait]
impl Tool for KnowledgeTool {
    fn name(&self) -> &str {
        &self.definition.name
    }

    fn toolset(&self) -> &str {
        "knowledge"
    }

    fn description(&self) -> &str {
        &self.definition.description
    }

    fn emoji(&self) -> &str {
        "🧠"
    }

    fn schema(&self) -> JsonValue {
        self.definition.parameters.clone()
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        self.provider
            .handle_tool_call(&self.definition.name, args)
            .await
    }
}
