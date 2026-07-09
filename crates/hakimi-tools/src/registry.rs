use std::collections::HashMap;
use std::sync::Arc;

use hakimi_common::{HakimiError, Result, ToolDefinition, ToolOutputConfig, ToolSearchConfig};
use serde_json::{Value as JsonValue, json};
use tokio::sync::RwLock;

use crate::tool_search::{
    TOOL_CALL_NAME, TOOL_DESCRIBE_NAME, TOOL_SEARCH_NAME, ToolAssemblyResult,
    assemble_tool_definitions, build_catalog_from_tools, is_bridge_tool, is_deferrable_tool,
    search_catalog, truncate_chars,
};
use crate::trait_def::Tool;

#[derive(Default)]
struct ToolRegistryInner {
    tools: HashMap<String, Arc<dyn Tool>>,
    generation: u64,
    tool_search: ToolSearchRuntimeConfig,
    tool_output: ToolOutputConfig,
}

#[derive(Clone, Debug)]
struct ToolSearchRuntimeConfig {
    config: ToolSearchConfig,
    context_length: usize,
}

impl Default for ToolSearchRuntimeConfig {
    fn default() -> Self {
        Self {
            config: ToolSearchConfig::default(),
            context_length: 128_000,
        }
    }
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

    /// Configure progressive tool disclosure for future model-facing assemblies.
    pub async fn configure_tool_search(&self, config: ToolSearchConfig, context_length: usize) {
        let mut inner = self.inner.write().await;
        inner.tool_search = ToolSearchRuntimeConfig {
            config: config.normalized(),
            context_length,
        };
    }

