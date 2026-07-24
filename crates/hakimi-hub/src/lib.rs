//! Hakimi Studio Hub — multi-device event relay.
//!
//! Principles (from DESIGN.md):
//! - Hub does **not** run tools or hold plaintext provider keys.
//! - Devices register via `hello`; events fan-out over WebSocket.
//! - Session state + agent loop live on the Active Runner (`StudioRuntime`
//!   on a worker); Hub coordinates attach / handoff / replay.

pub mod hub;
pub mod state;

pub use hub::{hub_router, HubConfig};
pub use state::{HubMode, HubState, Outbound};
