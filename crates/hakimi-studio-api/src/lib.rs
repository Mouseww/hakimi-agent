//! Hakimi Studio protocol + local runtime (Phase 0–2).
//!
//! - [`protocol`] — wire types (`StudioCommand`, `StudioEvent`, envelopes)
//! - [`event_bus`] — per-session seq + bounded replay + gap detection
//! - [`agent_host`] — pluggable turn executor (mock or real agent)
//! - [`runtime`] — sessions, queue+preempt, multi-device roles, agent host
//!
//! Full design: `docs/hakimi-studio/DESIGN.md`  
//! Protocol: `docs/hakimi-studio/protocol.md`

pub mod agent_host;
pub mod event_bus;
pub mod protocol;
pub mod runtime;

pub use agent_host::{AgentHost, AgentTurnRequest, MockAgentHost};
pub use event_bus::{DEFAULT_WINDOW, EventBus, ReplayResult};
pub use protocol::{
    AttachRole, DeviceKind, DeviceSummary, PROTOCOL_VERSION, PreferRunner, StudioCommand,
    StudioEvent, StudioEventEnvelope, WorkerDispatch,
};
pub use runtime::StudioRuntime;
