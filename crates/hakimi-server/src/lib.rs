//! hakimi-server — HTTP API server for the Hakimi Agent.
//!
//! Provides a REST API for controlling the agent programmatically and
//! eventually serving a web dashboard.

pub mod api;
pub mod core_agent_host;
pub mod hub_worker;
pub mod server;
pub mod studio;

pub use hub_worker::{HubWorkerConfig, spawn_hub_worker};
pub use server::Server;
pub use studio::{StudioState, studio_router};
