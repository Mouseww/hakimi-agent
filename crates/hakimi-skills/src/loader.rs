use std::path::Path;

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::skill::Skill;

/// Loads skills from a directory of markdown files with YAML frontmatter.
pub struct SkillLoader {
    skills: Vec<Skill>,
}

impl SkillLoader {
    /// Create a new empty SkillLoader.
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    /// Load all `.md` files from the given directory as skills.
    ///
    /// Each file is expected to have YAML frontmatter delimited by `---`.
    /// The frontmatter must contain at least a `name` field. The rest of the
    /// file becomes the skill's content.
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let mut loader = Self::new();

        if !dir.exists() {
            debug!(path = %dir.display(), "Skills directory does not exist");
            return Ok(loader);
        }

        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("failed to read skills directory: {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            match path.extension().and_then(|e| e.to_str()) {
                Some("md") => {}
                _ => continue,
            }

            match loader.load_file(&path) {
                Ok(skill) => {
                    debug!(name = %skill.name, path = %path.display(), "Loaded skill");
                    loader.skills.push(skill);
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to load skill file");
                }
            }
        }

        debug!(count = loader.skills.len(), "Loaded skills from directory");
        Ok(loader)
    }

    /// Load a single skill file.
    fn load_file(&self, path: &Path) -> Result<Skill> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read skill file: {}", path.display()))?;

        parse_skill(&raw, path)
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Find skills that match the given query.
    pub fn find_matching(&self, query: &str) -> Vec<&Skill> {
        self.skills.iter().filter(|s| s.matches_query(query)).collect()
    }
}

impl Default for SkillLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a skill from raw markdown content with YAML frontmatter.
///
/// The format is:
/// ```markdown
/// ---
/// name: my-skill
/// description: Does something useful
/// trigger: when the user asks about X
/// tags:
///   - coding
///   - testing
/// ---
/// # Content starts here
/// ```
///
/// If there's no frontmatter, the filename (without extension) is used as the name
/// and the entire content becomes the skill body.
fn parse_skill(raw: &str, path: &Path) -> Result<Skill> {
    let trimmed = raw.trim();

    // Look for frontmatter delimited by --- on their own lines
    if trimmed.starts_with("---") {
        if let Some(end) = trimmed[3..].find("\n---") {
            let frontmatter_end = 3 + end;
            let yaml_content = &trimmed[3..frontmatter_end].trim();
            let body_start = frontmatter_end + 4; // skip past "\n---"

            let mut skill: Skill = serde_yaml::from_str(yaml_content)
                .with_context(|| format!("invalid YAML frontmatter in {}", path.display()))?;

            // Use filename as fallback name
            if skill.name.is_empty() {
                skill.name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unnamed")
                    .to_string();
            }

            skill.content = trimmed[body_start..].trim().to_string();
            return Ok(skill);
        }
    }

    // No frontmatter — use filename as name, entire content as body
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed")
        .to_string();

    Ok(Skill {
        name,
        description: String::new(),
        content: trimmed.to_string(),
        trigger: None,
        tags: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_skill(dir: &Path, filename: &str, content: &str) -> PathBuf {
        let path = dir.join(filename);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_parse_skill_with_frontmatter() {
        let raw = r#"---
name: test-skill
description: A test skill
trigger: when testing
tags:
  - test
  - example
---
# Test Skill
This is the content."#;

        let path = Path::new("test-skill.md");
        let skill = parse_skill(raw, path).unwrap();

        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill");
        assert_eq!(skill.trigger, Some("when testing".to_string()));
        assert_eq!(skill.tags, vec!["test", "example"]);
        assert!(skill.content.contains("# Test Skill"));
        assert!(skill.content.contains("This is the content."));
    }

    #[test]
    fn test_parse_skill_without_frontmatter() {
        let raw = "# Simple Skill\nJust some content.";
        let path = Path::new("simple.md");
        let skill = parse_skill(raw, path).unwrap();

        assert_eq!(skill.name, "simple");
        assert_eq!(skill.description, "");
        assert_eq!(skill.trigger, None);
        assert!(skill.tags.is_empty());
        assert_eq!(skill.content, "# Simple Skill\nJust some content.");
    }

    #[test]
    fn test_parse_skill_frontmatter_fallback_name() {
        let raw = r#"---
description: No name provided
---
Content here"#;

        let path = Path::new("fallback-name.md");
        let skill = parse_skill(raw, path).unwrap();
        assert_eq!(skill.name, "fallback-name");
    }

    #[test]
    fn test_parse_skill_invalid_yaml() {
        let raw = r#"---
name: [invalid yaml
---
Content"#;

        let path = Path::new("bad.md");
        let result = parse_skill(raw, path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_dir() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path();

        write_skill(
            dir_path,
            "skill1.md",
            r#"---
name: skill-one
description: First skill
tags:
  - coding
---
Content one"#,
        );

        write_skill(
            dir_path,
            "skill2.md",
            r#"---
name: skill-two
description: Second skill
tags:
  - testing
---
Content two"#,
        );

        // Non-md file should be ignored
        write_skill(dir_path, "readme.txt", "not a skill");

        let loader = SkillLoader::load_from_dir(dir_path).unwrap();
        assert_eq!(loader.skills().len(), 2);

        let names: Vec<&str> = loader.skills().iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"skill-one"));
        assert!(names.contains(&"skill-two"));
    }

    #[test]
    fn test_load_from_nonexistent_dir() {
        let loader = SkillLoader::load_from_dir(Path::new("/nonexistent/path")).unwrap();
        assert_eq!(loader.skills().len(), 0);
    }

    #[test]
    fn test_find_matching() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path();

        write_skill(
            dir_path,
            "rust.md",
            r#"---
name: rust-helper
tags:
  - rust
  - cargo
---
Rust help"#,
        );

        write_skill(
            dir_path,
            "python.md",
            r#"---
name: python-helper
tags:
  - python
  - pip
---
Python help"#,
        );

        let loader = SkillLoader::load_from_dir(dir_path).unwrap();

        let matching = loader.find_matching("help me with rust ownership");
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].name, "rust-helper");

        let matching = loader.find_matching("python pip install");
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].name, "python-helper");

        let matching = loader.find_matching("javascript code");
        assert_eq!(matching.len(), 0);
    }
}
