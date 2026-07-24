//! Pluggable agent runner for Studio sessions.
//!
//! The runtime owns queue/preempt/session state. The host owns how a single
//! turn is executed (mock streamer in unit tests, real `AIAgent` in server).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;

use crate::event_bus::EventBus;
use crate::protocol::StudioEvent;

/// Arguments for one agent turn inside a Studio session.
pub struct AgentTurnRequest {
    pub session_id: String,
    pub run_id: String,
    pub user_text: String,
    pub cancel: Arc<Notify>,
    pub bus: EventBus,
    /// Absolute workspace root (path jail + tool workdir base).
    pub workspace_root: String,
    /// Workspace-relative cwd the user is browsing (may be empty = root).
    pub cwd: String,
    /// Workspace-relative focused file, if any.
    pub focused_path: Option<String>,
}

/// Host that executes a single chat turn and emits Studio events on `bus`.
///
/// Implementations must:
/// - stream `message.delta` / `message.completed` / `tool.*` as appropriate
/// - always emit terminal `session.ended` (reason: `done` | `cancelled` | `preempted` | `error`)
/// - honour `cancel` (and ideally interrupt the underlying agent)
pub trait AgentHost: Send + Sync {
    fn run_turn(
        &self,
        req: AgentTurnRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}

/// Deterministic mock used by unit tests and when no core agent is wired.
pub struct MockAgentHost;

impl AgentHost for MockAgentHost {
    fn run_turn(
        &self,
        req: AgentTurnRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            mock_agent_loop(
                req.bus,
                req.session_id,
                req.run_id,
                req.user_text,
                req.cancel,
            )
            .await
        })
    }
}

async fn mock_agent_loop(
    bus: EventBus,
    session_id: String,
    run_id: String,
    user_text: String,
    cancel: Arc<Notify>,
) -> Result<()> {
    let end = run_id.len().min(12);
    let call_id = format!("call_{}", &run_id[4..end]);
    bus.emit(
        &session_id,
        StudioEvent::ToolStarted {
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            name: "mock_echo".into(),
            call_id: call_id.clone(),
        },
    )
    .await;

    tokio::select! {
        _ = cancel.notified() => {
            bus.emit(
                &session_id,
                StudioEvent::SessionEnded {
                    session_id: session_id.clone(),
                    run_id: run_id.clone(),
                    reason: "preempted".into(),
                },
            )
            .await;
            return Ok(());
        }
        _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {}
    }

    bus.emit(
        &session_id,
        StudioEvent::ToolCompleted {
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            call_id,
            ok: true,
        },
    )
    .await;

    // Explicit mock marker so UI/users never confuse this with CoreAgentHost.
    let reply = format!(
        "[mock] Studio mock reply (no AIAgent wired — run `hakimi --serve` for real agent):\n\n{user_text}"
    );
    for ch in reply.chars() {
        tokio::select! {
            _ = cancel.notified() => {
                bus.emit(
                    &session_id,
                    StudioEvent::SessionEnded {
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        reason: "preempted".into(),
                    },
                )
                .await;
                return Ok(());
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(1)) => {
                bus.emit(
                    &session_id,
                    StudioEvent::MessageDelta {
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        delta: ch.to_string(),
                    },
                )
                .await;
            }
        }
    }

    bus.emit(
        &session_id,
        StudioEvent::MessageCompleted {
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            text: reply,
        },
    )
    .await;

    bus.emit(
        &session_id,
        StudioEvent::SessionEnded {
            session_id: session_id.clone(),
            run_id,
            reason: "done".into(),
        },
    )
    .await;
    Ok(())
}
