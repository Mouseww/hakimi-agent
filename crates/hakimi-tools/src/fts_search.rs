use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tracing::{info, warn};

use crate::Tool;

/// Tool for performing FTS5 (Full Text Search) over knowledge base.
pub struct FtsSearchTool;

#[async_trait]
impl Tool for FtsSearchTool {
    fn name(&self) -> &str {
        "fts_search"
    }

    fn toolset(&self) -> &str {
        "knowledge"
    }

    fn description(&self) -> &str {
        "Search the knowledge base using FTS5 full-text search. Returns matching entities and snippets."
    }

    fn emoji(&self) -> &str {
        "🔍"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return.",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        info!(query = %query, limit = limit, "Executing knowledge search");

        // Use the context's knowledge store if available
        let results = if let Some(ref store) = ctx.knowledge_store {
            let store_lock = store.read().await;
            let found_nodes = store_lock.graph().search(query);

            let items: Vec<JsonValue> = found_nodes
                .iter()
                .take(limit)
                .map(|n| {
                    json!({
                        "key": n.key(),
                        "kind": n.kind(),
                        "snippet": n.key() // In a real FTS we would have fragments, here we just return the key
                    })
                })
                .collect();

            json!({
                "results": items,
                "query": query,
                "total_matches": found_nodes.len(),
                "engine": "memory_fuzzy"
            })
        } else {
            json!({
                "results": [],
                "query": query,
                "message": "No knowledge store available in current context.",
                "engine": "none"
            })
        };

        Ok(serde_json::to_string_pretty(&results).unwrap())
    }
}
