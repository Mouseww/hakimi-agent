# Persona Team Collaboration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the persona a user talks to (the "lead") delegate scoped sub-tasks to named teammate personas via a new `team` tool; each teammate answers with its own model/skills/memory, and the result returns into the lead's context.

**Architecture:** Orchestrator/delegation model. A `TeamExecutor` trait (in `hakimi-common`) is injected into each agent's `ToolContext` (held on `AIAgent`, set by the dispatch layer). `PersonaTeamExecutor` (in `hakimi-core`) validates the target persona is addressable, builds its agent with the existing `build_persona_agent`, runs one bounded turn, and returns a structured result. Depth/cycle/concurrency guards prevent runaway. Synchronous, stateless teammate runs. Progress reuses the existing `hakimi_delegate:` bubble protocol.

**Tech Stack:** Rust (async-trait, tokio, anyhow/HakimiError), React 19 + TypeScript (WebUI). Builds run via Docker: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" <args>` (image `hakimi-rust:nightly`, `RUSTFLAGS=-Dwarnings`). WebUI: `npm run lint` + `npm run build` in `hakimi-webui/`.

**Spec:** `docs/superpowers/specs/2026-06-24-persona-team-collaboration-design.md`

---

## File Structure

**New files:**
- `crates/hakimi-core/src/team.rs` — `PersonaTeamExecutor` + pure helpers `validate_consult`, `build_roster`.
- `crates/hakimi-tools/src/builtin_team.rs` — `TeamTool` (reads `team_executor` from `ToolContext`).

**Modified files:**
- `crates/hakimi-common/src/tool.rs` — `TeamExecutor` trait, `TeammateInfo`, `TeamCallContext`, `ToolContext.team_executor`.
- `crates/hakimi-core/src/persona.rs` — `PersonaConfig.addressable` + `default_true()`.
- `crates/hakimi-core/src/agent.rs` — `AIAgent.team_executor` field, `Clone`, `AIAgentBuilder::build()`, `set_team_executor`, `build_tool_context`.
- `crates/hakimi-core/src/lib.rs` — exports.
- `crates/hakimi-tools/src/lib.rs` — `pub mod builtin_team;` + `pub use`.
- `crates/hakimi-cli/src/entry.rs` — register `TeamTool`; build + inject `PersonaTeamExecutor` in both gateway entry points.
- `crates/hakimi-server/src/main.rs` + `crates/hakimi-tui/src/main.rs` — register `TeamTool`.
- `crates/hakimi-server/src/api.rs` — `addressable` in `AgentUpdateRequest` + merge in `update_agent`; inject executor in `agent_chat_stream`.
- `hakimi-webui/src/api.ts` — `addressable` on `Agent`/`AgentUpdate`.
- `hakimi-webui/src/PersonaConfigForm.tsx` — `addressable` toggle.

**Phasing:** P1 foundations (compiles, no behavior) → P2 executor → P3 tool → P4 wiring (functional) → P5 API/UI polish.

---

## Phase 1 — Foundations

### Task 1: `TeamExecutor` trait + `ToolContext.team_executor`

**Files:**
- Modify: `crates/hakimi-common/src/tool.rs` (near `DelegateExecutor` at :253 and `ToolContext` at :284)

- [ ] **Step 1: Add the trait and types** after the `DelegateExecutor` trait block (after line ~280)

```rust
/// Metadata describing a teammate persona that can be consulted via the `team` tool.
#[derive(Debug, Clone)]
pub struct TeammateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// A single consultation request handed to a [`TeamExecutor`].
///
/// `depth` and `lineage` are NOT carried here: they live on the executor instance
/// bound to the calling agent (each consult descends into a child executor).
pub struct TeamCallContext {
    /// Target teammate persona id.
    pub teammate_id: String,
    /// The sub-task / question for the teammate.
    pub task: String,
    /// Optional shared context and constraints.
    pub context: String,
    /// Progress callback (reuses the delegate bubble protocol).
    pub progress: Option<ToolProgressCallback>,
}

/// Executes a sub-task on a named teammate persona and returns its answer.
///
/// Implemented by `hakimi-core`'s `PersonaTeamExecutor`. Tools reach it through
/// [`ToolContext::team_executor`].
#[async_trait]
pub trait TeamExecutor: Send + Sync {
    /// List teammate personas this agent may consult (id, name, description).
    async fn roster(&self) -> Vec<TeammateInfo>;

    /// Consult a single teammate; returns its final structured answer.
    async fn consult(&self, call: TeamCallContext) -> Result<String>;

    /// Consult several teammates concurrently; returns one answer per input
    /// (failures become `"Teammate <id> failed: ..."` strings, never aborting the batch).
    async fn consult_many(&self, calls: Vec<TeamCallContext>) -> Result<Vec<String>>;
}
```

- [ ] **Step 2: Add the field to `ToolContext`** immediately after the `delegate_executor` field (after line 306)

```rust
    /// Optional team executor for delegating to named teammate personas
    /// (`team` tool). Set by the dispatch layer; `None` disables team collaboration.
    #[serde(skip)]
    pub team_executor: Option<Arc<dyn TeamExecutor>>,
```

