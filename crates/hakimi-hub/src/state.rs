//! Shared hub state: devices, pure-relay routing, optional embedded runtime.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use hakimi_studio_api::{
    DeviceKind, DeviceSummary, EventBus, PROTOCOL_VERSION, PreferRunner, StudioCommand,
    StudioEvent, StudioEventEnvelope, StudioRuntime, WorkerDispatch,
};
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};

/// Hub execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubMode {
    /// Demo / single-process: in-process `StudioRuntime` (agent loop may run here).
    Embedded,
    /// Production: pure relay — no tools, no keys, no agent loop.
    /// Workers publish events; client commands are forwarded to the Active Runner.
    Relay,
}

/// Process-wide hub state.
#[derive(Clone)]
pub struct HubState {
    pub bus: EventBus,
    pub mode: HubMode,
    /// Only set in Embedded mode (or when explicitly injected).
    pub runtime: Option<Arc<StudioRuntime>>,
    pub devices: Arc<Mutex<HashMap<String, HubDevice>>>,
    /// Live WS connections keyed by device_id (set after hello).
    pub connections: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Outbound>>>>,
    /// session_id → active_runner_device_id
    pub sessions: Arc<Mutex<HashMap<String, SessionRoute>>>,
    pub hub_token: Option<String>,
    pub hub_id: String,
}

#[derive(Debug, Clone)]
pub struct HubDevice {
    pub device_id: String,
    pub device_name: Option<String>,
    pub kind: DeviceKind,
    pub is_runner: bool,
    pub connected_at: String,
    pub last_seen: String,
}

#[derive(Debug, Clone)]
pub struct SessionRoute {
    pub active_runner_device_id: String,
    pub title: Option<String>,
    pub last_seq: u64,
    pub updated_at: String,
}

/// Messages pushed onto a specific connection's outbound queue.
#[derive(Debug, Clone)]
pub enum Outbound {
    /// JSON text frame (StudioEventEnvelope or WorkerDispatch envelope).
    Text(String),
}

impl HubState {
    /// Production pure-relay hub (no embedded runtime).
    pub fn new_relay(hub_token: Option<String>) -> Self {
        let hub_id = format!("hub-{}", uuid::Uuid::new_v4());
        Self {
            bus: EventBus::default(),
            mode: HubMode::Relay,
            runtime: None,
            devices: Arc::new(Mutex::new(HashMap::new())),
            connections: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            hub_token,
            hub_id,
        }
    }

    /// Embedded demo hub (in-process StudioRuntime) — default for unit tests / local smoke.
    pub fn new_embedded(hub_token: Option<String>) -> Self {
        let hub_id = format!("hub-{}", uuid::Uuid::new_v4());
        let runtime = Arc::new(StudioRuntime::new(hub_id.clone()));
        Self {
            bus: runtime.bus().clone(),
            mode: HubMode::Embedded,
            runtime: Some(runtime),
            devices: Arc::new(Mutex::new(HashMap::new())),
            connections: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            hub_token,
            hub_id,
        }
    }

    /// Back-compat alias: embedded demo.
    pub fn new(hub_token: Option<String>) -> Self {
        Self::new_embedded(hub_token)
    }

    pub fn with_runtime(runtime: Arc<StudioRuntime>, hub_token: Option<String>) -> Self {
        Self {
            bus: runtime.bus().clone(),
            mode: HubMode::Embedded,
            hub_id: runtime.local_device_id().to_string(),
            runtime: Some(runtime),
            devices: Arc::new(Mutex::new(HashMap::new())),
            connections: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            hub_token,
        }
    }

    pub fn check_token(&self, token: Option<&str>) -> bool {
        match &self.hub_token {
            None => true,
            Some(expected) => token == Some(expected.as_str()),
        }
    }

    pub async fn register_connection(
        &self,
        device_id: String,
        tx: mpsc::UnboundedSender<Outbound>,
    ) {
        let mut g = self.connections.lock().await;
        g.insert(device_id, tx);
    }

    pub async fn unregister_connection(&self, device_id: &str) {
        let mut g = self.connections.lock().await;
        g.remove(device_id);
    }

    pub async fn register_device(
        &self,
        device_id: String,
        device_name: Option<String>,
        kind: DeviceKind,
        is_runner: bool,
    ) -> DeviceSummary {
        let now = Utc::now().to_rfc3339();
        let summary = DeviceSummary {
            device_id: device_id.clone(),
            device_name: device_name.clone(),
            kind,
            is_runner,
            connected_at: now.clone(),
        };
        let mut g = self.devices.lock().await;
        g.insert(
            device_id.clone(),
            HubDevice {
                device_id,
                device_name,
                kind,
                is_runner,
                connected_at: now.clone(),
                last_seen: now,
            },
        );
        info!(count = g.len(), "hub devices updated");
        summary
    }

