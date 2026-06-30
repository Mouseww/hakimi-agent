# Persona Activity Feed (backend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a process-global persona-activity event bus + two HTTP endpoints (`GET /api/activity/snapshot`, `GET /api/activity/stream` SSE) that report each persona's real-time work state, published from the persona CRUD, gateway turn loop, WebUI streaming chat, and the team executor.

**Architecture:** A `hakimi-common::activity` module holds a global `ActivityHub` (a `tokio::sync::broadcast` sender + a `Mutex<HashMap>` state snapshot), mirroring the existing global-singleton pattern of `MESSAGE_QUEUE` in `hakimi-tools/src/builtin_send_message.rs`. The state machine is a pure `apply(map, &event)` + `displayed_state(entry)` pair (unit-testable without the global). Publishers call `hakimi_common::activity::publish(event)`. The server joins the persona registry roster with the hub's live states for the snapshot, and streams `ActivityEvent`s over SSE.

**Tech Stack:** Rust (tokio broadcast, serde, chrono, axum SSE). Builds run via Docker `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" <args>` (`RUSTFLAGS=-Dwarnings`). This is the BACKEND half of the Persona Office Dashboard; the frontend office UI is a separate plan that consumes these endpoints.

**Spec:** `docs/superpowers/specs/2026-06-26-persona-office-dashboard-design.md`

**CI/toolchain notes:** CI nightly rustfmt/clippy differ from the local Docker image — do NOT `cargo fmt --all` to fix CI fmt; hand-apply CI's diff. Local Docker `clippy --workspace --all-targets --all-features` reliably catches stable lints + compile errors; run it before pushing. CI (`pull_request` to main) is the authoritative gate. See the repo memories on this.

---

## File Structure

**New:**
- `crates/hakimi-common/src/activity.rs` — `ActivityEvent`, `PersonaState`, `LiveState`, `PersonaActivity`, pure `apply`/`displayed_state`, global hub (`publish`/`subscribe`/`all_live_states`).

**Modified:**
- `crates/hakimi-common/Cargo.toml` — add `tokio` (for `broadcast`).
- `crates/hakimi-common/src/lib.rs` — `mod activity; pub use activity::*;`.
- `crates/hakimi-server/src/api.rs` — two handlers + routes; publish in `create_agent`/`update_agent`/`delete_agent` and `agent_chat_stream`.
- `crates/hakimi-cli/src/entry.rs` — publish `TurnStarted`/`TurnEnded` around the gateway turn.
- `crates/hakimi-core/src/team.rs` — publish `ConsultStarted`/`ConsultEnded` in `consult`, `TeamFormed`/`TeamDisbanded` in `consult_many`.

---

## Task 1: Activity module (types + pure state machine + global hub)

**Files:**
- Create: `crates/hakimi-common/src/activity.rs`
- Modify: `crates/hakimi-common/Cargo.toml`, `crates/hakimi-common/src/lib.rs`

- [ ] **Step 1: Add the tokio dependency.** In `crates/hakimi-common/Cargo.toml` under `[dependencies]` (after `anyhow`):

```toml
tokio = { workspace = true }
```

- [ ] **Step 2: Create `activity.rs` with types, the pure state machine, the global hub, and tests.**

