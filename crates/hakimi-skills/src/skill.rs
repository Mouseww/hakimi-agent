use serde::Deserialize;

/// Coarse harness phase used to decide when a skill should be active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessPhase {
    Analyze,
    Plan,
    Implement,
    Validate,
    Review,
    Summarize,
}

impl HarnessPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::Plan => "plan",
            Self::Implement => "implement",
            Self::Validate => "validate",
            Self::Review => "review",
            Self::Summarize => "summarize",
        }
    }

    /// Lightweight heuristic classifier for the current run phase.
    pub fn classify(text: &str) -> Self {
        let lower = text.to_lowercase();
        if contains_any(
            &lower,
            &[
                "test",
                "cargo check",
                "cargo clippy",
                "validate",
                "验证",
                "测试",
                "报错",
                "failed",
                "error[",
            ],
        ) {
            Self::Validate
        } else if contains_any(
            &lower,
            &["review", "diff", "risk", "风险", "审查", "检查修改"],
        ) {
            Self::Review
        } else if contains_any(
            &lower,
            &[
                "implement",
                "patch",
                "write",
                "edit",
                "modify",
                "修改",
                "实现",
                "落地",
                "补丁",
            ],
        ) {
            Self::Implement
        } else if contains_any(&lower, &["plan", "design", "方案", "设计", "架构"]) {
            Self::Plan
        } else if contains_any(&lower, &["summary", "summarize", "总结", "沉淀", "memory"]) {
            Self::Summarize
        } else {
            Self::Analyze
        }
    }
}

fn blank_string_as_none(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let mut normalized = String::new();
        let mut last_was_separator = false;
        for ch in value.chars() {
            let is_safe =
                ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/' | ':' | '@');
            if is_safe {
                normalized.push(ch);
                last_was_separator = false;
            } else if !last_was_separator {
                normalized.push('-');
                last_was_separator = true;
            }
        }

        let trimmed = normalized.trim_matches('-');
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.chars().take(120).collect())
        }
    })
}

/// Source metadata for a loaded skill.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct SkillProvenance {
    /// Hub/source adapter that installed the skill, for example `official` or `github`.
    #[serde(default)]
    pub source: Option<String>,

    /// Source-specific identifier, such as an official skill path or repo path.
    #[serde(default)]
    pub identifier: Option<String>,

    /// Trust classification carried by the source: `builtin`, `trusted`, or `community`.
    #[serde(default)]
    pub trust_level: Option<String>,

    /// Optional upstream repository for hub-installed skills.
    #[serde(default)]
    pub repo: Option<String>,

    /// Authoring origin, usually `user`, `agent`, or a human contributor id.
    #[serde(default)]
    pub created_by: Option<String>,
}

impl SkillProvenance {
    pub(crate) fn normalize(&mut self) {
        self.source = blank_string_as_none(self.source.take());
        self.identifier = blank_string_as_none(self.identifier.take());
        self.trust_level = blank_string_as_none(self.trust_level.take());
        self.repo = blank_string_as_none(self.repo.take());
        self.created_by = blank_string_as_none(self.created_by.take());
    }

    fn merge_missing(&mut self, other: &SkillProvenance) {
        if self.source.is_none() {
            self.source = other.source.clone();
        }
        if self.identifier.is_none() {
            self.identifier = other.identifier.clone();
        }
        if self.trust_level.is_none() {
            self.trust_level = other.trust_level.clone();
        }
        if self.repo.is_none() {
            self.repo = other.repo.clone();
        }
        if self.created_by.is_none() {
            self.created_by = other.created_by.clone();
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SkillMetadata {
    #[serde(default)]
    pub hermes: SkillProvenance,
}

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
/// phases:
///   - analyze
///   - validate
/// ttl_steps: 5
/// max_context_chars: 1200
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

    /// Harness phases where this skill is most useful.
    #[serde(default)]
    pub phases: Vec<HarnessPhase>,

    /// How many run steps a stale skill may remain before eviction.
    #[serde(default = "default_ttl_steps")]
    pub ttl_steps: u32,

    /// Optional per-skill rendering budget in characters.
    #[serde(default)]
    pub max_context_chars: Option<usize>,

    /// Provenance and trust metadata parsed from skill frontmatter.
    #[serde(default)]
    pub provenance: SkillProvenance,

    /// Hermes-compatible nested metadata block.
    #[serde(default)]
    pub metadata: SkillMetadata,
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
            phases: Vec::new(),
            ttl_steps: default_ttl_steps(),
            max_context_chars: None,
            provenance: SkillProvenance::default(),
            metadata: SkillMetadata::default(),
        }
    }

    /// Normalize and merge top-level and Hermes-style metadata after deserialization.
    pub fn normalize_metadata(&mut self) {
        self.provenance.normalize();
        self.metadata.hermes.normalize();
        self.provenance.merge_missing(&self.metadata.hermes);
    }

    pub fn merge_provenance(&mut self, provenance: &SkillProvenance) {
        self.provenance.merge_missing(provenance);
        self.provenance.normalize();
    }

