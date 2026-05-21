use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Role {
    Coder,
    Researcher,
    Writer,
    Analyst,
    Tutor,
    Assistant,
    DevOps,
    Reviewer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleProfile {
    pub role: Role,
    pub name: String,
    pub description: String,
    pub system_prompt_suffix: String,
    pub preferred_tools: Vec<String>,
    pub tone: String,
    pub verbosity: String,
}

pub struct RoleAdapter {
    profiles: HashMap<Role, RoleProfile>,
    current_role: Role,
    transition_history: Vec<(Role, String, String)>,
}

impl RoleAdapter {
    pub fn new() -> Self {
        let mut profiles = HashMap::new();

        profiles.insert(
            Role::Coder,
            RoleProfile {
                role: Role::Coder,
                name: "Coder".to_string(),
                description: "Focused on writing, refactoring, and debugging code".to_string(),
                system_prompt_suffix: "You are in coding mode. Focus on writing clean, efficient code. \
                     Show complete implementations. Use appropriate error handling."
                    .to_string(),
                preferred_tools: vec![
                    "terminal".to_string(),
                    "write_file".to_string(),
                    "patch".to_string(),
                    "search_files".to_string(),
                ],
                tone: "technical".to_string(),
                verbosity: "concise".to_string(),
            },
        );

        profiles.insert(
            Role::Researcher,
            RoleProfile {
                role: Role::Researcher,
                name: "Researcher".to_string(),
                description: "Focused on gathering and analyzing information".to_string(),
                system_prompt_suffix: "You are in research mode. Provide thorough, well-sourced analysis. \
                     Compare options and present balanced viewpoints."
                    .to_string(),
                preferred_tools: vec![
                    "web_search".to_string(),
                    "read_file".to_string(),
                    "session_search".to_string(),
                ],
                tone: "analytical".to_string(),
                verbosity: "detailed".to_string(),
            },
        );

        profiles.insert(
            Role::Writer,
            RoleProfile {
                role: Role::Writer,
                name: "Writer".to_string(),
                description: "Focused on creating written content".to_string(),
                system_prompt_suffix: "You are in writing mode. Focus on clear, engaging prose. \
                     Pay attention to structure, flow, and audience."
                    .to_string(),
                preferred_tools: vec!["write_file".to_string(), "read_file".to_string()],
                tone: "creative".to_string(),
                verbosity: "verbose".to_string(),
            },
        );

        profiles.insert(
            Role::Analyst,
            RoleProfile {
                role: Role::Analyst,
                name: "Analyst".to_string(),
                description: "Focused on data analysis and insights".to_string(),
                system_prompt_suffix: "You are in analyst mode. Focus on data-driven insights. \
                     Present findings with supporting evidence and metrics."
                    .to_string(),
                preferred_tools: vec![
                    "terminal".to_string(),
                    "read_file".to_string(),
                    "code_exec".to_string(),
                ],
                tone: "precise".to_string(),
                verbosity: "detailed".to_string(),
            },
        );

        profiles.insert(
            Role::Tutor,
            RoleProfile {
                role: Role::Tutor,
                name: "Tutor".to_string(),
                description: "Focused on teaching and explaining concepts".to_string(),
                system_prompt_suffix: "You are in tutor mode. Explain concepts clearly with examples. \
                     Build understanding step by step. Encourage learning."
                    .to_string(),
                preferred_tools: vec!["read_file".to_string(), "web_search".to_string()],
                tone: "encouraging".to_string(),
                verbosity: "verbose".to_string(),
            },
        );

        profiles.insert(
            Role::Assistant,
            RoleProfile {
                role: Role::Assistant,
                name: "Assistant".to_string(),
                description: "General-purpose assistant".to_string(),
                system_prompt_suffix:
                    "You are a helpful assistant. Be direct and efficient in your responses."
                        .to_string(),
                preferred_tools: vec![],
                tone: "neutral".to_string(),
                verbosity: "balanced".to_string(),
            },
        );

        profiles.insert(
            Role::DevOps,
            RoleProfile {
                role: Role::DevOps,
                name: "DevOps".to_string(),
                description: "Focused on infrastructure, deployment, and operations".to_string(),
                system_prompt_suffix: "You are in DevOps mode. Focus on reliable, secure infrastructure. \
                     Consider scalability and maintainability. Show commands clearly."
                    .to_string(),
                preferred_tools: vec![
                    "terminal".to_string(),
                    "read_file".to_string(),
                    "write_file".to_string(),
                ],
                tone: "authoritative".to_string(),
                verbosity: "concise".to_string(),
            },
        );

        profiles.insert(
            Role::Reviewer,
            RoleProfile {
                role: Role::Reviewer,
                name: "Reviewer".to_string(),
                description: "Focused on reviewing and improving code and content".to_string(),
                system_prompt_suffix: "You are in review mode. Provide constructive feedback. \
                     Highlight both strengths and areas for improvement."
                    .to_string(),
                preferred_tools: vec![
                    "read_file".to_string(),
                    "search_files".to_string(),
                    "terminal".to_string(),
                ],
                tone: "constructive".to_string(),
                verbosity: "detailed".to_string(),
            },
        );

        Self {
            profiles,
            current_role: Role::Assistant,
            transition_history: Vec::new(),
        }
    }

    pub fn detect_role(&self, message: &str, recent_tools: &[String]) -> (Role, f32) {
        let lower = message.to_lowercase();
        let mut scores: HashMap<Role, f32> = HashMap::new();

        // Coder keywords
        let coder_kws = [
            "implement", "function", "code", "refactor", "compile", "cargo", "npm",
        ];
        for kw in &coder_kws {
            if lower.contains(kw) {
                *scores.entry(Role::Coder).or_insert(0.0) += 0.3;
            }
        }

        // Researcher keywords
        let researcher_kws = ["compare", "what are", "analyze", "pros and cons", "evaluate"];
        for kw in &researcher_kws {
            if lower.contains(kw) {
                *scores.entry(Role::Researcher).or_insert(0.0) += 0.3;
            }
        }

        // Writer keywords
        let writer_kws = ["write a blog", "draft", "compose", "essay", "article"];
        for kw in &writer_kws {
            if lower.contains(kw) {
                *scores.entry(Role::Writer).or_insert(0.0) += 0.3;
            }
        }

        // Analyst keywords
        let analyst_kws = ["data", "metrics", "dashboard", "statistics", "trends"];
        for kw in &analyst_kws {
            if lower.contains(kw) {
                *scores.entry(Role::Analyst).or_insert(0.0) += 0.3;
            }
        }

        // Tutor keywords
        let tutor_kws = ["explain", "teach", "how does", "learn", "understand"];
        for kw in &tutor_kws {
            if lower.contains(kw) {
                *scores.entry(Role::Tutor).or_insert(0.0) += 0.3;
            }
        }

        // DevOps keywords
        let devops_kws = ["deploy", "docker", "kubernetes", "ci", "pipeline", "server"];
        for kw in &devops_kws {
            if lower.contains(kw) {
                *scores.entry(Role::DevOps).or_insert(0.0) += 0.3;
            }
        }

        // Reviewer keywords
        let reviewer_kws = ["review", "critique", "feedback", "improve", "suggestion"];
        for kw in &reviewer_kws {
            if lower.contains(kw) {
                *scores.entry(Role::Reviewer).or_insert(0.0) += 0.3;
            }
        }

        // Tool context boosts
        for tool in recent_tools {
            let tool_lower = tool.to_lowercase();
            if tool_lower == "terminal" || tool_lower == "patch" || tool_lower == "write_file" {
                *scores.entry(Role::Coder).or_insert(0.0) += 0.2;
            }
            if tool_lower == "web_search" {
                *scores.entry(Role::Researcher).or_insert(0.0) += 0.2;
            }
        }

        // Normalize
        let max_score = scores.values().copied().fold(0.0_f32, f32::max);
        if max_score > 1.0 {
            for score in scores.values_mut() {
                *score /= max_score;
            }
        }

        // Find the best match
        let mut sorted: Vec<(Role, f32)> = scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if let Some((role, score)) = sorted.into_iter().next() {
            (role, score)
        } else {
            (Role::Assistant, 0.3)
        }
    }

    pub fn transition(&mut self, new_role: Role, reason: &str) -> hakimi_common::Result<()> {
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.transition_history
            .push((new_role.clone(), reason.to_string(), timestamp));
        self.current_role = new_role;
        Ok(())
    }

    pub fn get_current_profile(&self) -> &RoleProfile {
        self.profiles
            .get(&self.current_role)
            .expect("current role must have a profile")
    }

    pub fn get_system_prompt_suffix(&self) -> &str {
        &self.get_current_profile().system_prompt_suffix
    }

    pub fn get_preferred_tools(&self) -> &[String] {
        &self.get_current_profile().preferred_tools
    }

    pub fn filter_tools(&self, available: &[String]) -> Vec<String> {
        let preferred = self.get_preferred_tools();
        let mut result: Vec<String> = Vec::new();
        let mut remaining: Vec<String> = Vec::new();

        // Add preferred tools first (in order), then the rest
        for tool in available {
            if preferred.contains(tool) && !result.contains(tool) {
                result.push(tool.clone());
            } else if !result.contains(tool) {
                remaining.push(tool.clone());
            }
        }

        result.extend(remaining);
        result
    }

    pub fn to_json(&self) -> hakimi_common::Result<String> {
        #[derive(Serialize)]
        struct RoleAdapterData {
            profiles: Vec<RoleProfile>,
            current_role: Role,
            transition_history: Vec<(Role, String, String)>,
        }

        let data = RoleAdapterData {
            profiles: self.profiles.values().cloned().collect(),
            current_role: self.current_role.clone(),
            transition_history: self.transition_history.clone(),
        };

        serde_json::to_string_pretty(&data)
            .map_err(|e| hakimi_common::HakimiError::Other(e.to_string()))
    }

    pub fn from_json(json: &str) -> hakimi_common::Result<Self> {
        #[derive(Deserialize)]
        struct RoleAdapterData {
            profiles: Vec<RoleProfile>,
            current_role: Role,
            transition_history: Vec<(Role, String, String)>,
        }

        let data: RoleAdapterData =
            serde_json::from_str(json).map_err(|e| hakimi_common::HakimiError::Other(e.to_string()))?;

        let mut profiles = HashMap::new();
        for profile in data.profiles {
            profiles.insert(profile.role.clone(), profile);
        }

        Ok(Self {
            profiles,
            current_role: data.current_role,
            transition_history: data.transition_history,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_roles_exist() {
        let adapter = RoleAdapter::new();
        assert!(adapter.profiles.contains_key(&Role::Coder));
        assert!(adapter.profiles.contains_key(&Role::Assistant));
    }

    #[test]
    fn test_detect_coder() {
        let adapter = RoleAdapter::new();
        let (role, _) = adapter.detect_role("implement a function for sorting", &[]);
        assert_eq!(role, Role::Coder);
    }

    #[test]
    fn test_detect_researcher() {
        let adapter = RoleAdapter::new();
        let (role, _) = adapter.detect_role("compare these two approaches", &[]);
        assert_eq!(role, Role::Researcher);
    }

    #[test]
    fn test_detect_writer() {
        let adapter = RoleAdapter::new();
        let (role, _) = adapter.detect_role("write a blog about Rust", &[]);
        assert_eq!(role, Role::Writer);
    }

    #[test]
    fn test_detect_debugger() {
        // DevOps is the closest to "debugger" with deploy/server
        let adapter = RoleAdapter::new();
        let (role, _) = adapter.detect_role("deploy this to the server", &[]);
        assert_eq!(role, Role::DevOps);
    }

    #[test]
    fn test_detect_devops() {
        let adapter = RoleAdapter::new();
        let (role, _) = adapter.detect_role("set up docker and kubernetes", &[]);
        assert_eq!(role, Role::DevOps);
    }

    #[test]
    fn test_transition_updates_role() {
        let mut adapter = RoleAdapter::new();
        assert_eq!(adapter.current_role, Role::Assistant);
        adapter.transition(Role::Coder, "user asked for code").unwrap();
        assert_eq!(adapter.current_role, Role::Coder);
    }

    #[test]
    fn test_transition_history() {
        let mut adapter = RoleAdapter::new();
        adapter.transition(Role::Coder, "coding task").unwrap();
        adapter.transition(Role::Researcher, "research task").unwrap();
        assert_eq!(adapter.transition_history.len(), 2);
        assert_eq!(adapter.transition_history[0].0, Role::Coder);
        assert_eq!(adapter.transition_history[1].0, Role::Researcher);
    }

    #[test]
    fn test_filter_tools_reorders() {
        let mut adapter = RoleAdapter::new();
        adapter.transition(Role::Coder, "test").unwrap();
        let available = vec![
            "read_file".to_string(),
            "terminal".to_string(),
            "web_search".to_string(),
            "patch".to_string(),
        ];
        let filtered = adapter.filter_tools(&available);
        // Preferred tools for Coder: terminal, write_file, patch, search_files
        // So terminal and patch should come first
        assert_eq!(filtered[0], "terminal");
        assert_eq!(filtered[1], "patch");
    }

    #[test]
    fn test_system_prompt_suffix_changes() {
        let mut adapter = RoleAdapter::new();
        let initial = adapter.get_system_prompt_suffix().to_string();
        adapter.transition(Role::Coder, "coding").unwrap();
        let after = adapter.get_system_prompt_suffix().to_string();
        assert_ne!(initial, after);
        assert!(after.contains("coding mode"));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut adapter = RoleAdapter::new();
        adapter.transition(Role::Coder, "test reason").unwrap();

        let json = adapter.to_json().unwrap();
        let restored = RoleAdapter::from_json(&json).unwrap();

        assert_eq!(restored.current_role, Role::Coder);
        assert_eq!(restored.transition_history.len(), 1);
        assert!(restored.profiles.contains_key(&Role::Coder));
    }

    #[test]
    fn test_all_roles_have_profiles() {
        let adapter = RoleAdapter::new();
        let all_roles = [
            Role::Coder,
            Role::Researcher,
            Role::Writer,
            Role::Analyst,
            Role::Tutor,
            Role::Assistant,
            Role::DevOps,
            Role::Reviewer,
        ];
        for role in &all_roles {
            assert!(
                adapter.profiles.contains_key(role),
                "Missing profile for {:?}",
                role
            );
        }
    }

    #[test]
    fn test_verbosity_settings() {
        let adapter = RoleAdapter::new();
        let coder = adapter.profiles.get(&Role::Coder).unwrap();
        assert_eq!(coder.verbosity, "concise");

        let writer = adapter.profiles.get(&Role::Writer).unwrap();
        assert_eq!(writer.verbosity, "verbose");

        let assistant = adapter.profiles.get(&Role::Assistant).unwrap();
        assert_eq!(assistant.verbosity, "balanced");
    }

    #[test]
    fn test_tone_settings() {
        let adapter = RoleAdapter::new();
        let coder = adapter.profiles.get(&Role::Coder).unwrap();
        assert_eq!(coder.tone, "technical");

        let tutor = adapter.profiles.get(&Role::Tutor).unwrap();
        assert_eq!(tutor.tone, "encouraging");
    }

    #[test]
    fn test_detect_assistant_default() {
        let adapter = RoleAdapter::new();
        let (role, score) = adapter.detect_role("ok", &[]);
        assert_eq!(role, Role::Assistant);
        assert!(score >= 0.0 && score <= 1.0);
    }
}