```rust
//! Process-global persona activity hub for the WebUI office dashboard.
//!
//! A broadcast bus carries [`ActivityEvent`]s; a `Mutex<HashMap>` keeps the latest
//! per-persona overlay state so a freshly-connected client can be seeded. Mirrors
//! the global-singleton pattern of `MESSAGE_QUEUE` in hakimi-tools. The state
//! machine is the pure `apply` + `displayed_state` pair so it is unit-testable
//! without touching the global.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const ACTIVITY_CHANNEL_CAPACITY: usize = 512;

/// Displayed work state of a persona (priority: in_team > consulting > working > idle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonaState {
    Idle,
    Working,
    Consulting,
    InTeam,
}

/// A real-time persona activity event, published by activity sources and streamed
/// to dashboard clients over SSE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActivityEvent {
    PersonaCreated { id: String, name: String, avatar: String },
    PersonaUpdated { id: String, name: String, avatar: String },
    PersonaDeleted { id: String },
    TurnStarted { persona_id: String, task_hint: Option<String>, model: Option<String> },
    TurnEnded { persona_id: String },
    ConsultStarted { from_id: String, to_id: String, task_hint: Option<String> },
    ConsultEnded { from_id: String, to_id: String },
    TeamFormed { team_id: String, lead_id: String, member_ids: Vec<String>, task_hint: Option<String> },
    TeamDisbanded { team_id: String },
}

/// Internal per-persona tracking: base (working) + overlays (consulting/team).
#[derive(Debug, Clone, Default)]
pub(crate) struct HubEntry {
    working: bool,
    consulting_to: Option<String>,
    team_id: Option<String>,
    task_hint: Option<String>,
    model: Option<String>,
}

/// Public, cloneable live state for one persona (overlay only; name/avatar come
/// from the registry at the snapshot join).
#[derive(Debug, Clone)]
pub struct LiveState {
    pub state: PersonaState,
    pub task_hint: Option<String>,
    pub model: Option<String>,
    pub team_id: Option<String>,
}

/// One persona's full activity row returned by `GET /api/activity/snapshot`.
#[derive(Debug, Clone, Serialize)]
pub struct PersonaActivity {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub state: PersonaState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
}

impl PersonaActivity {
    /// Build a row from registry identity + optional live overlay (defaults to idle).
    pub fn from_parts(id: &str, name: &str, avatar: &str, live: Option<&LiveState>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            avatar: avatar.to_string(),
            state: live.map(|l| l.state).unwrap_or(PersonaState::Idle),
            task_hint: live.and_then(|l| l.task_hint.clone()),
            model: live.and_then(|l| l.model.clone()),
            team_id: live.and_then(|l| l.team_id.clone()),
        }
    }
}

/// Compute the displayed state from an entry (overlay priority).
pub(crate) fn displayed_state(entry: &HubEntry) -> PersonaState {
    if entry.team_id.is_some() {
        PersonaState::InTeam
    } else if entry.consulting_to.is_some() {
        PersonaState::Consulting
    } else if entry.working {
        PersonaState::Working
    } else {
        PersonaState::Idle
    }
}

/// Apply an event to the state map. Pure (no global, no I/O). Unknown personas are
/// lazily inserted so out-of-order events never panic.
pub(crate) fn apply(map: &mut HashMap<String, HubEntry>, event: &ActivityEvent) {
    match event {
        ActivityEvent::PersonaCreated { id, .. } | ActivityEvent::PersonaUpdated { id, .. } => {
            map.entry(id.clone()).or_default();
        }
        ActivityEvent::PersonaDeleted { id } => {
            map.remove(id);
        }
        ActivityEvent::TurnStarted { persona_id, task_hint, model } => {
            let e = map.entry(persona_id.clone()).or_default();
            e.working = true;
            e.task_hint = task_hint.clone();
            e.model = model.clone();
        }
        ActivityEvent::TurnEnded { persona_id } => {
            let e = map.entry(persona_id.clone()).or_default();
            e.working = false;
            e.task_hint = None;
            e.model = None;
        }
        ActivityEvent::ConsultStarted { from_id, to_id, .. } => {
            map.entry(from_id.clone()).or_default().consulting_to = Some(to_id.clone());
        }
        ActivityEvent::ConsultEnded { from_id, .. } => {
            map.entry(from_id.clone()).or_default().consulting_to = None;
        }
        ActivityEvent::TeamFormed { team_id, lead_id, member_ids, .. } => {
            for id in std::iter::once(lead_id).chain(member_ids.iter()) {
                map.entry(id.clone()).or_default().team_id = Some(team_id.clone());
            }
        }
        ActivityEvent::TeamDisbanded { team_id } => {
            for entry in map.values_mut() {
                if entry.team_id.as_deref() == Some(team_id.as_str()) {
                    entry.team_id = None;
                }
            }
        }
    }
}

struct ActivityHub {
    sender: broadcast::Sender<ActivityEvent>,
    state: Mutex<HashMap<String, HubEntry>>,
}

static HUB: LazyLock<ActivityHub> = LazyLock::new(|| {
    let (sender, _rx) = broadcast::channel(ACTIVITY_CHANNEL_CAPACITY);
    ActivityHub { sender, state: Mutex::new(HashMap::new()) }
});

/// Publish an event: update the snapshot, then broadcast to subscribers.
pub fn publish(event: ActivityEvent) {
    {
        let mut map = HUB.state.lock().unwrap_or_else(|e| e.into_inner());
        apply(&mut map, &event);
    }
    let _ = HUB.sender.send(event); // ignore "no subscribers"
}

/// Subscribe to the live event stream.
pub fn subscribe() -> broadcast::Receiver<ActivityEvent> {
    HUB.sender.subscribe()
}

/// Snapshot the current live overlay state for every tracked persona.
pub fn all_live_states() -> HashMap<String, LiveState> {
    let map = HUB.state.lock().unwrap_or_else(|e| e.into_inner());
    map.iter()
        .map(|(id, entry)| {
            (
                id.clone(),
                LiveState {
                    state: displayed_state(entry),
                    task_hint: entry.task_hint.clone(),
                    model: entry.model.clone(),
                    team_id: entry.team_id.clone(),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev_turn_start(id: &str) -> ActivityEvent {
        ActivityEvent::TurnStarted { persona_id: id.into(), task_hint: Some("fix bug".into()), model: Some("opus".into()) }
    }

    #[test]
    fn turn_events_toggle_working() {
        let mut m = HashMap::new();
        apply(&mut m, &ev_turn_start("coder"));
        assert_eq!(displayed_state(&m["coder"]), PersonaState::Working);
        assert_eq!(m["coder"].task_hint.as_deref(), Some("fix bug"));
        apply(&mut m, &ActivityEvent::TurnEnded { persona_id: "coder".into() });
        assert_eq!(displayed_state(&m["coder"]), PersonaState::Idle);
        assert!(m["coder"].task_hint.is_none());
    }

    #[test]
    fn consult_overlays_working_then_restores() {
        let mut m = HashMap::new();
        apply(&mut m, &ev_turn_start("coder")); // working
        apply(&mut m, &ActivityEvent::ConsultStarted { from_id: "coder".into(), to_id: "writer".into(), task_hint: None });
        assert_eq!(displayed_state(&m["coder"]), PersonaState::Consulting);
        apply(&mut m, &ActivityEvent::ConsultEnded { from_id: "coder".into(), to_id: "writer".into() });
        // base turn still active -> back to working, NOT idle
        assert_eq!(displayed_state(&m["coder"]), PersonaState::Working);
    }

    #[test]
    fn team_masks_other_states_until_disbanded() {
        let mut m = HashMap::new();
        apply(&mut m, &ev_turn_start("coder"));
        apply(&mut m, &ActivityEvent::TeamFormed {
            team_id: "t1".into(), lead_id: "coder".into(),
            member_ids: vec!["writer".into(), "reviewer".into()], task_hint: None,
        });
        assert_eq!(displayed_state(&m["coder"]), PersonaState::InTeam);
        assert_eq!(displayed_state(&m["writer"]), PersonaState::InTeam);
        assert_eq!(displayed_state(&m["reviewer"]), PersonaState::InTeam);
        apply(&mut m, &ActivityEvent::TeamDisbanded { team_id: "t1".into() });
        assert_eq!(displayed_state(&m["coder"]), PersonaState::Working); // base restored
        assert_eq!(displayed_state(&m["writer"]), PersonaState::Idle);
    }

    #[test]
    fn delete_removes_entry() {
        let mut m = HashMap::new();
        apply(&mut m, &ActivityEvent::PersonaCreated { id: "x".into(), name: "X".into(), avatar: "🙂".into() });
        assert!(m.contains_key("x"));
        apply(&mut m, &ActivityEvent::PersonaDeleted { id: "x".into() });
        assert!(!m.contains_key("x"));
    }

    #[tokio::test]
    async fn publish_reaches_subscriber_and_updates_snapshot() {
        let mut rx = subscribe();
        publish(ActivityEvent::TurnStarted { persona_id: "globaltest_p".into(), task_hint: None, model: None });
        let got = rx.recv().await.unwrap();
        assert_eq!(got, ActivityEvent::TurnStarted { persona_id: "globaltest_p".into(), task_hint: None, model: None });
        let states = all_live_states();
        assert_eq!(states.get("globaltest_p").map(|l| l.state), Some(PersonaState::Working));
        // cleanup global state for other tests
        publish(ActivityEvent::PersonaDeleted { id: "globaltest_p".into() });
    }

    #[test]
    fn persona_activity_from_parts_defaults_idle() {
        let pa = PersonaActivity::from_parts("c", "Coder", "🤖", None);
        assert_eq!(pa.state, PersonaState::Idle);
        assert_eq!(pa.name, "Coder");
    }
}
```

