//! In-process Studio runtime: sessions, queue + preempt, pluggable agent host.
//!
//! Default host is [`MockAgentHost`] for unit tests. Production injects a host
//! that clones the shared `AIAgent` and streams via request-local callbacks.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use anyhow::{Result, bail};
use chrono::Utc;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::agent_host::{AgentHost, AgentTurnRequest, MockAgentHost};
use crate::event_bus::{EventBus, ReplayResult};
use crate::protocol::{
    AttachRole, CheckpointView, DeviceKind, DeviceSummary, PreferRunner, QueueItemView,
    SessionSummary, SnapshotMessage, StudioCommand, StudioEvent, StudioEventEnvelope,
    WorkspaceEntryView, WorkspaceGrepHitView,
};
use hakimi_workspace::Workspace;

#[derive(Clone)]
pub struct StudioRuntime {
    bus: EventBus,
    state: Arc<Mutex<RuntimeState>>,
    local_device_id: String,
    workspace: Arc<Workspace>,
    agent_host: Arc<dyn AgentHost>,
}

struct RuntimeState {
    sessions: HashMap<String, SessionState>,
    /// Registered devices (hello). Key = device_id.
    devices: HashMap<String, RegisteredDevice>,
    /// Connection currently acting as command source (last Hello on this process).
    last_device_id: Option<String>,
}

struct RegisteredDevice {
    device_id: String,
    device_name: Option<String>,
    kind: DeviceKind,
    is_runner: bool,
    connected_at: String,
}

struct SessionState {
    title: String,
    #[allow(dead_code)] // reserved for runner prefer switch
    prefer_runner: PreferRunner,
    active_runner_device_id: String,
    /// Controllers may submit; viewers are subscribe-only.
    controllers: std::collections::HashSet<String>,
    messages: Vec<SnapshotMessage>,
    updated_at: String,
    queue: VecDeque<QueuedPrompt>,
    current: Option<ActiveRun>,
    seen_requests: HashMap<String, String>,
}

struct QueuedPrompt {
    text: String,
    client_request_id: String,
    #[allow(dead_code)]
    preempt: bool,
}

struct ActiveRun {
    run_id: String,
    #[allow(dead_code)]
    client_request_id: String,
    cancel: Arc<Notify>,
    handle: JoinHandle<()>,
}

enum EnqueueAction {
    NotFound,
    Duplicate {
        existing: String,
    },
    Queue {
        position: usize,
        depth: usize,
        items: Vec<QueueItemView>,
        client_request_id: String,
    },
    Start {
        run_id: String,
        text: String,
        client_request_id: String,
        preempted_run: Option<String>,
    },
}