- [ ] **Step 3: Update the `Debug` impl** for `ToolContext` (the manual `fmt` near line 366) — add a line after the `delegate_executor` field line

```rust
            .field("team_executor", &self.team_executor.is_some())
```

- [ ] **Step 4: Build to verify it compiles**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" build -p hakimi-common`
Expected: compiles. `ToolContext` derives `Default`, and `Option<Arc<dyn TeamExecutor>>` defaults to `None`, so `Default`/`Clone` still work; `#[serde(skip)]` keeps serde happy (mirrors `delegate_executor`).

- [ ] **Step 5: Commit**

```bash
git add crates/hakimi-common/src/tool.rs
git commit -m "feat(common): TeamExecutor trait + ToolContext.team_executor"
```

---

### Task 2: `PersonaConfig.addressable`

**Files:**
- Modify: `crates/hakimi-core/src/persona.rs`

- [ ] **Step 1: Write the failing test** — add to the `tests` module in `persona.rs`

```rust
    #[test]
    fn addressable_defaults_true_when_absent() {
        // A persona.yaml written before the field existed must load as addressable.
        let dir = temp_dir();
        let persona_dir = dir.join("legacy");
        std::fs::create_dir_all(&persona_dir).unwrap();
        std::fs::write(
            persona_dir.join("persona.yaml"),
            "id: legacy\nname: Legacy\n",
        )
        .unwrap();

        let loaded = load_persona(&persona_dir).unwrap();
        assert!(loaded.addressable, "missing addressable must default to true");
    }

    #[test]
    fn new_persona_is_addressable_by_default() {
        assert!(PersonaConfig::new("coder").addressable);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-core persona::tests::addressable_defaults_true_when_absent persona::tests::new_persona_is_addressable_by_default`
Expected: FAIL (no field `addressable`).

- [ ] **Step 3: Add the field + default helper.** In the `PersonaConfig` struct (after `is_default` at line 50), add:

```rust
    /// Whether other personas may consult this one as a teammate (`team` tool).
    /// Defaults to `true` so teams work out of the box; toggle off to opt out.
    #[serde(default = "default_true")]
    pub addressable: bool,
```

After the struct (before `impl PersonaConfig`), add the helper:

```rust
fn default_true() -> bool {
    true
}
```

In `PersonaConfig::new` (the struct literal around line 58), add the field:

```rust
            is_default: false,
            addressable: true,
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-core persona::tests`
Expected: PASS (existing `persona_round_trips_through_yaml` still passes — `new()` sets `addressable: true`, and it serializes/round-trips).

- [ ] **Step 5: Commit**

```bash
git add crates/hakimi-core/src/persona.rs
git commit -m "feat(core): PersonaConfig.addressable (default true)"
```

---

### Task 3: `AIAgent.team_executor` field + wiring into `ToolContext`

**Files:**
- Modify: `crates/hakimi-core/src/agent.rs`

- [ ] **Step 1: Add the field to the `AIAgent` struct** (after `trajectory_config` at line 49, before the closing `}` at line 50)

```rust
    pub(crate) team_executor: Option<Arc<dyn hakimi_common::TeamExecutor>>,
```

- [ ] **Step 2: Clone it.** In `impl Clone for AIAgent` (after `trajectory_config: self.trajectory_config.clone(),` at line 82) add:

```rust
            team_executor: self.team_executor.clone(),
```

- [ ] **Step 3: Initialize it in the builder.** In `AIAgentBuilder::build()` (search `fn build(` around line ~183+; it returns an `AIAgent { ... }` struct literal), add to that literal:

```rust
            team_executor: None,
```

- [ ] **Step 4: Add a setter.** Near `set_model` (line ~724) add:

```rust
    /// Attach (or clear) the team executor used by the `team` tool. Set by the
    /// dispatch layer, which holds the persona registry.
    pub fn set_team_executor(
        &mut self,
        executor: Option<Arc<dyn hakimi_common::TeamExecutor>>,
    ) {
        self.team_executor = executor;
    }
```

- [ ] **Step 5: Pass it into `ToolContext`.** In `build_tool_context` (line 637 `ToolContext { ... }`), add after `delegate_executor,` (line 643):

```rust
            team_executor: self.team_executor.clone(),
```

- [ ] **Step 6: Build to verify it compiles**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" build -p hakimi-core`
Expected: compiles (the compiler will flag any struct-literal site that still misses the new field — fix those by adding `team_executor: None`).

- [ ] **Step 7: Commit**

```bash
git add crates/hakimi-core/src/agent.rs
git commit -m "feat(core): AIAgent.team_executor field wired into ToolContext"
```

---

## Phase 2 — PersonaTeamExecutor

### Task 4: `PersonaTeamExecutor` + pure guard/roster helpers

**Files:**
- Create: `crates/hakimi-core/src/team.rs`
- Modify: `crates/hakimi-core/src/lib.rs`

- [ ] **Step 1: Create `team.rs` with the pure helpers + their failing tests**

