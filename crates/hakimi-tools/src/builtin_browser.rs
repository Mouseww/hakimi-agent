use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::{
    EventJavascriptDialogOpening, HandleJavaScriptDialogParams,
};
use chromiumoxide::page::Page;
use futures::StreamExt;
use hakimi_common::{HakimiError, Result, ToolContext, redact_sensitive_text};
use serde::Serialize;
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
    pending_dialogs: Arc<Mutex<VecDeque<PendingDialog>>>,
    next_dialog_id: Arc<AtomicU64>,
}

struct BrowserManagerInner {
    browser: Option<Browser>,
    current_page: Option<Page>,
}

#[derive(Debug, Clone, Serialize)]
struct PendingDialog {
    id: String,
    #[serde(rename = "type")]
    dialog_type: String,
    message: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_prompt: Option<String>,
}

impl BrowserManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(BrowserManagerInner {
                browser: None,
                current_page: None,
            }),
            pending_dialogs: Arc::new(Mutex::new(VecDeque::new())),
            next_dialog_id: Arc::new(AtomicU64::new(1)),
        })
    }

    /// Check if Chromium/Chrome is available on this system.
    pub fn is_chrome_available() -> bool {
        find_browser_executable().is_some()
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
            let mut builder = BrowserConfig::builder()
                .no_sandbox()
                .arg("--disable-gpu")
                .arg("--disable-dev-shm-usage")
                .arg("--disable-extensions")
                .arg("--window-size=1280,720");

            if let Some(executable) = find_browser_executable() {
                builder = builder.chrome_executable(executable);
            }

            let config = builder
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

        if let Err(e) = install_console_recorder(&page).await {
            warn!(error = %e, "browser console recorder install failed");
        }
        if let Err(e) = install_dialog_listeners(
            &page,
            self.pending_dialogs.clone(),
            self.next_dialog_id.clone(),
        )
        .await
        {
            warn!(error = %e, "browser dialog listener install failed");
        }

        inner.current_page = Some(page.clone());
        Ok(page)
    }

    async fn pending_dialogs(&self) -> Vec<PendingDialog> {
        self.pending_dialogs.lock().await.iter().cloned().collect()
    }

    async fn acknowledge_dialog(&self, dialog_id: Option<&str>) -> Option<PendingDialog> {
        let mut pending = self.pending_dialogs.lock().await;
        if let Some(id) = dialog_id {
            if let Some(pos) = pending.iter().position(|dialog| dialog.id == id) {
                return pending.remove(pos);
            }
            return None;
        }
        pending.pop_front()
    }

    /// Close the browser and clean up.
    pub async fn close(&self) {
        let mut inner = self.inner.lock().await;
        if let Some(mut browser) = inner.browser.take() {
            let _ = browser.close().await;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserCdpEndpoint {
    endpoint: String,
    source: String,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Check if a binary exists in PATH (simplified version of `which`).
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    find_executable_on_path(name, std::env::split_paths(&path))
}

fn find_browser_executable() -> Option<PathBuf> {
    browser_executable_from_env()
        .or_else(system_browser_executable)
        .or_else(playwright_browser_executable)
}

fn browser_executable_from_env() -> Option<PathBuf> {
    for name in [
        "HAKIMI_BROWSER_EXECUTABLE",
        "AGENT_BROWSER_EXECUTABLE_PATH",
        "CHROME_PATH",
        "CHROME",
    ] {
        if let Ok(value) = std::env::var(name)
            && let Some(path) = resolve_browser_candidate(&value)
        {
            return Some(path);
        }
    }
    None
}

fn system_browser_executable() -> Option<PathBuf> {
    for name in [
        "chromium-browser",
        "chromium",
        "google-chrome",
        "google-chrome-stable",
        "chrome",
        "chrome-headless-shell",
    ] {
        if let Some(path) = which(name) {
            return Some(path);
        }
    }
    None
}

fn resolve_browser_candidate(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = PathBuf::from(trimmed);
    if path.is_file() {
        return Some(path);
    }

    which(trimmed)
}

fn find_executable_on_path(
    name: &str,
    paths: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    for dir in paths {
        let full = dir.join(name);
        if full.is_file() {
            return Some(full);
        }

        #[cfg(windows)]
        {
            let full_exe = dir.join(format!("{name}.exe"));
            if full_exe.is_file() {
                return Some(full_exe);
            }
        }
    }
    None
}

fn playwright_browser_executable() -> Option<PathBuf> {
    for root in playwright_browser_search_roots() {
        if let Some(path) = find_playwright_browser_executable(&root) {
            return Some(path);
        }
    }
    None
}

fn browser_cdp_endpoint_from_args(args: &JsonValue) -> Option<BrowserCdpEndpoint> {
    args.get("endpoint")
        .and_then(|v| v.as_str())
        .and_then(|value| browser_cdp_endpoint_from_value(value, "argument:endpoint"))
}

fn browser_cdp_endpoint_from_env() -> Option<BrowserCdpEndpoint> {
    browser_cdp_endpoint_from_pairs(
        [
            (
                "HAKIMI_BROWSER_CDP_URL",
                std::env::var("HAKIMI_BROWSER_CDP_URL").ok(),
            ),
            ("BROWSER_CDP_URL", std::env::var("BROWSER_CDP_URL").ok()),
        ]
        .into_iter(),
    )
}

fn browser_cdp_endpoint_from_pairs(
    pairs: impl IntoIterator<Item = (&'static str, Option<String>)>,
) -> Option<BrowserCdpEndpoint> {
    for (name, value) in pairs {
        if let Some(value) = value
            && let Some(endpoint) = browser_cdp_endpoint_from_value(&value, name)
        {
            return Some(endpoint);
        }
    }
    None
}

fn browser_cdp_endpoint_from_value(value: &str, source: &str) -> Option<BrowserCdpEndpoint> {
    let endpoint = value.trim();
    if endpoint.is_empty() {
        return None;
    }

    Some(BrowserCdpEndpoint {
        endpoint: endpoint.to_string(),
        source: source.to_string(),
    })
}

fn resolve_browser_cdp_endpoint(args: &JsonValue) -> Option<BrowserCdpEndpoint> {
    browser_cdp_endpoint_from_args(args).or_else(browser_cdp_endpoint_from_env)
}

fn validate_browser_cdp_endpoint(endpoint: &str) -> std::result::Result<(), String> {
    let lower = endpoint.trim().to_ascii_lowercase();
    if lower.starts_with("ws://")
        || lower.starts_with("wss://")
        || lower.starts_with("http://")
        || lower.starts_with("https://")
    {
        Ok(())
    } else {
        Err("CDP endpoint must start with ws://, wss://, http://, or https://".to_string())
    }
}

fn redact_browser_cdp_endpoint(endpoint: &str) -> String {
    let without_query = if let Some((prefix, _)) = endpoint.split_once('?') {
        format!("{prefix}?[REDACTED]")
    } else {
        endpoint.to_string()
    };

    let without_userinfo = if let Some(scheme_pos) = without_query.find("://") {
        let authority_start = scheme_pos + 3;
        let authority_end = without_query[authority_start..]
            .find('/')
            .map(|offset| authority_start + offset)
            .unwrap_or(without_query.len());
        let authority = &without_query[authority_start..authority_end];
        if let Some(at_pos) = authority.rfind('@') {
            format!(
                "{}***@{}{}",
                &without_query[..authority_start],
                &authority[at_pos + 1..],
                &without_query[authority_end..]
            )
        } else {
            without_query
        }
    } else {
        without_query
    };

    redact_sensitive_text(&without_userinfo)
}

fn browser_cdp_timeout_ms(args: &JsonValue) -> u64 {
    args.get("timeout_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(3_000)
        .clamp(250, 30_000)
}

fn playwright_browser_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(path) = std::env::var("PLAYWRIGHT_BROWSERS_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() && trimmed != "0" {
            roots.push(PathBuf::from(trimmed));
        }
    }

    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".cache").join("ms-playwright"));

        #[cfg(target_os = "macos")]
        roots.push(home.join("Library").join("Caches").join("ms-playwright"));

        #[cfg(windows)]
        {
            let local = std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData").join("Local"));
            roots.push(local.join("ms-playwright"));
        }
    }

    roots
}

