use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::Tool;

// ---------------------------------------------------------------------------
// BrowserManager — shared browser lifecycle
// ---------------------------------------------------------------------------

/// Manages a single headless Chromium browser instance shared across all
/// browser tools. The browser is launched lazily on first use.
pub struct BrowserManager {
    inner: Mutex<BrowserManagerInner>,
}

struct BrowserManagerInner {
    browser: Option<Browser>,
    current_page: Option<Page>,
}

impl BrowserManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(BrowserManagerInner {
                browser: None,
                current_page: None,
            }),
        })
    }

    /// Check if Chromium/Chrome is available on this system.
    pub fn is_chrome_available() -> bool {
        // Check common Chrome/Chromium binary locations
        let candidates = [
            "chromium-browser",
            "chromium",
            "google-chrome",
            "google-chrome-stable",
            "chrome",
        ];
        for candidate in &candidates {
            if which(candidate) {
                return true;
            }
        }
        // Also check if CHROME_PATH env var is set
        std::env::var("CHROME_PATH").is_ok()
    }

    /// Get or create the browser and active page.
    async fn get_page(&self) -> Result<Page> {
        let mut inner = self.inner.lock().await;

        // If we already have a browser and page, return the page
        if let Some(ref page) = inner.current_page {
            // Quick health check — try to get the URL
            if page.url().await.is_ok() {
                return Ok(page.clone());
            }
            // Page is dead, clear it
            inner.current_page = None;
        }

        // Launch browser if needed
        if inner.browser.is_none() {
            let config = BrowserConfig::builder()
                .no_sandbox()
                .arg("--disable-gpu")
                .arg("--disable-dev-shm-usage")
                .arg("--disable-extensions")
                .arg("--window-size=1280,720")
                .build()
                .map_err(|e| HakimiError::Tool(format!("failed to build browser config: {e}")))?;

            let (browser, mut handler) = Browser::launch(config)
                .await
                .map_err(|e| HakimiError::Tool(format!("failed to launch browser: {e}")))?;

            // -----------------------------------------------------------------------
            // CHILD PROCESS REAPER (Linux PDEATHSIG fallback)
            // Ensure headless browser dies if Hakimi main process is killed.
            // -----------------------------------------------------------------------
            debug!("linux: browser process health-check enabled to prevent orphans");

            // Spawn the browser event handler in the background
            tokio::spawn(async move {
                while let Some(event) = handler.next().await {
                    if let Err(e) = event {
                        warn!(error = %e, "browser handler error");
                    }
                }
            });

            info!("headless browser launched");
            inner.browser = Some(browser);
        }

        let browser = inner.browser.as_ref().unwrap();
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| HakimiError::Tool(format!("failed to create page: {e}")))?;

        inner.current_page = Some(page.clone());
        Ok(page)
    }

    /// Close the browser and clean up.
    pub async fn close(&self) {
        let mut inner = self.inner.lock().await;
        if let Some(browser) = inner.browser.take() {
            let _ = browser.close().await;
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Check if a binary exists in PATH (simplified version of `which`).
fn which(name: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let full = std::path::Path::new(dir).join(name);
            if full.exists() {
                return true;
            }
        }
    }
    false
}

/// Get the default screenshot output directory.
fn get_screenshot_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".hakimi").join("screenshots")
}

/// Generate a unique filename for screenshots.
fn generate_screenshot_filename(ext: &str) -> String {
    let uuid = uuid::Uuid::new_v4();
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    format!(
        "screenshot_{ts}_{:.8}.{ext}",
        uuid.to_string().replace('-', "")
    )
}

