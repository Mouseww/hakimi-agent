use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use scraper::{Html, Selector};
use serde_json::{Value as JsonValue, json};
use tracing::{debug, warn};

use crate::Tool;

const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

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
        "Search the web for information. Supports SearXNG, Tavily, Brave Search, and DuckDuckGo backends."
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

        let provider = std::env::var("HAKIMI_SEARCH_PROVIDER")
            .unwrap_or_default()
            .to_lowercase();

        // Explicit provider selection
        match provider.as_str() {
            "searxng" | "searx" => {
                let base_url = require_env_key("HAKIMI_SEARXNG_URL")?;
                return search_searxng(query, max_results, &base_url).await;
            }
            "tavily" => {
                let key = require_env_key("HAKIMI_SEARCH_API_KEY")?;
                return search_tavily(query, max_results, &key).await;
            }
            "brave" => {
                let key = require_env_key("HAKIMI_SEARCH_API_KEY")?;
                return search_brave(query, max_results, &key).await;
            }
            "bing" => {
                return search_bing_cn(query, max_results).await;
            }
            "ddg" | "duckduckgo" => {
                return search_ddg_html(query, max_results).await;
            }
            _ => {}
        }

        // Auto-detect: try SearXNG first (free, self-hosted), then API providers, then DDG
        if let Ok(searxng_url) = std::env::var("HAKIMI_SEARXNG_URL") {
            match search_searxng(query, max_results, &searxng_url).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    debug!("SearXNG failed ({e}), trying other providers");
                }
            }
        }

        if let Ok(api_key) = std::env::var("HAKIMI_SEARCH_API_KEY") {
            // Try Tavily first (most reliable for AI agents)
            match search_tavily(query, max_results, &api_key).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    debug!("Tavily failed ({e}), trying Brave Search");
                    // Try Brave Search as fallback
                    match search_brave(query, max_results, &api_key).await {
                        Ok(result) => return Ok(result),
                        Err(e2) => {
                            debug!("Brave Search also failed ({e2}), falling back to DDG");
                        }
                    }
                }
            }
        }

        // Try DuckDuckGo first (works outside China)
        match search_ddg_html(query, max_results).await {
            Ok(ref result) if !result.starts_with("No search results") => {
                return Ok(result.clone());
            }
            _ => {}
        }

        // Final fallback: Bing China (works in China)
        search_bing_cn(query, max_results).await
    }
}

fn require_env_key(var: &str) -> Result<String> {
    std::env::var(var).map_err(|_| {
        HakimiError::Tool(format!(
            "{var} not set. Please set it to use the configured search provider."
        ))
    })
}

fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(BROWSER_UA)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))
}

fn format_results(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No search results found. Web search may be unavailable from this environment."
            .to_string();
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
    output
}

// ---------------------------------------------------------------------------
// SearXNG (self-hosted, free, no API key needed)
// ---------------------------------------------------------------------------