- [ ] **Step 3: Export the module.** In `crates/hakimi-common/src/lib.rs` add `mod activity;` (after `mod account_usage;`, keeping alphabetical-ish order is fine) and `pub use activity::*;` (after `pub use account_usage::*;`).

- [ ] **Step 4: Run the tests.**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-common activity`
Expected: PASS (6 tests). If `tokio` isn't resolvable, confirm the workspace `tokio` dep exists in the root `Cargo.toml [workspace.dependencies]` (it does — used across the workspace).

- [ ] **Step 5: Commit.**

```bash
git add crates/hakimi-common/Cargo.toml crates/hakimi-common/src/activity.rs crates/hakimi-common/src/lib.rs
git commit -m "feat(activity): persona activity hub (events, state machine, broadcast)"
```

---

## Task 2: Server endpoints — snapshot + SSE stream

**Files:**
- Modify: `crates/hakimi-server/src/api.rs` (handlers near the other dashboard handlers; routes in `build_router` ~line 2025, in the agents group)

- [ ] **Step 1: Add the two handlers.** Add near `dashboard_status` (the SSE imports `sse::{Event, KeepAlive, Sse}` and `std::convert::Infallible` are already present; `futures` is already used):

```rust
/// Response body for GET /api/activity/snapshot.
#[derive(Debug, Serialize)]
struct ActivitySnapshotResponse {
    personas: Vec<hakimi_common::PersonaActivity>,
}