impl StudioRuntime {
    pub fn new(local_device_id: impl Into<String>) -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self::with_workspace(local_device_id, root)
    }

    pub fn with_workspace(
        local_device_id: impl Into<String>,
        workspace_root: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self::with_workspace_and_host(local_device_id, workspace_root, Arc::new(MockAgentHost))
    }

    pub fn with_agent_host(
        local_device_id: impl Into<String>,
        agent_host: Arc<dyn AgentHost>,
    ) -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self::with_workspace_and_host(local_device_id, root, agent_host)
    }

    pub fn with_workspace_and_host(
        local_device_id: impl Into<String>,
        workspace_root: impl Into<std::path::PathBuf>,
        agent_host: Arc<dyn AgentHost>,
    ) -> Self {
        let workspace = Workspace::open(workspace_root.into()).unwrap_or_else(|e| {
            // Fallback: temp-ish path under /tmp if open fails
            tracing::warn!(error = %e, "workspace open failed, using /tmp/hakimi-studio-ws");
            Workspace::open("/tmp/hakimi-studio-ws").expect("fallback workspace")
        });
        Self {
            bus: EventBus::default(),
            state: Arc::new(Mutex::new(RuntimeState {
                sessions: HashMap::new(),
                devices: HashMap::new(),
                last_device_id: None,
            })),
            local_device_id: local_device_id.into(),
            workspace: Arc::new(workspace),
            agent_host,
        }
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn local_device_id(&self) -> &str {
        &self.local_device_id
    }

    pub async fn subscribe(&self) -> tokio::sync::broadcast::Receiver<StudioEventEnvelope> {
        self.bus.subscribe().await
    }

    /// Handle a command without a connection-bound device (tests / single-tenant).
    /// Prefer [`Self::handle_command_as`] from WebSocket handlers so multi-device
    /// roles stay correct under concurrent connections.
    pub async fn handle_command(&self, cmd: StudioCommand) -> Result<Vec<StudioEventEnvelope>> {
        self.handle_command_as(None, cmd).await
    }

    /// `actor_device_id` is the device that owns this WebSocket connection.
    /// On `hello`, the actor is taken from the command body; subsequent commands
    /// use the connection-local id (not process-global last Hello).
    pub async fn handle_command_as(
        &self,
        actor_device_id: Option<&str>,
        cmd: StudioCommand,
    ) -> Result<Vec<StudioEventEnvelope>> {
        match cmd {
            StudioCommand::Hello {
                device_id,
                protocol_version,
                device_name,
                kind,
                ..
            } => {
                if protocol_version != crate::protocol::PROTOCOL_VERSION {
                    let e = self
                        .bus
                        .emit_global(StudioEvent::HelloError {
                            message: format!(
                                "unsupported protocol_version {protocol_version}, need {}",
                                crate::protocol::PROTOCOL_VERSION
                            ),
                        })
                        .await;
                    return Ok(vec![e]);
                }
                let is_runner = device_id == self.local_device_id || kind == DeviceKind::Server;
                let connected_at = Utc::now().to_rfc3339();
                {
                    let mut g = self.state.lock().await;
                    g.last_device_id = Some(device_id.clone());
                    g.devices.insert(
                        device_id.clone(),
                        RegisteredDevice {
                            device_id: device_id.clone(),
                            device_name: device_name.clone(),
                            kind,
                            is_runner,
                            connected_at,
                        },
                    );
                }
                let reg = self
                    .bus
                    .emit_global(StudioEvent::DeviceRegistered {
                        device_id: device_id.clone(),
                        device_name,
                        kind,
                        is_runner,
                    })
                    .await;
                let e = self
                    .bus
                    .emit_global(StudioEvent::HelloOk {
                        device_id,
                        protocol_version: crate::protocol::PROTOCOL_VERSION,
                        prefer_runner: PreferRunner::Local,
                    })
                    .await;
                Ok(vec![reg, e])
            }
            StudioCommand::Ping { nonce } => {
                let e = self.bus.emit_global(StudioEvent::Pong { nonce }).await;
                Ok(vec![e])
            }
            StudioCommand::DevicesList {} => {
                let g = self.state.lock().await;
                let devices: Vec<DeviceSummary> = g
                    .devices
                    .values()
                    .map(|d| DeviceSummary {
                        device_id: d.device_id.clone(),
                        device_name: d.device_name.clone(),
                        kind: d.kind,
                        is_runner: d.is_runner,
                        connected_at: d.connected_at.clone(),
                    })
                    .collect();
                drop(g);
                let e = self
                    .bus
                    .emit_global(StudioEvent::DevicesListed { devices })
                    .await;
                Ok(vec![e])
            }
            StudioCommand::SessionCreate {
                title,
                prefer_runner,
                ..
            } => {
                let session_id = format!("sess_{}", Uuid::new_v4());
                let title = title.unwrap_or_else(|| "Untitled".into());
                let active = self.local_device_id.clone();
                let creator = {
                    let g = self.state.lock().await;
                    actor_device_id
                        .map(|s| s.to_string())
                        .or_else(|| g.last_device_id.clone())
                        .unwrap_or_else(|| active.clone())
                };
                {
                    let mut g = self.state.lock().await;
                    let mut controllers = std::collections::HashSet::new();
                    controllers.insert(creator);
                    g.sessions.insert(
                        session_id.clone(),
                        SessionState {
                            title: title.clone(),
                            prefer_runner,
                            active_runner_device_id: active.clone(),
                            controllers,
                            messages: Vec::new(),
                            updated_at: Utc::now().to_rfc3339(),
                            queue: VecDeque::new(),
                            current: None,
                            seen_requests: HashMap::new(),
                        },
                    );
                }
                let e = self
                    .bus
                    .emit(
                        &session_id,
                        StudioEvent::SessionCreated {
                            session_id: session_id.clone(),
                            title,
                            active_runner_device_id: active,
                            prefer_runner,
                        },
                    )
                    .await;
                Ok(vec![e])
            }
            StudioCommand::SessionList { limit } => {
                let limit = limit.unwrap_or(50).min(200);
                let g = self.state.lock().await;
                let mut sessions: Vec<SessionSummary> = g
                    .sessions
                    .iter()
                    .map(|(id, s)| SessionSummary {
                        session_id: id.clone(),
                        title: s.title.clone(),
                        updated_at: s.updated_at.clone(),
                        active_runner_device_id: s.active_runner_device_id.clone(),
                        last_seq: 0,
                    })
                    .collect();
                drop(g);
                for s in sessions.iter_mut() {
                    s.last_seq = self.bus.last_seq(&s.session_id).await;
                }
                sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                sessions.truncate(limit);
                let e = self
                    .bus
                    .emit_global(StudioEvent::SessionListed { sessions })
                    .await;
                Ok(vec![e])
            }
            StudioCommand::SessionAttach {
                session_id,
                after_seq,
                role,
            } => {
                let device_id = {
                    let g = self.state.lock().await;
                    actor_device_id
                        .map(|s| s.to_string())
                        .or_else(|| g.last_device_id.clone())
                };
                let g = self.state.lock().await;
                let Some(sess) = g.sessions.get(&session_id) else {
                    drop(g);
                    let e = self
                        .bus
                        .emit_global(StudioEvent::Error {
                            session_id: Some(session_id),
                            message: "session not found".into(),
                            code: Some("session_not_found".into()),
                        })
                        .await;
                    return Ok(vec![e]);
                };
                let title = sess.title.clone();
                let active_runner_device_id = sess.active_runner_device_id.clone();
                let messages = sess.messages.clone();
                let queue_depth = sess.queue.len() + usize::from(sess.current.is_some());
                drop(g);

                // Role bookkeeping: controllers may submit; viewers subscribe-only.
                if let Some(ref did) = device_id {
                    let mut g = self.state.lock().await;
                    if let Some(sess) = g.sessions.get_mut(&session_id) {
                        match role {
                            AttachRole::Controller => {
                                sess.controllers.insert(did.clone());
                            }
                            AttachRole::Viewer => {
                                // Viewers stay out of controller set (remove if re-attaching).
                                sess.controllers.remove(did);
                            }
                        }
                    }
                }

                let last = self.bus.last_seq(&session_id).await;
                let snap = StudioEvent::SessionSnapshot {
                    session_id: session_id.clone(),
                    last_seq: last,
                    title,
                    active_runner_device_id,
                    messages,
                    queue_depth,
                };
                let env = self.bus.emit(&session_id, snap).await;
                let mut out = vec![env];
                if let Some(after) = after_seq {
                    match self.bus.replay_after(&session_id, after).await {
                        ReplayResult::Ok(events) => out.extend(events),
                        ReplayResult::Gap {
                            last_seq,
                            window_oldest_seq,
                        } => {
                            let reset = self
                                .bus
                                .emit(
                                    &session_id,
                                    StudioEvent::SessionReset {
                                        session_id: session_id.clone(),
                                        reason: "replay_window_gap".into(),
                                        last_seq,
                                        window_oldest_seq,
                                    },
                                )
                                .await;
                            out.push(reset);
                        }
                    }
                }
                Ok(out)
            }
            StudioCommand::ChatSubmit {
                session_id,
                text,
                client_request_id,
                preempt,
            } => {
                if let Some(err) = self.require_controller(&session_id, actor_device_id).await {
                    return Ok(vec![err]);
                }
                self.enqueue_or_start(session_id, text, client_request_id, preempt)
                    .await
            }
            StudioCommand::ChatPreempt {
                session_id,
                text,
                client_request_id,
            } => {
                if let Some(err) = self.require_controller(&session_id, actor_device_id).await {
                    return Ok(vec![err]);
                }
                self.enqueue_or_start(session_id, text, client_request_id, true)
                    .await
            }
            StudioCommand::ChatCancel { session_id, run_id } => {
                if let Some(err) = self.require_controller(&session_id, actor_device_id).await {
                    return Ok(vec![err]);
                }
                self.cancel_run(&session_id, run_id.as_deref()).await
            }
            StudioCommand::RunnerHandoff {
                session_id,
                to_device_id,
                from_device_id,
            } => {
                if let Some(err) = self.require_controller(&session_id, actor_device_id).await {
                    return Ok(vec![err]);
                }
                let from = {
                    let g = self.state.lock().await;
                    from_device_id
                        .or_else(|| actor_device_id.map(|s| s.to_string()))
                        .or_else(|| g.last_device_id.clone())
                        .unwrap_or_else(|| self.local_device_id.clone())
                };
                // Optional: ensure target device is registered (warn only if not).
                {
                    let g = self.state.lock().await;
                    if !g.devices.contains_key(&to_device_id) {
                        info!(
                            to = %to_device_id,
                            "handoff target not yet registered; accepting anyway"
                        );
                    }
                }
                let mut g = self.state.lock().await;
                let Some(sess) = g.sessions.get_mut(&session_id) else {
                    drop(g);
                    let e = self
                        .bus
                        .emit_global(StudioEvent::Error {
                            session_id: Some(session_id),
                            message: "session not found".into(),
                            code: Some("session_not_found".into()),
                        })
                        .await;
                    return Ok(vec![e]);
                };
                // New runner becomes a controller too.
                sess.controllers.insert(to_device_id.clone());
                sess.active_runner_device_id = to_device_id.clone();
                sess.updated_at = Utc::now().to_rfc3339();
                drop(g);
                let e = self
                    .bus
                    .emit(
                        &session_id,
                        StudioEvent::RunnerChanged {
                            session_id: session_id.clone(),
                            active_runner_device_id: to_device_id,
                            from_device_id: Some(from),
                        },
                    )
                    .await;
                Ok(vec![e])
            }
            StudioCommand::WorkspaceList { session_id, path } => {
                match self.workspace.list(&path).await {
                    Ok(entries) => {
                        let views: Vec<WorkspaceEntryView> = entries
                            .into_iter()
                            .map(|e| WorkspaceEntryView {
                                name: e.name,
                                path: e.path,
                                is_dir: e.is_dir,
                                size: e.size,
                                git_status: e.git_status,
                            })
                            .collect();
                        let e = self
                            .bus
                            .emit_global(StudioEvent::WorkspaceListed {
                                session_id,
                                path,
                                entries: views,
                            })
                            .await;
                        Ok(vec![e])
                    }
                    Err(err) => Ok(vec![
                        self.bus
                            .emit_global(StudioEvent::Error {
                                session_id,
                                message: err.to_string(),
                                code: Some("workspace_error".into()),
                            })
                            .await,
                    ]),
                }
            }
            StudioCommand::WorkspaceRead { session_id, path } => {
                match self.workspace.read(&path).await {
                    Ok(content) => {
                        let e = self
                            .bus
                            .emit_global(StudioEvent::WorkspaceContent {
                                session_id,
                                path,
                                content,
                            })
                            .await;
                        Ok(vec![e])
                    }
                    Err(err) => Ok(vec![
                        self.bus
                            .emit_global(StudioEvent::Error {
                                session_id,
                                message: err.to_string(),
                                code: Some("workspace_error".into()),
                            })
                            .await,
                    ]),
                }
            }
            StudioCommand::WorkspaceWrite {
                session_id,
                path,
                content,
            } => {
                if let Some(sid) = session_id.as_deref() {
                    if let Some(err) = self.require_controller(sid, actor_device_id).await {
                        return Ok(vec![err]);
                    }
                }
                let mut out = self
                    .maybe_auto_checkpoint(session_id.clone(), &path, "pre-write")
                    .await;
                match self.workspace.write(&path, &content).await {
                    Ok(()) => {
                        let e = self
                            .bus
                            .emit_global(StudioEvent::WorkspaceWritten { session_id, path })
                            .await;
                        out.push(e);
                        Ok(out)
                    }
                    Err(err) => {
                        out.push(
                            self.bus
                                .emit_global(StudioEvent::Error {
                                    session_id,
                                    message: err.to_string(),
                                    code: Some("workspace_error".into()),
                                })
                                .await,
                        );
                        Ok(out)
                    }
                }
            }
            StudioCommand::WorkspaceCreate {
                session_id,
                path,
                is_dir,
            } => {
                if let Some(sid) = session_id.as_deref() {
                    if let Some(err) = self.require_controller(sid, actor_device_id).await {
                        return Ok(vec![err]);
                    }
                }
                match self.workspace.create(&path, is_dir).await {
                    Ok(()) => {
                        let e = self
                            .bus
                            .emit_global(StudioEvent::WorkspaceCreated {
                                session_id,
                                path,
                                is_dir,
                            })
                            .await;
                        Ok(vec![e])
                    }
                    Err(err) => Ok(vec![
                        self.bus
                            .emit_global(StudioEvent::Error {
                                session_id,
                                message: err.to_string(),
                                code: Some("workspace_error".into()),
                            })
                            .await,
                    ]),
                }
            }
            StudioCommand::WorkspaceDelete {
                session_id,
                path,
                recursive,
            } => {
                if let Some(sid) = session_id.as_deref() {
                    if let Some(err) = self.require_controller(sid, actor_device_id).await {
                        return Ok(vec![err]);
                    }
                }
                let mut out = self
                    .maybe_auto_checkpoint(session_id.clone(), &path, "pre-delete")
                    .await;
                match self.workspace.delete(&path, recursive).await {
                    Ok(()) => {
                        let e = self
                            .bus
                            .emit_global(StudioEvent::WorkspaceDeleted { session_id, path })
                            .await;
                        out.push(e);
                        Ok(out)
                    }
                    Err(err) => {
                        out.push(
                            self.bus
                                .emit_global(StudioEvent::Error {
                                    session_id,
                                    message: err.to_string(),
                                    code: Some("workspace_error".into()),
                                })
                                .await,
                        );
                        Ok(out)
                    }
                }
            }
            StudioCommand::WorkspaceGrep {
                session_id,
                path,
                pattern,
                limit,
            } => {
                let limit = limit.unwrap_or(50).min(200);
                match self.workspace.grep(&path, &pattern, limit).await {
                    Ok(hits) => {
                        let views: Vec<WorkspaceGrepHitView> = hits
                            .into_iter()
                            .map(|h| WorkspaceGrepHitView {
                                path: h.path,
                                line: h.line,
                                text: h.text,
                            })
                            .collect();
                        let e = self
                            .bus
                            .emit_global(StudioEvent::WorkspaceGrepResult {
                                session_id,
                                pattern,
                                hits: views,
                            })
                            .await;
                        Ok(vec![e])
                    }
                    Err(err) => Ok(vec![
                        self.bus
                            .emit_global(StudioEvent::Error {
                                session_id,
                                message: err.to_string(),
                                code: Some("workspace_error".into()),
                            })
                            .await,
                    ]),
                }
            }
            StudioCommand::CheckpointCreate {
                session_id,
                label,
                paths,
            } => {
                if let Some(sid) = session_id.as_deref() {
                    if let Some(err) = self.require_controller(sid, actor_device_id).await {
                        return Ok(vec![err]);
                    }
                }
                match self
                    .workspace
                    .create_checkpoint(label.as_deref(), &paths)
                    .await
                {
                    Ok(info) => {
                        let e = self
                            .bus
                            .emit_global(StudioEvent::CheckpointCreated {
                                session_id,
                                checkpoint: CheckpointView {
                                    id: info.id,
                                    label: info.label,
                                    created_at: info.created_at,
                                    files: info.files,
                                    path: info.path,
                                },
                            })
                            .await;
                        Ok(vec![e])
                    }
                    Err(err) => Ok(vec![
                        self.bus
                            .emit_global(StudioEvent::Error {
                                session_id,
                                message: err.to_string(),
                                code: Some("checkpoint_error".into()),
                            })
                            .await,
                    ]),
                }
            }
            StudioCommand::CheckpointList { session_id } => {
                match self.workspace.list_checkpoints().await {
                    Ok(list) => {
                        let views: Vec<CheckpointView> = list
                            .into_iter()
                            .map(|info| CheckpointView {
                                id: info.id,
                                label: info.label,
                                created_at: info.created_at,
                                files: info.files,
                                path: info.path,
                            })
                            .collect();
                        let e = self
                            .bus
                            .emit_global(StudioEvent::CheckpointsListed {
                                session_id,
                                checkpoints: views,
                            })
                            .await;
                        Ok(vec![e])
                    }
                    Err(err) => Ok(vec![
                        self.bus
                            .emit_global(StudioEvent::Error {
                                session_id,
                                message: err.to_string(),
                                code: Some("checkpoint_error".into()),
                            })
                            .await,
                    ]),
                }
            }
            StudioCommand::CheckpointRestore {
                session_id,
                checkpoint_id,
            } => {
                if let Some(sid) = session_id.as_deref() {
                    if let Some(err) = self.require_controller(sid, actor_device_id).await {
                        return Ok(vec![err]);
                    }
                }
                match self.workspace.restore_checkpoint(&checkpoint_id).await {
                    Ok(info) => {
                        let e = self
                            .bus
                            .emit_global(StudioEvent::CheckpointRestored {
                                session_id,
                                checkpoint: CheckpointView {
                                    id: info.id,
                                    label: info.label,
                                    created_at: info.created_at,
                                    files: info.files,
                                    path: info.path,
                                },
                            })
                            .await;
                        Ok(vec![e])
                    }
                    Err(err) => Ok(vec![
                        self.bus
                            .emit_global(StudioEvent::Error {
                                session_id,
                                message: err.to_string(),
                                code: Some("checkpoint_error".into()),
                            })
                            .await,
                    ]),
                }
            }
            // WorkerPublish is handled by hakimi-hub pure-relay, not local runtime.
            StudioCommand::WorkerPublish { .. } => {
                let e = self
                    .bus
                    .emit_global(StudioEvent::Error {
                        session_id: None,
                        message: "worker_publish is only valid on hub pure-relay connections"
                            .into(),
                        code: Some("not_supported".into()),
                    })
                    .await;
                Ok(vec![e])
            }
        }
    }

    /// Snapshot a single path before mutating writes/deletes.
    /// Best-effort: failures are logged, never block the mutation.
    async fn maybe_auto_checkpoint(
        &self,
        session_id: Option<String>,
        path: &str,
        reason: &str,
    ) -> Vec<StudioEventEnvelope> {
        if path.is_empty() || path.starts_with(".hakimi/") {
            return Vec::new();
        }
        let label = format!("auto:{reason}:{path}");
        match self
            .workspace
            .create_checkpoint(Some(&label), &[path.to_string()])
            .await
        {
            Ok(info) => {
                debug!(path = %path, id = %info.id, reason, "auto-checkpoint created");
                let e = self
                    .bus
                    .emit_global(StudioEvent::CheckpointCreated {
                        session_id,
                        checkpoint: CheckpointView {
                            id: info.id,
                            label: info.label,
                            created_at: info.created_at,
                            files: info.files,
                            path: info.path,
                        },
                    })
                    .await;
                vec![e]
            }
            Err(err) => {
                // File may not exist yet (new write) — silent skip.
                debug!(path = %path, error = %err, "auto-checkpoint skipped");
                Vec::new()
            }
        }
    }

    /// Controllers may mutate the run queue; viewers are subscribe-only.
    /// When no device is known (unit tests without Hello), allow all.
    async fn require_controller(
        &self,
        session_id: &str,
        actor_device_id: Option<&str>,
    ) -> Option<StudioEventEnvelope> {
        let g = self.state.lock().await;
        let Some(device_id) = actor_device_id
            .map(|s| s.to_string())
            .or_else(|| g.last_device_id.clone())
        else {
            return None;
        };
        let Some(sess) = g.sessions.get(session_id) else {
            drop(g);
            return Some(
                self.bus
                    .emit_global(StudioEvent::Error {
                        session_id: Some(session_id.to_string()),
                        message: "session not found".into(),
                        code: Some("session_not_found".into()),
                    })
                    .await,
            );
        };
        // Empty controllers set = legacy allow; non-empty enforces membership.
        if sess.controllers.is_empty() || sess.controllers.contains(&device_id) {
            return None;
        }
        drop(g);
        Some(
            self.bus
                .emit_global(StudioEvent::Error {
                    session_id: Some(session_id.to_string()),
                    message: format!("device {device_id} is viewer-only on this session"),
                    code: Some("viewer_readonly".into()),
                })
                .await,
        )
    }

    async fn enqueue_or_start(
        &self,
        session_id: String,
        text: String,
        client_request_id: String,
        preempt: bool,
    ) -> Result<Vec<StudioEventEnvelope>> {
        let mut out = Vec::new();

        let action = {
            let mut g = self.state.lock().await;
            match g.sessions.get_mut(&session_id) {
                None => EnqueueAction::NotFound,
                Some(sess) => {
                    if let Some(existing) = sess.seen_requests.get(&client_request_id).cloned() {
                        info!(%client_request_id, %existing, "dedupe chat.submit");
                        EnqueueAction::Duplicate { existing }
                    } else {
                        sess.messages.push(SnapshotMessage {
                            role: "user".into(),
                            content: text.clone(),
                        });
                        sess.updated_at = Utc::now().to_rfc3339();

                        if let Some(active) = sess.current.take() {
                            if preempt {
                                active.cancel.notify_one();
                                active.handle.abort();
                                let preempted_run = active.run_id.clone();
                                let run_id = format!("run_{}", Uuid::new_v4());
                                sess.seen_requests
                                    .insert(client_request_id.clone(), run_id.clone());
                                EnqueueAction::Start {
                                    run_id,
                                    text,
                                    client_request_id,
                                    preempted_run: Some(preempted_run),
                                }
                            } else {
                                sess.current = Some(active);
                                let position = sess.queue.len() + 1;
                                sess.queue.push_back(QueuedPrompt {
                                    text,
                                    client_request_id: client_request_id.clone(),
                                    preempt: false,
                                });
                                let depth = sess.queue.len();
                                let items: Vec<QueueItemView> = sess
                                    .queue
                                    .iter()
                                    .map(|q| QueueItemView {
                                        client_request_id: q.client_request_id.clone(),
                                        text_preview: q.text.chars().take(80).collect(),
                                        preempt: q.preempt,
                                    })
                                    .collect();
                                EnqueueAction::Queue {
                                    position,
                                    depth,
                                    items,
                                    client_request_id,
                                }
                            }
                        } else {
                            let run_id = format!("run_{}", Uuid::new_v4());
                            sess.seen_requests
                                .insert(client_request_id.clone(), run_id.clone());
                            EnqueueAction::Start {
                                run_id,
                                text,
                                client_request_id,
                                preempted_run: None,
                            }
                        }
                    }
                }
            }
        };

        match action {
            EnqueueAction::NotFound => {
                let e = self
                    .bus
                    .emit_global(StudioEvent::Error {
                        session_id: Some(session_id),
                        message: "session not found".into(),
                        code: Some("session_not_found".into()),
                    })
                    .await;
                Ok(vec![e])
            }
            EnqueueAction::Duplicate { existing } => {
                let e = self
                    .bus
                    .emit(
                        &session_id,
                        StudioEvent::Error {
                            session_id: Some(session_id.clone()),
                            message: format!("duplicate client_request_id (run {existing})"),
                            code: Some("duplicate_request".into()),
                        },
                    )
                    .await;
                Ok(vec![e])
            }
            EnqueueAction::Queue {
                position,
                depth,
                items,
                client_request_id,
            } => {
                out.push(
                    self.bus
                        .emit(
                            &session_id,
                            StudioEvent::RunQueued {
                                session_id: session_id.clone(),
                                client_request_id,
                                position,
                            },
                        )
                        .await,
                );
                out.push(
                    self.bus
                        .emit(
                            &session_id,
                            StudioEvent::QueueUpdated {
                                session_id: session_id.clone(),
                                depth,
                                items,
                            },
                        )
                        .await,
                );
                Ok(out)
            }
            EnqueueAction::Start {
                run_id,
                text,
                client_request_id,
                preempted_run,
            } => {
                if let Some(prev) = preempted_run {
                    out.push(
                        self.bus
                            .emit(
                                &session_id,
                                StudioEvent::RunPreempted {
                                    session_id: session_id.clone(),
                                    run_id: prev,
                                    reason: "preempt".into(),
                                },
                            )
                            .await,
                    );
                }
                out.extend(
                    self.spawn_run(session_id, run_id, text, client_request_id)
                        .await?,
                );
                Ok(out)
            }
        }
    }

    async fn spawn_run(
        &self,
        session_id: String,
        run_id: String,
        text: String,
        client_request_id: String,
    ) -> Result<Vec<StudioEventEnvelope>> {
        let cancel = Arc::new(Notify::new());
        let bus = self.bus.clone();
        let state = self.state.clone();
        let agent_host = self.agent_host.clone();
        let sid = session_id.clone();
        let first_rid = run_id.clone();
        let first_text = text;
        let first_req = client_request_id.clone();
        let first_cancel = cancel.clone();

        let handle = tokio::spawn(async move {
            let mut job: Option<(String, String, String, Arc<Notify>)> =
                Some((first_rid, first_text, first_req, first_cancel));

            while let Some((rid, job_text, _req, job_cancel)) = job.take() {
                let turn = AgentTurnRequest {
                    session_id: sid.clone(),
                    run_id: rid.clone(),
                    user_text: job_text,
                    cancel: job_cancel,
                    bus: bus.clone(),
                };
                if let Err(e) = agent_host.run_turn(turn).await {
                    warn!(error = %e, "agent host turn error");
                    let _ = bus
                        .emit(
                            &sid,
                            StudioEvent::Error {
                                session_id: Some(sid.clone()),
                                message: e.to_string(),
                                code: Some("run_error".into()),
                            },
                        )
                        .await;
                    let _ = bus
                        .emit(
                            &sid,
                            StudioEvent::SessionEnded {
                                session_id: sid.clone(),
                                run_id: rid.clone(),
                                reason: "error".into(),
                            },
                        )
                        .await;
                }

                // Clear current if it matches this run, then start next queued item.
                let mut g = state.lock().await;
                let Some(sess) = g.sessions.get_mut(&sid) else {
                    break;
                };
                if sess
                    .current
                    .as_ref()
                    .map(|c| c.run_id == rid)
                    .unwrap_or(false)
                {
                    sess.current = None;
                }
                let Some(q) = sess.queue.pop_front() else {
                    break;
                };
                let next_run = format!("run_{}", Uuid::new_v4());
                let next_cancel = Arc::new(Notify::new());
                sess.seen_requests
                    .insert(q.client_request_id.clone(), next_run.clone());
                // Keep cancel handle for preempt; JoinHandle stays on the original task.
                if let Some(cur) = sess.current.as_mut() {
                    cur.run_id = next_run.clone();
                    cur.client_request_id = q.client_request_id.clone();
                    cur.cancel = next_cancel.clone();
                } else {
                    // Original handle still running this loop — store cancel only.
                    sess.current = Some(ActiveRun {
                        run_id: next_run.clone(),
                        client_request_id: q.client_request_id.clone(),
                        cancel: next_cancel.clone(),
                        handle: tokio::spawn(async {}),
                    });
                }
                let depth = sess.queue.len();
                let items: Vec<QueueItemView> = sess
                    .queue
                    .iter()
                    .map(|item| QueueItemView {
                        client_request_id: item.client_request_id.clone(),
                        text_preview: item.text.chars().take(80).collect(),
                        preempt: item.preempt,
                    })
                    .collect();
                let next_req = q.client_request_id.clone();
                let next_text = q.text;
                drop(g);

                let _ = bus
                    .emit(
                        &sid,
                        StudioEvent::QueueUpdated {
                            session_id: sid.clone(),
                            depth,
                            items,
                        },
                    )
                    .await;
                let _ = bus
                    .emit(
                        &sid,
                        StudioEvent::RunStarted {
                            session_id: sid.clone(),
                            run_id: next_run.clone(),
                            client_request_id: next_req.clone(),
                        },
                    )
                    .await;
                job = Some((next_run, next_text, next_req, next_cancel));
            }
        });

        {
            let mut g = self.state.lock().await;
            if let Some(sess) = g.sessions.get_mut(&session_id) {
                sess.current = Some(ActiveRun {
                    run_id: run_id.clone(),
                    client_request_id: client_request_id.clone(),
                    cancel,
                    handle,
                });
            }
        }

        let e = self
            .bus
            .emit(
                &session_id,
                StudioEvent::RunStarted {
                    session_id: session_id.clone(),
                    run_id,
                    client_request_id,
                },
            )
            .await;
        Ok(vec![e])
    }

    async fn cancel_run(
        &self,
        session_id: &str,
        run_id: Option<&str>,
    ) -> Result<Vec<StudioEventEnvelope>> {
        let mut g = self.state.lock().await;
        let Some(sess) = g.sessions.get_mut(session_id) else {
            drop(g);
            let e = self
                .bus
                .emit_global(StudioEvent::Error {
                    session_id: Some(session_id.into()),
                    message: "session not found".into(),
                    code: Some("session_not_found".into()),
                })
                .await;
            return Ok(vec![e]);
        };
        let Some(active) = sess.current.take() else {
            return Ok(vec![]);
        };
        if let Some(want) = run_id {
            if active.run_id != want {
                sess.current = Some(active);
                bail!("run_id mismatch");
            }
        }
        active.cancel.notify_one();
        active.handle.abort();
        let rid = active.run_id;
        drop(g);
        let e = self
            .bus
            .emit(
                session_id,
                StudioEvent::SessionEnded {
                    session_id: session_id.into(),
                    run_id: rid,
                    reason: "cancelled".into(),
                },
            )
            .await;
        Ok(vec![e])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_submit_streams_and_ends() {
        let rt = StudioRuntime::new("device-local");
        let mut rx = rt.subscribe().await;

        let created = rt
            .handle_command(StudioCommand::SessionCreate {
                workspace_id: None,
                title: Some("t1".into()),
                prefer_runner: PreferRunner::Local,
            })
            .await
            .unwrap();
        let session_id = match &created[0].event {
            StudioEvent::SessionCreated { session_id, .. } => session_id.clone(),
            other => panic!("expected SessionCreated, got {other:?}"),
        };

        rt.handle_command(StudioCommand::ChatSubmit {
            session_id: session_id.clone(),
            text: "hello".into(),
            client_request_id: "req-1".into(),
            preempt: false,
        })
        .await
        .unwrap();

        let mut saw_delta = false;
        let mut saw_end = false;
        for _ in 0..500 {
            let env = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
                .await
                .expect("timeout")
                .expect("recv");
            match env.event {
                StudioEvent::MessageDelta { .. } => saw_delta = true,
                StudioEvent::SessionEnded { reason, .. } if reason == "done" => {
                    saw_end = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_delta, "expected message.delta");
        assert!(saw_end, "expected session.ended");
    }

    #[tokio::test]
    async fn busy_submit_queues_without_preempt() {
        let rt = StudioRuntime::new("device-local");
        let created = rt
            .handle_command(StudioCommand::SessionCreate {
                workspace_id: None,
                title: None,
                prefer_runner: PreferRunner::Local,
            })
            .await
            .unwrap();
        let session_id = match &created[0].event {
            StudioEvent::SessionCreated { session_id, .. } => session_id.clone(),
            _ => panic!("no session"),
        };

        rt.handle_command(StudioCommand::ChatSubmit {
            session_id: session_id.clone(),
            text: "first".into(),
            client_request_id: "a".into(),
            preempt: false,
        })
        .await
        .unwrap();

        let queued = rt
            .handle_command(StudioCommand::ChatSubmit {
                session_id: session_id.clone(),
                text: "second".into(),
                client_request_id: "b".into(),
                preempt: false,
            })
            .await
            .unwrap();
        assert!(
            queued
                .iter()
                .any(|e| matches!(e.event, StudioEvent::RunQueued { .. })),
            "expected run.queued: {queued:?}"
        );
    }

    #[tokio::test]
    async fn multi_device_viewer_cannot_submit() {
        let rt = StudioRuntime::new("runner-1");
        // Device A controller
        rt.handle_command(StudioCommand::Hello {
            device_id: "dev-a".into(),
            token: None,
            device_name: Some("A".into()),
            kind: DeviceKind::Web,
            protocol_version: crate::protocol::PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        let created = rt
            .handle_command(StudioCommand::SessionCreate {
                workspace_id: None,
                title: Some("relay".into()),
                prefer_runner: PreferRunner::Local,
            })
            .await
            .unwrap();
        let session_id = match &created[0].event {
            StudioEvent::SessionCreated { session_id, .. } => session_id.clone(),
            _ => panic!("expected session.created"),
        };

        // Device B attaches as viewer
        rt.handle_command(StudioCommand::Hello {
            device_id: "dev-b".into(),
            token: None,
            device_name: Some("B".into()),
            kind: DeviceKind::Web,
            protocol_version: crate::protocol::PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        rt.handle_command(StudioCommand::SessionAttach {
            session_id: session_id.clone(),
            after_seq: None,
            role: AttachRole::Viewer,
        })
        .await
        .unwrap();

        let denied = rt
            .handle_command(StudioCommand::ChatSubmit {
                session_id: session_id.clone(),
                text: "nope".into(),
                client_request_id: "v1".into(),
                preempt: false,
            })
            .await
            .unwrap();
        assert!(
            denied.iter().any(|e| matches!(
                &e.event,
                StudioEvent::Error {
                    code: Some(c),
                    ..
                } if c == "viewer_readonly"
            )),
            "viewer should be denied: {denied:?}"
        );

        // Handoff to B then B can submit as controller
        rt.handle_command(StudioCommand::Hello {
            device_id: "dev-a".into(),
            token: None,
            device_name: None,
            kind: DeviceKind::Web,
            protocol_version: crate::protocol::PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        let handoff = rt
            .handle_command(StudioCommand::RunnerHandoff {
                session_id: session_id.clone(),
                to_device_id: "dev-b".into(),
                from_device_id: Some("dev-a".into()),
            })
            .await
            .unwrap();
        assert!(
            handoff
                .iter()
                .any(|e| matches!(e.event, StudioEvent::RunnerChanged { .. }))
        );

        rt.handle_command(StudioCommand::Hello {
            device_id: "dev-b".into(),
            token: None,
            device_name: None,
            kind: DeviceKind::Web,
            protocol_version: crate::protocol::PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        // B becomes controller via handoff insertion
        let ok = rt
            .handle_command(StudioCommand::ChatSubmit {
                session_id,
                text: "hi".into(),
                client_request_id: "b1".into(),
                preempt: false,
            })
            .await
            .unwrap();
        assert!(
            ok.iter()
                .any(|e| matches!(e.event, StudioEvent::RunStarted { .. })),
            "controller after handoff should run: {ok:?}"
        );
    }

    #[tokio::test]
    async fn concurrent_connections_use_actor_device_id() {
        let rt = StudioRuntime::new("runner-1");
        // Device A creates session as controller (actor = a)
        rt.handle_command_as(
            Some("dev-a"),
            StudioCommand::Hello {
                device_id: "dev-a".into(),
                token: None,
                device_name: Some("A".into()),
                kind: DeviceKind::Web,
                protocol_version: crate::protocol::PROTOCOL_VERSION,
            },
        )
        .await
        .unwrap();
        let created = rt
            .handle_command_as(
                Some("dev-a"),
                StudioCommand::SessionCreate {
                    workspace_id: None,
                    title: Some("multi".into()),
                    prefer_runner: PreferRunner::Local,
                },
            )
            .await
            .unwrap();
        let session_id = match &created[0].event {
            StudioEvent::SessionCreated { session_id, .. } => session_id.clone(),
            _ => panic!("expected session.created"),
        };

        // Device B hello + attach as viewer WITHOUT overwriting A's connection identity.
        // (handle_command_as keeps actors separate even if last Hello is B.)
        rt.handle_command_as(
            Some("dev-b"),
            StudioCommand::Hello {
                device_id: "dev-b".into(),
                token: None,
                device_name: Some("B".into()),
                kind: DeviceKind::Web,
                protocol_version: crate::protocol::PROTOCOL_VERSION,
            },
        )
        .await
        .unwrap();
        rt.handle_command_as(
            Some("dev-b"),
            StudioCommand::SessionAttach {
                session_id: session_id.clone(),
                after_seq: None,
                role: AttachRole::Viewer,
            },
        )
        .await
        .unwrap();

        // A can still submit as controller even after B's hello updated last_device_id.
        let ok = rt
            .handle_command_as(
                Some("dev-a"),
                StudioCommand::ChatSubmit {
                    session_id: session_id.clone(),
                    text: "from-a".into(),
                    client_request_id: "a1".into(),
                    preempt: false,
                },
            )
            .await
            .unwrap();
        assert!(
            ok.iter()
                .any(|e| matches!(e.event, StudioEvent::RunStarted { .. })),
            "A should still submit: {ok:?}"
        );

        // B remains viewer
        let denied = rt
            .handle_command_as(
                Some("dev-b"),
                StudioCommand::ChatSubmit {
                    session_id,
                    text: "from-b".into(),
                    client_request_id: "b1".into(),
                    preempt: false,
                },
            )
            .await
            .unwrap();
        assert!(
            denied.iter().any(|e| matches!(
                &e.event,
                StudioEvent::Error {
                    code: Some(c),
                    ..
                } if c == "viewer_readonly"
            )),
            "B viewer denied: {denied:?}"
        );
    }

    #[tokio::test]
    async fn write_existing_file_auto_checkpoints() {
        let dir = tempfile::tempdir().unwrap();
        // Seed file before runtime owns the workspace.
        {
            let ws = Workspace::open(dir.path()).unwrap();
            ws.write("note.txt", "v1").await.unwrap();
        }
        let rt = StudioRuntime::with_workspace("device-local", dir.path());
        let out = rt
            .handle_command(StudioCommand::WorkspaceWrite {
                session_id: None,
                path: "note.txt".into(),
                content: "v2".into(),
            })
            .await
            .unwrap();
        assert!(
            out.iter()
                .any(|e| matches!(e.event, StudioEvent::CheckpointCreated { .. })),
            "expected auto checkpoint: {out:?}"
        );
        assert!(
            out.iter()
                .any(|e| matches!(e.event, StudioEvent::WorkspaceWritten { .. })),
            "expected write ack: {out:?}"
        );
        // Restore should yield v1 content via library.
        let listed = rt.workspace().list_checkpoints().await.unwrap();
        assert!(!listed.is_empty());
        rt.workspace()
            .restore_checkpoint(&listed[0].id)
            .await
            .unwrap();
        assert_eq!(rt.workspace().read("note.txt").await.unwrap(), "v1");
    }

    #[tokio::test]
    async fn viewer_cannot_workspace_write() {
        let dir = tempfile::tempdir().unwrap();
        {
            let ws = Workspace::open(dir.path()).unwrap();
            ws.write("note.txt", "v1").await.unwrap();
        }
        let rt = StudioRuntime::with_workspace("device-local", dir.path());

        rt.handle_command_as(
            Some("dev-a"),
            StudioCommand::Hello {
                device_id: "dev-a".into(),
                token: None,
                device_name: Some("A".into()),
                kind: DeviceKind::Web,
                protocol_version: crate::protocol::PROTOCOL_VERSION,
            },
        )
        .await
        .unwrap();
        let created = rt
            .handle_command_as(
                Some("dev-a"),
                StudioCommand::SessionCreate {
                    workspace_id: None,
                    title: Some("perm".into()),
                    prefer_runner: PreferRunner::Local,
                },
            )
            .await
            .unwrap();
        let session_id = match &created[0].event {
            StudioEvent::SessionCreated { session_id, .. } => session_id.clone(),
            other => panic!("expected SessionCreated, got {other:?}"),
        };

        rt.handle_command_as(
            Some("dev-b"),
            StudioCommand::Hello {
                device_id: "dev-b".into(),
                token: None,
                device_name: Some("B".into()),
                kind: DeviceKind::Web,
                protocol_version: crate::protocol::PROTOCOL_VERSION,
            },
        )
        .await
        .unwrap();
        rt.handle_command_as(
            Some("dev-b"),
            StudioCommand::SessionAttach {
                session_id: session_id.clone(),
                after_seq: None,
                role: AttachRole::Viewer,
            },
        )
        .await
        .unwrap();

        let denied = rt
            .handle_command_as(
                Some("dev-b"),
                StudioCommand::WorkspaceWrite {
                    session_id: Some(session_id),
                    path: "note.txt".into(),
                    content: "hacked".into(),
                },
            )
            .await
            .unwrap();
        assert!(
            denied.iter().any(|e| matches!(
                &e.event,
                StudioEvent::Error {
                    code: Some(c),
                    ..
                } if c == "viewer_readonly"
            )),
            "viewer write denied: {denied:?}"
        );
        assert_eq!(rt.workspace().read("note.txt").await.unwrap(), "v1");
    }
}