```rust
//! Persona team collaboration: delegate sub-tasks to named teammate personas.
//!
//! `PersonaTeamExecutor` implements [`hakimi_common::TeamExecutor`]. A consultation
//! validates the target is addressable and within depth/cycle limits, builds the
//! teammate's agent via [`crate::build_persona_agent`] (sharing the instance
//! `SharedRuntime`), runs one bounded turn, and returns the teammate's answer.
//! Synchronous and stateless: the teammate reads its persona prompt/skills/memory
//! but this consultation does not persist to its session/memory.
//!
//! Design: `docs/superpowers/specs/2026-06-24-persona-team-collaboration-design.md`.

use std::sync::Arc;

use async_trait::async_trait;
use hakimi_common::{
    HakimiError, Result, TeamCallContext, TeamExecutor, TeammateInfo, ToolProgressCallback,
};
use tokio::sync::{RwLock, Semaphore};

use crate::AIAgent;
use crate::persona::PersonaConfig;
use crate::persona_registry::PersonaRegistry;

/// Maximum collaboration depth (lead = depth 0). A teammate at the cap cannot consult further.
const MAX_TEAM_DEPTH: usize = 2;
/// Maximum concurrent teammate consultations from one executor.
const MAX_CONCURRENT_CONSULTS: usize = 5;
/// Retry attempts for a single teammate turn.
const MAX_CONSULT_ATTEMPTS: usize = 3;

/// How a consulted teammate should frame its answer (prepended to the seed message,
/// leaving the persona's own system prompt/identity intact).
const TEAM_RESULT_CONTRACT: &str = "You are being consulted by a teammate agent on a focused sub-task. Use your own skills and knowledge. Return a concise, self-contained answer in this shape:\nStatus: success | partial | failed\nSummary:\nDetails:\nRisks/Assumptions:";

/// Validate a consultation request against the registry, depth, and lineage.
/// Pure (no I/O beyond the already-held registry): returns the teammate config or
/// a descriptive error the model can act on.
pub(crate) fn validate_consult(
    reg: &PersonaRegistry,
    teammate_id: &str,
    depth: usize,
    lineage: &[String],
) -> Result<PersonaConfig> {
    if depth >= MAX_TEAM_DEPTH {
        return Err(HakimiError::Tool(format!(
            "team consultation depth limit ({MAX_TEAM_DEPTH}) reached; cannot delegate further"
        )));
    }
    if lineage.iter().any(|id| id == teammate_id) {
        return Err(HakimiError::Tool(format!(
            "cycle detected: '{teammate_id}' is already in the collaboration chain"
        )));
    }
    let cfg = reg.get(teammate_id).ok_or_else(|| {
        HakimiError::Tool(format!("teammate persona '{teammate_id}' not found"))
    })?;
    if !cfg.addressable {
        return Err(HakimiError::Tool(format!(
            "teammate persona '{teammate_id}' is not addressable (its addressable switch is off)"
        )));
    }
    Ok(cfg.clone())
}

/// Build the teammate roster visible to an executor: addressable personas not
/// already in the collaboration chain (excludes the lead and ancestors).
pub(crate) fn build_roster(reg: &PersonaRegistry, lineage: &[String]) -> Vec<TeammateInfo> {
    reg.list()
        .into_iter()
        .filter(|cfg| cfg.addressable && !lineage.iter().any(|id| id == &cfg.id))
        .map(|cfg| TeammateInfo {
            id: cfg.id.clone(),
            name: cfg.name.clone(),
            description: cfg.description.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persona_registry::DEFAULT_PERSONA_ID;

    fn temp_agents_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("hakimi-team-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path.join("agents")
    }

    fn registry_with(personas: &[(&str, bool)]) -> PersonaRegistry {
        let mut reg = PersonaRegistry::load(temp_agents_dir()).unwrap();
        for (id, addressable) in personas {
            let mut cfg = PersonaConfig::new(*id);
            cfg.addressable = *addressable;
            reg.create(cfg).unwrap();
        }
        reg
    }

    #[test]
    fn validate_rejects_unknown_teammate() {
        let reg = registry_with(&[]);
        let err = validate_consult(&reg, "ghost", 0, &["lead".into()]).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn validate_rejects_non_addressable() {
        let reg = registry_with(&[("writer", false)]);
        let err = validate_consult(&reg, "writer", 0, &["lead".into()]).unwrap_err();
        assert!(err.to_string().contains("not addressable"));
    }

    #[test]
    fn validate_rejects_depth_overflow() {
        let reg = registry_with(&[("writer", true)]);
        let err = validate_consult(&reg, "writer", MAX_TEAM_DEPTH, &["lead".into()]).unwrap_err();
        assert!(err.to_string().contains("depth limit"));
    }

    #[test]
    fn validate_rejects_cycle() {
        let reg = registry_with(&[("writer", true)]);
        let err =
            validate_consult(&reg, "writer", 1, &["lead".into(), "writer".into()]).unwrap_err();
        assert!(err.to_string().contains("cycle detected"));
    }

    #[test]
    fn validate_accepts_addressable_teammate() {
        let reg = registry_with(&[("writer", true)]);
        let cfg = validate_consult(&reg, "writer", 0, &["lead".into()]).unwrap();
        assert_eq!(cfg.id, "writer");
    }

    #[test]
    fn roster_excludes_lineage_and_non_addressable() {
        let reg = registry_with(&[("coder", true), ("writer", false), ("lead", true)]);
        let roster = build_roster(&reg, &["lead".into()]);
        let ids: Vec<&str> = roster.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"coder"));
        assert!(ids.contains(&DEFAULT_PERSONA_ID)); // seeded default is addressable
        assert!(!ids.contains(&"writer")); // not addressable
        assert!(!ids.contains(&"lead")); // in lineage
    }
}
```

