//! Server implementation — manages shared state and starts the HTTP listener.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::info;

use crate::api;

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

/// Application state shared across all request handlers.
///
/// The agent is behind a `tokio::sync::Mutex` because `AIAgent::chat()` takes
/// App state shared across all HTTP handlers. Each resource has a
/// separate mutex so POST /config can update fields concurrently.
#[derive(Clone)]
pub struct AppState {
    pub agent: Arc<Mutex<hakimi_core::AIAgent>>,
    pub config: Arc<Mutex<hakimi_config::HakimiConfig>>,
    pub session_db: Arc<Mutex<hakimi_session::SessionDB>>,
    pub response_store: Arc<Mutex<crate::api::ResponsesStore>>,
    pub run_store: Arc<Mutex<crate::api::RunsStore>>,
    pub knowledge_provider: Arc<Mutex<hakimi_knowledge::KnowledgeProvider>>,
    pub webui_password: Arc<Mutex<String>>,
    /// Gateway handle for unified mode (None in WebUI-only mode).
    pub gateway: Option<Arc<hakimi_gateway::Gateway>>,
    /// Persona registry for multi-agent isolation. Existing endpoints operate on
    /// the default persona via [`AppState::agent`]; agent-scoped endpoints use this.
    pub persona_registry: Arc<tokio::sync::RwLock<hakimi_core::PersonaRegistry>>,
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// HTTP API server for the Hakimi Agent.
pub struct Server {
    state: AppState,
}

impl Server {
    /// Create a new server bound to the given address.
    ///
    /// The `agent` will be wrapped in shared state accessible by all handlers.
    pub fn new(
        _addr: &str,
        agent: hakimi_core::AIAgent,
        config: hakimi_config::HakimiConfig,
        session_db: hakimi_session::SessionDB,
    ) -> Result<Self> {
        let hakimi_dir = dirs::home_dir()
            .map(|h| h.join(".hakimi"))
            .unwrap_or_else(|| std::path::PathBuf::from(".hakimi"));
        let knowledge_path = hakimi_dir.join("knowledge.json");
        let knowledge_provider = hakimi_knowledge::KnowledgeProvider::new(knowledge_path);

        // Load webui password from config, fallback to env var
        let initial_webui_password = if !config.webui.password.is_empty() {
            config.webui.password.clone()
        } else {
            std::env::var("HAKIMI_WEBUI_PASSWORD").unwrap_or_default()
        };
        let persona_registry = hakimi_core::PersonaRegistry::load(hakimi_dir.join("agents"))?;
        let state = AppState {
            agent: Arc::new(Mutex::new(agent)),
            config: Arc::new(Mutex::new(config)),
            session_db: Arc::new(Mutex::new(session_db)),
            response_store: Arc::new(Mutex::new(crate::api::ResponsesStore::default())),
            run_store: Arc::new(Mutex::new(crate::api::RunsStore::default())),
            knowledge_provider: Arc::new(Mutex::new(knowledge_provider)),
            webui_password: Arc::new(Mutex::new(initial_webui_password)),
            gateway: None, // WebUI-only mode
            persona_registry: Arc::new(tokio::sync::RwLock::new(persona_registry)),
        };
        Ok(Self { state })
    }

    /// Start the HTTP server and block until it shuts down.
    pub async fn serve(self, addr: SocketAddr) -> Result<()> {
        let app = api::build_router(self.state);

        info!(addr = %addr, "starting HTTP API server");

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}