    pub async fn list_devices(&self) -> Vec<DeviceSummary> {
        let g = self.devices.lock().await;
        g.values()
            .map(|d| DeviceSummary {
                device_id: d.device_id.clone(),
                device_name: d.device_name.clone(),
                kind: d.kind,
                is_runner: d.is_runner,
                connected_at: d.connected_at.clone(),
            })
            .collect()
    }

    pub async fn handle_command(
        &self,
        cmd: StudioCommand,
    ) -> anyhow::Result<Vec<StudioEventEnvelope>> {
        self.handle_command_as(None, cmd).await
    }

    pub async fn handle_command_as(
        &self,
        actor_device_id: Option<&str>,
        cmd: StudioCommand,
    ) -> anyhow::Result<Vec<StudioEventEnvelope>> {
        // Token / hello registration.
        if let StudioCommand::Hello {
            ref device_id,
            ref token,
            ref device_name,
            kind,
            protocol_version,
            ..
        } = cmd
        {
            if protocol_version != PROTOCOL_VERSION {
                let e = self
                    .bus
                    .emit_global(StudioEvent::HelloError {
                        message: format!(
                            "unsupported protocol_version {protocol_version}, need {PROTOCOL_VERSION}"
                        ),
                    })
                    .await;
                return Ok(vec![e]);
            }
            if !self.check_token(token.as_deref()) {
                let e = self
                    .bus
                    .emit_global(StudioEvent::HelloError {
                        message: "invalid hub token".into(),
                    })
                    .await;
                return Ok(vec![e]);
            }
            let is_runner = kind == DeviceKind::Server || kind == DeviceKind::Desktop;
            let _ = self
                .register_device(device_id.clone(), device_name.clone(), kind, is_runner)
                .await;
            let reg = self
                .bus
                .emit_global(StudioEvent::DeviceRegistered {
                    device_id: device_id.clone(),
                    device_name: device_name.clone(),
                    kind,
                    is_runner,
                })
                .await;
            let prefer = match self.mode {
                HubMode::Embedded => PreferRunner::Local,
                HubMode::Relay => PreferRunner::Server,
            };
            let ok = self
                .bus
                .emit_global(StudioEvent::HelloOk {
                    device_id: device_id.clone(),
                    protocol_version: PROTOCOL_VERSION,
                    prefer_runner: prefer,
                })
                .await;
            // Also register in embedded runtime for consistency.
            if let Some(rt) = &self.runtime {
                let _ = rt.handle_command_as(actor_device_id, cmd).await;
            }
            return Ok(vec![reg, ok]);
        }

        if matches!(cmd, StudioCommand::DevicesList {}) {
            let devices = self.list_devices().await;
            let e = self
                .bus
                .emit_global(StudioEvent::DevicesListed { devices })
                .await;
            return Ok(vec![e]);
        }

        if matches!(cmd, StudioCommand::Ping { .. }) {
            if let StudioCommand::Ping { nonce } = cmd {
                let e = self.bus.emit_global(StudioEvent::Pong { nonce }).await;
                return Ok(vec![e]);
            }
        }

        // Workers inject events into the hub bus (pure relay path).
        if let StudioCommand::WorkerPublish { events } = cmd {
            return self.handle_worker_publish(actor_device_id, events).await;
        }

        match self.mode {
            HubMode::Embedded => {
                let Some(rt) = &self.runtime else {
                    anyhow::bail!("embedded mode missing runtime");
                };
                rt.handle_command_as(actor_device_id, cmd).await
            }
            HubMode::Relay => self.relay_to_runner(actor_device_id, cmd).await,
        }
    }

