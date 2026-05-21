use std::path::PathBuf;

use anyhow::Result;
use tracing::{debug, info};

use crate::loader::SkillLoader;
use crate::skill::Skill;

/// High-level skill store that wraps [`SkillLoader`] and provides
/// system prompt injection based on user message matching.
pub struct SkillStore {
    loader: SkillLoader,
}

impl SkillStore {
    /// Create a new SkillStore by loading skills from the default location
    /// (`~/.hakimi/skills/`).
    pub fn load_default() -> Result<Self> {
        let dir = default_skills_dir();
        Self::load_from_dir(&dir)
    }

    /// Create a new SkillStore by loading skills from the given directory.
    pub fn load_from_dir(dir: &std::path::Path) -> Result<Self> {
        let loader = SkillLoader::load_from_dir(dir)?;
        info!(count = loader.skills().len(), path = %dir.display(), "SkillStore loaded");
        Ok(Self { loader })
    }

    /// Create an empty SkillStore (no skills loaded).
    pub fn empty() -> Self {
        Self {
            loader: SkillLoader::new(),
        }
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> &[Skill] {
        self.loader.skills()
    }

    /// Get system prompt additions based on the user's message.
    ///
    /// Searches loaded skills for ones that match the user's message and
    /// returns their content formatted for injection into the system prompt.
    /// Returns an empty string if no skills match.
    pub fn get_system_prompt_additions(&self, user_message: &str) -> String {
        let matching = self.loader.find_matching(user_message);

        if matching.is_empty() {
            return String::new();
        }

        debug!(
            count = matching.len(),
            "Found matching skills for user message"
        );

        let mut output = String::new();
        for skill in &matching {
            output.push_str(&format!("### Skill: {}\n{}\n\n", skill.name, skill.content));
        }

        output
    }

    /// Get a summary of all loaded skills (for display purposes).
    pub fn summary(&self) -> String {
        let skills = self.loader.skills();
        if skills.is_empty() {
            return "No skills loaded.".to_string();
        }

        let mut out = format!("Loaded {} skill(s):\n", skills.len());
        for skill in skills {
            let desc = if skill.description.is_empty() {
                ""
            } else {
                &skill.description
            };
            let tags = if skill.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", skill.tags.join(", "))
            };
            out.push_str(&format!("  • {}{} — {}\n", skill.name, tags, desc));
        }
        out
    }
}

/// Get the default skills directory path (~/.hakimi/skills/).
pub fn default_skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/root"))
        .join(".hakimi")
        .join("skills")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_skill(dir: &std::path::Path, filename: &str, content: &str) {
        let path = dir.join(filename);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_get_system_prompt_additions() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path();

        write_skill(
            dir_path,
            "rust.md",
            r#"---
name: rust-helper
description: Helps with Rust
tags:
  - rust
---
Use `cargo check` before committing."#,
        );

        let store = SkillStore::load_from_dir(dir_path).unwrap();

        let additions = store.get_system_prompt_additions("help me with rust");
        assert!(additions.contains("rust-helper"));
        assert!(additions.contains("cargo check"));

        let additions = store.get_system_prompt_additions("python code");
        assert!(additions.is_empty());
    }

    #[test]
    fn test_summary() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path();

        write_skill(
            dir_path,
            "skill1.md",
            r#"---
name: my-skill
description: Does things
tags:
  - code
---
Content"#,
        );

        let store = SkillStore::load_from_dir(dir_path).unwrap();
        let summary = store.summary();
        assert!(summary.contains("my-skill"));
        assert!(summary.contains("Does things"));
        assert!(summary.contains("code"));
    }

    #[test]
    fn test_empty_store() {
        let store = SkillStore::empty();
        assert!(store.skills().is_empty());
        assert_eq!(store.summary(), "No skills loaded.");
        assert!(store.get_system_prompt_additions("anything").is_empty());
    }
}
