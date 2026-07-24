//! Studio Protocol v1 — command and event types.
//!
//! Wire format: JSON objects discriminated by `"type"`.
//! See `docs/hakimi-studio/protocol.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub const PROTOCOL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Device / roles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Desktop,
    Web,
    Server,
    #[default]
    Cli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AttachRole {
    #[default]
    Controller,
    Viewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PreferRunner {
    #[default]
    Local,
    Server,
}

// ---------------------------------------------------------------------------
// Commands (client → runner / hub)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StudioCommand {
    Hello {
        device_id: String,
        #[serde(default)]
        token: Option<String>,
        #[serde(default)]
        device_name: Option<String>,
        #[serde(default)]
        kind: DeviceKind,
        #[serde(default = "default_protocol_version")]
        protocol_version: u32,
    },
    SessionCreate {
        #[serde(default)]
        workspace_id: Option<String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        prefer_runner: PreferRunner,
    },
    SessionAttach {
        session_id: String,
        #[serde(default)]
        after_seq: Option<u64>,
        #[serde(default)]
        role: AttachRole,
    },
    SessionList {
        #[serde(default)]
        limit: Option<usize>,
    },
    ChatSubmit {
        session_id: String,
        text: String,
        client_request_id: String,
        /// If true and a run is active, preempt it (cancel + start this).
        #[serde(default)]
        preempt: bool,
    },
    ChatCancel {
        session_id: String,
        #[serde(default)]
        run_id: Option<String>,
    },
    ChatPreempt {
        session_id: String,
        text: String,
        client_request_id: String,
    },
    RunnerHandoff {
        session_id: String,
        to_device_id: String,
        /// Device requesting handoff (controller). Optional for back-compat.
        #[serde(default)]
        from_device_id: Option<String>,
    },
    /// List currently registered devices on this hub/runtime.
    DevicesList {},
    WorkspaceList {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        path: String,
    },
    WorkspaceRead {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
    },
    WorkspaceWrite {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
        content: String,
    },
    WorkspaceCreate {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
        #[serde(default)]
        is_dir: bool,
    },
    WorkspaceDelete {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
        #[serde(default)]
        recursive: bool,
    },
    WorkspaceGrep {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        path: String,
        pattern: String,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Create a file snapshot under `.hakimi/checkpoints/` (Phase 5 rewind primitive).
    CheckpointCreate {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        label: Option<String>,
        /// Relative paths to include; empty = top-level non-hidden entries.
        #[serde(default)]
        paths: Vec<String>,
    },
    CheckpointList {
        #[serde(default)]
        session_id: Option<String>,
    },
    /// Restore files from a checkpoint (overwrites workspace). Requires client danger-confirm.
    CheckpointRestore {
        #[serde(default)]
        session_id: Option<String>,
        checkpoint_id: String,
    },
    Ping {
        #[serde(default)]
        nonce: Option<String>,
    },
    /// Worker → Hub: publish envelopes produced by the Active Runner (pure-relay mode).
    /// Hub re-sequences and fans out to all clients. Clients never send this.
    WorkerPublish { events: Vec<StudioEventEnvelope> },
}

fn default_protocol_version() -> u32 {
    PROTOCOL_VERSION
}

// ---------------------------------------------------------------------------
// Events (runner → clients)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StudioEvent {
    HelloOk {
        device_id: String,
        protocol_version: u32,
        /// Local runner prefers local execution by default.
        prefer_runner: PreferRunner,
    },
    HelloError {
        message: String,
    },
    SessionCreated {
        session_id: String,
        title: String,
        active_runner_device_id: String,
        prefer_runner: PreferRunner,
    },
    SessionSnapshot {
        session_id: String,
        last_seq: u64,
        title: String,
        active_runner_device_id: String,
        messages: Vec<SnapshotMessage>,
        queue_depth: usize,
    },
    SessionListed {
        sessions: Vec<SessionSummary>,
    },
    QueueUpdated {
        session_id: String,
        depth: usize,
        items: Vec<QueueItemView>,
    },
    RunStarted {
        session_id: String,
        run_id: String,
        client_request_id: String,
    },
    RunQueued {
        session_id: String,
        client_request_id: String,
        position: usize,
    },
    RunPreempted {
        session_id: String,
        run_id: String,
        reason: String,
    },
    MessageDelta {
        session_id: String,
        run_id: String,
        delta: String,
    },
    MessageCompleted {
        session_id: String,
        run_id: String,
        text: String,
    },
    ToolStarted {
        session_id: String,
        run_id: String,
        name: String,
        call_id: String,
    },
    ToolCompleted {
        session_id: String,
        run_id: String,
        call_id: String,
        ok: bool,
    },
    RunnerChanged {
        session_id: String,
        active_runner_device_id: String,
        #[serde(default)]
        from_device_id: Option<String>,
    },
    /// Device joined this hub/runtime (Phase 2 multi-device).
    DeviceRegistered {
        device_id: String,
        #[serde(default)]
        device_name: Option<String>,
        kind: DeviceKind,
        #[serde(default)]
        is_runner: bool,
    },
    /// Snapshot of connected devices.
    DevicesListed {
        devices: Vec<DeviceSummary>,
    },
    /// Client after_seq is older than the bounded replay window; must resync from snapshot.
    SessionReset {
        session_id: String,
        reason: String,
        last_seq: u64,
        window_oldest_seq: Option<u64>,
    },
    Error {
        session_id: Option<String>,
        message: String,
        #[serde(default)]
        code: Option<String>,
    },
    Pong {
        #[serde(default)]
        nonce: Option<String>,
    },
    SessionEnded {
        session_id: String,
        run_id: String,
        reason: String,
    },
    WorkspaceListed {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
        entries: Vec<WorkspaceEntryView>,
    },
    WorkspaceContent {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
        content: String,
    },
    WorkspaceWritten {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
    },
    WorkspaceCreated {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
        is_dir: bool,
    },
    WorkspaceDeleted {
        #[serde(default)]
        session_id: Option<String>,
        path: String,
    },
    WorkspaceGrepResult {
        #[serde(default)]
        session_id: Option<String>,
        pattern: String,
        hits: Vec<WorkspaceGrepHitView>,
    },
    CheckpointCreated {
        #[serde(default)]
        session_id: Option<String>,
        checkpoint: CheckpointView,
    },
    CheckpointsListed {
        #[serde(default)]
        session_id: Option<String>,
        checkpoints: Vec<CheckpointView>,
    },
    CheckpointRestored {
        #[serde(default)]
        session_id: Option<String>,
        checkpoint: CheckpointView,
    },
    /// Escape hatch for forward-compatible payloads.
    Custom {
        name: String,
        payload: JsonValue,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEntryView {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    #[serde(default)]
    pub git_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceGrepHitView {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointView {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub created_at: String,
    pub files: Vec<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub title: String,
    pub updated_at: String,
    pub active_runner_device_id: String,
    pub last_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub device_id: String,
    #[serde(default)]
    pub device_name: Option<String>,
    pub kind: DeviceKind,
    pub is_runner: bool,
    pub connected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItemView {
    pub client_request_id: String,
    pub text_preview: String,
    pub preempt: bool,
}

/// Wire envelope: monotonic `seq` is per-session (0 for non-session events).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudioEventEnvelope {
    pub seq: u64,
    #[serde(default)]
    pub session_id: Option<String>,
    pub event: StudioEvent,
    #[serde(default)]
    pub ts: Option<String>,
}

impl StudioEventEnvelope {
    pub fn new(seq: u64, session_id: Option<String>, event: StudioEvent) -> Self {
        Self {
            seq,
            session_id,
            event,
            ts: Some(chrono::Utc::now().to_rfc3339()),
        }
    }
}

/// Hub → Active Runner frame (pure-relay). Not a `StudioCommand`; sent only on
/// the runner's WebSocket as a distinct JSON object with `"type":"worker_dispatch"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerDispatch {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub actor_device_id: Option<String>,
    pub command: StudioCommand,
}

impl WorkerDispatch {
    pub const TYPE: &'static str = "worker_dispatch";

    pub fn new(actor_device_id: Option<String>, command: StudioCommand) -> Self {
        Self {
            kind: Self::TYPE.into(),
            actor_device_id,
            command,
        }
    }
}
