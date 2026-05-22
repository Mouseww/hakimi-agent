use async_trait::async_trait;
use hakimi_common::{Result, ToolContext, HakimiError};
use serde_json::{Value as JsonValue, json};
use tracing::info;

use crate::Tool;

/// Tool for performing search over knowledge base.
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
        "Search the knowledge base for entities, notes, and snippets. Use this to retrieve information from memory."
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
                    "description": "The search query (e.g. 'project architecture', 'API keys')."
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
        let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
            HakimiError::Tool("missing 'query' argument".into())
        })?;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        info!(query = %query, limit = limit, "Executing knowledge search");

        if let Some(searcher) = &ctx.knowledge_searcher {
            let results = searcher.search(query, limit).await?;
            Ok(serde_json::to_string_pretty(&results).unwrap())
        } else {
            // Fallback for when knowledge searcher is not yet implemented or injected
            Ok(json!({
                "results": [],
                "query": query,
                "message": "Knowledge base search is currently unavailable in this context."
            }).to_string())
        }
    }
}