/// GET /api/activity/snapshot — current activity row per registered persona
/// (registry identity joined with the live activity overlay).
async fn activity_snapshot(State(state): State<AppState>) -> Json<ActivitySnapshotResponse> {
    let states = hakimi_common::all_live_states();
    let reg = state.persona_registry.read().await;
    let personas = reg
        .list()
        .into_iter()
        .map(|cfg| {
            hakimi_common::PersonaActivity::from_parts(&cfg.id, &cfg.name, &cfg.avatar, states.get(&cfg.id))
        })
        .collect();
    Json(ActivitySnapshotResponse { personas })
}

/// GET /api/activity/stream — SSE of live ActivityEvents.
async fn activity_stream() -> Response {
    use tokio::sync::broadcast::error::RecvError;
    let rx = hakimi_common::subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
                    return Some((
                        Ok::<Event, Infallible>(Event::default().event("activity").data(data)),
                        rx,
                    ));
                }
                Err(RecvError::Lagged(_)) => continue, // slow consumer dropped events; resync on reconnect
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}
```

- [ ] **Step 2: Register the routes.** In `build_router`, after the agent routes block (after `.route("/agents/{id}/sessions", ...)` / the last agent route, ~line 2034), add:

```rust
        .route("/activity/snapshot", get(activity_snapshot))
        .route("/activity/stream", get(activity_stream))
```

- [ ] **Step 3: Write a smoke test.** Add to the `tests` module in `api.rs` (mirrors the router-based tests):

```rust
    #[tokio::test]
    async fn test_activity_snapshot_includes_personas_as_idle() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/activity/snapshot")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        let arr = json["personas"].as_array().unwrap();
        // default persona present, idle by default
        let def = arr.iter().find(|p| p["id"] == "default").unwrap();
        assert_eq!(def["state"], "idle");
    }
```

- [ ] **Step 4: Run the test.**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-server test_activity_snapshot_includes_personas_as_idle`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/hakimi-server/src/api.rs
git commit -m "feat(activity): /api/activity snapshot + SSE stream endpoints"
```

---

## Task 3: Publish persona lifecycle events (CRUD)

**Files:**
- Modify: `crates/hakimi-server/src/api.rs` (`create_agent`, `update_agent`, `delete_agent`)

- [ ] **Step 1: Publish in `create_agent`.** After `sync_gateway_persona_agent(&state, &created, &skills_dir).await;` (just before `Ok(Json(created))`):

```rust
    hakimi_common::publish(hakimi_common::ActivityEvent::PersonaCreated {
        id: created.id.clone(),
        name: created.name.clone(),
        avatar: created.avatar.clone(),
    });
