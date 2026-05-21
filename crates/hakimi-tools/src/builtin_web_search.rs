use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tracing::debug;

use crate::Tool;

/// Built-in tool that performs web searches.
pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn toolset(&self) -> &str {
        "web"
    }

    fn description(&self) -> &str {
        "Search the web for information. Uses a configured search API if available, otherwise falls back to DuckDuckGo HTML scraping."
    }

    fn emoji(&self) -> &str {
        "\u{1f310}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return. Defaults to 5.",
                    "minimum": 1,
                    "maximum": 20
                }
            },
            "required": ["query"]
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(64 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: query".into()))?;

        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(20) as usize;

        debug!(query = %query, max_results, "performing web search");

        // Check for configured search API key
        if let Ok(api_key) = std::env::var("HAKIMI_SEARCH_API_KEY") {
            return search_with_api(query, max_results, &api_key).await;
        }

        // Fall back to DuckDuckGo HTML scraping
        search_ddg_html(query, max_results).await
    }
}

/// Search using a configured API (placeholder for extensibility).
async fn search_with_api(query: &str, max_results: usize, _api_key: &str) -> Result<String> {
    // This is a placeholder for future API integration.
    // Users can set HAKIMI_SEARCH_API_KEY to enable a specific provider.
    // For now, fall back to DDG.
    debug!(query = %query, "search API key found but no provider configured, falling back to DDG");
    search_ddg_html(query, max_results).await
}

/// Search using DuckDuckGo HTML scraping.
async fn search_ddg_html(query: &str, max_results: usize) -> Result<String> {
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; HakimiAgent/0.1)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("web search request failed: {e}")))?;

    let body = response
        .text()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read search response: {e}")))?;

    // Parse DDG HTML results
    let results = parse_ddg_html(&body, max_results);

    if results.is_empty() {
        return Ok(
            "No search results found. Web search may be unavailable from this environment."
                .to_string(),
        );
    }

    let mut output = String::new();
    for (i, result) in results.iter().enumerate() {
        output.push_str(&format!(
            "{}. {}\n   {}\n   {}\n\n",
            i + 1,
            result.title,
            result.url,
            result.snippet
        ));
    }

    Ok(output)
}

struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// Parse DuckDuckGo HTML search results.
fn parse_ddg_html(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // Simple HTML parsing for DDG results
    // DDG HTML results have <a class="result__a" ...> for titles and
    // <a class="result__url" ...> for URLs and
    // <a class="result__snippet" ...> for snippets

    let lines: Vec<&str> = html.lines().collect();

    let mut i = 0;
    while i < lines.len() && results.len() < max_results {
        let line = lines[i];

        // Look for result links
        if line.contains("result__a") || line.contains("result__heading") {
            let title = extract_tag_content(line);
            let mut url = String::new();
            let mut snippet = String::new();

            // Look ahead for URL and snippet
            for next in lines.iter().take((i + 10).min(lines.len())).skip(i + 1) {
                let next = *next;
                if next.contains("result__url") && url.is_empty() {
                    url = extract_tag_content(next);
                    if !url.starts_with("http") {
                        url = format!("https://{url}");
                    }
                }
                if next.contains("result__snippet") && snippet.is_empty() {
                    snippet = extract_tag_content(next);
                }
            }

            if !title.is_empty() {
                results.push(SearchResult {
                    title: clean_html(&title),
                    url: if url.is_empty() {
                        "(no url)".to_string()
                    } else {
                        url
                    },
                    snippet: clean_html(&snippet),
                });
            }
        }

        i += 1;
    }

    results
}

/// Extract text content from an HTML tag.
fn extract_tag_content(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut in_content = false;

    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                in_content = false;
            }
            '>' => {
                in_tag = false;
                in_content = true;
            }
            _ => {
                if in_content && !in_tag {
                    result.push(c);
                }
            }
        }
    }

    result.trim().to_string()
}

/// Clean HTML entities and tags from text.
fn clean_html(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("<b>", "")
        .replace("</b>", "")
        .replace("<em>", "")
        .replace("</em>", "")
        .trim()
        .to_string()
}