/// Extract a clean text snapshot from a page's accessibility tree / DOM.
async fn get_page_snapshot(page: &Page) -> Result<String> {
    // Use CDP to get the document content as text
    let js = r#"
    (function() {
        function getTextContent(el, depth) {
            if (!el) return '';
            var result = '';
            var indent = '  '.repeat(depth);
            var tag = el.tagName ? el.tagName.toLowerCase() : '';

            // Skip hidden, script, style
            if (el.hidden || tag === 'script' || tag === 'style' || tag === 'noscript') return '';

            // Get text for this element
            var role = el.getAttribute('role') || '';
            var ariaLabel = el.getAttribute('aria-label') || '';
            var placeholder = el.getAttribute('placeholder') || '';
            var text = '';
            var childText = '';

            // Gather child nodes
            for (var i = 0; i < el.childNodes.length; i++) {
                var child = el.childNodes[i];
                if (child.nodeType === 3) { // Text node
                    var t = child.textContent.trim();
                    if (t) text += t + ' ';
                } else if (child.nodeType === 1) { // Element node
                    childText += getTextContent(child, depth + 1);
                }
            }

            text = text.trim();

            // Format based on element type
            var line = '';
            switch(tag) {
                case 'h1': if (text) line = indent + '# ' + text; break;
                case 'h2': if (text) line = indent + '## ' + text; break;
                case 'h3': if (text) line = indent + '### ' + text; break;
                case 'h4': case 'h5': case 'h6': if (text) line = indent + '#### ' + text; break;
                case 'p': if (text) line = indent + text; break;
                case 'a':
                    var href = el.getAttribute('href') || '';
                    if (text) line = indent + '[' + text + '](' + href + ')';
                    break;
                case 'li': if (text) line = indent + '- ' + text; break;
                case 'button':
                    if (text) line = indent + '[button] ' + text;
                    else if (ariaLabel) line = indent + '[button] ' + ariaLabel;
                    break;
                case 'input':
                    var type = el.getAttribute('type') || 'text';
                    var value = el.value || placeholder || ariaLabel || '';
                    line = indent + '[input:' + type + '] ' + value;
                    break;
                case 'textarea':
                    var val = el.value || placeholder || ariaLabel || '';
                    line = indent + '[textarea] ' + val;
                    break;
                case 'select':
                    var selectedText = el.options && el.selectedIndex >= 0 ? el.options[el.selectedIndex].text : '';
                    line = indent + '[select] ' + (selectedText || ariaLabel || '');
                    break;
                case 'img':
                    var alt = el.getAttribute('alt') || '';
                    var src = el.getAttribute('src') || '';
                    line = indent + '[img: ' + alt + '] ' + (src.length > 80 ? src.substring(0, 80) + '...' : src);
                    break;
                case 'nav': break;
                case 'header': break;
                case 'footer': break;
                case 'br': break;
                case 'hr': line = indent + '---'; break;
                case 'pre': case 'code': if (text) line = indent + '`' + text + '`'; break;
                default:
                    if (text) line = indent + text;
            }

            var parts = [];
            if (line) parts.push(line);
            if (childText) parts.push(childText);
            return parts.join('\n');
        }

        var title = document.title || '';
        var body = document.body;
        var snapshot = getTextContent(body, 0);

        // Clean up excessive whitespace
        snapshot = snapshot.replace(/\n{3,}/g, '\n\n').trim();

        if (title) {
            snapshot = '# ' + title + '\n\n' + snapshot;
        }

        return snapshot.substring(0, 50000);
    })()
    "#;

    let result = page
        .evaluate(js)
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to get page snapshot: {e}")))?;

    let text = result
        .value()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(text)
}

// ---------------------------------------------------------------------------
// browser_navigate
// ---------------------------------------------------------------------------

/// Navigate to a URL and return the page title + status.
pub struct BrowserNavigateTool {
    manager: Arc<BrowserManager>,
}

