# P3 Gateway Persona Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route each inbound gateway message to the persona that owns its `platform:bot_id` channel (falling back to the default persona), dispatch to that persona's isolated agent, and scope per-chat histories to the persona.

**Architecture:** The gateway message loop (`process_gateway_messages_loop`) gains a shared `Arc<RwLock<PersonaRegistry>>` (for live binding resolution) and an `Arc<HashMap<persona_id, Arc<Mutex<AIAgent>>>>` of pre-built per-persona base agents. Per message it resolves the persona via `resolve_for_channel`, selects that persona's base agent, applies the persona's system prompt + persona-scoped memory directory, and keys all in-memory history operations by `persona_id:chat_id`. The persona with id `default` keeps byte-for-byte legacy behavior (reuses the existing shared `agent_arc`, `config.roles["default"]` prompt, and root-level memory), so single-agent deployments are unchanged.

**Tech Stack:** Rust (Tokio async), `hakimi-core` (`PersonaRegistry`, `build_persona_agent`), `hakimi-cli` (`entry.rs` gateway loop). Cargo runs via Docker: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" <args>`. CI is the authoritative gate (`fmt --check` + `clippy --all-targets --all-features` + `test`).

---

## Design decisions (locked)

- **Legacy persona = id `default`.** Only the persona whose id equals `hakimi_core::DEFAULT_PERSONA_ID` ("default") uses the legacy path: the shared `agent_arc`, the `config.roles["default"].identity` prompt, and the root memory dir (`config.memory.path` or `runtime_home.memory_dir()`). Every other (named) persona uses its own pre-built agent, its own `system_prompt`, and `runtime_home.persona_dir(id)/memory`.
- **Robust fallback.** If `resolve_for_channel` returns a named persona that has no entry in the pre-built agent map (e.g. created at runtime by a future P4 endpoint before a restart), the loop falls back fully to legacy behavior for that turn. New-persona hot-add is explicitly P4 scope.
- **History keying.** All in-memory history operations key by `gateway_history_key(persona_id, chat_id)` = `"{persona_id}:{chat_id}"`. Histories are in-memory only (not persisted in this loop), so there is no migration concern. For the default persona this is a purely additive `"default:"` prefix.
- **Scope boundary.** `turn_trackers`, `last_usage`, `active_tasks`, `message_queues`, `voice_states` keys are left unchanged. `active_tasks`/`message_queues`/`voice_states` already key by `task_key = platform:bot_id:chat_id` (which determines the persona). `turn_trackers`/`last_usage` keep their existing `chat_id` keys; cross-persona collision there is a pre-existing minor concern, out of P3 scope (the handoff scopes P3 to "per-chat histories 下沉到人格").
- **Shared registry.** In unified mode the gateway loop and the WebUI `AppState` share one `Arc<RwLock<PersonaRegistry>>`, so future P4 binding edits affect routing live. The per-persona agent map is built once at startup.

## File structure

- Modify: `crates/hakimi-cli/src/entry.rs`
  - Add free fn `gateway_history_key(persona_id, chat_id) -> String` next to `gateway_task_key` (entry.rs:34).
  - Add async fn `build_gateway_persona_agents(...)` (builds the named-persona agent map).
  - Extend `process_gateway_messages_loop` signature (+2 params) and its per-turn body.
  - Wire `start_gateway` (entry.rs:6816) and `start_unified_server` (entry.rs:7070); share the registry with `AppState` in unified mode (entry.rs:7361-7371).
  - Add `gateway_history_key` to the `mod tests` import list (entry.rs:8146) + a unit test.

No `hakimi-core` changes are needed; P2 already provides `PersonaRegistry`, `build_persona_agent`, `DEFAULT_PERSONA_ID`, and `RuntimeHome::persona_dir`.

---

## Task 1: History key helper (TDD)

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs:34` (add helper after `gateway_task_key`)
- Test: `crates/hakimi-cli/src/entry.rs:8146` (tests module)

- [ ] **Step 1: Write the failing test**

In the `mod tests` block, add the import and a test. First extend the `use super::{...}` list (entry.rs:8153 area) to include `gateway_history_key` (insert alphabetically near `gateway_cron_response_for_path`):

```rust
        gateway_cron_response_for_path, gateway_cron_response_for_path_with_delivery,
        gateway_history_key,
        gateway_mcp_response, gateway_service_exe_path, gateway_service_unit,
