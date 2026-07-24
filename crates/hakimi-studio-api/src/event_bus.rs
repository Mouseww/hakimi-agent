//! Per-session monotonic event bus with bounded replay window.
//!
//! Phase 2: `replay_after` reports a gap when `after_seq` is older than the
//! oldest retained event so clients can `SessionReset` + resync.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};
use tracing::debug;

use crate::protocol::{StudioEvent, StudioEventEnvelope};

/// Default max events retained per session for after_seq replay.
pub const DEFAULT_WINDOW: usize = 4096;

/// Result of a bounded replay request.
#[derive(Debug, Clone)]
pub enum ReplayResult {
    /// Events with seq > after_seq, in order (may be empty if fully caught up).
    Ok(Vec<StudioEventEnvelope>),
    /// Client is behind the window; must take a fresh snapshot.
    Gap {
        last_seq: u64,
        window_oldest_seq: Option<u64>,
    },
}

#[derive(Clone)]
pub struct EventBus {
    inner: Arc<RwLock<EventBusInner>>,
    capacity: usize,
}

struct EventBusInner {
    /// Global fan-out for all envelopes (WS subscribers).
    tx: broadcast::Sender<StudioEventEnvelope>,
    sessions: HashMap<String, SessionStream>,
}

struct SessionStream {
    next_seq: u64,
    window: VecDeque<StudioEventEnvelope>,
}

impl EventBus {
    pub fn new(broadcast_capacity: usize, window_capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(broadcast_capacity.max(16));
        Self {
            inner: Arc::new(RwLock::new(EventBusInner {
                tx,
                sessions: HashMap::new(),
            })),
            // Allow tiny windows in tests; production uses DEFAULT_WINDOW (4096).
            capacity: window_capacity.max(1),
        }
    }

    pub fn window_capacity(&self) -> usize {
        self.capacity
    }

    pub async fn subscribe(&self) -> broadcast::Receiver<StudioEventEnvelope> {
        self.inner.read().await.tx.subscribe()
    }

    /// Emit a session-scoped event; assigns next seq and retains in window.
    pub async fn emit(&self, session_id: &str, event: StudioEvent) -> StudioEventEnvelope {
        let mut g = self.inner.write().await;
        let stream = g
            .sessions
            .entry(session_id.to_string())
            .or_insert_with(|| SessionStream {
                next_seq: 1,
                window: VecDeque::new(),
            });
        let seq = stream.next_seq;
        stream.next_seq += 1;
        let env = StudioEventEnvelope::new(seq, Some(session_id.to_string()), event);
        stream.window.push_back(env.clone());
        while stream.window.len() > self.capacity {
            stream.window.pop_front();
        }
        let _ = g.tx.send(env.clone());
        debug!(session_id, seq, "studio event emitted");
        env
    }

    /// Emit without session seq (hello/pong/global errors). seq = 0.
    pub async fn emit_global(&self, event: StudioEvent) -> StudioEventEnvelope {
        let g = self.inner.read().await;
        let env = StudioEventEnvelope::new(0, None, event);
        let _ = g.tx.send(env.clone());
        env
    }

    pub async fn last_seq(&self, session_id: &str) -> u64 {
        let g = self.inner.read().await;
        g.sessions
            .get(session_id)
            .map(|s| s.next_seq.saturating_sub(1))
            .unwrap_or(0)
    }

    pub async fn oldest_seq(&self, session_id: &str) -> Option<u64> {
        let g = self.inner.read().await;
        g.sessions
            .get(session_id)
            .and_then(|s| s.window.front().map(|e| e.seq))
    }

    /// Replay events with seq > after_seq from the bounded window.
    ///
    /// If the window has slid past `after_seq` (gap), returns [`ReplayResult::Gap`].
    pub async fn replay_after(&self, session_id: &str, after_seq: u64) -> ReplayResult {
        let g = self.inner.read().await;
        let Some(stream) = g.sessions.get(session_id) else {
            // No events yet — treat as empty ok.
            return ReplayResult::Ok(Vec::new());
        };
        let last_seq = stream.next_seq.saturating_sub(1);
        let oldest = stream.window.front().map(|e| e.seq);

        // Gap: client asked for events older than what we still have.
        // after_seq == 0 means "from the beginning of retained window" → no gap.
        if after_seq > 0 {
            if let Some(old) = oldest {
                if after_seq + 1 < old {
                    return ReplayResult::Gap {
                        last_seq,
                        window_oldest_seq: Some(old),
                    };
                }
            } else if last_seq > after_seq {
                // Window empty but seq advanced? shouldn't happen; treat as gap.
                return ReplayResult::Gap {
                    last_seq,
                    window_oldest_seq: None,
                };
            }
        }

        let events: Vec<_> = stream
            .window
            .iter()
            .filter(|e| e.seq > after_seq)
            .cloned()
            .collect();
        ReplayResult::Ok(events)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024, DEFAULT_WINDOW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn seq_is_monotonic_per_session() {
        let bus = EventBus::default();
        let e1 = bus
            .emit(
                "s1",
                StudioEvent::MessageDelta {
                    session_id: "s1".into(),
                    run_id: "r1".into(),
                    delta: "a".into(),
                },
            )
            .await;
        let e2 = bus
            .emit(
                "s1",
                StudioEvent::MessageDelta {
                    session_id: "s1".into(),
                    run_id: "r1".into(),
                    delta: "b".into(),
                },
            )
            .await;
        assert_eq!(e1.seq, 1);
        assert_eq!(e2.seq, 2);
        match bus.replay_after("s1", 1).await {
            ReplayResult::Ok(replay) => {
                assert_eq!(replay.len(), 1);
                assert_eq!(replay[0].seq, 2);
            }
            ReplayResult::Gap { .. } => panic!("unexpected gap"),
        }
    }

    #[tokio::test]
    async fn replay_detects_gap_when_window_slides() {
        // Tiny window of 2.
        let bus = EventBus::new(32, 2);
        for i in 0..4 {
            bus.emit(
                "s1",
                StudioEvent::MessageDelta {
                    session_id: "s1".into(),
                    run_id: "r1".into(),
                    delta: format!("{i}"),
                },
            )
            .await;
        }
        // Window should hold seq 3,4. after_seq=1 needs 2 which is gone → gap.
        match bus.replay_after("s1", 1).await {
            ReplayResult::Gap {
                last_seq,
                window_oldest_seq,
            } => {
                assert_eq!(last_seq, 4);
                assert_eq!(window_oldest_seq, Some(3));
            }
            ReplayResult::Ok(_) => panic!("expected gap"),
        }
        // after_seq=2 (one before oldest) → still gap (2+1=3 is ok actually)
        // after_seq=2 means we want seq>2, oldest is 3 → ok
        match bus.replay_after("s1", 2).await {
            ReplayResult::Ok(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].seq, 3);
            }
            ReplayResult::Gap { .. } => panic!("should not gap for after_seq=2"),
        }
    }
}