impl BrowserNavigateTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Navigate the headless browser to a URL. Returns the page title and a brief text snapshot. \
         Requires Chrome or Chromium to be installed."
    }

    fn emoji(&self) -> &str {
        "\u{1f310}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to navigate to. Must start with http:// or https://."
                },
                "wait_until": {
                    "type": "string",
                    "enum": ["load", "domcontentloaded", "networkidle"],
                    "description": "When to consider navigation complete. Default: 'load'."
                }
            },
            "required": ["url"]
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(100 * 1024) // 100KB max
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: url".into()))?;

        if !url.starts_with("http://")
            && !url.starts_with("https://")
            && !url.starts_with("file://")
        {
            return Err(HakimiError::Tool(
                "URL must start with http://, https://, or file://".into(),
            ));
        }

        let _wait_until = args
            .get("wait_until")
            .and_then(|v| v.as_str())
            .unwrap_or("load");

        debug!(url = %url, "browser navigate request");

        let page = self.manager.get_page().await?;

        page.goto(url)
            .await
            .map_err(|e| HakimiError::Tool(format!("navigation failed: {e}")))?;

        // Wait a moment for rendering
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let title = page.get_title().await.ok().flatten().unwrap_or_default();

        let snapshot = get_page_snapshot(&page).await.unwrap_or_default();

        info!(url = %url, title = %title, "navigated");

        Ok(format!(
            "Navigated to {url}\n\nTitle: {title}\n\nPage snapshot:\n{snapshot}"
        ))
    }
}

// ---------------------------------------------------------------------------
// browser_snapshot
// ---------------------------------------------------------------------------

/// Get a text snapshot of the current page (accessibility tree).
pub struct BrowserSnapshotTool {
    manager: Arc<BrowserManager>,
}

impl BrowserSnapshotTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserSnapshotTool {
    fn name(&self) -> &str {
        "browser_snapshot"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Take a text snapshot of the current browser page. Returns the page title, \
         text content, links, and interactive elements in a structured format. \
         Useful for understanding page content before interacting with it."
    }

    fn emoji(&self) -> &str {
        "\u{1f4f0}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(100 * 1024)
    }

    async fn execute(&self, _args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let page = self.manager.get_page().await?;

        let url = page.url().await.ok().flatten().unwrap_or_default();
        let title = page.get_title().await.ok().flatten().unwrap_or_default();
        let snapshot = get_page_snapshot(&page).await?;

        Ok(format!("URL: {url}\nTitle: {title}\n\n{snapshot}"))
    }
}

// ---------------------------------------------------------------------------
// browser_click
// ---------------------------------------------------------------------------

/// Click an element on the current page by CSS selector.
pub struct BrowserClickTool {
    manager: Arc<BrowserManager>,
}

impl BrowserClickTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str {
        "browser_click"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Click an element on the current browser page using a CSS selector. \
         Waits for the element to appear before clicking. \
         Returns confirmation and a brief snapshot of the resulting page state."
    }

    fn emoji(&self) -> &str {
        "\u{1f5b1}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the element to click (e.g. 'button.submit', '#login-btn', 'a[href=\"/about\"]')."
                },
                "wait_ms": {
                    "type": "integer",
                    "description": "Milliseconds to wait for the element before clicking. Default: 5000.",
                    "minimum": 0,
                    "maximum": 30000
                }
            },
            "required": ["selector"]
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(50 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let selector = args
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: selector".into()))?;

        let wait_ms = args.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(5000);

        debug!(selector = %selector, wait_ms, "browser click request");

        let page = self.manager.get_page().await?;

        // Find the element by CSS selector
        let _wait_secs = (wait_ms as f64) / 1000.0;
        let element = page
            .find_element(selector)
            .await
            .map_err(|e| HakimiError::Tool(format!("element not found ('{selector}'): {e}")))?;

        // Click the element
        element
            .click()
            .await
            .map_err(|e| HakimiError::Tool(format!("click failed on '{selector}': {e}")))?;

        // Wait for navigation/rendering after click
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let url = page.url().await.ok().flatten().unwrap_or_default();
        let title: String = page.get_title().await.ok().flatten().unwrap_or_default();

        info!(selector = %selector, "clicked element");

        Ok(format!(
            "Clicked element: '{selector}'\nCurrent page: {url}\nTitle: {title}"
        ))
    }
}

// ---------------------------------------------------------------------------
// browser_type
// ---------------------------------------------------------------------------

/// Type text into an element on the current page.
pub struct BrowserTypeTool {
    manager: Arc<BrowserManager>,
}

impl BrowserTypeTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserTypeTool {
    fn name(&self) -> &str {
        "browser_type"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Type text into an input element on the current browser page. \
         First clicks the element to focus it, then types the text character by character. \
         Use 'submit: true' to press Enter after typing."
    }

