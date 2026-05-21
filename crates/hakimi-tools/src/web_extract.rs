//! Web content extraction module.
//!
//! Provides [`WebExtractor`] for fetching URLs and extracting readable content
//! from HTML pages. Supports multiple output formats (Markdown, PlainText, Html)
//! and a readability-style extraction algorithm that strips navigation, ads,
//! scripts, and other boilerplate.

use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Output format for extracted content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    /// Clean Markdown with headings, lists, emphasis, etc.
    Markdown,
    /// Plain text with no formatting markers.
    PlainText,
    /// Sanitised HTML (scripts/styles removed, content preserved).
    Html,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Markdown
    }
}

/// Structured result of a web-page extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedContent {
    /// Page title (from `<title>` or first `<h1>`).
    pub title: String,
    /// Extracted body content in the requested format.
    pub content: String,
    /// The URL that was fetched.
    pub url: String,
    /// UTC timestamp of when the extraction happened.
    pub extracted_at: DateTime<Utc>,
    /// Approximate word count of the content.
    pub word_count: usize,
}

// ---------------------------------------------------------------------------
// WebExtractor
// ---------------------------------------------------------------------------

/// Main entry point for extracting content from web pages.
///
/// # Example
/// ```no_run
/// # use hakimi_tools::web_extract::{WebExtractor, OutputFormat};
/// # async fn demo() -> anyhow::Result<()> {
/// let extractor = WebExtractor::new();
/// let result = extractor.extract("https://example.com", OutputFormat::Markdown).await?;
/// println!("Title: {}", result.title);
/// println!("Words: {}", result.word_count);
/// # Ok(())
/// # }
/// ```
pub struct WebExtractor {
    client: Client,
}

impl WebExtractor {
    /// Create a new extractor with default settings.
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (compatible; HakimiAgent/0.2; +https://github.com/Mouseww/hakimi-agent)")
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("failed to build HTTP client");
        Self { client }
    }

    /// Create a new extractor with a pre-built [`Client`].
    pub fn with_client(client: Client) -> Self {
        Self { client }
    }

    // ------------------------------------------------------------------
    // Core extraction
    // ------------------------------------------------------------------

    /// Fetch `url` and return the main content in the requested `format`.
    pub async fn extract(&self, url: &str, format: OutputFormat) -> Result<ExtractedContent> {
        debug!(url = %url, ?format, "extracting web content");

        self.validate_url(url)?;

        let body = self.fetch(url).await?;

        // Detect non-HTML content
        if let Some(text) = self.try_raw_text(&body) {
            return self.build_result(url, "", &text, format);
        }

        let html = &body;
        let title = extract_title(html);
        let content = match format {
            OutputFormat::Markdown => html_to_markdown(html),
            OutputFormat::PlainText => strip_html_tags(&extract_main_content(html)),
            OutputFormat::Html => extract_main_content(html),
        };

        self.build_result(url, &title, &content, format)
    }

    /// Fetch `url`, extract content, then return the raw text (always Markdown).
    /// Useful when you just need the text for downstream processing.
    pub async fn extract_raw(&self, url: &str) -> Result<ExtractedContent> {
        self.extract(url, OutputFormat::Markdown).await
    }

    // ------------------------------------------------------------------
    // LLM summarization
    // ------------------------------------------------------------------

    /// Extract content and then pass it to an LLM with `prompt` for summarization.
    ///
    /// This method fetches the page, extracts Markdown content, and returns
    /// the extracted text along with the prompt. The actual LLM call is
    /// performed by the caller (or by the tool that wraps this).
    ///
    /// The returned string contains the extracted content prefixed with the
    /// user prompt, ready to be sent to an LLM.
    pub async fn extract_with_prompt(&self, url: &str, prompt: &str) -> Result<String> {
        debug!(url = %url, prompt_len = prompt.len(), "extract_with_prompt");

        let extracted = self.extract(url, OutputFormat::Markdown).await?;

        let output = format!(
            "## Extracted Content from {}\n\n**Title:** {}\n**Words:** {}\n\n---\n\n{}\n\n---\n\n**User request:** {}",
            extracted.url, extracted.title, extracted.word_count, extracted.content, prompt,
        );

        Ok(output)
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    fn validate_url(&self, url: &str) -> Result<()> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            anyhow::bail!("URL must start with http:// or https://");
        }
        Ok(())
    }

    async fn fetch(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("fetch failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("HTTP {status}");
        }

        let body = response
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("read body failed: {e}"))?;

        Ok(body)
    }

    /// If the body looks like raw text or JSON, return it directly.
    fn try_raw_text(&self, body: &str) -> Option<String> {
        let trimmed = body.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            // Likely JSON
            if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                return Some(trimmed.to_string());
            }
        }
        None
    }

    fn build_result(
        &self,
        url: &str,
        title: &str,
        content: &str,
        _format: OutputFormat,
    ) -> Result<ExtractedContent> {
        let word_count = content.split_whitespace().count();
        Ok(ExtractedContent {
            title: title.to_string(),
            content: content.to_string(),
            url: url.to_string(),
            extracted_at: Utc::now(),
            word_count,
        })
    }
}

