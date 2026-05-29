use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use hakimi_common::{ToolDefinition, ToolSearchConfig, ToolSearchMode};
use serde_json::{Value as JsonValue, json};

use crate::Tool;

pub const TOOL_SEARCH_NAME: &str = "tool_search";
pub const TOOL_DESCRIBE_NAME: &str = "tool_describe";
pub const TOOL_CALL_NAME: &str = "tool_call";

const CHARS_PER_TOKEN: f64 = 4.0;

const CORE_TOOL_NAMES: &[&str] = &[
    "browser_back",
    "browser_click",
    "browser_console",
    "browser_dialog",
    "browser_get_images",
    "browser_navigate",
    "browser_press",
    "browser_screenshot",
    "browser_scroll",
    "browser_snapshot",
    "browser_type",
    "checkpoint",
    "clarify",
    "code_exec",
    "cronjob",
    "delegate_task",
    "ha_call_service",
    "ha_get_state",
    "ha_list_entities",
    "ha_list_services",
    "fts_search",
    "image_describe",
    "image_generate",
    "knowledge_add_entity",
    "knowledge_add_relation",
    "knowledge_get_context",
    "knowledge_list",
    "knowledge_search",
    "knowledge_stats",
    "memory",
    "memory_list",
    "memory_read",
    "memory_search",
    "memory_write",
    "patch",
    "process",
    "read_file",
    "search_files",
    "send_message",
    "session_search",
    "skill_manage",
    "terminal",
    "text_to_speech",
    "todo",
    "transcribe_audio",
    "video_analyze",
    "vision_analyze",
    "web_extract",
    "web_search",
    "write_file",
];

#[derive(Debug, Clone, PartialEq)]
pub struct ToolAssemblyResult {
    pub tool_defs: Vec<ToolDefinition>,
    pub activated: bool,
    pub deferred_count: usize,
    pub deferred_tokens: usize,
    pub threshold_tokens: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSearchHit {
    pub name: String,
    pub description: String,
    pub source: String,
    pub source_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CatalogEntry {
    name: String,
    description: String,
    source: String,
    source_name: String,
    tokens: Vec<String>,
}

pub fn assemble_tool_definitions(
    tool_defs: &[ToolDefinition],
    config: &ToolSearchConfig,
    context_length: usize,
) -> ToolAssemblyResult {
    let config = config.normalized();
    let incoming = tool_defs
        .iter()
        .filter(|tool| !is_bridge_tool(&tool.name))
        .cloned()
        .collect::<Vec<_>>();
    let (visible, deferrable): (Vec<_>, Vec<_>) = incoming
        .iter()
        .cloned()
        .partition(|tool| !is_deferrable_tool(&tool.name, &tool.toolset));

    if deferrable.is_empty() {
        return ToolAssemblyResult {
            tool_defs: incoming,
            activated: false,
            deferred_count: 0,
            deferred_tokens: 0,
            threshold_tokens: threshold_tokens(context_length, config.threshold_pct),
        };
    }

    let deferred_tokens = estimate_tokens_from_definitions(&deferrable);
    let threshold_tokens = threshold_tokens(context_length, config.threshold_pct);
    if !should_activate_tool_search(&config, deferred_tokens, context_length) {
        return ToolAssemblyResult {
            tool_defs: incoming,
            activated: false,
            deferred_count: deferrable.len(),
            deferred_tokens,
            threshold_tokens,
        };
    }

    let mut assembled = visible;
    assembled.extend(bridge_tool_definitions(deferrable.len()));
    ToolAssemblyResult {
        tool_defs: assembled,
        activated: true,
        deferred_count: deferrable.len(),
        deferred_tokens,
        threshold_tokens,
    }
}

fn should_activate_tool_search(
    config: &ToolSearchConfig,
    deferred_tokens: usize,
    context_length: usize,
) -> bool {
    if deferred_tokens == 0 {
        return false;
    }
    match config.enabled {
        ToolSearchMode::Off => false,
        ToolSearchMode::On => true,
        ToolSearchMode::Auto if context_length == 0 => deferred_tokens >= 20_000,
        ToolSearchMode::Auto => {
            deferred_tokens >= threshold_tokens(context_length, config.threshold_pct)
        }
    }
}

fn threshold_tokens(context_length: usize, threshold_pct: f64) -> usize {
    ((context_length as f64) * (threshold_pct / 100.0)).floor() as usize
}

fn estimate_tokens_from_definitions(tool_defs: &[ToolDefinition]) -> usize {
    let chars = tool_defs
        .iter()
        .map(|tool| {
            serde_json::to_string(tool)
                .map(|s| s.len())
                .unwrap_or_else(|_| tool.name.len() + tool.description.len())
        })
        .sum::<usize>();
    ((chars as f64) / CHARS_PER_TOKEN).ceil() as usize
}

fn bridge_tool_definitions(deferred_count: usize) -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: TOOL_SEARCH_NAME.to_string(),
            description: format!(
                "Search {deferred_count} additional MCP/plugin tools loaded on demand. Returns matches with name and description. Follow with `{TOOL_DESCRIBE_NAME}` for a full schema, then `{TOOL_CALL_NAME}` to invoke."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keywords describing the capability you need."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of matches to return."
                    }
                },
                "required": ["query"]
            }),
            toolset: "tool_search".to_string(),
        },
        ToolDefinition {
            name: TOOL_DESCRIBE_NAME.to_string(),
            description: format!(
                "Load the full JSON schema for one tool returned by `{TOOL_SEARCH_NAME}`."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Exact tool name returned by tool_search."
                    }
                },
                "required": ["name"]
            }),
            toolset: "tool_search".to_string(),
        },
        ToolDefinition {
            name: TOOL_CALL_NAME.to_string(),
            description: format!(
                "Invoke a deferred tool by exact name with arguments matching its `{TOOL_DESCRIBE_NAME}` schema."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Exact deferred tool name to invoke."
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments for the deferred tool."
                    }
                },
                "required": ["name", "arguments"]
            }),
            toolset: "tool_search".to_string(),
        },
    ]
}