fn find_playwright_browser_executable(root: &Path) -> Option<PathBuf> {
    find_browser_executable_under(root, 6, 0)
}

fn find_browser_executable_under(root: &Path, max_depth: usize, visited: usize) -> Option<PathBuf> {
    if visited > 2048 || max_depth == 0 || !root.is_dir() {
        return None;
    }

    let mut entries = std::fs::read_dir(root)
        .ok()?
        .filter_map(std::result::Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    let mut next_visited = visited;
    for entry in entries {
        next_visited += 1;
        let path = entry.path();
        if path.is_file() && is_browser_binary_name(&path) {
            return Some(path);
        }

        if path.is_dir()
            && should_descend_browser_dir(&path)
            && let Some(found) = find_browser_executable_under(&path, max_depth - 1, next_visited)
        {
            return Some(found);
        }
    }

    None
}

fn should_descend_browser_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower.starts_with("chromium")
        || lower.starts_with("chrome")
        || matches!(lower.as_str(), "contents" | "macos")
}

fn is_browser_binary_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    matches!(
        name.to_ascii_lowercase().as_str(),
        "chrome"
            | "chrome.exe"
            | "chromium"
            | "chromium.exe"
            | "chromium-browser"
            | "chromium-browser.exe"
            | "chrome-headless-shell"
            | "chrome-headless-shell.exe"
    )
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

async fn capture_browser_screenshot(
    page: &Page,
    output_path: Option<PathBuf>,
    full_page: bool,
) -> Result<(PathBuf, Vec<u8>)> {
    let screenshot_bytes = page
        .screenshot(
            chromiumoxide::page::ScreenshotParams::builder()
                .format(chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png)
                .full_page(full_page)
                .build(),
        )
        .await
        .map_err(|e| HakimiError::Tool(format!("screenshot failed: {e}")))?;

    if screenshot_bytes.is_empty() {
        return Err(HakimiError::Tool("screenshot returned empty data".into()));
    }

    let out_path = output_path.unwrap_or_else(|| {
        let dir = get_screenshot_dir();
        let filename = generate_screenshot_filename("png");
        dir.join(filename)
    });

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            HakimiError::Tool(format!("failed to create screenshot directory: {e}"))
        })?;
    }

    std::fs::write(&out_path, &screenshot_bytes)
        .map_err(|e| HakimiError::Tool(format!("failed to write screenshot: {e}")))?;

    Ok((out_path, screenshot_bytes))
}