    /// Configure framework-level tool result truncation.
    pub async fn configure_tool_output(&self, config: ToolOutputConfig) {
        let mut inner = self.inner.write().await;
        inner.tool_output = config.normalized();
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
                toolset: t.toolset().to_string(),
            })
            .collect()
    }

    /// Get the tool definitions that should be exposed to the model.
    pub async fn get_model_definitions(&self) -> ToolAssemblyResult {
        let (tool_defs, runtime) = {
            let inner = self.inner.read().await;
            let tool_defs = inner
                .tools
                .values()
                .map(|t| ToolDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.schema(),
                    toolset: t.toolset().to_string(),
                })
                .collect::<Vec<_>>();
            (tool_defs, inner.tool_search.clone())
        };
        assemble_tool_definitions(&tool_defs, &runtime.config, runtime.context_length)
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
            let max_bytes = {
                let inner = self.inner.read().await;
                tool.max_result_size()
                    .unwrap_or(inner.tool_output.max_bytes)
            };
            let result = tool.execute(args, ctx).await?;
            Ok(truncate_tool_output(result, max_bytes))
        } else if is_bridge_tool(name) {
            self.dispatch_tool_search_bridge(name, args, ctx).await
        } else {
            Err(HakimiError::ToolSimple(format!("Tool not found: {}", name)))
        }
    }

    async fn dispatch_tool_search_bridge(
        &self,
        name: &str,
        args: &JsonValue,
        ctx: &hakimi_common::ToolContext,
    ) -> Result<String> {
        match name {
            TOOL_SEARCH_NAME => self.dispatch_tool_search(args).await,
            TOOL_DESCRIBE_NAME => self.dispatch_tool_describe(args).await,
            TOOL_CALL_NAME => self.dispatch_deferred_tool(args, ctx).await,
            _ => Err(HakimiError::ToolSimple(format!("Tool not found: {}", name))),
        }
    }

    async fn dispatch_tool_search(&self, args: &JsonValue) -> Result<String> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim();
        if query.is_empty() {
            return Ok(json!({"error": "query is required"}).to_string());
        }

        let (catalog, config) = {
            let inner = self.inner.read().await;
            (
                build_catalog_from_tools(inner.tools.values(), true),
                inner.tool_search.config.clone().normalized(),
            )
        };
        let requested_limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(config.search_default_limit);
        let limit = requested_limit.clamp(1, config.max_search_limit);
        let matches = search_catalog(&catalog, query, limit)
            .into_iter()
            .map(|hit| {
                json!({
                    "name": hit.name,
                    "source": hit.source,
                    "source_name": hit.source_name,
                    "description": truncate_chars(&hit.description, 400),
                })
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "query": query,
            "total_available": catalog.len(),
            "matches": matches,
        })
        .to_string())
    }

    async fn dispatch_tool_describe(&self, args: &JsonValue) -> Result<String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim();
        if name.is_empty() {
            return Ok(json!({"error": "name is required"}).to_string());
        }

        let tool = {
            let inner = self.inner.read().await;
            inner.tools.get(name).cloned()
        };
        let Some(tool) = tool else {
            return Ok(json!({"error": format!("'{name}' is not currently available. Re-run tool_search to refresh.")}).to_string());
        };
        if !is_deferrable_tool(tool.name(), tool.toolset()) {
            return Ok(json!({
                "error": format!("'{name}' is not a deferrable tool. If it appears in the tools list already, call it directly.")
            })
            .to_string());
        }

        Ok(json!({
            "name": tool.name(),
            "description": tool.description(),
            "parameters": tool.schema(),
        })
        .to_string())
    }

    async fn dispatch_deferred_tool(
        &self,
        args: &JsonValue,
        ctx: &hakimi_common::ToolContext,
    ) -> Result<String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim();
        if name.is_empty() {
            return Ok(json!({"error": "tool_call requires a 'name' argument"}).to_string());
        }
        if is_bridge_tool(name) {
            return Ok(json!({"error": format!("tool_call cannot invoke '{name}' because it is a bridge tool")}).to_string());
        }

        let tool = {
            let inner = self.inner.read().await;
            inner.tools.get(name).cloned()
        };
        let Some(tool) = tool else {
            return Ok(json!({"error": format!("'{name}' is not currently available. Re-run tool_search to refresh.")}).to_string());
        };
        if !is_deferrable_tool(tool.name(), tool.toolset()) {
            return Ok(json!({
                "error": format!("'{name}' is not a deferrable tool. If it appears in the model-facing tools list already, call it directly instead of via tool_call.")
            })
            .to_string());
        }

        let Ok(deferred_args) = parse_deferred_arguments(args.get("arguments")) else {
            return Ok(
                json!({"error": "tool_call 'arguments' must be an object or JSON object string"})
                    .to_string(),
            );
        };
        let max_bytes = {
            let inner = self.inner.read().await;
            tool.max_result_size()
                .unwrap_or(inner.tool_output.max_bytes)
        };
        let result = tool.execute(&deferred_args, ctx).await?;
        Ok(truncate_tool_output(result, max_bytes))
    }

    /// Get the current registry generation (incremented on each registration).
    pub async fn generation(&self) -> u64 {
        self.inner.read().await.generation
    }
}

fn truncate_tool_output(output: String, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output;
    }

    let mut end = max_bytes.min(output.len());
    while !output.is_char_boundary(end) {
        end -= 1;
    }

    let omitted = output.len().saturating_sub(end);
    format!(
        "{}\n\n[Tool output truncated: omitted {omitted} bytes; increase tools.output.max_bytes or the tool-specific max_result_size to see more.]",
        &output[..end]
    )
}