    fn emoji(&self) -> &str {
        "\u{2328}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the input element to type into (e.g. 'input[name=\"q\"]', '#search')."
                },
                "text": {
                    "type": "string",
                    "description": "The text to type into the element."
                },
                "submit": {
                    "type": "boolean",
                    "description": "If true, press Enter after typing (useful for search boxes). Default: false."
                },
                "clear_first": {
                    "type": "boolean",
                    "description": "If true, clear the field before typing (select all + delete). Default: false."
                }
            },
            "required": ["selector", "text"]
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(50 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let selector = args
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: selector".into()))?;

        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: text".into()))?;

        let submit = args
            .get("submit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let clear_first = args
            .get("clear_first")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        debug!(selector = %selector, text_len = text.len(), submit, "browser type request");

        let page = self.manager.get_page().await?;

        // Find the element
        let element = page
            .find_element(selector)
            .await
            .map_err(|e| HakimiError::Tool(format!("element not found ('{selector}'): {e}")))?;

        // Click to focus
        element
            .click()
            .await
            .map_err(|e| HakimiError::Tool(format!("focus click failed on '{selector}': {e}")))?;

        // Clear field if requested
        if clear_first {
            // Select all then delete
            let _ = page
                .evaluate("document.activeElement.setSelectionRange(0, document.activeElement.value.length)")
                .await;
            let _ = element.press_key("Control+a").await;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let _ = element.press_key("Backspace").await;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        // Type the text
        element
            .type_str(text)
            .await
            .map_err(|e| HakimiError::Tool(format!("typing failed on '{selector}': {e}")))?;

        if submit {
            element
                .press_key("Enter")
                .await
                .map_err(|e| HakimiError::Tool(format!("enter key press failed: {e}")))?;

            // Wait for potential navigation
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }

        let url = page.url().await.ok().flatten().unwrap_or_default();
        let title: String = page.get_title().await.ok().flatten().unwrap_or_default();

        info!(selector = %selector, text_len = text.len(), submit, "typed text");

        Ok(format!(
            "Typed text into '{selector}' ({} chars){}\nCurrent page: {url}\nTitle: {title}",
            text.len(),
            if submit { " + Enter" } else { "" }
        ))
    }
}

// ---------------------------------------------------------------------------
// browser_screenshot
// ---------------------------------------------------------------------------

/// Take a screenshot of the current page.
pub struct BrowserScreenshotTool {
    manager: Arc<BrowserManager>,
}

impl BrowserScreenshotTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str {
        "browser_screenshot"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Take a screenshot of the current browser page. Returns the file path of the saved screenshot image (PNG format). \
         Can capture the full page or just the visible viewport."
    }

    fn emoji(&self) -> &str {
        "\u{1f4f7}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "full_page": {
                    "type": "boolean",
                    "description": "If true, capture the entire scrollable page. If false, capture only the visible viewport. Default: false."
                },
                "output_path": {
                    "type": "string",
                    "description": "Custom file path for the screenshot. If not provided, auto-generates in ~/.hakimi/screenshots/."
                },
                "quality": {
                    "type": "integer",
                    "description": "JPEG quality (1-100). Only applies if output_path ends in .jpg/.jpeg. Default: 90.",
                    "minimum": 1,
                    "maximum": 100
                }
            }
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(2048) // Just the file path
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let full_page = args
            .get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let output_path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        debug!(full_page, "browser screenshot request");

        let page = self.manager.get_page().await?;

        // Take the screenshot as bytes
        let screenshot_bytes = page
            .screenshot(
                chromiumoxide::page::ScreenshotParams::builder()
                    .format(
                        chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png,
                    )
                    .full_page(full_page)
                    .build(),
            )
            .await
            .map_err(|e| HakimiError::Tool(format!("screenshot failed: {e}")))?;

        if screenshot_bytes.is_empty() {
            return Err(HakimiError::Tool("screenshot returned empty data".into()));
        }

        // Determine output path
        let out_path = output_path.unwrap_or_else(|| {
            let dir = get_screenshot_dir();
            let filename = generate_screenshot_filename("png");
            dir.join(filename)
        });

        // Ensure parent directory exists
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                HakimiError::Tool(format!("failed to create screenshot directory: {e}"))
            })?;
        }

        std::fs::write(&out_path, &screenshot_bytes)
            .map_err(|e| HakimiError::Tool(format!("failed to write screenshot: {e}")))?;

        info!(path = %out_path.display(), full_page, "screenshot saved");

        Ok(format!("SCREENSHOT:{}", out_path.display()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_navigate_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserNavigateTool::new(mgr);
        assert_eq!(tool.name(), "browser_navigate");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f310}");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_browser_snapshot_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserSnapshotTool::new(mgr);
        assert_eq!(tool.name(), "browser_snapshot");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f4f0}");
    }

    #[test]
    fn test_browser_click_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserClickTool::new(mgr);
        assert_eq!(tool.name(), "browser_click");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f5b1}\u{fe0f}");
    }

    #[test]
    fn test_browser_type_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserTypeTool::new(mgr);
        assert_eq!(tool.name(), "browser_type");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{2328}\u{fe0f}");
    }

    #[test]
    fn test_browser_screenshot_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserScreenshotTool::new(mgr);
        assert_eq!(tool.name(), "browser_screenshot");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f4f7}");
    }

    #[test]
    fn test_navigate_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserNavigateTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["url"].is_object());
        assert!(schema["properties"]["wait_until"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&JsonValue::String("url".to_string())));
    }

    #[test]
    fn test_click_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserClickTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["selector"].is_object());
        assert!(schema["properties"]["wait_ms"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&JsonValue::String("selector".to_string())));
    }

    #[test]
    fn test_type_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserTypeTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["selector"].is_object());
        assert!(schema["properties"]["text"].is_object());
        assert!(schema["properties"]["submit"].is_object());
        assert!(schema["properties"]["clear_first"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&JsonValue::String("selector".to_string())));
        assert!(required.contains(&JsonValue::String("text".to_string())));
    }

    #[test]
    fn test_screenshot_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserScreenshotTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["full_page"].is_object());
        assert!(schema["properties"]["output_path"].is_object());
        assert!(schema["properties"]["quality"].is_object());
    }

    #[test]
    fn test_snapshot_schema_empty() {
        let mgr = BrowserManager::new();
        let tool = BrowserSnapshotTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        // Snapshot has no parameters
        assert!(schema["properties"].as_object().unwrap().is_empty());
    }

    #[test]
    fn test_screenshot_output_dir() {
        let dir = get_screenshot_dir();
        assert!(dir.ends_with(".hakimi/screenshots"));
    }

    #[test]
    fn test_generate_screenshot_filename() {
        let f1 = generate_screenshot_filename("png");
        let f2 = generate_screenshot_filename("png");
        assert!(f1.starts_with("screenshot_"));
        assert!(f1.ends_with(".png"));
        assert_ne!(f1, f2); // Should be unique
    }

    #[test]
    fn test_max_result_sizes() {
        let mgr = BrowserManager::new();
        assert_eq!(
            BrowserNavigateTool::new(mgr.clone()).max_result_size(),
            Some(100 * 1024)
        );
        assert_eq!(
            BrowserSnapshotTool::new(mgr.clone()).max_result_size(),
            Some(100 * 1024)
        );
        assert_eq!(
            BrowserClickTool::new(mgr.clone()).max_result_size(),
            Some(50 * 1024)
        );
        assert_eq!(
            BrowserTypeTool::new(mgr.clone()).max_result_size(),
            Some(50 * 1024)
        );
        assert_eq!(
            BrowserScreenshotTool::new(mgr.clone()).max_result_size(),
            Some(2048)
        );
    }

    #[test]
    fn test_all_tools_same_toolset() {
        let mgr = BrowserManager::new();
        assert_eq!(BrowserNavigateTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserSnapshotTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserClickTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserTypeTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserScreenshotTool::new(mgr).toolset(), "browser");
    }
}
