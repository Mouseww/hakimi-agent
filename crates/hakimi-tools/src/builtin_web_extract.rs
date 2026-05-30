use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use scraper::{Html, Selector};
use serde_json::{Value as JsonValue, json};
use tracing::debug;

use crate::Tool;
use crate::url_safety::{assert_safe_http_url, safe_http_redirect_policy};

/// Built-in tool that fetches a URL and extracts readable text content
/// using a readability-style scoring algorithm.
pub struct WebExtractTool;

#[async_trait]
impl Tool for WebExtractTool {
    fn name(&self) -> &str {
        "web_extract"
    }

    fn toolset(&self) -> &str {
        "web"
    }

    fn description(&self) -> &str {
        "Fetch a URL and extract the main readable text content, stripping navigation, ads, scripts, and other boilerplate. Returns clean markdown-like text."
    }

    fn emoji(&self) -> &str {
        "\u{1f4d6}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch and extract content from."
                },
                "max_length": {
                    "type": "integer",
                    "description": "Maximum length of extracted text in characters. Defaults to 50000.",
                    "minimum": 100,
                    "maximum": 200000
                }
            },
            "required": ["url"]
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(200 * 1024) // 200KB max
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: url".into()))?;

        let max_length = args
            .get("max_length")
            .and_then(|v| v.as_u64())
            .unwrap_or(50_000)
            .min(200_000) as usize;

        debug!(url = %url, max_length, "web extract request");

        assert_safe_http_url(url)?;

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; HakimiAgent/0.1; +https://github.com/Mouseww/hakimi-agent)")
            .timeout(std::time::Duration::from_secs(30))
            .redirect(safe_http_redirect_policy(5))
            .build()
            .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| HakimiError::Tool(format!("fetch failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(HakimiError::Tool(format!(
                "HTTP request failed with status: {}",
                status
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = response
            .text()
            .await
            .map_err(|e| HakimiError::Tool(format!("failed to read response body: {e}")))?;

        // Detect if this is plain text or JSON
        if content_type.contains("text/plain") {
            let text = body.chars().take(max_length).collect::<String>();
            return Ok(text);
        }

        if content_type.contains("application/json") {
            let text = body.chars().take(max_length).collect::<String>();
            return Ok(text);
        }

        // For HTML content, extract readable text
        let extracted = extract_readable_text(&body);

        if extracted.trim().is_empty() {
            return Ok(format!(
                "No readable content could be extracted from {url}. The page may be dynamically rendered (requires JavaScript) or may not contain article-like content."
            ));
        }

        // Truncate to max_length
        let result = if extracted.len() > max_length {
            let truncated: String = extracted.chars().take(max_length).collect();
            format!("{truncated}\n\n[Content truncated at {max_length} characters]")
        } else {
            extracted
        };

        Ok(result)
    }
}

/// Extract readable text content from HTML using a readability-style algorithm.
///
/// Strategy:
/// 1. Parse the HTML and remove noise elements (script, style, nav, footer, etc.)
/// 2. Find content blocks (paragraphs, divs, article, main, section)
/// 3. Score blocks by text density and content quality
/// 4. Extract text from the highest-scoring region
fn extract_readable_text(html: &str) -> String {
    let document = Html::parse_document(html);

    // Extract title
    let title = extract_title(&document);

    // Remove noise elements
    let cleaned = remove_noise_elements(html);

    // Re-parse after cleaning
    let cleaned_doc = Html::parse_document(&cleaned);

    // Extract content from paragraphs and block-level elements
    let mut content_parts = Vec::new();

    // Try <article> first, then <main>, then <body>
    let article_text = extract_from_container(&cleaned_doc, "article");
    if !article_text.is_empty() && article_text.len() > 100 {
        content_parts.push(article_text);
    } else {
        let main_text = extract_from_container(&cleaned_doc, "main");
        if !main_text.is_empty() && main_text.len() > 100 {
            content_parts.push(main_text);
        } else {
            // Fall back to all paragraphs with scoring
            let body_text = extract_scored_paragraphs(&cleaned_doc);
            if !body_text.is_empty() {
                content_parts.push(body_text);
            }
        }
    }

    // Build result
    let mut result = String::new();

    if let Some(t) = title
        && !t.is_empty()
    {
        result.push_str(&format!("# {t}\n\n"));
    }

    for part in &content_parts {
        if !result.is_empty() && !result.ends_with("\n\n") {
            result.push_str("\n\n");
        }
        result.push_str(part);
    }

    // Clean up excessive whitespace
    clean_whitespace(&result)
}