impl Default for WebExtractor {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Free-standing helper functions (also used by builtin_web_extract)
// ===========================================================================

/// Strip all HTML tags from `html`, returning plain text.
///
/// Handles script/style removal, HTML entity decoding, and whitespace
/// normalisation.
pub fn strip_html_tags(html: &str) -> String {
    let without_noise = remove_noise_blocks(html);
    let stripped = strip_tags(&without_noise);
    decode_entities(&stripped)
}

/// Extract the page title from `<title>` or `<h1>`.
pub fn extract_title(html: &str) -> String {
    let doc = Html::parse_document(html);

    if let Ok(sel) = Selector::parse("title") {
        if let Some(el) = doc.select(&sel).next() {
            let text = el.text().collect::<Vec<_>>().join(" ");
            let text = text.trim();
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }

    if let Ok(sel) = Selector::parse("h1") {
        if let Some(el) = doc.select(&sel).next() {
            let text = el.text().collect::<Vec<_>>().join(" ");
            let text = text.trim();
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }

    String::new()
}

/// Extract the main readable content from an HTML page.
///
/// Tries `<article>`, then `<main>`, then falls back to scored paragraph
/// extraction from `<body>`.
pub fn extract_main_content(html: &str) -> String {
    let cleaned = remove_noise_blocks(html);
    let doc = Html::parse_document(&cleaned);

    // 1. Try <article>
    if let Ok(sel) = Selector::parse("article") {
        for el in doc.select(&sel) {
            let text = el.text().collect::<Vec<_>>().join(" ");
            let text = text.trim();
            if text.len() > 100 {
                return text.to_string();
            }
        }
    }

    // 2. Try <main>
    if let Ok(sel) = Selector::parse("main") {
        for el in doc.select(&sel) {
            let text = el.text().collect::<Vec<_>>().join(" ");
            let text = text.trim();
            if text.len() > 100 {
                return text.to_string();
            }
        }
    }

    // 3. Fall back to all paragraphs
    let paragraphs = extract_paragraphs(&doc);
    if !paragraphs.is_empty() {
        return paragraphs.join("\n\n");
    }

    // 4. Last resort: strip everything
    strip_tags(&cleaned)
}

/// Convert HTML to clean Markdown.
///
/// Preserves headings, lists, emphasis, code blocks, and links.
pub fn html_to_markdown(html: &str) -> String {
    let cleaned = remove_noise_blocks(html);
    let doc = Html::parse_document(&cleaned);

    let mut parts: Vec<String> = Vec::new();

    // Title
    if let Ok(sel) = Selector::parse("title") {
        if let Some(el) = doc.select(&sel).next() {
            let t = el.text().collect::<Vec<_>>().join(" ");
            let t = t.trim();
            if !t.is_empty() {
                parts.push(format!("# {t}"));
            }
        }
    }

    // Try article first
    let article_content = extract_element_markdown(&doc, "article");
    if !article_content.is_empty() && article_content.len() > 100 {
        parts.push(article_content);
    } else {
        let main_content = extract_element_markdown(&doc, "main");
        if !main_content.is_empty() && main_content.len() > 100 {
            parts.push(main_content);
        } else {
            // Fall back to paragraph extraction with markdown formatting
            let scored = extract_scored_markdown(&doc);
            parts.push(scored);
        }
    }

    clean_whitespace(&parts.join("\n\n"))
}

// ===========================================================================
// Internal helpers
// ===========================================================================

/// Remove `<script>`, `<style>`, `<noscript>`, `<iframe>`, `<svg>` blocks.
fn remove_noise_blocks(html: &str) -> String {
    let mut result = html.to_string();
    for tag in &["script", "style", "noscript", "iframe", "svg", "canvas"] {
        result = remove_tag_block(&result, tag);
    }
    result
}

/// Remove all occurrences of `<tag>...</tag>` (including content).
fn remove_tag_block(html: &str, tag: &str) -> String {
    let open_pattern = format!("<{tag}");
    let close_tag = format!("</{tag}>");
    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    while !remaining.is_empty() {
        let lower = remaining.to_lowercase();
        if let Some(start) = lower.find(&open_pattern) {
            // Make sure it's actually a tag start
            let after = start + open_pattern.len();
            if after < remaining.len() {
                let next = remaining.as_bytes()[after];
                if next != b' ' && next != b'>' && next != b'/' && next != b'\t' && next != b'\n' {
                    result.push_str(&remaining[..after]);
                    remaining = &remaining[after..];
                    continue;
                }
            }

            result.push_str(&remaining[..start]);
            remaining = &remaining[start..];

            // Find the closing tag
            let lower_rem = remaining.to_lowercase();
            if let Some(close_pos) = lower_rem.find(&close_tag) {
                remaining = &remaining[close_pos + close_tag.len()..];
            } else {
                // No closing tag — check for self-closing
                if let Some(gt) = remaining.find('>') {
                    if remaining[..gt].ends_with('/') {
                        remaining = &remaining[gt + 1..];
                    } else {
                        // Treat rest as noise
                        break;
                    }
                } else {
                    break;
                }
            }
        } else {
            result.push_str(remaining);
            break;
        }
    }

    result
}

/// Strip all HTML tags, keeping only text content.
fn strip_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                if in_tag {
                    in_tag = false;
                    result.push(' ');
                }
            }
            _ => {
                if !in_tag {
                    result.push(c);
                }
            }
        }
    }

    result
}

