//! Core agent host: clones shared `AIAgent` and streams Studio events.
//!
//! Pattern matches `chat_stream` in api.rs:
//! - clone agent from shared AppState (do NOT store request callbacks on shared agent)
//! - request-local streaming callback → MessageDelta / ToolStarted / ToolCompleted
//! - cancel via interrupt flag + Notify

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use hakimi_studio_api::{AgentHost, AgentTurnRequest, StudioEvent};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Wraps the process-wide shared agent for Studio turns.
pub struct CoreAgentHost {
    agent: Arc<Mutex<hakimi_core::AIAgent>>,
    turn_counter: AtomicU64,
}

impl CoreAgentHost {
    pub fn new(agent: Arc<Mutex<hakimi_core::AIAgent>>) -> Self {
        Self {
            agent,
            turn_counter: AtomicU64::new(0),
        }
    }
}

impl AgentHost for CoreAgentHost {
    fn run_turn(
        &self,
        req: AgentTurnRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let turn_n = self.turn_counter.fetch_add(1, Ordering::Relaxed);
            info!(
                session_id = %req.session_id,
                run_id = %req.run_id,
                turn = turn_n,
                "studio core agent turn start"
            );

            // Clone agent; attach streaming callback only to the clone.
            let mut cloned = {
                let guard = self.agent.lock().await;
                guard.clone()
            };
            cloned.clear_interrupt();
            cloned.set_streaming(true);

            // Align tool workdir with Studio focus so relative paths resolve where the
            // user is browsing (workspace_root/cwd), not the process CWD alone.
            let workspace_root = req.workspace_root.clone();
            let cwd_rel = req.cwd.trim().trim_start_matches('/').trim_end_matches('/');
            let tool_workdir = if workspace_root.is_empty() {
                if cwd_rel.is_empty() {
                    ".".to_string()
                } else {
                    cwd_rel.to_string()
                }
            } else if cwd_rel.is_empty() {
                workspace_root.clone()
            } else {
                format!(
                    "{}/{}",
                    workspace_root.trim_end_matches('/'),
                    cwd_rel
                )
            };
            cloned.set_workdir(tool_workdir.clone());

            // Inject Studio focus context so the model knows where the user is.
            let user_text = {
                let mut ctx = String::new();
                ctx.push_str("[Studio context]\n");
                ctx.push_str(&format!("workspace_root: {workspace_root}\n"));
                if cwd_rel.is_empty() {
                    ctx.push_str("cwd: / (workspace root)\n");
                } else {
                    ctx.push_str(&format!("cwd: {cwd_rel}\n"));
                }
                ctx.push_str(&format!("tool_workdir: {tool_workdir}\n"));
                if let Some(ref focus) = req.focused_path {
                    let f = focus.trim().trim_start_matches('/');
                    if !f.is_empty() {
                        ctx.push_str(&format!("focused_file: {f}\n"));
                    }
                }
                ctx.push_str(
                    "Tools run with tool_workdir as the current directory. Prefer cwd/focused_file when the user says \"this folder\", \"current file\", or \"here\". Paths may be relative to tool_workdir or workspace_root.\n\n",
                );
                ctx.push_str(&req.user_text);
                ctx
            };

            // Map streaming tokens → Studio events (tool markers + text deltas).
            // Process tokens on a single ordered task so MessageDelta / ToolStarted
            // never race (spawn-per-token reorders and breaks the chat timeline).
            let bus = req.bus.clone();
            let sid = req.session_id.clone();
            let rid = req.run_id.clone();
            let open_tools: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));

            let (token_tx, mut token_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            // Wrap sender so we can force-close the channel after the turn even if
            // the agent still holds a callback Arc.
            let token_tx = Arc::new(std::sync::Mutex::new(Some(token_tx)));
            let open_tools_worker = open_tools.clone();
            let bus_worker = bus.clone();
            let sid_worker = sid.clone();
            let rid_worker = rid.clone();
            let stream_worker = tokio::spawn(async move {
                while let Some(token) = token_rx.recv().await {
                    handle_stream_token(
                        bus_worker.clone(),
                        sid_worker.clone(),
                        rid_worker.clone(),
                        open_tools_worker.clone(),
                        token,
                    )
                    .await;
                }
            });

            let token_tx_cb = token_tx.clone();
            cloned.set_streaming_callback(Some(Arc::new(move |token: String| {
                if let Ok(guard) = token_tx_cb.lock() {
                    if let Some(tx) = guard.as_ref() {
                        let _ = tx.send(token);
                    }
                }
            })));

            // Race chat against cancel.
            let cancel = req.cancel.clone();
            // Keep interrupt handle so cancel can stop the agent loop.
            let interrupt = cloned.interrupt_handle();

            let chat_fut = async { cloned.chat(&user_text).await };

            let cancel_fut = async {
                cancel.notified().await;
                interrupt.store(true, std::sync::atomic::Ordering::Relaxed);
            };

            let outcome = tokio::select! {
                biased;
                _ = cancel_fut => {
                    // Agent may still be winding down; emit preempted terminal.
                    None
                }
                result = chat_fut => Some(result),
            };

            // Clear clone callbacks so nothing holds the bus sender.
            cloned.set_streaming_callback(None);
            cloned.set_event_callback(None);

            // Force-close the token channel and drain the ordered worker so every
            // MessageDelta / ToolStarted is emitted before MessageCompleted.
            if let Ok(mut guard) = token_tx.lock() {
                *guard = None;
            }
            let _ = stream_worker.await;

            match outcome {
                None => {
                    bus.emit(
                        &sid,
                        StudioEvent::SessionEnded {
                            session_id: sid.clone(),
                            run_id: rid.clone(),
                            reason: "preempted".into(),
                        },
                    )
                    .await;
                    Ok(())
                }
                Some(Ok(text)) => {
                    // Flush any open tools as completed.
                    {
                        let mut tools = open_tools.lock().await;
                        for (call_id, _) in tools.drain(..) {
                            let _ = bus
                                .emit(
                                    &sid,
                                    StudioEvent::ToolCompleted {
                                        session_id: sid.clone(),
                                        run_id: rid.clone(),
                                        call_id,
                                        ok: true,
                                    },
                                )
                                .await;
                        }
                    }
                    bus.emit(
                        &sid,
                        StudioEvent::MessageCompleted {
                            session_id: sid.clone(),
                            run_id: rid.clone(),
                            text,
                        },
                    )
                    .await;
                    bus.emit(
                        &sid,
                        StudioEvent::SessionEnded {
                            session_id: sid.clone(),
                            run_id: rid,
                            reason: "done".into(),
                        },
                    )
                    .await;
                    Ok(())
                }
                Some(Err(e)) => {
                    warn!(error = %e, "studio core agent turn failed");
                    bus.emit(
                        &sid,
                        StudioEvent::Error {
                            session_id: Some(sid.clone()),
                            message: e.to_string(),
                            code: Some("agent_error".into()),
                        },
                    )
                    .await;
                    bus.emit(
                        &sid,
                        StudioEvent::SessionEnded {
                            session_id: sid.clone(),
                            run_id: rid,
                            reason: "error".into(),
                        },
                    )
                    .await;
                    // Return Ok so spawn_run does not double-emit SessionEnded.
                    Ok(())
                }
            }
        })
    }
}