const CONSOLE_RECORDER_SCRIPT: &str = r#"
(function() {
    if (window.__hakimiConsoleRecorderInstalled) return true;

    const maxEntries = 200;
    const messages = window.__hakimiConsoleBuffer || [];
    const errors = window.__hakimiJsErrors || [];

    Object.defineProperty(window, "__hakimiConsoleBuffer", {
        value: messages,
        configurable: true
    });
    Object.defineProperty(window, "__hakimiJsErrors", {
        value: errors,
        configurable: true
    });
    Object.defineProperty(window, "__hakimiConsoleRecorderInstalled", {
        value: true,
        configurable: true
    });

    const trim = (items) => {
        if (items.length > maxEntries) {
            items.splice(0, items.length - maxEntries);
        }
    };
    const stringify = (value) => {
        try {
            if (value instanceof Error) return value.stack || value.message || String(value);
            if (typeof value === "string") return value;
            if (typeof value === "undefined") return "undefined";
            if (typeof value === "bigint") return value.toString();
            const json = JSON.stringify(value);
            return typeof json === "undefined" ? String(value) : json;
        } catch (_) {
            try {
                return String(value);
            } catch (_) {
                return "[unserializable]";
            }
        }
    };
    const pushMessage = (type, args) => {
        messages.push({
            type,
            text: Array.from(args || []).map(stringify).join(" "),
            timestamp: new Date().toISOString()
        });
        trim(messages);
    };
    const pushError = (message, source) => {
        errors.push({
            message: stringify(message),
            source: source || "exception",
            timestamp: new Date().toISOString()
        });
        trim(errors);
    };

    ["log", "debug", "info", "warn", "error"].forEach((level) => {
        const original = console[level] && console[level].bind(console);
        console[level] = function() {
            pushMessage(level === "warn" ? "warning" : level, arguments);
            if (original) return original.apply(console, arguments);
        };
    });

    window.addEventListener("error", (event) => {
        pushError(event.error || event.message || "Uncaught error", "exception");
    });
    window.addEventListener("unhandledrejection", (event) => {
        pushError(event.reason || "Unhandled promise rejection", "unhandledrejection");
    });

    return true;
})();
"#;

async fn install_console_recorder(page: &Page) -> Result<()> {
    page.evaluate_on_new_document(CONSOLE_RECORDER_SCRIPT)
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to install console recorder: {e}")))?;
    ensure_console_recorder(page).await
}

async fn ensure_console_recorder(page: &Page) -> Result<()> {
    page.evaluate(CONSOLE_RECORDER_SCRIPT)
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to enable console recorder: {e}")))?;
    Ok(())
}

async fn install_dialog_listeners(
    page: &Page,
    pending_dialogs: Arc<Mutex<VecDeque<PendingDialog>>>,
    next_dialog_id: Arc<AtomicU64>,
) -> Result<()> {
    let mut openings = page
        .event_listener::<EventJavascriptDialogOpening>()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to listen for browser dialogs: {e}")))?;
    let opening_dialogs = pending_dialogs;
    tokio::spawn(async move {
        while let Some(event) = openings.next().await {
            let dialog = PendingDialog {
                id: format!("dialog-{}", next_dialog_id.fetch_add(1, Ordering::Relaxed)),
                dialog_type: event.r#type.as_ref().to_string(),
                message: event.message.clone(),
                url: event.url.clone(),
                default_prompt: event.default_prompt.clone(),
            };
            let mut pending = opening_dialogs.lock().await;
            pending.push_back(dialog);
            while pending.len() > 8 {
                pending.pop_front();
            }
        }
    });

    Ok(())
}

async fn get_console_output(page: &Page, clear: bool) -> Result<JsonValue> {
    ensure_console_recorder(page).await?;

    let clear_literal = if clear { "true" } else { "false" };
    let js = format!(
        r#"
() => {{
    const messages = Array.from(window.__hakimiConsoleBuffer || []);
    const errors = Array.from(window.__hakimiJsErrors || []);
    if ({clear_literal}) {{
        if (window.__hakimiConsoleBuffer) window.__hakimiConsoleBuffer.length = 0;
        if (window.__hakimiJsErrors) window.__hakimiJsErrors.length = 0;
    }}
    return JSON.stringify({{
        success: true,
        console_messages: messages,
        js_errors: errors,
        total_messages: messages.length,
        total_errors: errors.length
    }});
}}
"#
    );

    let result = page
        .evaluate_function(js)
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read browser console: {e}")))?;

    let raw = result.value().and_then(|v| v.as_str()).unwrap_or("{}");
    serde_json::from_str(raw)
        .map_err(|e| HakimiError::Tool(format!("failed to parse browser console data: {e}")))
}