    async fn handle_worker_publish(
        &self,
        actor: Option<&str>,
        events: Vec<StudioEventEnvelope>,
    ) -> anyhow::Result<Vec<StudioEventEnvelope>> {
        // Only registered runners may publish (or any device in embedded tests).
        if let Some(id) = actor {
            let g = self.devices.lock().await;
            if let Some(d) = g.get(id) {
                if !d.is_runner && self.mode == HubMode::Relay {
                    let e = self
                        .bus
                        .emit_global(StudioEvent::Error {
                            session_id: None,
                            message: "only runner devices may worker_publish".into(),
                            code: Some("not_a_runner".into()),
                        })
                        .await;
                    return Ok(vec![e]);
                }
            }
        }

        let mut out = Vec::with_capacity(events.len());
        for env in events {
            // Learn session routes from worker events.
            self.ingest_route_from_event(&env).await;
            // Re-emit with hub seq so all clients see a consistent stream.
            let re = if let Some(ref sid) = env.session_id {
                self.bus.emit(sid, env.event.clone()).await
            } else {
                self.bus.emit_global(env.event.clone()).await
            };
            out.push(re);
        }
        Ok(out)
    }

    async fn ingest_route_from_event(&self, env: &StudioEventEnvelope) {
        let now = Utc::now().to_rfc3339();
        match &env.event {
            StudioEvent::SessionCreated {
                session_id,
                title,
                active_runner_device_id,
                ..
            } => {
                let mut g = self.sessions.lock().await;
                g.insert(
                    session_id.clone(),
                    SessionRoute {
                        active_runner_device_id: active_runner_device_id.clone(),
                        title: Some(title.clone()),
                        last_seq: env.seq,
                        updated_at: now,
                    },
                );
            }
            StudioEvent::RunnerChanged {
                session_id,
                active_runner_device_id,
                ..
            } => {
                let mut g = self.sessions.lock().await;
                g.entry(session_id.clone())
                    .and_modify(|r| {
                        r.active_runner_device_id = active_runner_device_id.clone();
                        r.last_seq = env.seq;
                        r.updated_at = now.clone();
                    })
                    .or_insert(SessionRoute {
                        active_runner_device_id: active_runner_device_id.clone(),
                        title: None,
                        last_seq: env.seq,
                        updated_at: now,
                    });
            }
            StudioEvent::SessionSnapshot {
                session_id,
                last_seq,
                active_runner_device_id,
                title,
                ..
            } => {
                let mut g = self.sessions.lock().await;
                g.insert(
                    session_id.clone(),
                    SessionRoute {
                        active_runner_device_id: active_runner_device_id.clone(),
                        title: Some(title.clone()),
                        last_seq: *last_seq,
                        updated_at: now,
                    },
                );
            }
            _ => {}
        }
    }

    async fn relay_to_runner(
        &self,
        actor_device_id: Option<&str>,
        cmd: StudioCommand,
    ) -> anyhow::Result<Vec<StudioEventEnvelope>> {
        let session_id = session_id_of(&cmd);
        let runner_id = if let Some(sid) = session_id.as_deref() {
            let g = self.sessions.lock().await;
            g.get(sid).map(|r| r.active_runner_device_id.clone())
        } else {
            // No session yet (e.g. session_create): pick any online runner.
            let g = self.devices.lock().await;
            g.values()
                .find(|d| d.is_runner)
                .map(|d| d.device_id.clone())
        };

        let Some(runner_id) = runner_id else {
            let e = self
                .bus
                .emit_global(StudioEvent::Error {
                    session_id,
                    message: "no active runner available on hub".into(),
                    code: Some("no_runner".into()),
                })
                .await;
            return Ok(vec![e]);
        };

        let dispatch = WorkerDispatch::new(actor_device_id.map(|s| s.to_string()), cmd);
        let text = serde_json::to_string(&dispatch)?;
        let conns = self.connections.lock().await;
        if let Some(tx) = conns.get(&runner_id) {
            if tx.send(Outbound::Text(text)).is_err() {
                warn!(%runner_id, "runner connection closed");
            }
            // Async: runner will worker_publish results. Ack with empty (events fan-out later).
            Ok(vec![])
        } else {
            drop(conns);
            let e = self
                .bus
                .emit_global(StudioEvent::Error {
                    session_id,
                    message: format!("runner {runner_id} not connected"),
                    code: Some("runner_offline".into()),
                })
                .await;
            Ok(vec![e])
        }
    }

    pub async fn health_json(&self) -> serde_json::Value {
        let n = self.devices.lock().await.len();
        let sessions = self.sessions.lock().await.len();
        let mode = match self.mode {
            HubMode::Embedded => "embedded",
            HubMode::Relay => "relay",
        };
        serde_json::json!({
            "ok": true,
            "service": "hakimi-hub",
            "hub_id": self.hub_id,
            "protocol_version": PROTOCOL_VERSION,
            "mode": mode,
            "devices": n,
            "sessions": sessions,
            "prefer_runner": PreferRunner::Server,
            "role": "relay",
            "executes_tools": false,
            "stores_provider_keys": false,
            "embedded_runtime": self.runtime.is_some(),
        })
    }
}