async fn handle_stream_token(
    bus: hakimi_studio_api::EventBus,
    session_id: String,
    run_id: String,
    open_tools: Arc<Mutex<Vec<(String, String)>>>,
    token: String,
) {
    // Tool start: \u{001e}hakimi_tool:⚙️ name (args)
    if let Some(notice) = token.strip_prefix("\u{001e}hakimi_tool:") {
        let name = parse_tool_name(notice);
        let call_id = format!("tool_{}_{}", run_id, open_tools.lock().await.len());
        open_tools
            .lock()
            .await
            .push((call_id.clone(), name.clone()));
        let _ = bus
            .emit(
                &session_id,
                StudioEvent::ToolStarted {
                    session_id: session_id.clone(),
                    run_id: run_id.clone(),
                    name,
                    call_id,
                },
            )
            .await;
        return;
    }

    // Tool result: \u{001e}hakimi_tool_result:name|content
    if let Some(rest) = token.strip_prefix("\u{001e}hakimi_tool_result:") {
        let tool_name = rest.split('|').next().unwrap_or("tool");
        let mut tools = open_tools.lock().await;
        // Prefer matching open tool by name, else pop last.
        let idx = tools
            .iter()
            .rposition(|(_, n)| n == tool_name)
            .or_else(|| tools.len().checked_sub(1));
        if let Some(i) = idx {
            let (call_id, _) = tools.remove(i);
            drop(tools);
            let _ = bus
                .emit(
                    &session_id,
                    StudioEvent::ToolCompleted {
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        call_id,
                        ok: true,
                    },
                )
                .await;
        }
        return;
    }

    // Other control markers — skip from chat body.
    if token.starts_with('\u{001e}') || token.starts_with("\u{001e}") {
        return;
    }

    if token.is_empty() {
        return;
    }

    let _ = bus
        .emit(
            &session_id,
            StudioEvent::MessageDelta {
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                delta: token,
            },
        )
        .await;
}

fn parse_tool_name(notice: &str) -> String {
    // Formats like "⚙️ read_file (path: ...)" or plain "read_file"
    let s = notice.trim();
    let without_emoji = s
        .trim_start_matches(|c: char| c == '⚙' || c == '\u{fe0f}' || c.is_whitespace())
        .trim();
    let name = without_emoji
        .split([' ', '('])
        .next()
        .unwrap_or(without_emoji)
        .trim();
    if name.is_empty() {
        "tool".into()
    } else {
        name.to_string()
    }
}
