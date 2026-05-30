use std::path::Path;

use crate::lifecycle::SkillWorkingSet;
use crate::loader::SkillLoader;
use crate::preprocessing::SkillPreprocessOptions;
use crate::skill::Skill;
use crate::usage::SkillUsageStore;

/// Store for managing and loading skills.
///
/// The store owns the skill library and a per-agent runtime working set. The
/// working set dynamically activates, downgrades and evicts skills so skills do
/// not accumulate permanently in the system prompt.
#[derive(Clone, Default)]
pub struct SkillStore {
    skills: Vec<Skill>,
    working_set: SkillWorkingSet,
    usage_store: Option<SkillUsageStore>,
}

impl SkillStore {
    /// Create a new empty skill store.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a store from in-memory skills. Intended for tests and embedded
    /// callers that already have a loaded skill library.
    pub fn from_skills(skills: Vec<Skill>) -> Self {
        Self {
            skills,
            working_set: SkillWorkingSet::new(),
            usage_store: None,
        }
    }

    /// Create a new skill store and load skills from the given path.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let loader = SkillLoader::load_from_dir(path)?;
        Ok(Self {
            skills: loader.skills().to_vec(),
            working_set: SkillWorkingSet::new(),
            usage_store: Some(SkillUsageStore::new(path)),
        })
    }

    /// Create a new skill store with explicit SKILL.md preprocessing options.
    pub fn load_with_options(
        path: &Path,
        preprocess_options: SkillPreprocessOptions,
    ) -> anyhow::Result<Self> {
        let loader = SkillLoader::load_from_dir_with_options(path, preprocess_options)?;
        Ok(Self {
            skills: loader.skills().to_vec(),
            working_set: SkillWorkingSet::new(),
            usage_store: Some(SkillUsageStore::new(path)),
        })
    }

    /// Create a child-local skill store for a delegated subtask.
    ///
    /// This clones the skill library but starts with a fresh working set, then
    /// seeds that working set from the subtask goal/context. It intentionally
    /// avoids sharing the parent's active/evicted runtime state.
    pub fn fork_for_subtask(&self, seed_text: &str) -> Self {
        let mut fork = Self {
            skills: self.skills.clone(),
            working_set: SkillWorkingSet::new(),
            usage_store: self.usage_store.clone(),
        };
        fork.observe(seed_text);
        fork
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Get a reference to the runtime working set.
    pub fn working_set(&self) -> &SkillWorkingSet {
        &self.working_set
    }

    /// Get a mutable reference to the runtime working set.
    pub fn working_set_mut(&mut self) -> &mut SkillWorkingSet {
        &mut self.working_set
    }

    /// Observe a user/model/tool event and refresh the active skill working set.
    pub fn observe(&mut self, text: &str) {
        self.working_set.observe(text, &self.skills);
        self.record_active_skill_uses();
    }

    /// Observe a tool result and refresh skill lifecycle state.
    pub fn observe_tool_result(&mut self, text: &str) {
        self.working_set.observe_tool_result(text, &self.skills);
        self.record_active_skill_uses();
    }

    pub fn record_use(&self, skill_name: &str) {
        if let Some(usage_store) = &self.usage_store {
            usage_store.bump_use(skill_name);
        }
    }

    pub fn record_view(&self, skill_name: &str) {
        if let Some(usage_store) = &self.usage_store {
            usage_store.bump_view(skill_name);
        }
    }

    fn record_active_skill_uses(&self) {
        if let Some(usage_store) = &self.usage_store {
            usage_store.bump_uses(&self.working_set.active_skill_names());
        }
    }

    /// Get a summary of loaded skills.
    pub fn summary(&self) -> String {
        let mut summary = format!("🧠 Loaded {} skills:\n", self.skills.len());
        for skill in &self.skills {
            summary.push_str(&format!(
                "  • {} — {} [{}]\n",
                skill.name,
                skill.description,
                skill.provenance_label()
            ));
        }
        summary
    }

    /// Render currently active runtime skills for injection into the current
    /// system prompt. This is dynamic and may return fewer or no skills after
    /// pruning/eviction.
    pub fn render_active_skill_context(&self) -> String {
        self.working_set.render_context()
    }

    /// Get system prompt additions based on a user message.
    ///
    /// Backwards-compatible API: it now updates and renders the dynamic working
    /// set instead of returning every skill in the library.
    pub fn get_system_prompt_additions(&mut self, message: &str) -> String {
        self.observe(message);
        self.render_active_skill_context()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::{HarnessPhase, Skill};
    use tempfile::TempDir;

    fn skill(name: &str, tags: &[&str], phases: Vec<HarnessPhase>) -> Skill {
        Skill {
            name: name.to_string(),
            description: format!("{name} description"),
            content: format!("# {name}\n- Use this skill"),
            trigger: None,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            phases,
            platforms: Vec::new(),
            ttl_steps: 4,
            max_context_chars: None,
            provenance: crate::skill::SkillProvenance::default(),
            metadata: crate::skill::SkillMetadata::default(),
        }
    }

    #[test]
    fn fork_for_subtask_uses_fresh_working_set() {
        let mut parent = SkillStore::from_skills(vec![
            skill("rust-debugging", &["rust"], vec![HarnessPhase::Analyze]),
            skill("python-testing", &["python"], vec![HarnessPhase::Validate]),
        ]);

        parent.observe("rust cargo test failed");
        let parent_active = parent.working_set().active_skill_names();
        assert!(parent_active.contains(&"rust-debugging".to_string()));

        let child = parent.fork_for_subtask("python test failed");
        let child_active = child.working_set().active_skill_names();

        assert!(parent_active.contains(&"rust-debugging".to_string()));
        assert!(child_active.contains(&"python-testing".to_string()));
        assert!(!child_active.contains(&"rust-debugging".to_string()));
    }

    #[test]
    fn loaded_store_records_runtime_skill_use() {
        let tmp = TempDir::new().unwrap();
        let skill_path = tmp.path().join("release.md");
        std::fs::write(
            skill_path,
            r#"---
name: release-check
tags:
  - release
---
# Release
Check release evidence."#,
        )
        .unwrap();
        let mut store = SkillStore::load(tmp.path()).unwrap();

        store.observe("please review the release");

        let usage = crate::usage::SkillUsageStore::new(tmp.path()).report();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].name, "release-check");
        assert_eq!(usage[0].record.use_count, 1);
    }
}