```

Then add the test near the other small helper tests:

```rust
    #[test]
    fn gateway_history_key_scopes_chat_by_persona() {
        assert_eq!(gateway_history_key("default", "chat-1"), "default:chat-1");
        assert_eq!(gateway_history_key("coder", "chat-1"), "coder:chat-1");
        // Same chat id under different personas does not collide.
        assert_ne!(
            gateway_history_key("coder", "chat-1"),
            gateway_history_key("writer", "chat-1")
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-cli gateway_history_key`
Expected: FAIL to compile ("cannot find function `gateway_history_key`").

- [ ] **Step 3: Write minimal implementation**

After `gateway_task_key` (entry.rs:34-36) add:

```rust
/// Per-persona history bucket key. Scopes the in-memory per-chat history map so
/// two personas never share a chat's conversation, even if a `chat_id` collides
/// across channels. The default persona uses a plain `default:` prefix.
fn gateway_history_key(persona_id: &str, chat_id: &str) -> String {
    format!("{persona_id}:{chat_id}")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-cli gateway_history_key`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/hakimi-cli/src/entry.rs
git commit -m "feat(cli): persona-scoped gateway history key helper (P3-1)"
```

---

## Task 2: Per-persona agent map builder + loop routing + caller wiring

This task changes `process_gateway_messages_loop`'s signature, so it must land with both callers in a single compiling commit (workspace uses `RUSTFLAGS=-Dwarnings`; an unused fn or mismatched call breaks CI).

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs` (builder fn, loop signature + body, both callers)

### 2a. Add the per-persona agent map builder

- [ ] **Step 1: Add `build_gateway_persona_agents`**

Add this free async fn just above `process_gateway_messages_loop` (entry.rs:5575, before the `#[allow(clippy::too_many_arguments)]`):

```rust
/// Build the per-persona base agents for gateway routing.
///
/// The default persona (`DEFAULT_PERSONA_ID`) is intentionally omitted: it reuses
/// the shared legacy `agent_arc`, so existing single-agent behavior is preserved
/// byte-for-byte. Each named persona gets an isolated agent (own model / prompt /
/// context engine / skills), loading its skills from `<persona_dir>/skills`.
fn build_gateway_persona_agents(
    template: &hakimi_core::AIAgent,
    registry: &hakimi_core::PersonaRegistry,
    runtime_home: &hakimi_common::RuntimeHome,
    context_length: usize,
) -> std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<hakimi_core::AIAgent>>> {
    let mut map = std::collections::HashMap::new();
    for cfg in registry.list() {
        if cfg.id == hakimi_core::DEFAULT_PERSONA_ID {
            continue;
        }
        let skills_dir = runtime_home.persona_dir(&cfg.id).join("skills");
        let agent =
            hakimi_core::build_persona_agent(template, cfg, &skills_dir, context_length);
        map.insert(
            cfg.id.clone(),
            std::sync::Arc::new(tokio::sync::Mutex::new(agent)),
        );
    }
    map
}
```

### 2b. Extend the loop signature

- [ ] **Step 2: Add two params to `process_gateway_messages_loop`**

At entry.rs:5581, after the `agent_arc: ...,` parameter, insert:

```rust
    persona_registry: std::sync::Arc<tokio::sync::RwLock<hakimi_core::PersonaRegistry>>,
    persona_agents: std::sync::Arc<
        std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<hakimi_core::AIAgent>>>,
    >,
```

### 2c. Resolve persona per message (before command dispatch)

- [ ] **Step 3: Resolve persona at the top of the loop body**

In the loop body, immediately after the `media_id` binding (entry.rs:5615, before the `if platform == "__hakimi_system__"` block), insert:

```rust
        let persona_cfg = {
            let reg = persona_registry.read().await;
            reg.resolve_for_channel(&platform, &bot_id).clone()
        };
        let persona_id = persona_cfg.id.clone();
        let is_default_persona = persona_id == hakimi_core::DEFAULT_PERSONA_ID;
        let history_key = gateway_history_key(&persona_id, &chat_id);
```

Note: the `__hakimi_system__` and unauthorized-drop early-`continue` branches run after this; resolving first is harmless (cheap clone) and keeps `history_key` in scope for `/undo`.

### 2d. Use `history_key` in the outer `/undo` branch

- [ ] **Step 4: Update `/undo` history access**

At entry.rs:5708, change:

```rust
                                    let history = histories.entry(chat_id.clone()).or_default();
```
to:
```rust
                                    let history = histories.entry(history_key.clone()).or_default();
```

### 2e. Capture persona data into the spawned task

- [ ] **Step 5: Clone persona bindings for the spawn**

At entry.rs:5736 (in the block of `let ... = ....clone();` just before `tokio::spawn`), add:

```rust
        let persona_agents = persona_agents.clone();
        let persona_cfg = persona_cfg.clone();
        let persona_id = persona_id.clone();
        let history_key = history_key.clone();
        let is_default_persona = is_default_persona;
```

### 2f. Select the persona base agent inside the spawn

- [ ] **Step 6: Compute `base_agent` + `use_persona_config`**

Inside the spawned task, right after `let task_key = gateway_task_key(&platform, &bot_id, &chat_id);` (entry.rs:5746), insert:

```rust
            // Resolve which agent + config this persona uses. The default persona
            // reuses the shared legacy agent; a named persona uses its own. A named
            // persona without a pre-built agent (e.g. added at runtime) falls back to
            // legacy behavior for this turn.
            let (base_agent, use_persona_config) = if is_default_persona {
                (agent_clone.clone(), false)
            } else if let Some(agent) = persona_agents.get(&persona_id) {
                (agent.clone(), true)
            } else {
                (agent_clone.clone(), false)
            };
```

### 2g. Route command handlers and the turn through `base_agent`

- [ ] **Step 7: Replace `agent_clone` agent reads/writes with `base_agent`**

Inside the spawn, replace these `agent_clone` uses with `base_agent` (the `agent_clone` binding stays — it is the fallback source for `base_agent`):

- entry.rs:5923 (`Command::Model`): `let mut a = agent_clone.lock().await;` -> `let mut a = base_agent.lock().await;`
- entry.rs:5932 (`Command::Tools`): `let a = agent_clone.lock().await;` -> `let a = base_agent.lock().await;`
- entry.rs:5965 (`Command::Status`): `let a = agent_clone.lock().await;` -> `let a = base_agent.lock().await;`
- entry.rs:6210 (turn agent clone): `let mut a = agent_clone.lock().await.clone();` -> `let mut a = base_agent.lock().await.clone();`

### 2h. Branch system prompt + memory dir on the persona

- [ ] **Step 8: Persona-aware memory dir**

Replace the memory block (entry.rs:6220-6238) so the memory directory is persona-scoped. Change:

```rust
                let mut memory_text = String::new();
                if config.memory.enabled {
                    let memory_dir = if config.memory.path.is_empty() {
                        runtime_home.memory_dir()
                    } else {
                        std::path::PathBuf::from(&config.memory.path)
                    };
```
to:
```rust
                let mut memory_text = String::new();
                if config.memory.enabled {
                    let memory_dir = if use_persona_config {
                        runtime_home.persona_dir(&persona_id).join("memory")
                    } else if config.memory.path.is_empty() {
                        runtime_home.memory_dir()
                    } else {
                        std::path::PathBuf::from(&config.memory.path)
                    };
```

- [ ] **Step 9: Persona-aware base prompt**

Replace the base-prompt block (entry.rs:6242-6247). Change:

```rust
                let base_prompt = config
                    .roles
                    .get("default")
                    .map(|r| r.identity.clone())
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| hakimi_core::DEFAULT_SYSTEM_PROMPT.to_string());
```
to:
```rust
                let base_prompt = if use_persona_config {
                    if persona_cfg.system_prompt.trim().is_empty() {
                        hakimi_core::DEFAULT_SYSTEM_PROMPT.to_string()
                    } else {
                        persona_cfg.system_prompt.clone()
                    }
                } else {
                    config
                        .roles
                        .get("default")
                        .map(|r| r.identity.clone())
                        .filter(|id| !id.is_empty())
                        .unwrap_or_else(|| hakimi_core::DEFAULT_SYSTEM_PROMPT.to_string())
                };
```

### 2i. Use `history_key` for history read + write-back

- [ ] **Step 10: Update history read**

At entry.rs:6259, change:

```rust
                    let chat_msgs = histories.get(&chat_id).cloned().unwrap_or_default();
```
to:
```rust
                    let chat_msgs = histories.get(&history_key).cloned().unwrap_or_default();
```

- [ ] **Step 11: Update history write-back**

At entry.rs:6690, change:

```rust
                            let chat_history = histories.entry(chat_id.clone()).or_default();
```
to:
```rust
                            let chat_history = histories.entry(history_key.clone()).or_default();
```

### 2j. Update the `/clear` command history access

- [ ] **Step 12: Update `/clear` history removal**

At entry.rs:5907, change:

```rust
                            histories.remove(&chat_id).is_some()
```
to:
```rust
                            histories.remove(&history_key).is_some()
```

### 2k. Wire `start_gateway`

- [ ] **Step 13: Build registry + agents in `start_gateway` and pass them**

In `start_gateway` (entry.rs:6816), after `let runtime_home = Arc::new(runtime_home);` (entry.rs:6878) and after `let agent_arc = Arc::new(Mutex::new(agent));` (entry.rs:6861), add the registry + agent map. Insert right after the `agent_arc` line (6861):

```rust
    let persona_registry = Arc::new(tokio::sync::RwLock::new(
        hakimi_core::PersonaRegistry::load(runtime_home.agents_dir())?,
    ));
    let persona_agents = {
        let template = agent_arc.lock().await.clone();
        let resolved_context = hakimi_common::resolve_model_context_length(
            template.model(),
            Some(config.model.context_length).filter(|length| *length > 0),
            config.compression.context_length,
        );
        let reg = persona_registry.read().await;
        Arc::new(build_gateway_persona_agents(
            &template,
            &reg,
            &runtime_home,
            resolved_context.context_length,
        ))
    };
```

Note: at line 6861 `runtime_home` is not yet wrapped in `Arc` (that happens at 6878). Place this block AFTER line 6878 (`let runtime_home = Arc::new(runtime_home);`) so `runtime_home.agents_dir()` / `runtime_home.persona_dir()` resolve through the `Arc` (deref works). Concretely, insert it immediately after entry.rs:6878.

Then update the `process_gateway_messages_loop(...)` call (entry.rs:7043-7060) to pass the two new args right after `agent_arc,`:

```rust
    process_gateway_messages_loop(
        messages,
        gateway,
        gateway_bot_ids,
        agent_arc,
        persona_registry,
        persona_agents,
        histories_clone,
        ...
```

### 2l. Wire `start_unified_server` and share the registry with `AppState`

- [ ] **Step 14: Build registry + agents in `start_unified_server`**

In `start_unified_server` (entry.rs:7070), after `let runtime_home_arc = Arc::new(runtime_home);` (entry.rs:7141) and after `let agent_arc = Arc::new(Mutex::new(agent));` (entry.rs:7121), add (place after entry.rs:7141 so `runtime_home_arc` exists):

```rust
    let persona_registry = Arc::new(tokio::sync::RwLock::new(
        hakimi_core::PersonaRegistry::load(runtime_home_arc.agents_dir())?,
    ));
    let persona_agents = {
        let template = agent_arc.lock().await.clone();
        let resolved_context = hakimi_common::resolve_model_context_length(
            template.model(),
            Some(config.model.context_length).filter(|length| *length > 0),
            config.compression.context_length,
        );
        let reg = persona_registry.read().await;
        Arc::new(build_gateway_persona_agents(
            &template,
            &reg,
            &runtime_home_arc,
            resolved_context.context_length,
        ))
    };
```

- [ ] **Step 15: Pass the new args into the spawned loop**

In the pre-spawn clone block (entry.rs:7158-7172) add clones for the gateway task:

```rust
    let persona_registry_for_msg = persona_registry.clone();
    let persona_agents_for_msg = persona_agents.clone();
```

Then update the `process_gateway_messages_loop(...)` call inside `tokio::spawn` (entry.rs:7175-7192) to pass them after `agent_arc_for_msg,`:

```rust
        let _ = process_gateway_messages_loop(
            messages,
            gateway_for_msg,
            gateway_bot_ids_for_msg,
            agent_arc_for_msg,
            persona_registry_for_msg,
            persona_agents_for_msg,
            histories_for_msg,
            ...
```

- [ ] **Step 16: Reuse the same registry in `AppState`**

Replace the `AppState` registry construction (entry.rs:7361 + 7371). Change:

```rust
    let persona_registry = hakimi_core::PersonaRegistry::load(hakimi_dir.join("agents"))?;
    let app_state = hakimi_server::server::AppState {
        agent: agent_arc,
        ...
        gateway: Some(gateway.clone()),
        persona_registry: Arc::new(tokio::sync::RwLock::new(persona_registry)),
    };
```
to (delete the local `load` line; reuse the shared Arc built in Step 14):

```rust
    let app_state = hakimi_server::server::AppState {
        agent: agent_arc,
        ...
        gateway: Some(gateway.clone()),
        persona_registry,
    };
```

(`persona_registry` from Step 14 is moved here; it was `.clone()`d for the gateway task in Step 15, so the move is fine.)

### 2m. Compile, lint, test

- [ ] **Step 17: fmt + clippy + test (Docker)**

Run:
```
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" fmt --all
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" clippy -p hakimi-cli --all-targets --all-features
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-cli
```
Expected: fmt clean, clippy no warnings (`-Dwarnings`), tests PASS. (CLI cold-compile is slow; if local is impractical, push and let CI verify per the handoff guidance.)

- [ ] **Step 18: Commit**

```bash
git add crates/hakimi-cli/src/entry.rs
git commit -m "feat(cli): gateway routes per persona via resolve_for_channel; histories scoped per persona (P3-2)"
```

---

## Task 3: Verify on CI

- [ ] **Step 1: Push the branch**

`git push` (interactive GCM login required; user pushes or completes login in their terminal per handoff).

- [ ] **Step 2: Poll CI**

`GET /repos/Mouseww/hakimi-agent/actions/runs?head_sha=<sha>` until the run for the pushed sha is green (`fmt --check` + `clippy --workspace --all-targets --all-features` + `test --workspace`).

- [ ] **Step 3: Update the handoff doc**

Mark P3 done in `docs/superpowers/handoffs/2026-06-22-multi-agent-isolation-handoff.md` (move it from "剩余工作" to "已完成 + CI 验证" with the commit sha + CI run id), so P4/P5 start from accurate state.

---

## Self-review

- **Spec coverage (§3.4 inbound gateway, §3.7 entry.rs:6861, §5.2 P3):** resolve_for_channel routing (Task 2c/2f), dispatch to persona agent with its prompt/skills/memory/model (Task 2f/2h, plus `build_persona_agent` supplying skills/model/context), per-chat histories sunk to persona (Task 1 + 2d/2i/2j). Covered.
- **Backward compat (§3.8):** default persona (id `default`) keeps `agent_arc` + `config.roles["default"]` prompt + root memory; named-but-unbuilt personas fall back to legacy. Existing `/api/*` endpoints untouched. Covered.
- **Placeholders:** none; every code step shows exact before/after.
- **Type consistency:** `gateway_history_key(&str, &str) -> String` used identically in Task 1 and Task 2c. `build_gateway_persona_agents` returns `HashMap<String, Arc<Mutex<AIAgent>>>`, wrapped in `Arc` at call sites and consumed as `Arc<HashMap<...>>` by the loop param. `persona_cfg: PersonaConfig` (Clone) and `persona_id: String` thread consistently outer-scope -> spawn capture -> body. `resolve_model_context_length` args mirror `build_agent` (entry.rs:5440).
- **Known non-goals (documented):** runtime hot-add of brand-new persona agents (P4), `enabled_skills` filtering vs directory-based skills, `turn_trackers`/`last_usage` persona scoping.