async fn search_searxng(query: &str, max_results: usize, base_url: &str) -> Result<String> {
    debug!(query = %query, base_url = %base_url, "searching via SearXNG");

    let client = build_http_client()?;
    let url = format!(
        "{}/search?q={}&format=json&categories=general&pageno=1",
        base_url.trim_end_matches('/'),
        urlencoding::encode(query)
    );

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("SearXNG request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().await.unwrap_or_default();
        return Err(HakimiError::Tool(format!(
            "SearXNG returned {status}: {err_body}"
        )));
    }

    let data: JsonValue = response
        .json()
        .await
        .map_err(|e| HakimiError::Tool(format!("SearXNG response parse error: {e}")))?;

    let results: Vec<SearchResult> = data
        .get("results")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .take(max_results)
                .map(|item| SearchResult {
                    title: item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    url: item
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    snippet: item
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(format_results(&results))
}

// ---------------------------------------------------------------------------
// Tavily Search API
// ---------------------------------------------------------------------------

async fn search_tavily(query: &str, max_results: usize, api_key: &str) -> Result<String> {
    debug!(query = %query, "searching via Tavily API");

    let client = build_http_client()?;
    let body = json!({
        "query": query,
        "max_results": max_results,
        "search_depth": "basic",
        "include_answer": false
    });

    let response = client
        .post("https://api.tavily.com/search")
        .header("Content-Type", "application/json")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("Tavily request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().await.unwrap_or_default();
        return Err(HakimiError::Tool(format!(
            "Tavily API returned {status}: {err_body}"
        )));
    }

    let data: JsonValue = response
        .json()
        .await
        .map_err(|e| HakimiError::Tool(format!("Tavily response parse error: {e}")))?;

    let results: Vec<SearchResult> = data
        .get("results")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .take(max_results)
                .map(|item| SearchResult {
                    title: item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    url: item
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    snippet: item
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(format_results(&results))
}

// ---------------------------------------------------------------------------
// Brave Search API
// ---------------------------------------------------------------------------

async fn search_brave(query: &str, max_results: usize, api_key: &str) -> Result<String> {
    debug!(query = %query, "searching via Brave Search API");

    let client = build_http_client()?;
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        urlencoding::encode(query),
        max_results
    );

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .header("Accept-Encoding", "gzip")
        .header("X-Subscription-Token", api_key)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("Brave Search request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().await.unwrap_or_default();
        return Err(HakimiError::Tool(format!(
            "Brave Search API returned {status}: {err_body}"
        )));
    }

    let data: JsonValue = response
        .json()
        .await
        .map_err(|e| HakimiError::Tool(format!("Brave Search response parse error: {e}")))?;

    let results: Vec<SearchResult> = data
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .take(max_results)
                .map(|item| SearchResult {
                    title: item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    url: item
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    snippet: item
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(format_results(&results))
}

// ---------------------------------------------------------------------------
// DuckDuckGo HTML scraping (fallback)
// ---------------------------------------------------------------------------

async fn search_ddg_html(query: &str, max_results: usize) -> Result<String> {
    debug!(query = %query, "searching via DuckDuckGo HTML scraping");

    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let client = build_http_client()?;

    let response = client
        .get(&url)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Accept-Encoding", "gzip, deflate, br")
        .header("DNT", "1")
        .header("Upgrade-Insecure-Requests", "1")
        .header("Sec-Fetch-Dest", "document")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Site", "none")
        .header("Sec-Fetch-User", "?1")
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("web search request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(HakimiError::Tool(format!(
            "DuckDuckGo returned HTTP {status}"
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read search response: {e}")))?;

    let results = parse_ddg_html(&body, max_results);

    if results.is_empty() {
        warn!("DDG returned 0 results — page may contain CAPTCHA or changed structure");
    }

    Ok(format_results(&results))
}

struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// Parse DuckDuckGo HTML search results using the `scraper` crate.
fn parse_ddg_html(html: &str, max_results: usize) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let mut results = Vec::new();

    // Strategy 1: Modern DDG selectors (div.result or div.web-result)
    let result_sel = Selector::parse("div.result, div.web-result, div.results_links").ok();
    let title_sel = Selector::parse("a.result__a, h2 a, a.result-link").ok();
    let snippet_sel = Selector::parse("a.result__snippet, .result__snippet, td.result-snippet, .snippet").ok();
    let url_sel = Selector::parse("a.result__url, .result__extras__url a, span.result__url").ok();

    if let (Some(r_sel), Some(t_sel)) = (&result_sel, &title_sel) {
        for el in document.select(r_sel) {
            if results.len() >= max_results {
                break;
            }

            let title = el
                .select(t_sel)
                .next()
                .map(|a| a.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();

            if title.is_empty() {
                continue;
            }

            let mut url = el
                .select(t_sel)
                .next()
                .and_then(|a| a.value().attr("href"))
                .unwrap_or("")
                .to_string();

            // DDG sometimes wraps URLs in redirects: extract actual URL
            if url.contains("uddg=") {
                if let Some(decoded) = extract_ddg_redirect(&url) {
                    url = decoded;
                }
            }

            // Try dedicated URL element if href was empty or a redirect
            if url.is_empty() || url.starts_with('/') {
                if let Some(ref u_sel) = url_sel {
                    if let Some(url_el) = el.select(u_sel).next() {
                        let raw = url_el.text().collect::<String>().trim().to_string();
                        if !raw.is_empty() {
                            url = if raw.starts_with("http") {
                                raw
                            } else {
                                format!("https://{raw}")
                            };
                        }
                    }
                }
            }

            let snippet = snippet_sel
                .as_ref()
                .and_then(|s| el.select(s).next())
                .map(|s| clean_html_text(&s.text().collect::<String>()))
                .unwrap_or_default();

            results.push(SearchResult {
                title: clean_html_text(&title),
                url: if url.is_empty() {
                    "(no url)".to_string()
                } else {
                    url
                },
                snippet,
            });
        }
    }

    // Strategy 2: Fallback — scan all <a class="result__a"> directly
    if results.is_empty() {
        if let Some(ref t_sel) = title_sel {
            for a_el in document.select(t_sel) {
                if results.len() >= max_results {
                    break;
                }
                let title = a_el.text().collect::<String>().trim().to_string();
                if title.is_empty() {
                    continue;
                }
                let mut url = a_el.value().attr("href").unwrap_or("").to_string();
                if url.contains("uddg=") {
                    if let Some(decoded) = extract_ddg_redirect(&url) {
                        url = decoded;
                    }
                }
                results.push(SearchResult {
                    title: clean_html_text(&title),
                    url: if url.is_empty() {
                        "(no url)".to_string()
                    } else {
                        url
                    },
                    snippet: String::new(),
                });
            }
        }
    }

    results
}

/// Extract the actual URL from a DDG redirect like `/l/?uddg=https%3A%2F%2Fexample.com&rut=...`
fn extract_ddg_redirect(redirect_url: &str) -> Option<String> {
    let query_start = redirect_url.find("uddg=")?;
    let value_start = query_start + 5;
    let value_end = redirect_url[value_start..]
        .find('&')
        .map(|i| value_start + i)
        .unwrap_or(redirect_url.len());
    let encoded = &redirect_url[value_start..value_end];
    urlencoding::decode(encoded).ok().map(|s| s.into_owned())
}

// ---------------------------------------------------------------------------
// Bing China HTML scraping (works in China without VPN)
// ---------------------------------------------------------------------------

async fn search_bing_cn(query: &str, max_results: usize) -> Result<String> {
    debug!(query = %query, "searching via Bing China (cn.bing.com)");

    let url = format!(
        "https://cn.bing.com/search?q={}&count={}",
        urlencoding::encode(query),
        max_results.min(10)
    );

    let client = build_http_client()?;

    let response = client
        .get(&url)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .header("Accept-Encoding", "gzip, deflate, br")
        .header("Sec-Fetch-Dest", "document")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Site", "none")
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("Bing CN request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(HakimiError::Tool(format!("Bing CN returned HTTP {status}")));
    }

    let body = response
        .text()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read Bing CN response: {e}")))?;

    let results = parse_bing_html(&body, max_results);

    if results.is_empty() {
        warn!("Bing CN returned 0 results — page may contain CAPTCHA or changed structure");
    }

    Ok(format_results(&results))
}

/// Parse Bing search results HTML using the `scraper` crate.
fn parse_bing_html(html: &str, max_results: usize) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let mut results = Vec::new();

    // Bing results are in <li class="b_algo"> elements
    let algo_sel = Selector::parse("li.b_algo").ok();
    let title_sel = Selector::parse("h2 a").ok();
    let tilk_sel = Selector::parse("a.tilk").ok();
    let snippet_sel = Selector::parse(".b_caption p, .b_lineclamp2, .b_lineclamp3, .b_paractl").ok();

    if let (Some(a_sel), Some(t_sel)) = (&algo_sel, &title_sel) {
        for el in document.select(a_sel) {
            if results.len() >= max_results {
                break;
            }

            // Try h2 > a first for title and URL
            let (title, url) = if let Some(title_el) = el.select(t_sel).next() {
                let t = title_el.text().collect::<String>().trim().to_string();
                let u = title_el.value().attr("href").unwrap_or("").to_string();
                (t, u)
            } else if let Some(tilk_el) = tilk_sel.as_ref().and_then(|s| el.select(s).next()) {
                // Fallback: use a.tilk for URL, aria-label for title
                let t = tilk_el.value().attr("aria-label").unwrap_or("").to_string();
                let u = tilk_el.value().attr("href").unwrap_or("").to_string();
                (t, u)
            } else {
                continue;
            };

            if title.is_empty() {
                continue;
            }

            let snippet = snippet_sel
                .as_ref()
                .and_then(|s| el.select(s).next())
                .map(|s| clean_html_text(&s.text().collect::<String>()))
                .unwrap_or_default();

            results.push(SearchResult {
                title: clean_html_text(&title),
                url: if url.is_empty() {
                    "(no url)".to_string()
                } else {
                    url
                },
                snippet,
            });
        }
    }

    results
}

/// Clean whitespace and HTML entities from extracted text.
fn clean_html_text(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