- [ ] **Step 2: Register the module + run the helper tests to verify they fail then pass**

Add to `crates/hakimi-core/src/lib.rs` in the module list (after `pub mod shared;`):

```rust
pub mod team;
```

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-core team::tests`
Expected: PASS (helpers are fully implemented in Step 1; the module had to be registered to compile).

- [ ] **Step 3: Add the executor struct + impl** to `team.rs` (after `build_roster`, before `#[cfg(test)]`)

```rust
/// Executes teammate consultations for one agent. Cheap to clone (Arc-backed);
/// `for_lead`/`descend` produce repositioned instances for nested collaboration.
#[derive(Clone)]
pub struct PersonaTeamExecutor {
    registry: Arc<RwLock<PersonaRegistry>>,
    /// Template agent carrying the instance `SharedRuntime`; teammates are built from it.
    template: Arc<AIAgent>,
    context_length: usize,
    depth: usize,
    lineage: Vec<String>,
    semaphore: Arc<Semaphore>,
}

impl PersonaTeamExecutor {
    /// Create a base executor (depth 0, empty lineage). Use [`Self::for_lead`] per request.
    pub fn new(
        registry: Arc<RwLock<PersonaRegistry>>,
        template: Arc<AIAgent>,
        context_length: usize,
    ) -> Self {
        Self {
            registry,
            template,
            context_length,
            depth: 0,
            lineage: Vec::new(),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_CONSULTS)),
        }
    }

    /// Position this executor for a lead persona: lineage `[lead_id]`, depth 0.
    pub fn for_lead(&self, lead_id: &str) -> Self {
        let mut next = self.clone();
        next.depth = 0;
        next.lineage = vec![lead_id.to_string()];
        next
    }

    /// Child executor for a teammate: depth + 1, lineage + teammate id.
    fn descend(&self, teammate_id: &str) -> Self {
        let mut next = self.clone();
        next.depth += 1;
        next.lineage.push(teammate_id.to_string());
        next
    }
}

fn now_progress_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() % 86_400)
        .unwrap_or(0);
    format!("{:02}:{:02}:{:02}", secs / 3_600, (secs % 3_600) / 60, secs % 60)
}

fn truncate_for_title(value: &str, max: usize) -> String {
    let normalized = value.replace('\n', " ");
    let mut chars = normalized.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() { format!("{head}...") } else { head }
}

/// Emit a progress bubble in the existing `hakimi_delegate:` protocol so gateway +
/// WebUI render teammate progress without new plumbing.
fn emit_team_progress(progress: &Option<ToolProgressCallback>, task_id: &str, title: &str, line: &str) {
    if let Some(cb) = progress {
        cb(format!(
            "\u{001e}hakimi_delegate:{task_id}|{title}|{line}|{}",
            now_progress_timestamp()
        ));
    }
}

#[async_trait]
impl TeamExecutor for PersonaTeamExecutor {
    async fn roster(&self) -> Vec<TeammateInfo> {
        let reg = self.registry.read().await;
        build_roster(&reg, &self.lineage)
    }

    async fn consult(&self, call: TeamCallContext) -> Result<String> {
        // Validate + capture the teammate config and skills dir under one read lock.
        let (cfg, skills_dir) = {
            let reg = self.registry.read().await;
            let cfg = validate_consult(&reg, &call.teammate_id, self.depth, &self.lineage)?;
            let skills_dir = reg.agents_dir().join(&cfg.id).join("skills");
            (cfg, skills_dir)
        };

        let task_id = format!("team_{}", uuid::Uuid::new_v4().simple());
        let title = format!(
            "{} {} · {}",
            if cfg.avatar.is_empty() { "🤝" } else { cfg.avatar.as_str() },
            if cfg.name.is_empty() { cfg.id.as_str() } else { cfg.name.as_str() },
            truncate_for_title(&call.task, 32)
        );
        emit_team_progress(&call.progress, &task_id, &title, "已加入协作");

        let seed = if call.context.trim().is_empty() {
            format!("{TEAM_RESULT_CONTRACT}\n\nTask: {}", call.task)
        } else {
            format!(
                "{TEAM_RESULT_CONTRACT}\n\nTask: {}\n\nContext and constraints:\n{}",
                call.task, call.context
            )
        };

        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| HakimiError::Tool(format!("failed to acquire team permit: {e}")))?;

        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut teammate =
                crate::build_persona_agent(&self.template, &cfg, &skills_dir, self.context_length);
            teammate.set_session_id(task_id.clone());
            // Nested consults allowed up to the depth cap, with this teammate in the lineage.
            teammate.set_team_executor(Some(Arc::new(self.descend(&cfg.id))));

            // Forward the teammate's tool/progress markers as team bubbles.
            if let Some(parent) = call.progress.clone() {
                let (tid, ttitle) = (task_id.clone(), title.clone());
                teammate.set_streaming_callback(Some(Arc::new(move |token: String| {
                    if let Some(notice) = token.strip_prefix("\u{001e}hakimi_tool:") {
                        emit_team_progress(&Some(parent.clone()), &tid, &ttitle, notice.trim());
                    }
                })));
            }

            match teammate.run_conversation(&seed).await {
                Ok(res) => {
                    emit_team_progress(&call.progress, &task_id, &title, "完成，返回结果");
                    return Ok(res.final_response);
                }
                Err(e) if attempt >= MAX_CONSULT_ATTEMPTS => {
                    emit_team_progress(&call.progress, &task_id, &title, &format!("失败: {e}"));
                    return Err(HakimiError::Tool(format!(
                        "teammate '{}' failed after {MAX_CONSULT_ATTEMPTS} attempts: {e}",
                        cfg.id
                    )));
                }
                Err(e) => {
                    emit_team_progress(
                        &call.progress,
                        &task_id,
                        &title,
                        &format!("第 {attempt} 次失败，重试"),
                    );
                    tracing::warn!(error = %e, attempt, teammate = %cfg.id, "team consult retry");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    async fn consult_many(&self, calls: Vec<TeamCallContext>) -> Result<Vec<String>> {
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
        Ok(futures::future::join_all(futures).await)
    }
}
```

