//! Persona team collaboration: delegate sub-tasks to named teammate personas.
//!
//! `PersonaTeamExecutor` implements [`hakimi_common::TeamExecutor`]. A consultation
//! validates the target is addressable and within depth/cycle limits, builds the
//! teammate's agent via [`crate::build_persona_agent`] (sharing the instance
//! SharedRuntime), runs one bounded turn, and returns the teammate's answer.
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
    let cfg = reg
        .get(teammate_id)
        .ok_or_else(|| HakimiError::Tool(format!("teammate persona '{teammate_id}' not found")))?;
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

/// Executes teammate consultations for one agent. Cheap to clone (Arc-backed);
/// `for_lead`/`descend` produce repositioned instances for nested collaboration.
#[derive(Clone)]
pub struct PersonaTeamExecutor {
    registry: Arc<RwLock<PersonaRegistry>>,
    /// Template agent carrying the instance SharedRuntime; teammates are built from it.
    template: Arc<AIAgent>,
    context_length: usize,
    depth: usize,
    lineage: Vec<String>,
    semaphore: Arc<Semaphore>,
}

impl PersonaTeamExecutor {
    /// Create a base executor (depth 0, empty lineage). Use [`Self::for_lead`] per request.
    ///
    /// The concurrency semaphore created here is Arc-cloned into every executor
    /// derived via [`Self::for_lead`] and [`Self::descend`], so
    /// `MAX_CONCURRENT_CONSULTS` is an instance-wide cap shared across all
    /// concurrently-running lead personas -- not a per-lead limit.
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
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3_600,
        (secs % 3_600) / 60,
        secs % 60
    )
}

fn truncate_for_title(value: &str, max: usize) -> String {
    let normalized = value.replace('\n', " ");
    let mut chars = normalized.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

/// Emit a progress bubble in the existing `hakimi_delegate:` protocol so gateway +
/// WebUI render teammate progress without new plumbing.
fn emit_team_progress(
    progress: &Option<ToolProgressCallback>,
    task_id: &str,
    title: &str,
    line: impl AsRef<str>,
) {
    if let Some(cb) = progress {
        cb(format!(
            "\u{001e}hakimi_delegate:{task_id}|{title}|{}|{}",
            line.as_ref(),
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
            "{} {} \u{00b7} {}",
            if cfg.avatar.is_empty() {
                "\u{1f91d}"
            } else {
                cfg.avatar.as_str()
            },
            if cfg.name.is_empty() {
                cfg.id.as_str()
            } else {
                cfg.name.as_str()
            },
            truncate_for_title(&call.task, 32)
        );
        emit_team_progress(
            &call.progress,
            &task_id,
            &title,
            "\u{5df2}\u{52a0}\u{5165}\u{534f}\u{4f5c}",
        );

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
                crate::build_persona_agent(&*self.template, &cfg, &skills_dir, self.context_length);
            teammate.set_session_id(task_id.clone());
            // Nested consults allowed up to the depth cap, with this teammate in the lineage.
            teammate.set_team_executor(Some(Arc::new(self.descend(&cfg.id))));

            // Forward the teammate's tool/review progress markers as team bubbles.
            if let Some(parent) = call.progress.clone() {
                let (tid, ttitle) = (task_id.clone(), title.clone());
                teammate.set_streaming_callback(Some(Arc::new(move |token: String| {
                    if let Some(notice) = token.strip_prefix("\u{001e}hakimi_tool:") {
                        emit_team_progress(&Some(parent.clone()), &tid, &ttitle, notice.trim());
                    } else if let Some(review_notice) = token.strip_prefix("\u{001e}hakimi_review:")
                    {
                        emit_team_progress(
                            &Some(parent.clone()),
                            &tid,
                            &ttitle,
                            review_notice.trim(),
                        );
                    }
                })));
            }

            match teammate.run_conversation(&seed).await {
                Ok(res) => {
                    emit_team_progress(
                        &call.progress,
                        &task_id,
                        &title,
                        "\u{5b8c}\u{6210}，\u{8fd4}\u{56de}\u{7ed3}\u{679c}",
                    );
                    return Ok(res.final_response);
                }
                Err(e) if attempt >= MAX_CONSULT_ATTEMPTS => {
                    emit_team_progress(
                        &call.progress,
                        &task_id,
                        &title,
                        format!("\u{5931}\u{8d25}: {e}"),
                    );
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
                        format!("\u{7b2c} {attempt} \u{6b21}\u{5931}\u{8d25}，\u{91cd}\u{8bd5}"),
                    );
                    tracing::warn!(error = %e, attempt, teammate = %cfg.id, "team consult retry");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    /// Run multiple teammate consultations concurrently on a single task.
    ///
    /// The consults run cooperatively via `futures::future::join_all` -- they are
    /// interleaved on the calling async task, not spawned onto separate OS threads.
    /// Concurrency is bounded by the shared semaphore (`MAX_CONCURRENT_CONSULTS`).
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