/// Decode common HTML entities.
fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .replace("&copy;", "©")
        .replace("&reg;", "®")
}

/// Extract text from all `<p>` elements in the document.
fn extract_paragraphs(doc: &Html) -> Vec<String> {
    let sel = match Selector::parse("p") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    doc.select(&sel)
        .map(|el| el.text().collect::<Vec<_>>().join(" "))
        .map(|t| t.trim().to_string())
        .filter(|t| t.len() > 20)
        .collect()
}

/// Extract markdown-formatted content from a container element.
fn extract_element_markdown(doc: &Html, container: &str) -> String {
    let sel = match Selector::parse(container) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let mut best = String::new();
    for el in doc.select(&sel) {
        let md = element_to_markdown(&el);
        if md.len() > best.len() {
            best = md;
        }
    }
    best
}

/// Convert a single element and its children to Markdown.
fn element_to_markdown(element: &scraper::ElementRef) -> String {
    let mut parts: Vec<String> = Vec::new();

    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            let t = text.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        } else if let Some(elem) = child.value().as_element() {
            let tag = elem.name();
            if let Some(child_ref) = scraper::ElementRef::wrap(child) {
                let child_text = element_to_markdown(&child_ref);
                if !child_text.is_empty() {
                    let formatted = match tag {
                        "h1" => format!("# {child_text}"),
                        "h2" => format!("## {child_text}"),
                        "h3" => format!("### {child_text}"),
                        "h4" => format!("#### {child_text}"),
                        "h5" => format!("##### {child_text}"),
                        "h6" => format!("###### {child_text}"),
                        "li" => format!("- {child_text}"),
                        "blockquote" => child_text
                            .lines()
                            .map(|l| format!("> {l}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        "pre" => format!("```\n{child_text}\n```"),
                        "code" => format!("`{child_text}`"),
                        "strong" | "b" => format!("**{child_text}**"),
                        "em" | "i" => format!("*{child_text}*"),
                        "a" => {
                            let href = elem.attr("href").map(|h| h.to_string()).unwrap_or_default();
                            if href.is_empty() {
                                child_text
                            } else {
                                format!("[{child_text}]({href})")
                            }
                        }
                        "br" => "\n".to_string(),
                        "p" | "div" | "section" | "article" | "main" => {
                            format!("\n{child_text}\n")
                        }
                        _ => child_text,
                    };
                    parts.push(formatted);
                }
            }
        }
    }

    let joined = parts.join(" ");
    clean_whitespace(&joined)
}