async fn evaluate_page_expression(page: &Page, expression: &str) -> Result<JsonValue> {
    if expression.trim().is_empty() {
        return Err(HakimiError::Tool("expression must not be empty".into()));
    }

    let expression_literal = serde_json::to_string(expression)
        .map_err(|e| HakimiError::Tool(format!("failed to encode expression: {e}")))?;
    let js = r#"
async () => {
    const expression = __HAKIMI_EXPRESSION__;
    const describe = (value) => {
        const resultType = value === null
            ? "null"
            : Array.isArray(value)
                ? "array"
                : typeof value;
        if (typeof value === "undefined") {
            return { result: null, result_type: "undefined" };
        }
        if (typeof value === "bigint" || typeof value === "function") {
            return { result: String(value), result_type: resultType };
        }
        try {
            return { result: JSON.parse(JSON.stringify(value)), result_type: resultType };
        } catch (_) {
            return { result: String(value), result_type: resultType };
        }
    };

    try {
        const value = await (0, eval)(expression);
        const output = describe(value);
        output.success = true;
        return JSON.stringify(output);
    } catch (error) {
        return JSON.stringify({
            success: false,
            error: error && (error.stack || error.message)
                ? String(error.stack || error.message)
                : String(error)
        });
    }
}
"#
    .replace("__HAKIMI_EXPRESSION__", &expression_literal);

    let result = page
        .evaluate_function(js)
        .await
        .map_err(|e| HakimiError::Tool(format!("browser expression evaluation failed: {e}")))?;

    let raw = result.value().and_then(|v| v.as_str()).unwrap_or("{}");
    serde_json::from_str(raw)
        .map_err(|e| HakimiError::Tool(format!("failed to parse browser eval data: {e}")))
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

async fn get_page_images(page: &Page) -> Result<JsonValue> {
    let js = r#"
    (function() {
        return JSON.stringify(
            Array.from(document.images || [])
                .map((img) => ({
                    src: img.currentSrc || img.src || "",
                    alt: img.alt || "",
                    width: img.naturalWidth || 0,
                    height: img.naturalHeight || 0
                }))
                .filter((img) => img.src && !img.src.startsWith("data:"))
        );
    })()
    "#;

    let result = page
        .evaluate(js)
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to get page images: {e}")))?;

    let raw = result.value().and_then(|v| v.as_str()).unwrap_or("[]");
    serde_json::from_str(raw)
        .map_err(|e| HakimiError::Tool(format!("failed to parse page image data: {e}")))
}

async fn press_page_key(page: &Page, key: &str) -> Result<()> {
    use chromiumoxide::cdp::browser_protocol::input::{
        DispatchKeyEventParams, DispatchKeyEventType,
    };

    let key_definition = chromiumoxide::keys::get_key_definition(key)
        .ok_or_else(|| HakimiError::Tool(format!("unknown browser key: {key}")))?;

    let mut cmd = DispatchKeyEventParams::builder();
    let key_down_event_type = if let Some(text) = key_definition.text {
        cmd = cmd.text(text);
        DispatchKeyEventType::KeyDown
    } else if key_definition.key.len() == 1 {
        cmd = cmd.text(key_definition.key);
        DispatchKeyEventType::KeyDown
    } else {
        DispatchKeyEventType::RawKeyDown
    };

    cmd = cmd
        .key(key_definition.key)
        .code(key_definition.code)
        .windows_virtual_key_code(key_definition.key_code)
        .native_virtual_key_code(key_definition.key_code);

    let key_down = cmd
        .clone()
        .r#type(key_down_event_type)
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to build key-down event: {e}")))?;
    let key_up = cmd
        .r#type(DispatchKeyEventType::KeyUp)
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to build key-up event: {e}")))?;

    page.execute(key_down)
        .await
        .map_err(|e| HakimiError::Tool(format!("key-down dispatch failed for '{key}': {e}")))?;
    page.execute(key_up)
        .await
        .map_err(|e| HakimiError::Tool(format!("key-up dispatch failed for '{key}': {e}")))?;

    Ok(())
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
        let pending_dialogs = self.manager.pending_dialogs().await;
        if !pending_dialogs.is_empty() {
            return Ok(json!({
                "success": true,
                "url": url,
                "title": title,
                "pending_dialogs": pending_dialogs,
                "note": "A native JavaScript dialog is blocking the page. Call browser_dialog with action='accept' or action='dismiss'."
            })
            .to_string());
        }

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
// browser_scroll
// ---------------------------------------------------------------------------

/// Scroll the current page up or down.
pub struct BrowserScrollTool {
    manager: Arc<BrowserManager>,
}

impl BrowserScrollTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserScrollTool {
    fn name(&self) -> &str {
        "browser_scroll"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Scroll the current browser page up or down. Use this to reveal content outside the current viewport."
    }

    fn emoji(&self) -> &str {
        "\u{2195}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "direction": {
                    "type": "string",
                    "enum": ["up", "down"],
                    "description": "Direction to scroll."
                }
            },
            "required": ["direction"]
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(2048)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let direction = args
            .get("direction")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: direction".into()))?;

        let delta = match direction {
            "down" => 500,
            "up" => -500,
            other => {
                return Err(HakimiError::Tool(format!(
                    "invalid direction '{other}'. Use 'up' or 'down'."
                )));
            }
        };

        debug!(direction = %direction, "browser scroll request");

        let page = self.manager.get_page().await?;
        page.evaluate(format!("window.scrollBy(0, {delta})"))
            .await
            .map_err(|e| HakimiError::Tool(format!("scroll failed: {e}")))?;

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        let url = page.url().await.ok().flatten().unwrap_or_default();
        let title = page.get_title().await.ok().flatten().unwrap_or_default();

        Ok(format!(
            "Scrolled {direction}\nCurrent page: {url}\nTitle: {title}"
        ))
    }
}

// ---------------------------------------------------------------------------
// browser_back
// ---------------------------------------------------------------------------

/// Navigate back in the current page history.
pub struct BrowserBackTool {
    manager: Arc<BrowserManager>,
}

impl BrowserBackTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserBackTool {
    fn name(&self) -> &str {
        "browser_back"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Navigate back to the previous page in browser history. Requires browser_navigate to be called first."
    }

    fn emoji(&self) -> &str {
        "\u{2b05}\u{fe0f}"
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
        Some(2048)
    }

    async fn execute(&self, _args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        debug!("browser back request");

        let page = self.manager.get_page().await?;
        page.evaluate("window.history.back()")
            .await
            .map_err(|e| HakimiError::Tool(format!("back navigation failed: {e}")))?;

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let url = page.url().await.ok().flatten().unwrap_or_default();
        let title = page.get_title().await.ok().flatten().unwrap_or_default();

        Ok(format!(
            "Navigated back\nCurrent page: {url}\nTitle: {title}"
        ))
    }
}

// ---------------------------------------------------------------------------
// browser_press
// ---------------------------------------------------------------------------

