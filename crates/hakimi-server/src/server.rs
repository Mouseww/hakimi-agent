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
/// `&mut self` (it mutates conversation history). The config is behind a
/// separate mutex so POST /config can update fields concurrently.
#[derive(Clone)]
pub struct AppState {
    pub agent: Arc<Mutex<hakimi_core::AIAgent>>,
    pub config: Arc<Mutex<hakimi_config::HakimiConfig>>,
    pub session_db: Arc<Mutex<hakimi_session::SessionDB>>,
    pub response_store: Arc<Mutex<crate::api::ResponsesStore>>,
    pub run_store: Arc<Mutex<crate::api::RunsStore>>,
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
        let state = AppState {
            agent: Arc::new(Mutex::new(agent)),
            config: Arc::new(Mutex::new(config)),
            session_db: Arc::new(Mutex::new(session_db)),
            response_store: Arc::new(Mutex::new(crate::api::ResponsesStore::default())),
            run_store: Arc::new(Mutex::new(crate::api::RunsStore::default())),
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
