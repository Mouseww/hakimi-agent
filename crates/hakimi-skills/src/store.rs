use std::path::Path;

use crate::skill::Skill;
use crate::loader::SkillLoader;

/// Store for managing and loading skills.
#[derive(Clone, Default)]
pub struct SkillStore {
    skills: Vec<Skill>,
}

impl SkillStore {
    /// Create a new empty skill store.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a new skill store and load skills from the given path.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let loader = SkillLoader::load_from_dir(path)?;
        Ok(Self {
            skills: loader.skills().to_vec(),
        })
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Get a summary of loaded skills.
    pub fn summary(&self) -> String {
        let mut summary = format!("🧠 Loaded {} skills:\n", self.skills.len());
        for skill in &self.skills {
            summary.push_str(&format!("  • {} — {}\n", skill.name, skill.description));
        }
        summary
    }

    /// Get system prompt additions based on a user message.
    pub fn get_system_prompt_additions(&self, _message: &str) -> String {
        // Simple implementation: return all skill prompt contents
        let mut additions = String::new();
        for skill in &self.skills {
            additions.push_str(&skill.content);
            additions.push_str("\n\n");
        }
        additions
    }
}