    pub fn provenance_label(&self) -> String {
        let source = self.provenance.source.as_deref().unwrap_or("local");
        let trust = self.provenance.trust_level.as_deref().unwrap_or("local");
        match self.provenance.identifier.as_deref() {
            Some(identifier) => format!("{source}/{trust} ({identifier})"),
            None => format!("{source}/{trust}"),
        }
    }

    /// Check if this skill matches a given query string.
    ///
    /// Matching is done by checking if the query contains any of the skill's
    /// tags, name words, or trigger keywords.
    pub fn matches_query(&self, query: &str) -> bool {
        self.relevance_score(query, HarnessPhase::Analyze) > 0.0
    }

    /// Score how relevant this skill is for the current message and phase.
    pub fn relevance_score(&self, query: &str, phase: HarnessPhase) -> f32 {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let mut score = 0.0;

        if !self.phases.is_empty() && self.applies_to_phase(phase) {
            score += 0.35;
        }

        for tag in &self.tags {
            let tag = tag.to_lowercase();
            if !tag.is_empty() && query_lower.contains(&tag) {
                score += 0.45;
            }
        }

        for word in self.name.split(|c: char| !c.is_alphanumeric()) {
            let word = word.to_lowercase();
            if word.len() >= 3 && query_lower.contains(&word) {
                score += 0.35;
            }
        }

        if let Some(ref trigger) = self.trigger {
            for word in trigger.split(|c: char| !c.is_alphanumeric()) {
                let word_lower = word.to_lowercase();
                if word_lower.len() >= 3 && query_words.iter().any(|qw| qw.contains(&word_lower)) {
                    score += 0.25;
                }
            }
        }

        if score == 0.35 && !self.phases.is_empty() {
            // Phase-only matches are weak but useful as fallback skills.
            score = 0.1;
        }

        score
    }

    pub fn applies_to_phase(&self, phase: HarnessPhase) -> bool {
        self.phases.is_empty() || self.phases.contains(&phase)
    }

    pub fn context_cost(&self) -> usize {
        self.render_body_capped().len()
    }

    pub fn render_body_capped(&self) -> String {
        self.cap_text(
            &self.content,
            self.max_context_chars.unwrap_or(self.content.len()),
        )
    }

    pub fn cap_text(&self, text: &str, limit: usize) -> String {
        text.chars().take(limit).collect()
    }

    pub fn summary(&self) -> String {
        let tags = if self.tags.is_empty() {
            String::new()
        } else {
            format!(" Tags: {}.", self.tags.join(", "))
        };
        format!(
            "- {}: {}{} Source: {}.",
            self.name,
            self.description,
            tags,
            self.provenance_label()
        )
    }

    pub fn checklist(&self) -> String {
        let mut lines = Vec::new();
        for line in self.content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('-')
                || trimmed.starts_with('*')
                || trimmed.chars().next().is_some_and(|c| c.is_ascii_digit())
                || trimmed.starts_with('#')
            {
                lines.push(trimmed.to_string());
            }
            if lines.join("\n").len() >= self.max_context_chars.unwrap_or(1_200).min(1_200) {
                break;
            }
        }

        let limit = self.max_context_chars.unwrap_or(1_200).min(1_200);
        if lines.is_empty() {
            self.cap_text(&self.content, limit)
        } else {
            self.cap_text(&lines.join("\n"), limit)
        }
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn default_ttl_steps() -> u32 {
    4
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
            phases: Vec::new(),
            ttl_steps: default_ttl_steps(),
            max_context_chars: None,
            provenance: SkillProvenance::default(),
            metadata: SkillMetadata::default(),
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
            phases: Vec::new(),
            ttl_steps: default_ttl_steps(),
            max_context_chars: None,
            provenance: SkillProvenance::default(),
            metadata: SkillMetadata::default(),
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
            phases: Vec::new(),
            ttl_steps: default_ttl_steps(),
            max_context_chars: None,
            provenance: SkillProvenance::default(),
            metadata: SkillMetadata::default(),
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
            phases: Vec::new(),
            ttl_steps: default_ttl_steps(),
            max_context_chars: None,
            provenance: SkillProvenance::default(),
            metadata: SkillMetadata::default(),
        };
        // Name word "ai" is only 2 chars, should not match
        assert!(!skill.matches_query("the ai is here"));
    }

    #[test]
    fn test_skill_content_caps_are_strict() {
        let skill = Skill {
            name: "large".to_string(),
            description: "".to_string(),
            content: "# Long Header That Should Be Truncated\n- Another very long checklist item"
                .to_string(),
            trigger: None,
            tags: vec![],
            phases: Vec::new(),
            ttl_steps: default_ttl_steps(),
            max_context_chars: Some(10),
            provenance: SkillProvenance::default(),
            metadata: SkillMetadata::default(),
        };

        assert_eq!(skill.render_body_capped().chars().count(), 10);
        assert_eq!(skill.checklist().chars().count(), 10);
    }
}