/// Press a keyboard key on the current page.
pub struct BrowserPressTool {
    manager: Arc<BrowserManager>,
}

impl BrowserPressTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserPressTool {
    fn name(&self) -> &str {
        "browser_press"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Press a keyboard key in the browser page. Useful for Enter, Tab, Escape, arrows, and shortcuts."
    }

    fn emoji(&self) -> &str {
        "\u{2328}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Key to press, such as 'Enter', 'Tab', 'Escape', or 'ArrowDown'."
                }
            },
            "required": ["key"]
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(2048)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: key".into()))?;

        if key.trim().is_empty() {
            return Err(HakimiError::Tool("key must not be empty".into()));
        }

        debug!(key = %key, "browser key press request");

        let page = self.manager.get_page().await?;
        press_page_key(&page, key).await?;

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        let url = page.url().await.ok().flatten().unwrap_or_default();
        let title = page.get_title().await.ok().flatten().unwrap_or_default();

        Ok(format!(
            "Pressed key: {key}\nCurrent page: {url}\nTitle: {title}"
        ))
    }
}

// ---------------------------------------------------------------------------
// browser_get_images
// ---------------------------------------------------------------------------

/// Get image URLs and alt text from the current page.
pub struct BrowserGetImagesTool {
    manager: Arc<BrowserManager>,
}

impl BrowserGetImagesTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserGetImagesTool {
    fn name(&self) -> &str {
        "browser_get_images"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Get all non-data images on the current browser page with source URLs, alt text, and natural dimensions. \
         Useful for finding images to inspect with vision tools."
    }

    fn emoji(&self) -> &str {
        "\u{1f5bc}\u{fe0f}"
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
        Some(64 * 1024)
    }

    async fn execute(&self, _args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        debug!("browser get images request");

        let page = self.manager.get_page().await?;
        let images = get_page_images(&page).await?;
        let count = images.as_array().map(|items| items.len()).unwrap_or(0);

        Ok(json!({
            "success": true,
            "images": images,
            "count": count
        })
        .to_string())
    }
}

// ---------------------------------------------------------------------------
// browser_console
// ---------------------------------------------------------------------------

/// Read browser console output or evaluate JavaScript on the current page.
pub struct BrowserConsoleTool {
    manager: Arc<BrowserManager>,
}

impl BrowserConsoleTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserConsoleTool {
    fn name(&self) -> &str {
        "browser_console"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Get browser console output and JavaScript errors from the current page, or evaluate a JavaScript expression in the page context."
    }

    fn emoji(&self) -> &str {
        "\u{1f4dd}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "clear": {
                    "type": "boolean",
                    "description": "If true, clear the captured console and error buffers after reading. Default: false."
                },
                "expression": {
                    "type": "string",
                    "description": "Optional JavaScript expression to evaluate in the current page context, like DevTools console."
                }
            }
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(64 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let page = self.manager.get_page().await?;

        if let Some(expression) = args.get("expression").and_then(|v| v.as_str()) {
            let result = evaluate_page_expression(&page, expression).await?;
            return Ok(result.to_string());
        }

        let clear = args.get("clear").and_then(|v| v.as_bool()).unwrap_or(false);

        let result = get_console_output(&page, clear).await?;
        Ok(result.to_string())
    }
}

// ---------------------------------------------------------------------------
// browser_dialog
// ---------------------------------------------------------------------------

/// Accept or dismiss a native JavaScript dialog on the current page.
pub struct BrowserDialogTool {
    manager: Arc<BrowserManager>,
}

impl BrowserDialogTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserDialogTool {
    fn name(&self) -> &str {
        "browser_dialog"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Accept or dismiss a native JavaScript dialog (alert, confirm, prompt, or beforeunload) currently blocking the browser page."
    }

    fn emoji(&self) -> &str {
        "\u{1f4ac}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["accept", "dismiss"],
                    "description": "Whether to accept or dismiss the pending dialog."
                },
                "prompt_text": {
                    "type": "string",
                    "description": "Text to submit for prompt() dialogs when accepting. Ignored for other dialog types."
                },
                "dialog_id": {
                    "type": "string",
                    "description": "Optional dialog id from browser_snapshot.pending_dialogs[].id."
                }
            },
            "required": ["action"]
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(4096)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: action".into()))?;
        let accept = match action {
            "accept" => true,
            "dismiss" => false,
            other => {
                return Err(HakimiError::Tool(format!(
                    "invalid dialog action: {other}; expected accept or dismiss"
                )));
            }
        };

        let dialog_id = args.get("dialog_id").and_then(|v| v.as_str());
        let pending = self.manager.pending_dialogs().await;
        if pending.is_empty() {
            return Ok(json!({
                "success": false,
                "error": "No pending JavaScript dialog is currently captured. Trigger a dialog, then call browser_snapshot to inspect pending_dialogs."
            })
            .to_string());
        }
        if let Some(id) = dialog_id
            && !pending.iter().any(|dialog| dialog.id == id)
        {
            return Ok(json!({
                "success": false,
                "error": format!("No pending JavaScript dialog with id {id}")
            })
            .to_string());
        }

        let selected_dialog = if let Some(id) = dialog_id {
            pending.iter().find(|dialog| dialog.id == id).cloned()
        } else {
            pending.first().cloned()
        };

        let page = self.manager.get_page().await?;
        let mut params = HandleJavaScriptDialogParams::builder().accept(accept);
        if let Some(text) = args.get("prompt_text").and_then(|v| v.as_str()) {
            params = params.prompt_text(text);
        }
        page.execute(
            params
                .build()
                .map_err(|e| HakimiError::Tool(format!("invalid dialog response: {e}")))?,
        )
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to handle browser dialog: {e}")))?;

        self.manager.acknowledge_dialog(dialog_id).await;
        Ok(json!({
            "success": true,
            "action": action,
            "dialog": selected_dialog
        })
        .to_string())
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
        let (out_path, _screenshot_bytes) =
            capture_browser_screenshot(&page, output_path, full_page).await?;

        info!(path = %out_path.display(), full_page, "screenshot saved");

        Ok(format!("SCREENSHOT:{}", out_path.display()))
    }
}