- [ ] **Step 4: Export the executor.** In `crates/hakimi-core/src/lib.rs` add:

```rust
pub use team::PersonaTeamExecutor;
```

- [ ] **Step 5: Verify `futures` is available.** Check `crates/hakimi-core/Cargo.toml` has a `futures` dependency (delegate.rs uses `futures::future::join_all`).

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" build -p hakimi-core`
Expected: compiles. If `futures` is missing, add `futures = "0.3"` under `[dependencies]` (delegate.rs already imports it, so it should be present).

- [ ] **Step 6: Add the happy-path integration test.** Append to `crates/hakimi-core/tests/integration.rs` (reuses the existing `MockTransport`):

```rust
#[tokio::test]
async fn team_executor_consults_addressable_teammate() {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let agents_dir = std::env::temp_dir()
        .join(format!("hakimi-team-it-{}", uuid::Uuid::new_v4()))
        .join("agents");
    let mut reg = hakimi_core::PersonaRegistry::load(&agents_dir).unwrap();
    let mut writer = hakimi_core::PersonaConfig::new("writer");
    writer.system_prompt = "You are the writer.".to_string();
    reg.create(writer).unwrap();
    let registry = Arc::new(RwLock::new(reg));

    let transport = Arc::new(MockTransport::text_response(
        "Status: success\nSummary: drafted",
    ));
    let template = Arc::new(hakimi_core::AIAgent::new(
        "test-model",
        transport,
        hakimi_tools::ToolRegistry::new(),
        None,
    ));

    let exec = hakimi_core::PersonaTeamExecutor::new(registry, template, 128_000).for_lead("lead");
    let answer = hakimi_common::TeamExecutor::consult(
        &exec,
        hakimi_common::TeamCallContext {
            teammate_id: "writer".to_string(),
            task: "draft a title".to_string(),
            context: String::new(),
            progress: None,
        },
    )
    .await
    .unwrap();

    assert!(answer.contains("drafted"));
}
```

- [ ] **Step 7: Run the integration + unit tests**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-core team`
Expected: PASS (unit guard tests + integration happy path). Confirm `hakimi-tools` is a dev-dependency of `hakimi-core` tests (integration.rs already uses `hakimi_tools::ToolRegistry`).

- [ ] **Step 8: Commit**

```bash
git add crates/hakimi-core/src/team.rs crates/hakimi-core/src/lib.rs crates/hakimi-core/tests/integration.rs crates/hakimi-core/Cargo.toml
git commit -m "feat(core): PersonaTeamExecutor with depth/cycle/addressable guards"
```

---

## Phase 3 — The `team` tool

### Task 5: `TeamTool`

**Files:**
- Create: `crates/hakimi-tools/src/builtin_team.rs`
- Modify: `crates/hakimi-tools/src/lib.rs`

- [ ] **Step 1: Create `builtin_team.rs` with the tool + tests**

```rust
use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, TeamCallContext, ToolContext};
use serde_json::{Value as JsonValue, json};

use crate::Tool;

/// Built-in tool: delegate a sub-task to a named teammate persona, or list teammates.
pub struct TeamTool;

#[async_trait]
impl Tool for TeamTool {
    fn name(&self) -> &str {
        "team"
    }

    fn toolset(&self) -> &str {
        "collaboration"
    }

    fn description(&self) -> &str {
        "Delegate a focused sub-task to a named teammate persona (each has its own model, skills, and memory) and get their answer back. Use action='list' first to see available teammates. Use this when a teammate is better suited to part of the task."
    }

