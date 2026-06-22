//! Live per-persona agent construction.
//!
//! A persona's agent is built by cloning a template agent (which carries the
//! instance-wide [`SharedRuntime`](crate::SharedRuntime) via `Arc`) and then
//! overriding the per-persona state: model, system prompt, a fresh context
//! engine (so each persona tracks its own compression state), and the persona's
//! own skill set. The heavy shared resources (transport, tools, knowledge,
//! embeddings) stay shared through the cloned `Arc<SharedRuntime>`.
//!
//! Design: `docs/superpowers/specs/2026-06-22-multi-agent-isolation-and-webui-design.md`.

use std::path::Path;

use hakimi_skills::SkillStore;

use crate::AIAgent;
use crate::persona::PersonaConfig;

/// Build an isolated agent for `cfg` from a `template` agent.
///
/// The returned agent shares the template's [`SharedRuntime`](crate::SharedRuntime)
/// (same `Arc`) but has its own model, system prompt, context engine, and skill
/// store. When `cfg.model` is empty the template's model is kept; when
/// `cfg.system_prompt` is empty the template's prompt is kept. Skills are loaded
/// from `skills_dir`, falling back to an empty store when the directory is
/// absent or unreadable.
pub fn build_persona_agent(
    template: &AIAgent,
    cfg: &PersonaConfig,
    skills_dir: &Path,
    context_length: usize,
) -> AIAgent {
    let model = if cfg.model.trim().is_empty() {
        template.model().to_string()
    } else {
        cfg.model.clone()
    };

    // Each persona gets its own context engine instance so compression state is
    // not shared across personas. It reuses the shared transport for LLM-backed
    // summarization (Tier 2).
    let context_engine = hakimi_context::build_context_engine(
        "llm",
        context_length,
        Some(&model),
        Some(template.shared.transport.clone()),
    );

    let skills = SkillStore::load(skills_dir).unwrap_or_else(|_| SkillStore::empty());

    let mut agent = template.clone();
    agent.set_model(&model);
    if !cfg.system_prompt.trim().is_empty() {
        agent.set_system_prompt(&cfg.system_prompt);
    }
    agent
        .with_context_engine(context_engine)
        .with_skill_store(Some(skills))
}

/// A live persona: its config plus its base agent.
///
/// Per request, callers clone [`PersonaRuntime::agent`] and restore the relevant
/// session before running, mirroring the existing single-agent WebUI flow.
pub struct PersonaRuntime {
    pub config: PersonaConfig,
    pub agent: AIAgent,
}

impl PersonaRuntime {
    /// Build a live persona runtime from a template agent and a persona config.
    pub fn build(
        template: &AIAgent,
        config: PersonaConfig,
        skills_dir: &Path,
        context_length: usize,
    ) -> Self {
        let agent = build_persona_agent(template, &config, skills_dir, context_length);
        Self { config, agent }
    }
}