```

- [ ] **Step 2: Publish in `update_agent`.** After `sync_gateway_persona_agent(&state, &updated, &skills_dir).await;` (before `Ok(Json(updated))`):

```rust
    hakimi_common::publish(hakimi_common::ActivityEvent::PersonaUpdated {
        id: updated.id.clone(),
        name: updated.name.clone(),
        avatar: updated.avatar.clone(),
    });
```

- [ ] **Step 3: Publish in `delete_agent`.** Find where delete succeeds (after the registry delete + gateway map removal, before returning `AgentDeleteResponse`). Add:

```rust
    hakimi_common::publish(hakimi_common::ActivityEvent::PersonaDeleted { id: id.clone() });
```

(Read `delete_agent` first to place this after the successful delete and confirm `id` is the path param in scope.)

- [ ] **Step 4: Write a test.** Add to the `tests` module:

```rust
    #[tokio::test]
    async fn test_create_agent_publishes_activity_event() {
        let mut rx = hakimi_common::subscribe();
        let app = build_router(test_state());
        let resp = app
            .oneshot(json_post("/api/agents", json!({"id": "evt_coder", "name": "Coder"})))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        // drain until we see our PersonaCreated (other tests may share the global bus)
        let mut found = false;
        for _ in 0..50 {
            match rx.try_recv() {
                Ok(hakimi_common::ActivityEvent::PersonaCreated { id, .. }) if id == "evt_coder" => { found = true; break; }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        assert!(found, "expected PersonaCreated for evt_coder");
    }
```

- [ ] **Step 5: Run the test.**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-server test_create_agent_publishes_activity_event`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add crates/hakimi-server/src/api.rs
git commit -m "feat(activity): publish persona created/updated/deleted from CRUD"
```

---

## Task 4: Publish turn events from WebUI streaming chat

**Files:**
- Modify: `crates/hakimi-server/src/api.rs` (`agent_chat_stream`, ~line 3800)

`agent_chat_stream` spawns a task that runs the agent and feeds tokens to an mpsc, then returns `sse_response_from_rx(rx)`. Publish `TurnStarted` just before the agent run begins and `TurnEnded` when it finishes.

- [ ] **Step 1: Read `agent_chat_stream` fully** (from ~3800 to where it spawns the run task and returns `sse_response_from_rx(rx)`), to locate the spawned run closure and where the run completes.

- [ ] **Step 2: Publish `TurnStarted` before the run and `TurnEnded` after.** Inside the spawned task, immediately before the agent runs the conversation, add:

```rust
            hakimi_common::publish(hakimi_common::ActivityEvent::TurnStarted {
                persona_id: id.clone(),
                task_hint: None,
                model: Some(cloned_agent.model().to_string()),
            });
```

and after the run completes (all exit paths of the spawned task — success or error), add:

```rust
            hakimi_common::publish(hakimi_common::ActivityEvent::TurnEnded { persona_id: id.clone() });
```

Place `TurnEnded` so it runs on every exit (e.g. immediately after the run future resolves, before sending `__DONE__`/`__ERROR__`). Ensure `id` (the persona path param) is `clone`d into the spawned task; if it isn't already captured, add `let id = id.clone();` in the pre-spawn capture block alongside the other clones.

- [ ] **Step 3: Build to verify it compiles.**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" build -p hakimi-server`
Expected: compiles. (The existing `test_agent_chat_stream_default_emits_sse` test still passes; run it: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-server test_agent_chat_stream_default_emits_sse`.)

- [ ] **Step 4: Commit.**

```bash
git add crates/hakimi-server/src/api.rs
git commit -m "feat(activity): publish turn start/end from agent_chat_stream"
```

---

## Task 5: Publish turn events from the gateway loop

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs` (the per-message turn in `process_gateway_messages_loop`, around the `tokio::select!` at ~line 6760)

The turn runs in `let result = tokio::select! { ... run_conversation* ... };` at ~6760-6771. `persona_id` is in scope.

- [ ] **Step 1: Publish `TurnStarted` immediately before the `tokio::select!`** (before line 6760):

```rust
                hakimi_common::publish(hakimi_common::ActivityEvent::TurnStarted {
                    persona_id: persona_id.clone(),
                    task_hint: None,
                    model: Some(turn_agent.model().to_string()),
                });
```

- [ ] **Step 2: Publish `TurnEnded` immediately after the select resolves** (right after the `};` that closes the `tokio::select!`, ~line 6771, before `turn_agent.set_streaming_callback(None);`):

```rust
                hakimi_common::publish(hakimi_common::ActivityEvent::TurnEnded {
                    persona_id: persona_id.clone(),
                });
```

- [ ] **Step 3: Build + clippy.**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" build -p hakimi-cli`
Then: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" clippy -p hakimi-cli --all-targets --all-features`
Expected: compiles, no warnings. (`turn_agent.model()` returns `&str` — confirm the `model()` getter exists on `AIAgent`; it does.)

- [ ] **Step 4: Commit.**

```bash
git add crates/hakimi-cli/src/entry.rs
git commit -m "feat(activity): publish turn start/end from the gateway loop"
```

---

## Task 6: Publish consult + team events from the team executor

**Files:**
- Modify: `crates/hakimi-core/src/team.rs` (`consult` and `consult_many`)
- Test: `crates/hakimi-core/tests/integration.rs` (extend the existing team integration test path)

The executor's `self.lineage.last()` is the persona doing the consulting (the lead, or a nested teammate). `consult` wraps a single teammate run; `consult_many` wraps a parallel fan-out.

- [ ] **Step 1: Publish consult events in `consult`.** In `team.rs`, inside `consult`, after the teammate config is validated and `task_id`/`title` are computed (right after the existing `emit_team_progress(&call.progress, &task_id, &title, "...已加入协作")` line), add:

```rust
        let from_id = self.lineage.last().cloned().unwrap_or_default();
        hakimi_common::publish(hakimi_common::ActivityEvent::ConsultStarted {
            from_id: from_id.clone(),
            to_id: cfg.id.clone(),
            task_hint: Some(truncate_for_title(&call.task, 48)),
        });
```

Then publish `ConsultEnded` at BOTH exit points of the retry loop (the `Ok(res) =>` success arm right before `return Ok(res.final_response);`, and the `Err(e) if attempt >= MAX_CONSULT_ATTEMPTS =>` arm right before its `return Err(...)`):

```rust
                    hakimi_common::publish(hakimi_common::ActivityEvent::ConsultEnded {
                        from_id: from_id.clone(),
                        to_id: cfg.id.clone(),
                    });
```

- [ ] **Step 2: Publish team events in `consult_many`.** Wrap the fan-out so it emits `TeamFormed` before and `TeamDisbanded` after when there is more than one teammate. Replace the body of `consult_many` with:

```rust
    async fn consult_many(&self, calls: Vec<TeamCallContext>) -> Result<Vec<String>> {
        let lead_id = self.lineage.last().cloned().unwrap_or_default();
        let member_ids: Vec<String> = calls.iter().map(|c| c.teammate_id.clone()).collect();
        let team_id = format!("team_{}", uuid::Uuid::new_v4().simple());
        let is_team = member_ids.len() > 1;
        if is_team {
            hakimi_common::publish(hakimi_common::ActivityEvent::TeamFormed {
                team_id: team_id.clone(),
                lead_id: lead_id.clone(),
                member_ids: member_ids.clone(),
                task_hint: calls.first().map(|c| truncate_for_title(&c.task, 48)),
            });
        }
        // Concurrent (not spawned): each future awaits its own semaphore permit.
        let futures = calls.into_iter().map(|call| {
            let id = call.teammate_id.clone();
            async move {
                match self.consult(call).await {
                    Ok(answer) => answer,
                    Err(e) => format!("Teammate {id} failed: {e}"),
                }
            }
        });
        let results = futures::future::join_all(futures).await;
        if is_team {
            hakimi_common::publish(hakimi_common::ActivityEvent::TeamDisbanded { team_id });
        }
        Ok(results)
    }
```

(This keeps the existing `join_all` design and per-item `consult` calls; `TeamFormed` sets `in_team` which masks the per-member consult overlays in the displayed state — matching the spec's "sit together, no jogging" for teams.)

- [ ] **Step 3: Confirm `hakimi-core` depends on `hakimi-common`** (it does — `team.rs` already uses `hakimi_common::{HakimiError, Result, ...}`), so `hakimi_common::publish` / `ActivityEvent` resolve without a new dep.

- [ ] **Step 4: Add an integration test.** Append to `crates/hakimi-core/tests/integration.rs` (reuses `MockTransport`, mirrors `team_executor_consults_addressable_teammate`):

```rust
#[tokio::test]
async fn team_consult_publishes_activity_events() {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let mut rx = hakimi_common::subscribe();

    let agents_dir = std::env::temp_dir()
        .join(format!("hakimi-team-act-{}", uuid::Uuid::new_v4()))
        .join("agents");
    let mut reg = hakimi_core::PersonaRegistry::load(&agents_dir).unwrap();
    let mut writer = hakimi_core::PersonaConfig::new("writer");
    writer.addressable = true;
    reg.create(writer).unwrap();
    let registry = Arc::new(RwLock::new(reg));

    let transport = Arc::new(MockTransport::text_response("Status: success"));
    let template = Arc::new(hakimi_core::AIAgent::new(
        "test-model",
        transport,
        hakimi_tools::ToolRegistry::new(),
        None,
    ));
    let exec = hakimi_core::PersonaTeamExecutor::new(registry, template, 128_000).for_lead("lead");
    let _ = hakimi_common::TeamExecutor::consult(
        &exec,
        hakimi_common::TeamCallContext {
            teammate_id: "writer".to_string(),
            task: "draft".to_string(),
            context: String::new(),
            progress: None,
        },
    )
    .await
    .unwrap();

    let mut saw_started = false;
    let mut saw_ended = false;
    for _ in 0..200 {
        match rx.try_recv() {
            Ok(hakimi_common::ActivityEvent::ConsultStarted { from_id, to_id, .. })
                if from_id == "lead" && to_id == "writer" => saw_started = true,
            Ok(hakimi_common::ActivityEvent::ConsultEnded { from_id, to_id })
                if from_id == "lead" && to_id == "writer" => saw_ended = true,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    assert!(saw_started, "expected ConsultStarted");
    assert!(saw_ended, "expected ConsultEnded");
}
```

- [ ] **Step 5: Run the team tests.**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-core team_consult_publishes_activity_events`
Then the existing team suite: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-core team`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add crates/hakimi-core/src/team.rs crates/hakimi-core/tests/integration.rs
git commit -m "feat(activity): publish consult + team events from team executor"
```

---

## Final Verification

- [ ] **Local clippy (catches stable lints + compile across all crates):**

```
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" clippy --workspace --all-targets --all-features
```
Expected: clean. Fix any lints (hand-apply CI fmt diffs if CI fmt later complains; see toolchain memory).

- [ ] **Manual smoke (optional):** start the unified server, `curl -N http://localhost:<port>/api/activity/stream` with the bearer token, then trigger a persona chat / team consult and watch events stream.

---

## Self-Review (completed during planning)

**Spec coverage:** §3 ActivityHub → Task 1; §4 event model + base/overlay state machine → Task 1 (`apply`/`displayed_state` + tests for consult-restores-working and team-masks); §5.1 hub in hakimi-common → Task 1; §5.2 publish points → Tasks 3 (CRUD), 4 (chat), 5 (gateway), 6 (team); §5.3 endpoints → Task 2; §8 lazy-insert unknown persona → `apply` uses `entry().or_default()`; snapshot joins roster so the office renders before events → Task 2 handler.

**Type consistency:** `ActivityEvent` variants/fields, `PersonaState` (snake_case serde), `LiveState`, `PersonaActivity::from_parts`, `publish`/`subscribe`/`all_live_states` are defined in Task 1 and used identically in Tasks 2-6. `truncate_for_title` already exists in `team.rs` (reused in Task 6). `AIAgent::model()` getter exists (used in Tasks 4-5).

**Placeholders:** Tasks 3-step3, 4, 5 ask the implementer to read a region to confirm exact insertion lines/variable capture in large functions — these are placement confirmations with the exact code to insert provided, not unimplemented logic. No TODO/TBD.

**Scope:** Backend only; the office UI is a separate plan that consumes `/api/activity/*`. Deferred items (zoom, drag, history) are out of scope per spec §10.
