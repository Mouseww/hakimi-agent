# P4 Agent-Dimension REST API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add agent-dimension REST endpoints (`/api/agents` CRUD, `/api/agents/{id}/chat`, `/api/bindings`) backed by `AppState.persona_registry`, so the WebUI can manage personas while existing endpoints keep pointing at the default persona.

**Architecture:** New Axum handlers in `crates/hakimi-server/src/api.rs` operate on the shared `Arc<RwLock<PersonaRegistry>>`. CRUD mutations go through `PersonaRegistry::create/update/delete` (which persist `persona.yaml`/`registry.yaml` and rebuild the binding index, so gateway binding resolution updates live). `/api/agents/{id}/chat` builds a persona agent on demand from the template `AppState.agent` (default persona reuses the template directly, mirroring P3's legacy split), matching the non-streaming `/api/chat` flow. A small `PersonaRegistry::agents_dir()` accessor lets the chat handler locate per-persona skills.

**Tech Stack:** Rust, Axum 0.8, `hakimi-core` (`PersonaRegistry`, `PersonaConfig`, `build_persona_agent`, `DEFAULT_PERSONA_ID`), `hakimi-server`. Cargo via Docker: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" <args>`. CI is the gate (`fmt --check` + `clippy --workspace --all-targets --all-features` + `test`).

---

## Design decisions (locked)

- **Endpoints (handoff P4 scope, spec §3.6):** `GET/POST /api/agents`, `GET/PATCH/DELETE /api/agents/{id}`, `POST /api/agents/{id}/chat`, `GET /api/bindings`. Channel bind/unbind is done through `PATCH /api/agents/{id}` (setting `bindings`), so a separate `POST /api/agents/{id}/bindings` is not added in P4.
- **Backward compatibility:** existing `/api/chat`, `/api/sessions`, `/v1/*` are untouched and keep using `AppState.agent` (the default persona/template). New endpoints are additive.
- **Default persona chat parity:** `POST /api/agents/{id}/chat` where `id == DEFAULT_PERSONA_ID` clones `AppState.agent` directly (full default behavior incl. the real instance skill store), mirroring P3. Named personas build via `build_persona_agent` (own model/prompt/context/skills from `agents/<id>/skills`).
- **Non-streaming chat only:** matches the existing non-streaming `/api/chat` (build agent, `.chat()`, return `{response, session_id}`, no DB persistence). Streaming (`/chat/stream`-style) is a follow-up.
- **Live registry, deferred gateway agent hot-add:** CRUD updates the in-memory registry + disk, so the gateway's binding resolution (shared `Arc<RwLock<PersonaRegistry>>` in unified mode) is live. The gateway's pre-built `persona_agents` instance map is built at startup and is NOT updated here, so a brand-new persona routes on the gateway via P3 legacy fallback until restart. WebUI chat (`/api/agents/{id}/chat`) builds on demand and works immediately. This limitation is documented (handoff P4 note).
- **Concurrency:** read handlers take `persona_registry.read().await`; mutators take a single `persona_registry.write().await` for the read-modify-persist cycle.

## File structure

- Modify: `crates/hakimi-core/src/persona_registry.rs` — add `pub fn agents_dir(&self) -> &Path` + test.
- Modify: `crates/hakimi-server/src/api.rs`
  - Add request/response types: `AgentsListResponse`, `AgentUpdateRequest`, `AgentDeleteResponse`, `BindingsResponse` (module-private).
  - Add 7 handlers: `list_agents`, `create_agent`, `get_agent`, `update_agent`, `delete_agent`, `agent_chat`, `list_bindings`.
  - Register 7 routes in `build_router` (in the authenticated `api_routes` chain).
  - Add endpoint tests in `mod tests`.

---

## Task 1: `PersonaRegistry::agents_dir()` accessor (TDD)

**Files:**
- Modify: `crates/hakimi-core/src/persona_registry.rs`

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)] mod tests` block of `persona_registry.rs`, add:

```rust
    #[test]
    fn agents_dir_exposes_backing_path() {
        let dir = temp_dir();
        let reg = PersonaRegistry::load(dir.clone()).unwrap();
        assert_eq!(reg.agents_dir(), dir.as_path());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-core agents_dir_exposes_backing_path`
Expected: FAIL to compile ("no method named `agents_dir`").

- [ ] **Step 3: Write minimal implementation**

In `impl PersonaRegistry`, just after the `default_persona` method (before `resolve_for_channel`), add:

```rust
    /// Root directory backing this registry (`<home>/agents`).
    pub fn agents_dir(&self) -> &std::path::Path {
        &self.agents_dir
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-core agents_dir_exposes_backing_path`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/hakimi-core/src/persona_registry.rs
git commit -m "feat(core): PersonaRegistry::agents_dir 访问器(P4-1)"
```

---

## Task 2: Agent-dimension handlers, routes, and types

All in `crates/hakimi-server/src/api.rs`. Routes reference the handlers, so types + handlers + routes land in one compiling commit. Tests are added in the same commit.

### 2a. Request/response types

- [ ] **Step 1: Add the new types**

Add near the other request/response structs (e.g. just after `ChatResponse` at api.rs:246). `PersonaConfig` is `hakimi_core::PersonaConfig` (already `Serialize`/`Deserialize`/`Clone`):

```rust
/// Response body for GET /api/agents.
#[derive(Debug, Serialize)]
struct AgentsListResponse {
    agents: Vec<hakimi_core::PersonaConfig>,
    default: String,
}

/// Request body for PATCH /api/agents/{id}. Every field is optional; only
/// provided fields are applied. An empty-string `reasoning_effort` clears it.
#[derive(Debug, Default, Deserialize)]
struct AgentUpdateRequest {
    name: Option<String>,
    avatar: Option<String>,
    description: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    system_prompt: Option<String>,
    enabled_skills: Option<Vec<String>>,
    bindings: Option<Vec<String>>,
    is_default: Option<bool>,
}

/// Response body for DELETE /api/agents/{id}.
#[derive(Debug, Serialize)]
struct AgentDeleteResponse {
    id: String,
    deleted: bool,
}

/// Response body for GET /api/bindings (`platform:bot_id` -> persona id).
#[derive(Debug, Serialize)]
struct BindingsResponse {
    bindings: std::collections::BTreeMap<String, String>,
    default: String,
}
```

### 2b. List + create handlers

- [ ] **Step 2: Add `list_agents` and `create_agent`**

Add to the handlers section (e.g. after the `chat` handler, api.rs:3273):

```rust
/// GET /api/agents — list personas with the default persona id.
async fn list_agents(State(state): State<AppState>) -> Json<AgentsListResponse> {
    let reg = state.persona_registry.read().await;
    let agents = reg.list().into_iter().cloned().collect();
    Json(AgentsListResponse {
        agents,
        default: reg.default_id().to_string(),
    })
}

/// POST /api/agents — create a persona from the posted config.
async fn create_agent(
    State(state): State<AppState>,
    Json(cfg): Json<hakimi_core::PersonaConfig>,
) -> Result<Json<hakimi_core::PersonaConfig>, (StatusCode, Json<ErrorResponse>)> {
    let id = cfg.id.clone();
    let mut reg = state.persona_registry.write().await;
    reg.create(cfg).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e.to_string() }),
        )
    })?;
    let created = reg.get(&id).cloned().expect("persona present after create");
    Ok(Json(created))
}
```

### 2c. Get + update + delete handlers

- [ ] **Step 3: Add `get_agent`, `update_agent`, `delete_agent`**

```rust
/// GET /api/agents/{id} — fetch a persona config.
async fn get_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<hakimi_core::PersonaConfig>, (StatusCode, Json<ErrorResponse>)> {
    let reg = state.persona_registry.read().await;
    match reg.get(&id) {
        Some(cfg) => Ok(Json(cfg.clone())),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("persona '{id}' not found"),
            }),
        )),
    }
}

/// PATCH /api/agents/{id} — merge provided fields and persist.
async fn update_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AgentUpdateRequest>,
) -> Result<Json<hakimi_core::PersonaConfig>, (StatusCode, Json<ErrorResponse>)> {
    let mut reg = state.persona_registry.write().await;
    let mut cfg = match reg.get(&id) {
        Some(cfg) => cfg.clone(),
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("persona '{id}' not found"),
                }),
            ));
        }
    };

    if let Some(name) = req.name {
        cfg.name = name;
    }
    if let Some(avatar) = req.avatar {
        cfg.avatar = avatar;
    }
    if let Some(description) = req.description {
        cfg.description = description;
    }
    if let Some(model) = req.model {
        cfg.model = model;
    }
    if let Some(effort) = req.reasoning_effort {
        cfg.reasoning_effort = if effort.trim().is_empty() {
            None
        } else {
            Some(effort)
        };
    }
    if let Some(system_prompt) = req.system_prompt {
        cfg.system_prompt = system_prompt;
    }
    if let Some(enabled_skills) = req.enabled_skills {
        cfg.enabled_skills = enabled_skills;
    }
    if let Some(bindings) = req.bindings {
        cfg.bindings = bindings;
    }
    if let Some(is_default) = req.is_default {
        cfg.is_default = is_default;
    }

    reg.update(cfg).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e.to_string() }),
        )
    })?;
    let updated = reg.get(&id).cloned().expect("persona present after update");
    Ok(Json(updated))
}

/// DELETE /api/agents/{id} — remove a persona (default persona cannot be removed).
async fn delete_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AgentDeleteResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut reg = state.persona_registry.write().await;
    reg.delete(&id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e.to_string() }),
        )
    })?;
    Ok(Json(AgentDeleteResponse { id, deleted: true }))
}
```

### 2d. Persona chat handler

- [ ] **Step 4: Add `agent_chat`**

Reuses `ChatRequest`/`ChatResponse` (api.rs:85/243). Default persona clones the template; named personas build an isolated agent.

```rust
/// POST /api/agents/{id}/chat — chat with a specific persona (non-streaming).
async fn agent_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Fetch the persona config + the registry's backing dir under a read lock.
    let (cfg, agents_dir) = {
        let reg = state.persona_registry.read().await;
        match reg.get(&id) {
            Some(cfg) => (cfg.clone(), reg.agents_dir().to_path_buf()),
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("persona '{id}' not found"),
                    }),
                ));
            }
        }
    };

    // Build the agent for this persona. The default persona reuses the shared
    // template directly (full default behavior); named personas get an isolated
    // agent (own model/prompt/context/skills), mirroring the gateway split.
    let mut persona_agent = if id == hakimi_core::DEFAULT_PERSONA_ID {
        state.agent.lock().await.clone()
    } else {
        let template = state.agent.lock().await.clone();
        let context_length = {
            let config = state.config.lock().await;
            let model = if cfg.model.trim().is_empty() {
                template.model()
            } else {
                cfg.model.as_str()
            };
            hakimi_common::resolve_model_context_length(
                model,
                Some(config.model.context_length).filter(|length| *length > 0),
                config.compression.context_length,
            )
            .context_length
        };
        let skills_dir = agents_dir.join(&id).join("skills");
        hakimi_core::build_persona_agent(&template, &cfg, &skills_dir, context_length)
    };

    let session_id = persona_agent.session_id().to_string();
    match persona_agent.chat(&req.message).await {
        Ok(response) => Ok(Json(ChatResponse {
            response,
            session_id,
        })),
        Err(e) => {
            let msg = format!("Agent error: {e}");
            tracing::error!("{msg}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: msg }),
            ))
        }
    }
}
```

### 2e. Bindings overview handler

- [ ] **Step 5: Add `list_bindings`**

```rust
/// GET /api/bindings — channel-binding overview plus the default persona.
async fn list_bindings(State(state): State<AppState>) -> Json<BindingsResponse> {
    let reg = state.persona_registry.read().await;
    let bindings = reg
        .bindings()
        .iter()
        .map(|(channel, persona)| (channel.clone(), persona.clone()))
        .collect();
    Json(BindingsResponse {
        bindings,
        default: reg.default_id().to_string(),
    })
}
```

### 2f. Register routes

- [ ] **Step 6: Add the 7 routes to `build_router`**

In `build_router` (api.rs:1905), append to the `api_routes` chain before the `.route_layer(...)` auth call (e.g. right after the gateway routes at api.rs:1962, replacing the trailing `;` on `gateway_restart`):

```rust
        .route("/gateway/restart", post(gateway_restart))
        // Agent-dimension (persona) endpoints
        .route("/agents", get(list_agents))
        .route("/agents", post(create_agent))
        .route("/agents/{id}", get(get_agent))
        .route("/agents/{id}", patch(update_agent))
        .route("/agents/{id}", delete(delete_agent))
        .route("/agents/{id}/chat", post(agent_chat))
        .route("/bindings", get(list_bindings));
```

(`get`, `post`, `patch`, `delete` are already imported at api.rs:62.)

### 2g. Tests

- [ ] **Step 7: Add endpoint tests in `mod tests`**

Add after `test_health_endpoint` (api.rs:4969). These use `test_state()` (stub `StaticTransport`, empty password → auth bypassed, registry in a temp dir seeded with the `default` persona) and `oneshot`.

```rust
    async fn read_json(resp: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn json_post(uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    fn json_patch(uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("PATCH")
            .uri(uri)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    #[tokio::test]
    async fn test_agents_list_includes_default() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/agents")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["default"], "default");
        let ids: Vec<&str> = json["agents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"default"));
    }

    #[tokio::test]
    async fn test_agents_create_get_update_delete() {
        let app = build_router(test_state());

        // Create
        let resp = app
            .clone()
            .oneshot(json_post(
                "/api/agents",
                json!({"id": "coder", "name": "Coder"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["id"], "coder");
        assert_eq!(json["name"], "Coder");

        // Duplicate create rejected
        let resp = app
            .clone()
            .oneshot(json_post("/api/agents", json!({"id": "coder"})))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);

        // Get
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/agents/coder")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        // Update
        let resp = app
            .clone()
            .oneshot(json_patch(
                "/api/agents/coder",
                json!({"model": "claude-opus-4-8", "bindings": ["telegram:devbot"]}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["model"], "claude-opus-4-8");

        // Bindings overview reflects the update
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/bindings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = read_json(resp).await;
        assert_eq!(json["bindings"]["telegram:devbot"], "coder");

        // Delete
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/agents/coder")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        // Default persona cannot be deleted
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/agents/default")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_default_agent_chat_uses_stub_transport() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(json_post(
                "/api/agents/default/chat",
                json!({"message": "hello"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let json = read_json(resp).await;
        assert!(
            json["response"].as_str().unwrap().contains("hello"),
            "stub transport echoes the prompt: {json:?}"
        );
    }

    #[tokio::test]
    async fn test_agent_chat_unknown_persona_is_404() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(json_post(
                "/api/agents/ghost/chat",
                json!({"message": "hi"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);
    }
```

- [ ] **Step 8: fmt + clippy + test (Docker)**

Run:
```
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" fmt --all
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" clippy -p hakimi-server --all-targets --all-features
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-server agents
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-server agent_chat
```
Expected: fmt clean, clippy no warnings (`-Dwarnings`), tests PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/hakimi-server/src/api.rs
git commit -m "feat(server): Agent 维度 REST API(/api/agents CRUD + chat + /api/bindings)(P4-2)"
```

---

## Task 3: Verify on CI + update handoff

- [ ] **Step 1: Push the branch** — `git push` (interactive GCM login; user pushes).
- [ ] **Step 2: Poll CI** — `GET /repos/Mouseww/hakimi-agent/actions/runs?head_sha=<sha>` until green.
- [ ] **Step 3: Update the handoff** — in `docs/superpowers/handoffs/2026-06-22-multi-agent-isolation-handoff.md`, move P4 to "已完成" with commit shas, and note the remaining gateway agent-instance hot-add limitation as P5/follow-up.

---

## Self-review

- **Spec coverage (§3.6):** `GET/POST /api/agents` (Task 2b), `GET/PATCH/DELETE /api/agents/{id}` (Task 2c), `POST /api/agents/{id}/chat` (Task 2d), `GET /api/bindings` (Task 2e). `sessions`/`memory`/`skills` per-agent sub-resources and `/chat/stream` are spec-listed but out of P4's handoff scope (P5/follow-up); channel bind/unbind folds into PATCH. Documented.
- **Backward compat (§3.8):** existing endpoints unchanged; default persona chat reuses the template agent.
- **Placeholders:** none; every step shows exact code.
- **Type consistency:** `AgentUpdateRequest` fields match `PersonaConfig` fields (name/avatar/description/model/reasoning_effort/system_prompt/enabled_skills/bindings/is_default). `agents_dir()` (Task 1) is consumed in `agent_chat` (Task 2d). Handlers return `Result<Json<T>, (StatusCode, Json<ErrorResponse>)>` matching the existing `ErrorResponse { error }` envelope. Routes use handler names exactly as defined. `ChatRequest`/`ChatResponse` reused as-is.
- **Known non-goals (documented):** gateway `persona_agents` instance hot-add (binding resolution is live; new persona instances need restart), per-persona `sessions.db`/`memory` wiring in the chat handler, streaming persona chat, per-agent sessions/memory/skills sub-resource endpoints.