    fn emoji(&self) -> &str {
        "\u{1f91d}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["consult", "list"],
                    "description": "consult = delegate to teammate(s); list = show available teammates. Default consult." },
                "teammate": { "type": "string", "description": "Target teammate persona id (single consult)." },
                "teammates": { "type": "array", "items": {"type": "string"},
                    "description": "Multiple teammate ids for a parallel consult. Use instead of 'teammate'." },
                "task": { "type": "string", "description": "The sub-task or question for the teammate(s)." },
                "context": { "type": "string", "description": "Optional shared context and constraints." }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let Some(executor) = ctx.team_executor.clone() else {
            return Ok("Team collaboration is not enabled in this environment.".to_string());
        };

        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("consult");

        if action == "list" {
            let roster = executor.roster().await;
            if roster.is_empty() {
                return Ok("No teammates are available to consult.".to_string());
            }
            let lines: Vec<String> = roster
                .iter()
                .map(|t| format!("- {} ({}): {}", t.id, t.name, t.description))
                .collect();
            return Ok(format!("Available teammates:\n{}", lines.join("\n")));
        }

        if action != "consult" {
            return Err(HakimiError::Tool(format!(
                "unsupported team action '{action}'. Expected 'consult' or 'list'."
            )));
        }

        let task = args
            .get("task")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: task".into()))?;
        let context = args.get("context").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let progress = ctx.progress_callback.clone();

        // Multiple teammates -> parallel fan-out.
        if let Some(teammates) = args.get("teammates").and_then(|v| v.as_array()) {
            let ids: Vec<String> = teammates
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if ids.is_empty() {
                return Err(HakimiError::Tool("'teammates' must contain at least one id".into()));
            }
            let calls: Vec<TeamCallContext> = ids
                .iter()
                .map(|id| TeamCallContext {
                    teammate_id: id.clone(),
                    task: task.to_string(),
                    context: context.clone(),
                    progress: progress.clone(),
                })
                .collect();
            let answers = executor.consult_many(calls).await?;
            let sections: Vec<String> = ids
                .iter()
                .zip(answers.iter())
                .map(|(id, answer)| format!("## {id}\n{answer}"))
                .collect();
            return Ok(sections.join("\n\n"));
        }

        // Single teammate.
        let teammate = args
            .get("teammate")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                HakimiError::Tool("provide 'teammate' (single) or 'teammates' (array)".into())
            })?;

        executor
            .consult(TeamCallContext {
                teammate_id: teammate.to_string(),
                task: task.to_string(),
                context,
                progress,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = TeamTool;
        assert_eq!(tool.name(), "team");
        assert_eq!(tool.toolset(), "collaboration");
        let required = tool.schema()["required"].as_array().unwrap();
        assert!(required.is_empty());
    }

    #[tokio::test]
    async fn execute_without_executor_degrades_gracefully() {
        let result = TeamTool.execute(&json!({"action": "list"}), &ToolContext::default()).await;
        assert!(result.unwrap().contains("not enabled"));
    }

    #[tokio::test]
    async fn consult_requires_task() {
        // team_executor None still returns the "not enabled" message before task checks,
        // so this asserts the missing-task path with a stub executor.
        use std::sync::Arc;
        use async_trait::async_trait;
        use hakimi_common::{TeamExecutor, TeammateInfo};

        struct StubExec;
        #[async_trait]
        impl TeamExecutor for StubExec {
            async fn roster(&self) -> Vec<TeammateInfo> { Vec::new() }
            async fn consult(&self, _c: TeamCallContext) -> Result<String> { Ok("ok".into()) }
            async fn consult_many(&self, _c: Vec<TeamCallContext>) -> Result<Vec<String>> { Ok(vec![]) }
        }

        let mut ctx = ToolContext::default();
        ctx.team_executor = Some(Arc::new(StubExec));
        let err = TeamTool
            .execute(&json!({"action": "consult", "teammate": "writer"}), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("task"));
    }
}
```

- [ ] **Step 2: Register the module + export.** In `crates/hakimi-tools/src/lib.rs`, add `pub mod builtin_team;` near the other `builtin_*` modules, and `pub use builtin_team::TeamTool;` near the other `pub use builtin_*` lines (mirror how `builtin_send_message` / `SendMessageTool` are declared).

- [ ] **Step 3: Run the tool tests**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-tools builtin_team`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/hakimi-tools/src/builtin_team.rs crates/hakimi-tools/src/lib.rs
git commit -m "feat(tools): team tool (consult/list teammate personas)"
```

---

## Phase 4 — Wiring (makes it functional)

### Task 6: Register `TeamTool` at all entry points

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs` (near line 5473, after the `DelegateTaskTool` registration)
- Modify: `crates/hakimi-server/src/main.rs` (near line 351)
- Modify: `crates/hakimi-tui/src/main.rs` (near line 279)

- [ ] **Step 1: Add the registration.** In `entry.rs` after the `DelegateTaskTool` register call:

```rust
        .register(std::sync::Arc::new(hakimi_tools::TeamTool))
```