fn session_id_of(cmd: &StudioCommand) -> Option<String> {
    match cmd {
        StudioCommand::SessionAttach { session_id, .. }
        | StudioCommand::ChatSubmit { session_id, .. }
        | StudioCommand::ChatCancel { session_id, .. }
        | StudioCommand::ChatPreempt { session_id, .. }
        | StudioCommand::RunnerHandoff { session_id, .. } => Some(session_id.clone()),
        StudioCommand::WorkspaceList { session_id, .. }
        | StudioCommand::WorkspaceRead { session_id, .. }
        | StudioCommand::WorkspaceWrite { session_id, .. }
        | StudioCommand::WorkspaceCreate { session_id, .. }
        | StudioCommand::WorkspaceDelete { session_id, .. }
        | StudioCommand::WorkspaceGrep { session_id, .. } => session_id.clone(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hub_rejects_bad_token() {
        let hub = HubState::new_embedded(Some("secret".into()));
        let out = hub
            .handle_command(StudioCommand::Hello {
                device_id: "x".into(),
                token: Some("wrong".into()),
                device_name: None,
                kind: DeviceKind::Web,
                protocol_version: PROTOCOL_VERSION,
            })
            .await
            .unwrap();
        assert!(matches!(out[0].event, StudioEvent::HelloError { .. }));
    }

    #[tokio::test]
    async fn hub_hello_and_session_roundtrip_embedded() {
        let hub = HubState::new_embedded(None);
        let out = hub
            .handle_command(StudioCommand::Hello {
                device_id: "web-a".into(),
                token: None,
                device_name: Some("Browser A".into()),
                kind: DeviceKind::Web,
                protocol_version: PROTOCOL_VERSION,
            })
            .await
            .unwrap();
        assert!(
            out.iter()
                .any(|e| matches!(e.event, StudioEvent::HelloOk { .. }))
        );

        let created = hub
            .handle_command(StudioCommand::SessionCreate {
                workspace_id: None,
                title: Some("hub-demo".into()),
                prefer_runner: PreferRunner::Server,
            })
            .await
            .unwrap();
        assert!(matches!(
            created[0].event,
            StudioEvent::SessionCreated { .. }
        ));

        let health = hub.health_json().await;
        assert_eq!(health["ok"], true);
        assert_eq!(health["executes_tools"], false);
        assert_eq!(health["mode"], "embedded");
    }

    #[tokio::test]
    async fn pure_relay_no_runner_errors() {
        let hub = HubState::new_relay(None);
        hub.handle_command(StudioCommand::Hello {
            device_id: "web-a".into(),
            token: None,
            device_name: None,
            kind: DeviceKind::Web,
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        let out = hub
            .handle_command(StudioCommand::SessionCreate {
                workspace_id: None,
                title: Some("x".into()),
                prefer_runner: PreferRunner::Server,
            })
            .await
            .unwrap();
        assert!(
            out.iter().any(|e| matches!(
                &e.event,
                StudioEvent::Error {
                    code: Some(c),
                    ..
                } if c == "no_runner"
            )),
            "expected no_runner: {out:?}"
        );
        let health = hub.health_json().await;
        assert_eq!(health["mode"], "relay");
        assert_eq!(health["embedded_runtime"], false);
    }

    #[tokio::test]
    async fn worker_publish_fans_out_and_tracks_session() {
        let hub = HubState::new_relay(None);
        hub.handle_command(StudioCommand::Hello {
            device_id: "runner-1".into(),
            token: None,
            device_name: None,
            kind: DeviceKind::Server,
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();

        let env = StudioEventEnvelope::new(
            1,
            Some("sess_1".into()),
            StudioEvent::SessionCreated {
                session_id: "sess_1".into(),
                title: "t".into(),
                active_runner_device_id: "runner-1".into(),
                prefer_runner: PreferRunner::Server,
            },
        );
        let out = hub
            .handle_command_as(
                Some("runner-1"),
                StudioCommand::WorkerPublish { events: vec![env] },
            )
            .await
            .unwrap();
        assert!(
            out.iter()
                .any(|e| matches!(e.event, StudioEvent::SessionCreated { .. }))
        );
        let sessions = hub.sessions.lock().await;
        assert_eq!(
            sessions.get("sess_1").unwrap().active_runner_device_id,
            "runner-1"
        );
    }
}
