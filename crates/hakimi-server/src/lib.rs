//! hakimi-server — HTTP API server for the Hakimi Agent.
//!
//! Provides a REST API for controlling the agent programmatically and
//! eventually serving a web dashboard.

pub mod api;
pub mod server;

pub use server::Server;