/// Extract the page title.
fn extract_title(document: &Html) -> Option<String> {
    // Try <title> tag
    if let Ok(sel) = Selector::parse("title")
        && let Some(el) = document.select(&sel).next()
    {
        let text = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }

    // Try <h1> tag
    if let Ok(sel) = Selector::parse("h1")
        && let Some(el) = document.select(&sel).next()
    {
        let text = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

/// Remove noise elements from HTML by stripping script, style, nav, footer, etc.
fn remove_noise_elements(html: &str) -> String {
    // Use regex-based removal for noise tags (faster than re-parsing)
    let mut result = html.to_string();

    // Remove script and style blocks completely (including content)
    for tag in &["script", "style", "noscript", "iframe", "svg", "canvas"] {
        result = remove_tag_block(&result, tag);
    }

    // Remove nav, header, footer, aside (but keep their text for now - we'll score later)
    // We'll handle these by scoring rather than removal to be more robust

    result
}

/// Remove all occurrences of a tag and its content from HTML.
fn remove_tag_block(html: &str, tag: &str) -> String {
    let close = format!("</{tag}>");
    let mut result = String::with_capacity(html.len());
    let mut remaining = html;
    let mut depth = 0;

    while !remaining.is_empty() {
        if depth == 0 {
            // Look for opening tag
            if let Some(start) = find_tag_open(remaining, tag) {
                result.push_str(&remaining[..start]);
                remaining = &remaining[start..];
                depth = 1;

                // Check if it's self-closing
                if remaining.contains('>') {
                    let gt_pos = remaining.find('>').unwrap();
                    if remaining[..gt_pos].ends_with('/') {
                        remaining = &remaining[gt_pos + 1..];
                        depth = 0;
                        continue;
                    }
                }
            } else {
                result.push_str(remaining);
                break;
            }
        } else {
            // Inside noise tag, look for close
            if let Some(close_pos) = remaining.find(&close) {
                remaining = &remaining[close_pos + close.len()..];
                depth = 0;
            } else {
                // No closing tag found - skip rest
                break;
            }
        }
    }

    result
}

/// Find the start position of an opening tag (e.g. `<script` or `<script ` or `<script>`)
fn find_tag_open(html: &str, tag: &str) -> Option<usize> {
    let open_lower = format!("<{tag}");
    let html_lower = html.to_lowercase();
    let open_lower = open_lower.to_lowercase();

    let mut pos = 0;
    while pos < html.len() {
        if let Some(found) = html_lower[pos..].find(&open_lower) {
            let abs_pos = pos + found;
            let after_tag = abs_pos + open_lower.len();
            if after_tag >= html.len() {
                return Some(abs_pos);
            }
            let next_char = html_lower.as_bytes()[after_tag];
            // Must be followed by whitespace, >, or / to be a valid tag
            if next_char == b' ' || next_char == b'>' || next_char == b'/' {
                return Some(abs_pos);
            }
            pos = after_tag;
        } else {
            break;
        }
    }
    None
}

/// Extract text content from a specific container element (article, main, etc.)
fn extract_from_container(document: &Html, container_tag: &str) -> String {
    let sel_str = container_tag;
    let sel = match Selector::parse(sel_str) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let mut best_text = String::new();

    for element in document.select(&sel) {
        let text = extract_text_from_element(&element);
        if text.len() > best_text.len() {
            best_text = text;
        }
    }

    best_text
}

/// Score paragraphs by content quality and extract the best ones.
fn extract_scored_paragraphs(document: &Html) -> String {
    let p_sel = match Selector::parse("p, li, pre, blockquote, h1, h2, h3, h4, h5, h6") {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let mut scored_blocks: Vec<(f64, String)> = Vec::new();

    for element in document.select(&p_sel) {
        let text = element.text().collect::<Vec<_>>().join(" ");
        let text = text.trim();

        if text.len() < 20 {
            continue; // Skip very short blocks
        }

        let score = score_text_block(text);

        // Skip blocks that are likely navigation/boilerplate
        if score < 0.2 {
            continue;
        }

        let tag_name = element.value().name();
        let formatted = match tag_name {
            "h1" => format!("# {text}"),
            "h2" => format!("## {text}"),
            "h3" => format!("### {text}"),
            "h4" => format!("#### {text}"),
            "h5" => format!("##### {text}"),
            "h6" => format!("###### {text}"),
            "li" => format!("- {text}"),
            "blockquote" => format!("> {text}"),
            "pre" => format!("```\n{text}\n```"),
            _ => text.to_string(),
        };

        scored_blocks.push((score, formatted));
    }

    // Sort by score descending, take the best blocks
    scored_blocks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Take blocks with good scores (above threshold)
    let threshold = scored_blocks.first().map(|(s, _)| s * 0.3).unwrap_or(0.0);

    let mut result_parts: Vec<(usize, &str)> = Vec::new();
    for (i, (score, text)) in scored_blocks.iter().enumerate() {
        if *score >= threshold {
            result_parts.push((i, text.as_str()));
        }
    }

    // Restore original order for readability
    result_parts.sort_by_key(|(i, _)| *i);

    result_parts
        .iter()
        .map(|(_, text)| text.to_string())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Score a text block for content quality (0.0 to 1.0).
fn score_text_block(text: &str) -> f64 {
    let len = text.len() as f64;
    if len == 0.0 {
        return 0.0;
    }

    let word_count = text.split_whitespace().count() as f64;
    let avg_word_len =
        text.chars().filter(|c| c.is_alphabetic()).count() as f64 / word_count.max(1.0);

    // Penalize very short text
    let length_score = (len / 100.0).min(1.0);

    // Prefer reasonable word lengths (4-8 chars average)
    let word_score = if (3.0..=12.0).contains(&avg_word_len) {
        1.0
    } else if avg_word_len < 2.0 {
        0.1 // Likely not real text
    } else {
        0.5
    };

    // Penalize text with too many links or special chars
    let link_chars = text.matches("http").count() as f64;
    let special_ratio = text
        .chars()
        .filter(|c| {
            !c.is_alphanumeric()
                && !c.is_whitespace()
                && *c != '.'
                && *c != ','
                && *c != ';'
                && *c != ':'
                && *c != '-'
                && *c != '\''
        })
        .count() as f64
        / len;
    let link_penalty = (1.0 - (link_chars / word_count.max(1.0))).max(0.1);
    let special_penalty = (1.0 - special_ratio * 2.0).max(0.3);

    // Bonus for sentences (contains periods followed by spaces)
    let sentence_bonus = if text.contains(". ") || text.contains(".\n") {
        1.2
    } else {
        1.0
    };

    length_score * word_score * link_penalty * special_penalty * sentence_bonus
}

/// Extract all text from an HTML element, preserving some structure.
fn extract_text_from_element(element: &scraper::ElementRef) -> String {
    let mut parts = Vec::new();

    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            let t = text.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        } else if let Some(elem) = child.value().as_element() {
            let tag = elem.name();
            // Recurse into child elements
            let child_ref = scraper::ElementRef::wrap(child);
            if let Some(child_el) = child_ref {
                let child_text = extract_text_from_element(&child_el);
                if !child_text.is_empty() {
                    match tag {
                        "h1" => parts.push(format!("# {child_text}")),
                        "h2" => parts.push(format!("## {child_text}")),
                        "h3" => parts.push(format!("### {child_text}")),
                        "h4" | "h5" | "h6" => parts.push(format!("#### {child_text}")),
                        "li" => parts.push(format!("- {child_text}")),
                        "br" => parts.push("\n".to_string()),
                        "p" | "div" | "section" | "article" | "main" | "blockquote" => {
                            parts.push(format!("\n{child_text}\n"));
                        }
                        "pre" | "code" => parts.push(format!("`{child_text}`")),
                        "strong" | "b" => parts.push(format!("**{child_text}**")),
                        "em" | "i" => parts.push(format!("*{child_text}*")),
                        _ => parts.push(child_text),
                    }
                }
            }
        }
    }

    let joined = parts.join(" ");
    clean_whitespace(&joined)
}

/// Clean up excessive whitespace in text.
fn clean_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_newline = false;
    let mut newline_count = 0;

    for c in text.chars() {
        match c {
            '\n' => {
                newline_count += 1;
                if newline_count <= 2 {
                    result.push('\n');
                }
                prev_was_newline = true;
            }
            '\r' => {} // Skip carriage returns
            c if c.is_whitespace() => {
                if !prev_was_newline {
                    result.push(' ');
                }
                newline_count = 0;
                prev_was_newline = false;
            }
            c => {
                result.push(c);
                newline_count = 0;
                prev_was_newline = false;
            }
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title_from_html() {
        let html = r#"<html><head><title>Test Page Title</title></head><body></body></html>"#;
        let doc = Html::parse_document(html);
        let title = extract_title(&doc);
        assert_eq!(title, Some("Test Page Title".to_string()));
    }

    #[test]
    fn test_extract_title_from_h1() {
        let html = r#"<html><body><h1>My Article Title</h1><p>Content</p></body></html>"#;
        let doc = Html::parse_document(html);
        let title = extract_title(&doc);
        assert_eq!(title, Some("My Article Title".to_string()));
    }

    #[test]
    fn test_remove_script_blocks() {
        let html =
            r#"<html><body><p>Hello</p><script>alert('xss');</script><p>World</p></body></html>"#;
        let result = remove_tag_block(html, "script");
        assert!(!result.contains("alert"));
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
    }

    #[test]
    fn test_remove_style_blocks() {
        let html = r#"<html><head><style>body { color: red; }</style></head><body><p>Text</p></body></html>"#;
        let result = remove_tag_block(html, "style");
        assert!(!result.contains("color: red"));
        assert!(result.contains("Text"));
    }

    #[test]
    fn test_score_text_block_quality() {
        let good_text = "This is a well-written paragraph with multiple sentences. It contains enough words to be considered meaningful content. The average word length is reasonable.";
        let bad_text = "click here home about contact privacy";

        let good_score = score_text_block(good_text);
        let bad_score = score_text_block(bad_text);

        assert!(
            good_score > bad_score,
            "Good text should score higher than navigation text"
        );
    }

    #[test]
    fn test_score_text_block_empty() {
        assert_eq!(score_text_block(""), 0.0);
    }

    #[test]
    fn test_extract_readable_text_with_article() {
        let html = r#"
        <html>
        <head><title>Test Article</title></head>
        <body>
            <nav>Skip this navigation</nav>
            <article>
                <h1>Main Article Title</h1>
                <p>This is the main content of the article. It contains several sentences of meaningful text that should be extracted by the readability algorithm.</p>
                <p>Another paragraph with more content. This paragraph also has sufficient length to be scored well by the extraction algorithm.</p>
            </article>
            <footer>Copyright 2025</footer>
        </body>
        </html>"#;

        let result = extract_readable_text(html);
        assert!(result.contains("Main Article Title") || result.contains("main content"));
        assert!(result.len() > 50);
    }

    #[test]
    fn test_clean_whitespace() {
        let input = "Hello\n\n\n\n\nWorld\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\nToo many newlines";
        let result = clean_whitespace(input);
        assert!(!result.contains("\n\n\n"));
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
    }

    #[test]
    fn test_extract_from_paragraphs() {
        let html = r#"
        <html><body>
            <div>
                <p>This is a substantial paragraph with enough content to pass the scoring threshold. It has multiple sentences and provides meaningful information.</p>
                <p>Another meaningful paragraph that contains enough words and proper sentence structure to score well in the extraction algorithm.</p>
                <p>short</p>
            </div>
        </body></html>"#;

        let doc = Html::parse_document(html);
        let result = extract_scored_paragraphs(&doc);
        assert!(result.contains("substantial paragraph"));
        // Short paragraph may or may not appear depending on threshold
    }

    #[test]
    fn test_find_tag_open() {
        let html = "<div><script type=\"text/javascript\">alert('hi');</script></div>";
        let pos = find_tag_open(html, "script");
        assert_eq!(pos, Some(5));
    }

    #[test]
    fn test_find_tag_open_not_found() {
        let html = "<div><p>Hello</p></div>";
        let pos = find_tag_open(html, "script");
        assert_eq!(pos, None);
    }

    #[test]
    fn test_remove_nested_scripts() {
        let html = r#"<html><body>
            <p>Before</p>
            <script>var x = '<script>nested</script>';</script>
            <p>After</p>
        </body></html>"#;
        let result = remove_tag_block(html, "script");
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
    }

    #[test]
    fn test_web_extract_tool_name() {
        let tool = WebExtractTool;
        assert_eq!(tool.name(), "web_extract");
        assert_eq!(tool.toolset(), "web");
    }

    #[test]
    fn test_web_extract_tool_schema() {
        let tool = WebExtractTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["url"].is_object());
        assert!(schema["properties"]["max_length"].is_object());
    }

    #[tokio::test]
    async fn web_extract_blocks_metadata_urls_before_fetch() {
        let tool = WebExtractTool;
        let err = tool
            .execute(
                &json!({"url": "http://169.254.169.254/latest/meta-data"}),
                &ToolContext::default(),
            )
            .await
            .expect_err("metadata URL should be rejected before fetch");

        assert!(err.to_string().contains("metadata"));
    }
}
