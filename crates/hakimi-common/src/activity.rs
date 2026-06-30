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
    /// `to_id` is carried for client-side correlation with the matching
    /// `ConsultStarted`; the consulting overlay is keyed only on `from_id`, so
    /// `apply` intentionally does not read `to_id`.
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
            // Identity (name/avatar) lives in the registry; the hub only needs the
            // entry to exist so its live state can be tracked.
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