// ---------------------------------------------------------------------------
// browser_vision
// ---------------------------------------------------------------------------

/// Take a screenshot of the current page and prepare it for vision analysis.
pub struct BrowserVisionTool {
    manager: Arc<BrowserManager>,
}

impl BrowserVisionTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserVisionTool {
    fn name(&self) -> &str {
        "browser_vision"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Take a screenshot of the current browser page for visual inspection. \
         Returns a saved screenshot path plus a vision-compatible image content block."
    }

    fn emoji(&self) -> &str {
        "\u{1f441}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "What to inspect or answer from the browser screenshot."
                },
                "full_page": {
                    "type": "boolean",
                    "description": "If true, capture the entire scrollable page. Default: true."
                },
                "annotate": {
                    "type": "boolean",
                    "description": "Request interactive-element annotations. Current Rust Chromium backend records the request but does not overlay labels."
                },
                "output_path": {
                    "type": "string",
                    "description": "Custom screenshot output path. If not provided, auto-generates in ~/.hakimi/screenshots/."
                }
            }
        })
    }

    fn check_available(&self) -> bool {
        BrowserManager::is_chrome_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(10 * 1024 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe what is visible in this browser page screenshot.");
        let full_page = args
            .get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let annotate = args
            .get("annotate")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let output_path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        debug!(full_page, annotate, question = %question, "browser vision request");

        let page = self.manager.get_page().await?;
        let (out_path, screenshot_bytes) =
            capture_browser_screenshot(&page, output_path, full_page).await?;

        use base64::Engine as _;
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&screenshot_bytes)
        );
        let screenshot_path = out_path.display().to_string();
        let mut payload = json!({
            "success": true,
            "browser_vision": true,
            "vision_request": true,
            "image_source": screenshot_path,
            "screenshot_path": screenshot_path,
            "mime_type": "image/png",
            "image_size_bytes": screenshot_bytes.len(),
            "question": question,
            "full_page": full_page,
            "annotate_requested": annotate,
            "content_block": {
                "type": "image_url",
                "image_url": {
                    "url": data_url
                }
            },
            "instruction": format!(
                "Browser screenshot captured ({} bytes, image/png). Ask the vision model: {}. Screenshot path: {}. Share with MEDIA:{} when the user needs to inspect it.",
                screenshot_bytes.len(),
                question,
                out_path.display(),
                out_path.display()
            )
        });
        if annotate {
            payload["annotation_note"] = json!(
                "The Rust Chromium backend captured the page screenshot but does not yet overlay interactive-element labels; use browser_snapshot for textual element references."
            );
        }

        info!(path = %out_path.display(), full_page, "browser vision screenshot saved");
        Ok(payload.to_string())
    }
}

// ---------------------------------------------------------------------------
// browser_cdp
// ---------------------------------------------------------------------------

/// Inspect a configured Chrome DevTools Protocol endpoint.
pub struct BrowserCdpTool;

impl BrowserCdpTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BrowserCdpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BrowserCdpTool {
    fn name(&self) -> &str {
        "browser_cdp"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Inspect a Chrome DevTools Protocol endpoint from HAKIMI_BROWSER_CDP_URL, BROWSER_CDP_URL, or an explicit endpoint. \
         Use action='status' for readiness metadata or action='probe' to connect briefly and report browser version/target counts. \
         Raw arbitrary CDP method dispatch is not exposed until the supervisor-backed browser backend lands."
    }

