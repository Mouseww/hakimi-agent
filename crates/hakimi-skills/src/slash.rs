use crate::{Skill, SkillStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSlashInvocation {
    pub command: String,
    pub skill_name: String,
    pub user_instruction: String,
    pub message: String,
}

pub fn normalize_skill_command_name(name: &str) -> Option<String> {
    let mut normalized = String::new();
    let mut last_was_hyphen = false;

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_hyphen = false;
        } else if matches!(ch, '-' | '_' | ' ') && !last_was_hyphen {
            normalized.push('-');
            last_was_hyphen = true;
        }
    }

    let trimmed = normalized.trim_matches('-');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_slash_input(input: &str) -> Option<(String, String)> {
    let trimmed = input.trim();
    let rest = trimmed.strip_prefix('/')?;
    let (command, user_instruction) = match rest.split_once(char::is_whitespace) {
        Some((command, user_instruction)) => (command, user_instruction.trim()),
        None => (rest, ""),
    };

    let command = normalize_skill_command_name(command)?;
    Some((command, user_instruction.to_string()))
}

fn format_skill_invocation_message(skill: &Skill, command: &str, user_instruction: &str) -> String {
    let mut parts = vec![
        format!(
            "[IMPORTANT: The user has invoked the \"{}\" skill via /{}, indicating they want you to follow its instructions. The full skill content is loaded below.]",
            skill.name, command
        ),
        String::new(),
        format!("### {}", skill.name),
    ];

    if !skill.description.trim().is_empty() {
        parts.push(skill.description.trim().to_string());
        parts.push(String::new());
    }

    parts.push(skill.render_body_capped());

    if !user_instruction.trim().is_empty() {
        parts.push(String::new());
        parts.push(format!(
            "The user has provided the following instruction alongside the skill invocation: {}",
            user_instruction.trim()
        ));
    }

    parts.join("\n")
}

impl SkillStore {
    pub fn resolve_slash_invocation(&self, input: &str) -> Option<SkillSlashInvocation> {
        let (command, user_instruction) = parse_slash_input(input)?;
        let skill = self.skills().iter().find(|skill| {
            normalize_skill_command_name(&skill.name)
                .as_deref()
                .is_some_and(|candidate| candidate == command)
        })?;

        Some(SkillSlashInvocation {
            command: command.clone(),
            skill_name: skill.name.clone(),
            user_instruction: user_instruction.clone(),
            message: format_skill_invocation_message(skill, &command, &user_instruction),
        })
    }

    pub fn build_slash_invocation_message(&mut self, input: &str) -> Option<String> {
        let invocation = self.resolve_slash_invocation(input)?;
        self.record_use(&invocation.skill_name);
        Some(invocation.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Skill;

    fn skill(name: &str, description: &str, content: &str) -> Skill {
        let mut skill = Skill::new(name, content);
        skill.description = description.to_string();
        skill
    }

    #[test]
    fn normalizes_skill_names_to_safe_slash_commands() {
        assert_eq!(
            normalize_skill_command_name("GitHub Code Review"),
            Some("github-code-review".to_string())
        );
        assert_eq!(
            normalize_skill_command_name("c++/ffi_helper"),
            Some("cffi-helper".to_string())
        );
        assert_eq!(normalize_skill_command_name("!!!"), None);
    }

    #[test]
    fn resolves_hyphen_and_underscore_skill_aliases() {
        let store = SkillStore::from_skills(vec![skill(
            "release-check",
            "Release workflow",
            "# Release\nCheck evidence.",
        )]);

        let invocation = store
            .resolve_slash_invocation("/release_check v0.3.129")
            .unwrap();

        assert_eq!(invocation.command, "release-check");
        assert_eq!(invocation.skill_name, "release-check");
        assert_eq!(invocation.user_instruction, "v0.3.129");
        assert!(invocation.message.contains("Release workflow"));
        assert!(invocation.message.contains("Check evidence."));
        assert!(invocation.message.contains("v0.3.129"));
    }

    #[test]
    fn ignores_unknown_or_non_slash_inputs() {
        let store = SkillStore::from_skills(vec![skill("release-check", "", "# Release")]);

        assert!(store.resolve_slash_invocation("release-check").is_none());
        assert!(store.resolve_slash_invocation("/missing").is_none());
    }
}
