# P6 Follow-ups: ship WebUI + gateway hot-add + persona sub-resources

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`).

Three follow-ups after P1-P5, done in order, each verified + committed separately.

- **Item 1 — DONE (`c19066d`):** Vite builds the React app into `crates/hakimi-webui/static/` (fixed names, `/static/` base); committed bundle embedded by `api.rs`. Binary now serves React Layout A. Vanilla UI removed; its `workspace.js` file browser is a parity gap to port later.

---

## Item 2: gateway persona-agent hot-add

**Goal:** New/updated/deleted personas via `/api/agents*` immediately affect gateway routing in unified mode — no restart.

**Approach:** Make the gateway's pre-built persona-agent map a shared `Arc<RwLock<HashMap<String, Arc<Mutex<AIAgent>>>>>` stored in `AppState`. The gateway loop reads it per message; the API CRUD handlers rebuild/insert/remove entries from the same Arc (shared with the loop in unified mode). The default persona (`DEFAULT_PERSONA_ID`) is never in the map (it uses the legacy `agent_arc`).

**Files:**
- `crates/hakimi-server/src/server.rs`: add `pub type GatewayPersonaAgents = Arc<tokio::sync::RwLock<HashMap<String, Arc<Mutex<hakimi_core::AIAgent>>>>>;` + `AppState.persona_agents: GatewayPersonaAgents`; construct empty in `Server::new`.
- `crates/hakimi-server/src/api.rs`: helper `build_persona_agent_for(state, cfg, skills_dir)`; have create/update/delete maintain `state.persona_agents`; refactor `agent_chat` non-default branch to use the helper; `test_state` empty map; a sync test.
- `crates/hakimi-cli/src/entry.rs`: loop param + both callers wrap the built map in `Arc::new(RwLock::new(..))`; unified mode shares the same Arc into `AppState`; per-turn `persona_agents.read().await.get(..).cloned()`.

- [ ] **S1** server.rs: type alias + AppState field + Server::new empty map.
- [ ] **S2** entry.rs: loop param type → shared RwLock; per-turn read; wrap in both callers; unified shares Arc into AppState.
- [ ] **S3** api.rs: `build_persona_agent_for` helper; create inserts (non-default); update replaces (non-default); delete removes; refactor `agent_chat`; `test_state` field.
- [ ] **S4** api.rs test: create → map contains id; delete → map drops id.
- [ ] **S5** Docker: `fmt` + `clippy -p hakimi-server --all-targets` + `clippy -p hakimi-cli --all-targets` + `test -p hakimi-server agent`.
- [ ] **S6** Commit `feat: gateway persona-agent 热生效(CRUD 同步 AppState.persona_agents)`.

**Default/edge rules:** only `id != DEFAULT_PERSONA_ID` entries are built/inserted. Build happens after dropping the registry write lock (tokio RwLock is not reentrant) — capture `agents_dir`/cfg under the lock, release, then build+insert.

---

## Item 3: persona-scoped sub-resources + streaming chat

**Goal:** Per-persona `GET /api/agents/{id}/sessions|memory|skills` and `POST /api/agents/{id}/chat/stream`; wire the WebUI to use them.

**Scope decision (to confirm during execution):** sessions/cron are per-persona on disk per spec §7, but the server currently uses one shared `session_db`. Full per-persona sessions.db wiring is large. For Item 3, implement the read-only sub-resources against per-persona on-disk data where it exists and is cheap:
- `GET /api/agents/{id}/skills`: list skills from `agents/{id}/skills` (+ enabled flags from the persona's `enabled_skills`). For the default persona, fall back to the instance skills dir.
- `GET /api/agents/{id}/memory`: read `agents/{id}/memory` (MEMORY.md / files) summary; default persona → root memory dir.
- `GET /api/agents/{id}/sessions`: per-persona `agents/{id}/sessions.db` if present, else empty list (documented; shared session_db stays for the default/legacy chat).
- `POST /api/agents/{id}/chat/stream`: SSE mirroring `/chat/stream` but building the persona agent (default → template clone).
- WebUI: add `api.agentSkills/agentMemory/agentSessions/agentChatStream`; surface persona skills in the config form (replace the instance-wide skill list) and show persona sessions in the chat view.

- [ ] Plan the exact endpoint shapes + tests, then implement incrementally with Docker + npm verification, commit.

(Item 3 is re-planned in detail at execution time once Item 2 lands, since its surface is larger and benefits from the Item 2 shape.)