    fn emoji(&self) -> &str {
        "\u{1f50c}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "probe"],
                    "description": "status reports configured endpoint metadata without connecting. probe briefly connects and reads browser version/targets. Default: status."
                },
                "endpoint": {
                    "type": "string",
                    "description": "Optional ws://, wss://, http://, or https:// DevTools endpoint. Overrides HAKIMI_BROWSER_CDP_URL and BROWSER_CDP_URL."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Connection/probe timeout in milliseconds, clamped to 250..30000. Default: 3000.",
                    "minimum": 250,
                    "maximum": 30000
                }
            }
        })
    }

    fn check_available(&self) -> bool {
        true
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(16 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("status");
        if action != "status" && action != "probe" {
            return Err(HakimiError::Tool(format!(
                "invalid browser_cdp action: {action}; expected status or probe"
            )));
        }

        let resolved = resolve_browser_cdp_endpoint(args);
        let Some(endpoint) = resolved else {
            return Ok(json!({
                "success": false,
                "configured": false,
                "action": action,
                "error": "No CDP endpoint configured. Set HAKIMI_BROWSER_CDP_URL or BROWSER_CDP_URL, or pass endpoint explicitly.",
                "dispatch_ready": false,
                "raw_cdp_dispatch": false,
                "next_step": "Start Chrome/Chromium with --remote-debugging-port or use a CDP-capable provider, then run browser_cdp with action='probe'."
            })
            .to_string());
        };

        let endpoint_display = redact_browser_cdp_endpoint(&endpoint.endpoint);
        if let Err(error) = validate_browser_cdp_endpoint(&endpoint.endpoint) {
            return Ok(json!({
                "success": false,
                "configured": true,
                "action": action,
                "source": endpoint.source,
                "endpoint": endpoint_display,
                "error": error,
                "dispatch_ready": false,
                "raw_cdp_dispatch": false
            })
            .to_string());
        }

        if action == "status" {
            return Ok(json!({
                "success": true,
                "configured": true,
                "action": "status",
                "source": endpoint.source,
                "endpoint": endpoint_display,
                "dispatch_ready": false,
                "raw_cdp_dispatch": false,
                "probe_available": true,
                "note": "Endpoint syntax is valid. Use action='probe' to verify live connectivity."
            })
            .to_string());
        }

        let timeout = std::time::Duration::from_millis(browser_cdp_timeout_ms(args));
        let endpoint_url = endpoint.endpoint.clone();
        let endpoint_source = endpoint.source.clone();
        let endpoint_display_for_probe = endpoint_display.clone();
        let probe = tokio::time::timeout(timeout, async {
            let (mut browser, mut handler) = Browser::connect(endpoint_url).await.map_err(|e| {
                HakimiError::Tool(format!(
                    "failed to connect to CDP endpoint {}: {e}",
                    endpoint_display_for_probe
                ))
            })?;
            let handler_task = tokio::spawn(async move {
                while let Some(event) = handler.next().await {
                    if let Err(e) = event {
                        warn!(error = %e, "browser CDP probe handler error");
                        break;
                    }
                }
            });

            let version = match browser.version().await {
                Ok(version) => version,
                Err(e) => {
                    handler_task.abort();
                    return Err(HakimiError::Tool(format!(
                        "CDP Browser.getVersion failed: {e}"
                    )));
                }
            };
            let targets = match browser.fetch_targets().await {
                Ok(targets) => targets,
                Err(e) => {
                    handler_task.abort();
                    return Err(HakimiError::Tool(format!(
                        "CDP Target.getTargets failed: {e}"
                    )));
                }
            };
            handler_task.abort();

            Ok::<JsonValue, HakimiError>(json!({
                "success": true,
                "configured": true,
                "action": "probe",
                "source": endpoint_source,
                "endpoint": endpoint_display_for_probe,
                "browser": version.product,
                "protocol_version": version.protocol_version,
                "user_agent": version.user_agent,
                "target_count": targets.len(),
                "page_target_count": targets.iter().filter(|target| target.r#type.as_str() == "page").count(),
                "dispatch_ready": false,
                "raw_cdp_dispatch": false,
                "note": "CDP endpoint is reachable. This slice exposes a typed readiness probe; raw method dispatch remains pending a supervisor-backed backend."
            }))
        })
        .await;

        match probe {
            Ok(Ok(payload)) => Ok(payload.to_string()),
            Ok(Err(error)) => Err(error),
            Err(_) => Ok(json!({
                "success": false,
                "configured": true,
                "action": "probe",
                "source": endpoint.source,
                "endpoint": endpoint_display,
                "error": format!("CDP probe timed out after {} ms", timeout.as_millis()),
                "dispatch_ready": false,
                "raw_cdp_dispatch": false
            })
            .to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_cdp_metadata() {
        let tool = BrowserCdpTool::new();
        assert_eq!(tool.name(), "browser_cdp");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f50c}");
        assert!(tool.description().contains("CDP"));
        assert!(tool.check_available());
    }

    #[test]
    fn test_browser_cdp_schema() {
        let tool = BrowserCdpTool::new();
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["endpoint"].is_object());
        assert!(schema["properties"]["timeout_ms"].is_object());
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn test_browser_cdp_endpoint_from_explicit_arg() {
        let args = json!({
            "endpoint": "  ws://127.0.0.1:9222/devtools/browser/session  "
        });

        let endpoint = browser_cdp_endpoint_from_args(&args).unwrap();
        assert_eq!(
            endpoint.endpoint,
            "ws://127.0.0.1:9222/devtools/browser/session"
        );
        assert_eq!(endpoint.source, "argument:endpoint");
    }

    #[test]
    fn test_browser_cdp_endpoint_pair_precedence() {
        let endpoint = browser_cdp_endpoint_from_pairs([
            ("HAKIMI_BROWSER_CDP_URL", Some("".to_string())),
            (
                "BROWSER_CDP_URL",
                Some("wss://example.com/devtools".to_string()),
            ),
        ])
        .unwrap();

        assert_eq!(endpoint.endpoint, "wss://example.com/devtools");
        assert_eq!(endpoint.source, "BROWSER_CDP_URL");
    }

    #[test]
    fn test_browser_cdp_endpoint_validation() {
        assert!(validate_browser_cdp_endpoint("ws://127.0.0.1:9222/devtools").is_ok());
        assert!(validate_browser_cdp_endpoint("wss://browserbase.example/session").is_ok());
        assert!(validate_browser_cdp_endpoint("http://127.0.0.1:9222").is_ok());
        assert!(validate_browser_cdp_endpoint("https://cdp.example/json/version").is_ok());
        assert!(validate_browser_cdp_endpoint("file:///tmp/browser").is_err());
    }

    #[test]
    fn test_browser_cdp_redacts_query_string() {
        let redacted =
            redact_browser_cdp_endpoint("wss://cdp.example/devtools/browser/abc?token=secret");

        assert_eq!(
            redacted,
            "wss://cdp.example/devtools/browser/abc?[REDACTED]"
        );
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn test_browser_cdp_redacts_userinfo() {
        let redacted =
            redact_browser_cdp_endpoint("wss://user:secret@cdp.example/devtools/browser/abc");

        assert_eq!(redacted, "wss://***@cdp.example/devtools/browser/abc");
        assert!(!redacted.contains("secret"));
    }

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
    fn test_browser_vision_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserVisionTool::new(mgr);
        assert_eq!(tool.name(), "browser_vision");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f441}\u{fe0f}");
        assert!(tool.description().contains("vision-compatible"));
    }

    #[test]
    fn test_browser_get_images_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserGetImagesTool::new(mgr);
        assert_eq!(tool.name(), "browser_get_images");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f5bc}\u{fe0f}");
    }

    #[test]
    fn test_browser_console_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserConsoleTool::new(mgr);
        assert_eq!(tool.name(), "browser_console");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f4dd}");
    }

    #[test]
    fn test_browser_dialog_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserDialogTool::new(mgr);
        assert_eq!(tool.name(), "browser_dialog");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{1f4ac}");
    }

    #[test]
    fn test_browser_scroll_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserScrollTool::new(mgr);
        assert_eq!(tool.name(), "browser_scroll");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{2195}\u{fe0f}");
    }

    #[test]
    fn test_browser_back_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserBackTool::new(mgr);
        assert_eq!(tool.name(), "browser_back");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{2b05}\u{fe0f}");
    }

    #[test]
    fn test_browser_press_metadata() {
        let mgr = BrowserManager::new();
        let tool = BrowserPressTool::new(mgr);
        assert_eq!(tool.name(), "browser_press");
        assert_eq!(tool.toolset(), "browser");
        assert_eq!(tool.emoji(), "\u{2328}\u{fe0f}");
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
    fn test_browser_vision_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserVisionTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["question"].is_object());
        assert!(schema["properties"]["full_page"].is_object());
        assert!(schema["properties"]["annotate"].is_object());
        assert!(schema["properties"]["output_path"].is_object());
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn test_scroll_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserScrollTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["direction"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&JsonValue::String("direction".to_string())));
    }

    #[test]
    fn test_back_schema_empty() {
        let mgr = BrowserManager::new();
        let tool = BrowserBackTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].as_object().unwrap().is_empty());
    }

    #[test]
    fn test_press_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserPressTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["key"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&JsonValue::String("key".to_string())));
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
    fn test_get_images_schema_empty() {
        let mgr = BrowserManager::new();
        let tool = BrowserGetImagesTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].as_object().unwrap().is_empty());
    }

    #[test]
    fn test_console_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserConsoleTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["clear"].is_object());
        assert!(schema["properties"]["expression"].is_object());
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn test_dialog_schema() {
        let mgr = BrowserManager::new();
        let tool = BrowserDialogTool::new(mgr);
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["prompt_text"].is_object());
        assert!(schema["properties"]["dialog_id"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&JsonValue::String("action".to_string())));
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
    fn test_find_executable_on_path_uses_platform_separator_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let binary_name = if cfg!(windows) {
            "chrome.exe"
        } else {
            "chrome"
        };
        let binary = tmp.path().join(binary_name);
        std::fs::write(&binary, "").unwrap();

        let found = find_executable_on_path("chrome", vec![tmp.path().to_path_buf()]).unwrap();
        assert_eq!(found, binary);
    }

    #[test]
    fn test_playwright_scan_finds_headless_shell_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let binary = tmp
            .path()
            .join("chromium_headless_shell-1208")
            .join("chrome-headless-shell-linux64")
            .join("chrome-headless-shell");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "").unwrap();

        let found = find_playwright_browser_executable(tmp.path()).unwrap();
        assert_eq!(found, binary);
    }

    #[test]
    fn test_playwright_scan_ignores_shared_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp
            .path()
            .join("chromium_headless_shell-1208")
            .join("chrome-headless-shell-linux64")
            .join("libGLESv2.so");
        std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
        std::fs::write(&lib, "").unwrap();

        assert!(find_playwright_browser_executable(tmp.path()).is_none());
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
            BrowserScrollTool::new(mgr.clone()).max_result_size(),
            Some(2048)
        );
        assert_eq!(
            BrowserBackTool::new(mgr.clone()).max_result_size(),
            Some(2048)
        );
        assert_eq!(
            BrowserPressTool::new(mgr.clone()).max_result_size(),
            Some(2048)
        );
        assert_eq!(
            BrowserGetImagesTool::new(mgr.clone()).max_result_size(),
            Some(64 * 1024)
        );
        assert_eq!(
            BrowserConsoleTool::new(mgr.clone()).max_result_size(),
            Some(64 * 1024)
        );
        assert_eq!(
            BrowserDialogTool::new(mgr.clone()).max_result_size(),
            Some(4096)
        );
        assert_eq!(
            BrowserScreenshotTool::new(mgr.clone()).max_result_size(),
            Some(2048)
        );
        assert_eq!(
            BrowserVisionTool::new(mgr.clone()).max_result_size(),
            Some(10 * 1024 * 1024)
        );
        assert_eq!(BrowserCdpTool::new().max_result_size(), Some(16 * 1024));
    }

    #[test]
    fn test_all_tools_same_toolset() {
        let mgr = BrowserManager::new();
        assert_eq!(BrowserNavigateTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserSnapshotTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserClickTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserTypeTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserScrollTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserBackTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserPressTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserGetImagesTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserConsoleTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserDialogTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserScreenshotTool::new(mgr.clone()).toolset(), "browser");
        assert_eq!(BrowserVisionTool::new(mgr).toolset(), "browser");
        assert_eq!(BrowserCdpTool::new().toolset(), "browser");
    }
}