In `server/main.rs` and `tui/main.rs`, add to the tool list array alongside `DelegateTaskTool`:

```rust
        Arc::new(hakimi_tools::TeamTool),
```

- [ ] **Step 2: Build all three crates**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" build -p hakimi-cli -p hakimi-server -p hakimi-tui`
Expected: compiles. (The tool degrades gracefully when no executor is set, so registering everywhere is safe.)

- [ ] **Step 3: Commit**

```bash
git add crates/hakimi-cli/src/entry.rs crates/hakimi-server/src/main.rs crates/hakimi-tui/src/main.rs
git commit -m "feat: register team tool in gateway, server, and tui"
```

---

### Task 7: Build + inject `PersonaTeamExecutor` in the gateway

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs`

The loop already holds `persona_registry` and `agent_arc`. Build a base executor once per loop start and position it per message onto `turn_agent`.

- [ ] **Step 1: Build the base executor at the top of `process_gateway_messages_loop`.** Just before the `while let Some(msg) = messages.recv().await {` line (~5646), add:

```rust
    // Base team executor: teammates are built from the instance template (the
    // default agent carries the shared runtime). Repositioned per message via for_lead.
    let team_base = {
        let template = std::sync::Arc::new(agent_arc.lock().await.clone());
        std::sync::Arc::new(hakimi_core::PersonaTeamExecutor::new(
            persona_registry.clone(),
            template,
            128_000,
        ))
    };
```

- [ ] **Step 2: Inject onto the turn agent.** Immediately after `let mut a = base_agent.lock().await.clone();` (line 6289), add:

```rust
                a.set_team_executor(Some(std::sync::Arc::new(team_base.for_lead(&persona_id))));
```

(Confirm `persona_id` is in scope here — it is resolved earlier in the loop body; if the binding is named differently at this exact point, use the resolved persona id variable used for `persona_agents.read().await.get(&persona_id)` at line 5806.)

- [ ] **Step 3: Build**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" build -p hakimi-cli`
Expected: compiles. `PersonaTeamExecutor` is `Arc<dyn TeamExecutor>` via `set_team_executor`; the `for_lead` clone is cheap.

- [ ] **Step 4: Clippy (CI parity)**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" clippy -p hakimi-cli --all-targets --all-features`
Expected: no warnings (`-Dwarnings`).

- [ ] **Step 5: Commit**

```bash
git add crates/hakimi-cli/src/entry.rs
git commit -m "feat(gateway): inject PersonaTeamExecutor into per-message lead agent"
```

---

### Task 8: Inject the executor into the WebUI streaming chat

**Files:**
- Modify: `crates/hakimi-server/src/api.rs` (in `agent_chat_stream`, around line 3834 where `cloned_agent` is built)

This makes `team` work when the user drives a persona from the WebUI console.

- [ ] **Step 1: After `cloned_agent` is constructed** (line ~3834, the `let mut cloned_agent = if is_default { ... }` block) and before it runs, add:

```rust
    {
        let template = std::sync::Arc::new(state.agent.lock().await.clone());
        let team_base = hakimi_core::PersonaTeamExecutor::new(
            state.persona_registry.clone(),
            template,
            128_000,
        );
        let lead_id = if is_default {
            hakimi_core::DEFAULT_PERSONA_ID.to_string()
        } else {
            cfg.id.clone()
        };
        cloned_agent.set_team_executor(Some(std::sync::Arc::new(team_base.for_lead(&lead_id))));
    }
```

(Confirm `cfg` is the resolved `PersonaConfig` and `is_default` is in scope at this point — both are bound at the top of `agent_chat_stream` per the existing `let (cfg, skills_dir, is_default) = { ... }` at line 3801.)

- [ ] **Step 2: Build**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" build -p hakimi-server`
Expected: compiles.

- [ ] **Step 3: Run the existing agent-chat-stream test to confirm no regression**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-server test_agent_chat_stream_default_emits_sse`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/hakimi-server/src/api.rs
git commit -m "feat(server): inject PersonaTeamExecutor into agent_chat_stream"
```

---

## Phase 5 — `addressable` API + WebUI toggle

### Task 9: Expose `addressable` over the API

**Files:**
- Modify: `crates/hakimi-server/src/api.rs` (`AgentUpdateRequest` at line 258; `update_agent` merge near line 3493)

`list/get/create` already serialize `PersonaConfig` directly, so they expose `addressable` automatically once Task 2 landed. Only PATCH needs the field.

- [ ] **Step 1: Write the failing test.** Add to the `tests` module in `api.rs` (mirror `test_agent_skills_reflects_enabled`):

```rust
    #[tokio::test]
    async fn test_update_agent_toggles_addressable() {
        let state = test_state();
        create_test_agent(&state, "coder").await; // mirror the helper used by sibling tests

        let resp = update_agent(
            State(state.clone()),
            Path("coder".to_string()),
            Json(serde_json::from_value(json!({"addressable": false})).unwrap()),
        )
        .await
        .unwrap();
        assert!(!resp.0.addressable);
    }
```

(If sibling tests construct the update request differently, follow their exact pattern for building `AgentUpdateRequest` and calling `update_agent`; the assertion on `addressable == false` is the point.)

- [ ] **Step 2: Run to verify it fails**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-server test_update_agent_toggles_addressable`
Expected: FAIL (no field `addressable` on `AgentUpdateRequest`).

- [ ] **Step 3: Add the field + merge.** In `AgentUpdateRequest` (after `is_default: Option<bool>,` at line 267):

```rust
    addressable: Option<bool>,
```

In `update_agent`, after the `is_default` merge block (around line 3493-3495):

```rust
        if let Some(addressable) = req.addressable {
            cfg.addressable = addressable;
        }
```

- [ ] **Step 4: Run to verify it passes**

Run: `& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test -p hakimi-server test_update_agent_toggles_addressable`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/hakimi-server/src/api.rs
git commit -m "feat(server): PATCH /api/agents/{id} accepts addressable"
```

---

### Task 10: WebUI `addressable` toggle

**Files:**
- Modify: `hakimi-webui/src/api.ts` (`Agent` at line 259, `AgentUpdate` at line 277)
- Modify: `hakimi-webui/src/PersonaConfigForm.tsx`

- [ ] **Step 1: Add the field to the API types.** In `api.ts`, add to `Agent` (after `is_default: boolean;` line 269):

```typescript
  addressable: boolean;
```

and to `AgentUpdate` (after `is_default?: boolean;` line 286):

```typescript
  addressable?: boolean;
```

- [ ] **Step 2: Add state + control to the form.** In `PersonaConfigForm.tsx`:

After the `isDefault` state (line 32) add:

```typescript
  const [addressable, setAddressable] = useState(agent?.addressable ?? true);
```

In the `Model` fieldset, after the "Default persona" `switch-row` label (line 211), add:

```tsx
          <label className="switch-row">
            <span>Allow other agents to consult this persona (team)</span>
            <input
              type="checkbox"
              checked={addressable}
              onChange={(e) => setAddressable(e.target.checked)}
            />
          </label>
```

In `handleSubmit`, add `addressable,` to BOTH the `updateAgent` payload (after `is_default: isDefault,` line 99) and the `createAgent` payload (after `is_default: isDefault,` line 118):

```typescript
          is_default: isDefault,
          addressable,
```

- [ ] **Step 3: Lint + build the WebUI**

Run (from `hakimi-webui/`): `npm run lint` then `npm run build`
Expected: clean lint, successful `tsc` + `vite` build.

- [ ] **Step 4: Rebuild the embedded bundle.** The running binary serves `crates/hakimi-webui/static/`. Per the handoff, after editing the React app run the build that emits into `crates/hakimi-webui/static/` (fixed names `app.js`/`app.css`) and commit those artifacts so the binary ships the change. Confirm the `vite.config.ts` `outDir`/`base` already target `crates/hakimi-webui/static/` and `/static/`.

- [ ] **Step 5: Commit**

```bash
git add hakimi-webui/src/api.ts hakimi-webui/src/PersonaConfigForm.tsx crates/hakimi-webui/static/
git commit -m "feat(webui): addressable toggle on persona config form"
```

---

## Final Verification

- [ ] **Full CI-parity gate (Docker):**

```
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" fmt --all -- --check
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" clippy --workspace --all-targets --all-features
& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" test --workspace --all-features
```

Expected: fmt clean, clippy no warnings (`-Dwarnings`), all tests pass.

- [ ] **Manual smoke (optional):** create two personas (e.g. `coder`, `writer`, both addressable), talk to `coder` and ask it to "use the team tool to ask writer to draft X"; confirm a teammate progress bubble appears and writer's answer is folded into coder's reply.

---

## Self-Review (completed during planning)

**Spec coverage:** §2.1 items 1-8 all mapped — addressable (Task 2/9/10), TeamExecutor+ToolContext (Task 1), PersonaTeamExecutor (Task 4), team tool (Task 5), gateway/server wiring (Tasks 6-8), progress bubbles (Task 4 `emit_team_progress`), guards (Task 4), WebUI toggle (Task 10). §6 guards: depth (`MAX_TEAM_DEPTH`), cycle (`lineage`), concurrency (`Semaphore`), addressable gate, graceful degradation (tool returns "not enabled"). §7 observability: `hakimi_delegate:` protocol reused.

**Type consistency:** `TeamExecutor` / `TeammateInfo` / `TeamCallContext` defined in Task 1 are used identically in Tasks 4-5; `set_team_executor`/`team_executor` consistent across Tasks 1, 3, 7, 8; `addressable` field name consistent across persona.rs, api.rs, api.ts, form.

**Placeholders:** Wiring tasks (7, 8, 9) note "confirm variable X is in scope" where exact local bindings could not be verified line-for-line; the surrounding code is cited so the executor can confirm. These are verification notes, not unimplemented logic. No "TODO/TBD" remain.

**Deferred (per spec §2.2, intentionally not in this plan):** async `send_message(agent:<id>)`, stateful team sessions, team-orchestration workflow + WebUI team view, teammate memory write-back.
