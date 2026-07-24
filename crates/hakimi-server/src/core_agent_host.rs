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

            // Map streaming tokens → Studio events (tool markers + text deltas).
            let bus = req.bus.clone();
            let sid = req.session_id.clone();
            let rid = req.run_id.clone();
            let open_tools: Arc<Mutex<Vec<(String, String)>>> =
                Arc::new(Mutex::new(Vec::new()));

            let open_tools_cb = open_tools.clone();
            let bus_cb = bus.clone();
            let sid_cb = sid.clone();
            let rid_cb = rid.clone();
            cloned.set_streaming_callback(Some(Arc::new(move |token: String| {
                let bus = bus_cb.clone();
                let sid = sid_cb.clone();
                let rid = rid_cb.clone();
                let open_tools = open_tools_cb.clone();
                tokio::spawn(async move {
                    handle_stream_token(bus, sid, rid, open_tools, token).await;
                });
            })));

            // Race chat against cancel.
            let cancel = req.cancel.clone();
            let user_text = req.user_text.clone();
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
        .split(|c: char| c == ' ' || c == '(')
        .next()
        .unwrap_or(without_emoji)
        .trim();
    if name.is_empty() {
        "tool".into()
    } else {
        name.to_string()
    }
}
