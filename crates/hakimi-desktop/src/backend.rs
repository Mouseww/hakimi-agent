//! Local Studio + WebUI backend for the desktop shell.
//!
//! Serves the same embedded static assets as `hakimi-server` and mounts the
//! Studio WebSocket at `/v1/studio`. No full agent/gateway stack — Studio uses
//! the mock or core host depending on how `StudioState` is constructed.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::Path,
    http::{Response, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use hakimi_server::{StudioState, studio_router};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::info;

// Mirror hakimi-server embedded assets (stable filenames from vite build).
const WEBUI_INDEX_HTML: &str = include_str!("../../hakimi-webui/static/index.html");
const WEBUI_APP_JS: &str = include_str!("../../hakimi-webui/static/app.js");
const WEBUI_APP_CSS: &str = include_str!("../../hakimi-webui/static/app.css");
const WEBUI_FAVICON_SVG: &str = include_str!("../../hakimi-webui/static/favicon.svg");
const WEBUI_ICONS_SVG: &str = include_str!("../../hakimi-webui/static/icons.svg");
const WEBUI_MANIFEST: &str = include_str!("../../hakimi-webui/static/manifest.webmanifest");
const WEBUI_SW: &str = include_str!("../../hakimi-webui/static/sw.js");

#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Bind address, e.g. `127.0.0.1:0` for ephemeral port.
    pub bind: String,
    /// Workspace root for Studio path jail (defaults to cwd).
    pub workspace: Option<std::path::PathBuf>,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:0".into(),
            workspace: None,
        }
    }
}

pub struct BackendHandle {
    pub addr: SocketAddr,
    pub base_url: String,
    /// Dropping / aborting stops the server.
    pub join: tokio::task::JoinHandle<()>,
}

/// Start local backend; returns bound address and join handle.
pub async fn start_backend(cfg: BackendConfig) -> Result<BackendHandle> {
    if let Some(ref root) = cfg.workspace {
        // StudioRuntime opens cwd by default; set process cwd when requested.
        std::env::set_current_dir(root)
            .with_context(|| format!("set workspace cwd {}", root.display()))?;
    }

    let studio = StudioState::new();
    let app = Router::new()
        .merge(studio_router(studio))
        .route("/", get(webui_index))
        .route("/index.html", get(webui_index))
        .route("/favicon.svg", get(webui_favicon))
        .route("/static/{*path}", get(webui_static_asset))
        .route("/manifest.webmanifest", get(webui_manifest))
        .route("/sw.js", get(webui_sw))
        .fallback(get(webui_index))
        .layer(CorsLayer::permissive());

    let listener = TcpListener::bind(&cfg.bind)
        .await
        .with_context(|| format!("bind {}", cfg.bind))?;
    let addr = listener.local_addr()?;
    let base_url = format!("http://{addr}");
    info!(%addr, "hakimi-desktop backend listening");

    let join = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "desktop backend exited");
        }
    });

    // Brief readiness: health endpoint.
    let health = format!("{base_url}/v1/studio/health");
    for _ in 0..20 {
        if reqwest_get_ok(&health).await {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    Ok(BackendHandle {
        addr,
        base_url,
        join,
    })
}

async fn reqwest_get_ok(url: &str) -> bool {
    // Avoid hard dep in lib path: use tokio TcpStream simple check via hyper not needed;
    // use std::net for connect only.
    if let Ok(u) = url::Url::parse(url) {
        let host = u.host_str().unwrap_or("127.0.0.1");
        let port = u.port_or_known_default().unwrap_or(80);
        tokio::net::TcpStream::connect((host, port)).await.is_ok()
    } else {
        false
    }
}

fn static_response(content_type: &'static str, body: &'static str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn webui_index() -> Response<Body> {
    static_response("text/html; charset=utf-8", WEBUI_INDEX_HTML)
}

async fn webui_favicon() -> Response<Body> {
    static_response("image/svg+xml; charset=utf-8", WEBUI_FAVICON_SVG)
}

async fn webui_manifest() -> Response<Body> {
    static_response("application/manifest+json; charset=utf-8", WEBUI_MANIFEST)
}

async fn webui_sw() -> Response<Body> {
    static_response("text/javascript; charset=utf-8", WEBUI_SW)
}

async fn webui_static_asset(Path(path): Path<String>) -> Response<Body> {
    match path.as_str() {
        "app.js" => static_response("text/javascript; charset=utf-8", WEBUI_APP_JS),
        "app.css" => static_response("text/css; charset=utf-8", WEBUI_APP_CSS),
        "favicon.svg" => static_response("image/svg+xml; charset=utf-8", WEBUI_FAVICON_SVG),
        "icons.svg" => static_response("image/svg+xml; charset=utf-8", WEBUI_ICONS_SVG),
        "manifest.webmanifest" => {
            static_response("application/manifest+json; charset=utf-8", WEBUI_MANIFEST)
        }
        "sw.js" => static_response("text/javascript; charset=utf-8", WEBUI_SW),
        "index.html" => static_response("text/html; charset=utf-8", WEBUI_INDEX_HTML),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

// Minimal URL parse without pulling url crate into default features heavily —
// use a tiny helper. Prefer dependency `url` is light; add to Cargo if needed.
mod url {
    pub struct Url {
        host: String,
        port: Option<u16>,
    }
    impl Url {
        pub fn parse(s: &str) -> Result<Self, ()> {
            // http://host:port/path
            let rest = s
                .strip_prefix("http://")
                .or_else(|| s.strip_prefix("https://"))
                .ok_or(())?;
            let authority = rest.split('/').next().unwrap_or(rest);
            let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
                (h.to_string(), p.parse().ok())
            } else {
                (authority.to_string(), None)
            };
            Ok(Self { host, port })
        }
        pub fn host_str(&self) -> Option<&str> {
            Some(&self.host)
        }
        pub fn port_or_known_default(&self) -> Option<u16> {
            self.port.or(Some(80))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn backend_serves_studio_health_and_static() {
        let handle = start_backend(BackendConfig {
            bind: "127.0.0.1:0".into(),
            workspace: None,
        })
        .await
        .expect("start");
        let client = reqwest::Client::new();
        let health = client
            .get(format!("{}/v1/studio/health", handle.base_url))
            .send()
            .await
            .expect("health req");
        assert!(health.status().is_success());
        let body: serde_json::Value = health.json().await.unwrap();
        assert_eq!(body["ok"], true);
        assert_eq!(body["service"], "hakimi-studio");

        let index = client
            .get(format!("{}/", handle.base_url))
            .send()
            .await
            .expect("index");
        assert!(index.status().is_success());
        let html = index.text().await.unwrap();
        assert!(html.contains("html") || html.contains("Hakimi") || html.contains("root"));

        let js = client
            .get(format!("{}/static/app.js", handle.base_url))
            .send()
            .await
            .expect("js");
        assert!(js.status().is_success());

        handle.join.abort();
    }
}
