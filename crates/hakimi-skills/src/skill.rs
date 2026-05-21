use serde::Deserialize;

/// A skill is a reusable prompt template stored as a markdown file with YAML frontmatter.
///
/// The file format is:
/// ```markdown
/// ---
/// name: my-skill
/// description: Does something useful
/// trigger: when the user asks about X
/// tags:
///   - coding
///   - testing
/// ---
/// # Skill Content
/// Here is the actual skill content...
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct Skill {
    /// Unique name for the skill.
    #[serde(default)]
    pub name: String,

    /// Short description of what the skill does.
    #[serde(default)]
    pub description: String,

    /// The actual skill content (everything after the frontmatter).
    #[serde(skip)]
    pub content: String,

    /// Optional trigger condition — when to load this skill.
    /// Can be a natural language description or keywords.
    #[serde(default)]
    pub trigger: Option<String>,

    /// Tags for categorization and matching.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Skill {
    /// Create a new skill with the given name and content.
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            content: content.into(),
            trigger: None,
            tags: Vec::new(),
        }
    }

    /// Check if this skill matches a given query string.
    ///
    /// Matching is done by checking if the query contains any of the skill's
    /// tags, name words, or trigger keywords.
    pub fn matches_query(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        // Check tags
        for tag in &self.tags {
            if query_lower.contains(&tag.to_lowercase()) {
                return true;
            }
        }

        // Check name words
        for word in self.name.split(|c: char| !c.is_alphanumeric()) {
            if word.len() >= 3 && query_lower.contains(&word.to_lowercase()) {
                return true;
            }
        }

        // Check trigger keywords
        if let Some(ref trigger) = self.trigger {
            for word in trigger.split_whitespace() {
                let word_lower = word.to_lowercase();
                if word_lower.len() >= 3 && query_words.iter().any(|qw| qw.contains(&word_lower)) {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_matches_by_tag() {
        let skill = Skill {
            name: "rust-helper".to_string(),
            description: "Helps with Rust".to_string(),
            content: "...".to_string(),
            trigger: None,
            tags: vec!["rust".to_string(), "cargo".to_string()],
        };
        assert!(skill.matches_query("help me with rust ownership"));
        assert!(skill.matches_query("cargo build error"));
        assert!(!skill.matches_query("python code"));
    }

    #[test]
    fn test_skill_matches_by_name() {
        let skill = Skill {
            name: "docker-helper".to_string(),
            description: "".to_string(),
            content: "...".to_string(),
            trigger: None,
            tags: vec![],
        };
        assert!(skill.matches_query("help me with docker"));
        assert!(!skill.matches_query("help me with kubernetes"));
    }

    #[test]
    fn test_skill_matches_by_trigger() {
        let skill = Skill {
            name: "git".to_string(),
            description: "".to_string(),
            content: "...".to_string(),
            trigger: Some("version control commit push pull branch".to_string()),
            tags: vec![],
        };
        assert!(skill.matches_query("how to commit changes"));
        assert!(skill.matches_query("push to remote"));
    }

    #[test]
    fn test_skill_no_match_short_words() {
        let skill = Skill {
            name: "ai".to_string(),
            description: "".to_string(),
            content: "...".to_string(),
            trigger: None,
            tags: vec![],
        };
        // Name word "ai" is only 2 chars, should not match
        assert!(!skill.matches_query("the ai is here"));
    }
}