pub fn is_bridge_tool(name: &str) -> bool {
    matches!(name, TOOL_SEARCH_NAME | TOOL_DESCRIBE_NAME | TOOL_CALL_NAME)
}

fn is_core_tool_name(name: &str) -> bool {
    CORE_TOOL_NAMES.contains(&name)
}

pub fn is_deferrable_tool(name: &str, toolset: &str) -> bool {
    if is_bridge_tool(name) || is_core_tool_name(name) {
        return false;
    }
    matches!(toolset, "mcp" | "http" | "plugin") || toolset.starts_with("mcp-")
}

pub(crate) fn build_catalog_from_tools<'a>(
    tools: impl Iterator<Item = &'a Arc<dyn Tool>>,
    deferrable_only: bool,
) -> Vec<CatalogEntry> {
    tools
        .filter_map(|tool| {
            if deferrable_only && !is_deferrable_tool(tool.name(), tool.toolset()) {
                return None;
            }
            let parameters = tool.schema();
            let tokens = tokenize(&entry_search_text(
                tool.name(),
                tool.description(),
                &parameters,
            ));
            Some(CatalogEntry {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                source: source_kind(tool.toolset()).to_string(),
                source_name: tool.toolset().to_string(),
                tokens,
            })
        })
        .collect()
}

fn source_kind(toolset: &str) -> &'static str {
    if toolset == "mcp" || toolset.starts_with("mcp-") {
        "mcp"
    } else if toolset == "http" || toolset == "plugin" {
        "plugin"
    } else {
        "other"
    }
}

fn entry_search_text(name: &str, description: &str, parameters: &JsonValue) -> String {
    let name_words = name.replace(['_', '.', '-', ':'], " ");
    let param_names = parameters
        .get("properties")
        .and_then(|v| v.as_object())
        .map(|properties| properties.keys().cloned().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    format!("{name_words} {description} {param_names}")
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

pub(crate) fn search_catalog(
    catalog: &[CatalogEntry],
    query: &str,
    limit: usize,
) -> Vec<ToolSearchHit> {
    if catalog.is_empty() || limit == 0 {
        return Vec::new();
    }
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let doc_freq = document_frequencies(catalog);
    let avg_dl = catalog
        .iter()
        .map(|entry| entry.tokens.len())
        .sum::<usize>() as f64
        / catalog.len() as f64;
    let mut scored = catalog
        .iter()
        .filter_map(|entry| {
            let score = bm25_score(
                &query_tokens,
                &entry.tokens,
                avg_dl,
                &doc_freq,
                catalog.len(),
            );
            (score > 0.0).then_some((score, entry))
        })
        .collect::<Vec<_>>();

    if scored.is_empty() {
        let query_lower = query.to_ascii_lowercase();
        scored = catalog
            .iter()
            .filter(|entry| entry.name.to_ascii_lowercase().contains(&query_lower))
            .map(|entry| (0.1, entry))
            .collect();
    }

    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.name.cmp(&right.name))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| ToolSearchHit {
            name: entry.name.clone(),
            description: entry.description.clone(),
            source: entry.source.clone(),
            source_name: entry.source_name.clone(),
        })
        .collect()
}

fn document_frequencies(catalog: &[CatalogEntry]) -> HashMap<String, usize> {
    let mut frequencies = HashMap::new();
    for entry in catalog {
        let mut seen = HashSet::new();
        for token in &entry.tokens {
            if seen.insert(token) {
                *frequencies.entry(token.clone()).or_insert(0) += 1;
            }
        }
    }
    frequencies
}

fn bm25_score(
    query_tokens: &[String],
    doc_tokens: &[String],
    avg_dl: f64,
    doc_freq: &HashMap<String, usize>,
    n_docs: usize,
) -> f64 {
    if doc_tokens.is_empty() {
        return 0.0;
    }
    let mut term_freq = HashMap::new();
    for token in doc_tokens {
        *term_freq.entry(token).or_insert(0usize) += 1;
    }
    let dl = doc_tokens.len() as f64;
    let k1 = 1.5;
    let b = 0.75;
    let mut score = 0.0;
    for query in query_tokens {
        let df = *doc_freq.get(query).unwrap_or(&0) as f64;
        let Some(tf) = term_freq.get(query).copied() else {
            continue;
        };
        if df <= 0.0 {
            continue;
        }
        let idf = (1.0 + ((n_docs as f64 - df + 0.5) / (df + 0.5))).ln();
        let tf = tf as f64;
        let norm = tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * dl / avg_dl.max(1.0)));
        score += idf * norm;
    }
    score
}

pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct NamedTool {
        name: String,
        toolset: String,
        description: String,
    }

    impl NamedTool {
        fn new(name: &str, toolset: &str, description: &str) -> Self {
            Self {
                name: name.to_string(),
                toolset: toolset.to_string(),
                description: description.to_string(),
            }
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

        async fn execute(
            &self,
            _args: &JsonValue,
            _ctx: &hakimi_common::ToolContext,
        ) -> hakimi_common::Result<String> {
            Ok(self.name.clone())
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
        let tools = vec![github, slack];
        let catalog = build_catalog_from_tools(tools.iter(), true);
        let hits = search_catalog(&catalog, "create github issue", 2);

        assert_eq!(hits[0].name, "github_create_issue");
    }
}