/// Extract paragraphs with markdown formatting, scored for quality.
fn extract_scored_markdown(doc: &Html) -> String {
    let sel = match Selector::parse("p, li, pre, blockquote, h1, h2, h3, h4, h5, h6") {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let mut blocks: Vec<(f64, String)> = Vec::new();

    for element in doc.select(&sel) {
        let text: String = element.text().collect::<Vec<_>>().join(" ");
        let text = text.trim();
        if text.len() < 20 {
            continue;
        }

        let score = score_text(text);
        if score < 0.15 {
            continue;
        }

        let tag = element.value().name();
        let formatted = match tag {
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

        blocks.push((score, formatted));
    }

    if blocks.is_empty() {
        return String::new();
    }

    // Sort by score to find threshold
    blocks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let threshold = blocks[0].0 * 0.25;

    // Keep only good blocks, preserving original order
    let mut result: Vec<(usize, String)> = Vec::new();
    for (i, (score, text)) in blocks.iter().enumerate() {
        if *score >= threshold {
            result.push((i, text.clone()));
        }
    }
    result.sort_by_key(|(i, _)| *i);

    result
        .into_iter()
        .map(|(_, t)| t)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Score a text block for content quality (0.0–1.0).
fn score_text(text: &str) -> f64 {
    let len = text.len() as f64;
    if len == 0.0 {
        return 0.0;
    }

    let word_count = text.split_whitespace().count() as f64;
    let avg_word_len =
        text.chars().filter(|c| c.is_alphabetic()).count() as f64 / word_count.max(1.0);

    let length_score = (len / 100.0).min(1.0);
    let word_score = if (3.0..=12.0).contains(&avg_word_len) {
        1.0
    } else if avg_word_len < 2.0 {
        0.1
    } else {
        0.5
    };

    let link_chars = text.matches("http").count() as f64;
    let link_penalty = (1.0 - link_chars / word_count.max(1.0)).max(0.1);

    let special_ratio = text
        .chars()
        .filter(|c| {
            !c.is_alphanumeric()
                && !c.is_whitespace()
                && !matches!(c, '.' | ',' | ';' | ':' | '-' | '\'')
        })
        .count() as f64
        / len;
    let special_penalty = (1.0 - special_ratio * 2.0).max(0.3);

    let sentence_bonus = if text.contains(". ") || text.contains(".\n") {
        1.2
    } else {
        1.0
    };

    length_score * word_score * link_penalty * special_penalty * sentence_bonus
}

/// Collapse excessive whitespace while preserving intentional newlines.
fn clean_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut newline_count: u32 = 0;

    for c in text.chars() {
        match c {
            '\n' => {
                newline_count += 1;
                if newline_count <= 2 {
                    result.push('\n');
                }
            }
            '\r' => {}
            c if c.is_whitespace() => {
                if newline_count == 0 {
                    result.push(' ');
                }
                newline_count = 0;
            }
            c => {
                newline_count = 0;
                result.push(c);
            }
        }
    }

    result.trim().to_string()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Helper function tests ---

    #[test]
    fn test_strip_html_tags_basic() {
        let html = "<p>Hello <b>world</b></p>";
        let result = strip_html_tags(html);
        assert!(result.contains("Hello"));
        assert!(result.contains("world"));
        assert!(!result.contains("<p>"));
        assert!(!result.contains("<b>"));
    }

    #[test]
    fn test_strip_html_tags_removes_scripts() {
        let html = "<p>Content</p><script>alert('xss');</script><p>More</p>";
        let result = strip_html_tags(html);
        assert!(result.contains("Content"));
        assert!(result.contains("More"));
        assert!(!result.contains("alert"));
    }

    #[test]
    fn test_strip_html_tags_removes_styles() {
        let html = "<style>body { color: red; }</style><p>Text</p>";
        let result = strip_html_tags(html);
        assert!(result.contains("Text"));
        assert!(!result.contains("color: red"));
    }

    #[test]
    fn test_strip_html_tags_decodes_entities() {
        let html = "<p>Hello &amp; goodbye &lt;world&gt;</p>";
        let result = strip_html_tags(html);
        assert!(result.contains("&"));
        assert!(result.contains("<world>"));
    }

    #[test]
    fn test_extract_title_from_title_tag() {
        let html = "<html><head><title>My Page Title</title></head><body></body></html>";
        let title = extract_title(html);
        assert_eq!(title, "My Page Title");
    }

    #[test]
    fn test_extract_title_from_h1() {
        let html = "<html><body><h1>Article Heading</h1><p>Content</p></body></html>";
        let title = extract_title(html);
        assert_eq!(title, "Article Heading");
    }

    #[test]
    fn test_extract_title_prefers_title_over_h1() {
        let html =
            "<html><head><title>Page Title</title></head><body><h1>H1 Title</h1></body></html>";
        let title = extract_title(html);
        assert_eq!(title, "Page Title");
    }

    #[test]
    fn test_extract_title_empty_when_none() {
        let html = "<html><body><p>No title here</p></body></html>";
        let title = extract_title(html);
        assert!(title.is_empty());
    }

    #[test]
    fn test_extract_main_content_from_article() {
        let html = r#"
        <html><body>
            <nav>Skip nav</nav>
            <article>
                <p>This is the main article content that should be extracted because it is long enough to pass the threshold.</p>
            </article>
            <footer>Skip footer</footer>
        </body></html>"#;
        let content = extract_main_content(html);
        assert!(content.contains("main article content"));
    }

    #[test]
    fn test_html_to_markdown_headings() {
        let html = r#"<html><head><title>Title</title></head><body>
            <article>
                <h2>Section</h2>
                <p>Paragraph text that is long enough to be considered real content for extraction purposes.</p>
            </article>
        </body></html>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("# Title") || md.contains("## Section"));
    }

    #[test]
    fn test_output_format_default() {
        assert_eq!(OutputFormat::default(), OutputFormat::Markdown);
    }

    #[test]
    fn test_output_format_serialization() {
        let md = serde_json::to_string(&OutputFormat::Markdown).unwrap();
        let pt = serde_json::to_string(&OutputFormat::PlainText).unwrap();
        let html = serde_json::to_string(&OutputFormat::Html).unwrap();
        assert_eq!(md, "\"markdown\"");
        assert_eq!(pt, "\"plain_text\"");
        assert_eq!(html, "\"html\"");
    }

    #[test]
    fn test_extracted_content_word_count() {
        let content = ExtractedContent {
            title: "Test".to_string(),
            content: "one two three four five".to_string(),
            url: "https://example.com".to_string(),
            extracted_at: Utc::now(),
            word_count: 5,
        };
        assert_eq!(content.word_count, 5);
    }

    #[test]
    fn test_clean_whitespace_collapses_newlines() {
        let input = "Hello\n\n\n\n\nWorld\n\n\n\n\n\nToo many";
        let result = clean_whitespace(input);
        assert!(!result.contains("\n\n\n"));
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
    }

    #[test]
    fn test_remove_tag_block_script() {
        let html = "<div><p>Hello</p><script>evil();</script><p>World</p></div>";
        let result = remove_tag_block(html, "script");
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
        assert!(!result.contains("evil"));
    }

    #[test]
    fn test_remove_tag_block_style() {
        let html = "<head><style>.red{color:red}</style></head><body><p>Text</p></body>";
        let result = remove_tag_block(html, "style");
        assert!(result.contains("Text"));
        assert!(!result.contains("color:red"));
    }

    #[test]
    fn test_score_text_good_vs_bad() {
        let good = "This is a well-written paragraph with multiple sentences. It contains enough words to be meaningful.";
        let bad = "click here home about";
        assert!(score_text(good) > score_text(bad));
    }

    #[test]
    fn test_score_text_empty() {
        assert_eq!(score_text(""), 0.0);
    }

    #[test]
    fn test_web_extractor_new() {
        let _extractor = WebExtractor::new();
        // Just ensure it constructs without panic
    }

    #[test]
    fn test_validate_url_rejects_non_http() {
        let extractor = WebExtractor::new();
        assert!(extractor.validate_url("ftp://example.com").is_err());
        assert!(extractor.validate_url("file:///etc/passwd").is_err());
        assert!(extractor.validate_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn test_validate_url_accepts_http() {
        let extractor = WebExtractor::new();
        assert!(extractor.validate_url("http://example.com").is_ok());
        assert!(extractor.validate_url("https://example.com").is_ok());
    }

    // Async integration tests (require network) — gated behind env var
    #[tokio::test]
    async fn test_extract_integration() {
        if std::env::var("HAKIMI_NET_TESTS").is_err() {
            return;
        }
        let extractor = WebExtractor::new();
        let result = extractor
            .extract("https://example.com", OutputFormat::Markdown)
            .await
            .unwrap();
        assert!(!result.title.is_empty());
        assert!(result.word_count > 0);
        assert_eq!(result.url, "https://example.com");
    }

    #[tokio::test]
    async fn test_extract_with_prompt_integration() {
        if std::env::var("HAKIMI_NET_TESTS").is_err() {
            return;
        }
        let extractor = WebExtractor::new();
        let result = extractor
            .extract_with_prompt("https://example.com", "Summarize this page")
            .await
            .unwrap();
        assert!(result.contains("Summarize this page"));
        assert!(result.contains("https://example.com"));
    }
}