fn parse_deferred_arguments(raw: Option<&JsonValue>) -> std::result::Result<JsonValue, ()> {
    match raw {
        Some(value) if value.is_object() => Ok(value.clone()),
        Some(JsonValue::String(value)) => serde_json::from_str::<JsonValue>(value)
            .ok()
            .filter(JsonValue::is_object)
            .ok_or(()),
        Some(JsonValue::Null) | None => Ok(json!({})),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use hakimi_common::ToolSearchMode;

    struct NamedTool {
        name: String,
        toolset: String,
        description: String,
        output: Option<String>,
        max_result_size: Option<usize>,
    }

    impl NamedTool {
        fn new(name: &str, toolset: &str, description: &str) -> Self {
            Self {
                name: name.to_string(),
                toolset: toolset.to_string(),
                description: description.to_string(),
                output: None,
                max_result_size: None,
            }
        }

        fn with_output(mut self, output: impl Into<String>) -> Self {
            self.output = Some(output.into());
            self
        }

        fn with_max_result_size(mut self, max_result_size: usize) -> Self {
            self.max_result_size = Some(max_result_size);
            self
        }
    }

    #[async_trait]
    impl Tool for NamedTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn toolset(&self) -> &str {
            &self.toolset
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn schema(&self) -> JsonValue {
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "repo": {"type": "string"}
                }
            })
        }

        fn max_result_size(&self) -> Option<usize> {
            self.max_result_size
        }

        async fn execute(
            &self,
            args: &JsonValue,
            ctx: &hakimi_common::ToolContext,
        ) -> Result<String> {
            Ok(self.output.clone().unwrap_or_else(|| {
                json!({"tool": self.name, "args": args, "workdir": ctx.workdir}).to_string()
            }))
        }
    }

    fn def(name: &str, toolset: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("Description for {name}"),
            parameters: json!({"type": "object", "properties": {}}),
            toolset: toolset.to_string(),
        }
    }

    #[test]
    fn core_tools_never_defer_even_when_tool_search_forced_on() {
        let defs = vec![def("terminal", "shell"), def("read_file", "file")];
        let result = assemble_tool_definitions(
            &defs,
            &ToolSearchConfig {
                enabled: ToolSearchMode::On,
                ..ToolSearchConfig::default()
            },
            128_000,
        );

        assert!(!result.activated);
        assert_eq!(result.tool_defs.len(), 2);
        assert!(result.tool_defs.iter().any(|tool| tool.name == "terminal"));
    }

    #[test]
    fn deferrable_plugin_tools_are_replaced_with_bridge_tools() {
        let defs = vec![
            def("terminal", "shell"),
            def("github_create_issue", "mcp-github"),
            def("weather_lookup", "http"),
        ];
        let result = assemble_tool_definitions(
            &defs,
            &ToolSearchConfig {
                enabled: ToolSearchMode::On,
                ..ToolSearchConfig::default()
            },
            128_000,
        );
        let names = result
            .tool_defs
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();

        assert!(result.activated);
        assert_eq!(result.deferred_count, 2);
        assert!(names.contains(&"terminal"));
        assert!(names.contains(&TOOL_SEARCH_NAME));
        assert!(names.contains(&TOOL_DESCRIBE_NAME));
        assert!(names.contains(&TOOL_CALL_NAME));
        assert!(!names.contains(&"github_create_issue"));
        assert!(!names.contains(&"weather_lookup"));
    }

    #[test]
    fn auto_mode_skips_tiny_deferred_schema_below_threshold() {
        let defs = vec![def("github_create_issue", "mcp-github")];
        let result = assemble_tool_definitions(&defs, &ToolSearchConfig::default(), 128_000);

        assert!(!result.activated);
        assert_eq!(result.deferred_count, 1);
        assert_eq!(result.tool_defs[0].name, "github_create_issue");
    }

    #[test]
    fn search_catalog_prefers_relevant_tool() {
        let github = Arc::new(NamedTool::new(
            "github_create_issue",
            "mcp-github",
            "Open a new issue in a GitHub repository",
        )) as Arc<dyn Tool>;
        let slack = Arc::new(NamedTool::new(
            "slack_send_message",
            "mcp-slack",
            "Post a message into a Slack channel",
        )) as Arc<dyn Tool>;
        let tools = [github, slack];
        let catalog = build_catalog_from_tools(tools.iter(), true);
        let hits = search_catalog(&catalog, "create github issue", 2);

        assert_eq!(hits[0].name, "github_create_issue");
    }

    #[test]
    fn parses_tool_call_arguments_object_json_string_and_null() {
        assert_eq!(
            parse_deferred_arguments(Some(&json!({"repo": "owner/repo"}))).unwrap()["repo"],
            "owner/repo"
        );
        assert_eq!(
            parse_deferred_arguments(Some(&json!("{\"repo\":\"owner/repo\"}"))).unwrap()["repo"],
            "owner/repo"
        );
        assert_eq!(
            parse_deferred_arguments(Some(&JsonValue::Null)).unwrap(),
            json!({})
        );
        assert!(parse_deferred_arguments(Some(&json!("not-json"))).is_err());
        assert!(parse_deferred_arguments(Some(&json!("[]"))).is_err());
    }

    #[test]
    fn truncate_tool_output_keeps_short_output() {
        assert_eq!(truncate_tool_output("short".to_string(), 50), "short");
    }

    #[test]
    fn truncate_tool_output_is_utf8_safe() {
        let output = "甲乙丙丁戊".to_string();
        let truncated = truncate_tool_output(output, 7);

        assert!(truncated.starts_with("甲乙"));
        assert!(truncated.contains("[Tool output truncated: omitted"));
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[tokio::test]
    async fn dispatch_truncates_by_configured_default_limit() {
        let registry = ToolRegistry::new();
        registry
            .configure_tool_output(ToolOutputConfig { max_bytes: 8 })
            .await;
        registry
            .register(Arc::new(
                NamedTool::new("verbose_tool", "plugin", "Verbose tool")
                    .with_output("abcdefghijklmnopqrstuvwxyz"),
            ))
            .await;

        let result = registry
            .dispatch("verbose_tool", &json!({}), &Default::default())
            .await
            .unwrap();

        assert!(result.starts_with("abcdefgh"));
        assert!(result.contains("omitted 18 bytes"));
        assert!(result.contains("tools.output.max_bytes"));
    }

    #[tokio::test]
    async fn dispatch_uses_tool_specific_result_limit() {
        let registry = ToolRegistry::new();
        registry
            .configure_tool_output(ToolOutputConfig { max_bytes: 20 })
            .await;
        registry
            .register(Arc::new(
                NamedTool::new("small_tool", "plugin", "Small tool")
                    .with_output("abcdefghij")
                    .with_max_result_size(4),
            ))
            .await;

        let result = registry
            .dispatch("small_tool", &json!({}), &Default::default())
            .await
            .unwrap();

        assert!(result.starts_with("abcd"));
        assert!(result.contains("omitted 6 bytes"));
    }

    #[tokio::test]
    async fn bridge_search_describe_and_call_use_live_registry() {
        let registry = ToolRegistry::new();
        registry
            .register(Arc::new(NamedTool::new(
                "github_create_issue",
                "mcp-github",
                "Open a new issue in a GitHub repository",
            )))
            .await;

        let search = registry
            .dispatch(
                TOOL_SEARCH_NAME,
                &json!({"query": "github issue"}),
                &Default::default(),
            )
            .await
            .unwrap();
        let search_json: JsonValue = serde_json::from_str(&search).unwrap();
        assert_eq!(search_json["total_available"], 1);
        assert_eq!(search_json["matches"][0]["name"], "github_create_issue");

        let describe = registry
            .dispatch(
                TOOL_DESCRIBE_NAME,
                &json!({"name": "github_create_issue"}),
                &Default::default(),
            )
            .await
            .unwrap();
        let describe_json: JsonValue = serde_json::from_str(&describe).unwrap();
        assert_eq!(describe_json["name"], "github_create_issue");
        assert!(describe_json["parameters"]["properties"]["repo"].is_object());

        let called = registry
            .dispatch(
                TOOL_CALL_NAME,
                &json!({"name": "github_create_issue", "arguments": {"repo": "owner/repo"}}),
                &hakimi_common::ToolContext {
                    workdir: "workspace".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let called_json: JsonValue = serde_json::from_str(&called).unwrap();
        assert_eq!(called_json["tool"], "github_create_issue");
        assert_eq!(called_json["args"]["repo"], "owner/repo");
        assert_eq!(called_json["workdir"], "workspace");
    }
}
