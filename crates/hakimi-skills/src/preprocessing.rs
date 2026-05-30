use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const INLINE_SHELL_MAX_OUTPUT: usize = 4_000;

/// Configurable preprocessing for SKILL.md prompt bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillPreprocessOptions {
    /// Substitute `${HERMES_SKILL_DIR}` / `${HAKIMI_SKILL_DIR}` and session id tokens.
    pub template_vars: bool,
    /// Expand opt-in inline shell snippets.
    pub inline_shell: bool,
    /// Maximum seconds to wait for each inline shell snippet.
    pub inline_shell_timeout_secs: u64,
    /// Optional session identifier used by `${HERMES_SESSION_ID}` / `${HAKIMI_SESSION_ID}`.
    pub session_id: Option<String>,
}

impl Default for SkillPreprocessOptions {
    fn default() -> Self {
        Self {
            template_vars: env_bool("HAKIMI_SKILLS_TEMPLATE_VARS")
                .or_else(|| env_bool("HERMES_SKILLS_TEMPLATE_VARS"))
                .unwrap_or(true),
            inline_shell: env_bool("HAKIMI_SKILLS_INLINE_SHELL")
                .or_else(|| env_bool("HERMES_SKILLS_INLINE_SHELL"))
                .unwrap_or(false),
            inline_shell_timeout_secs: env_u64("HAKIMI_SKILLS_INLINE_SHELL_TIMEOUT")
                .or_else(|| env_u64("HERMES_SKILLS_INLINE_SHELL_TIMEOUT"))
                .unwrap_or(10)
                .max(1),
            session_id: std::env::var("HAKIMI_SESSION_ID")
                .ok()
                .or_else(|| std::env::var("HERMES_SESSION_ID").ok()),
        }
    }
}

impl SkillPreprocessOptions {
    pub fn without_inline_shell() -> Self {
        Self {
            inline_shell: false,
            ..Self::default()
        }
    }
}

/// Apply Hermes-compatible SKILL.md template preprocessing.
pub fn preprocess_skill_content(
    content: &str,
    skill_dir: Option<&Path>,
    options: &SkillPreprocessOptions,
) -> String {
    if content.is_empty() {
        return String::new();
    }

    let mut processed = content.to_string();
    if options.template_vars {
        processed = substitute_template_vars(&processed, skill_dir, options.session_id.as_deref());
    }
    if options.inline_shell {
        processed = expand_inline_shell(
            &processed,
            skill_dir,
            options.inline_shell_timeout_secs.max(1),
        );
    }
    processed
}

pub fn substitute_template_vars(
    content: &str,
    skill_dir: Option<&Path>,
    session_id: Option<&str>,
) -> String {
    let mut processed = content.to_string();
    if let Some(skill_dir) = skill_dir {
        let skill_dir = skill_dir.display().to_string();
        processed = processed.replace("${HERMES_SKILL_DIR}", &skill_dir);
        processed = processed.replace("${HAKIMI_SKILL_DIR}", &skill_dir);
    }
    if let Some(session_id) = session_id.filter(|value| !value.is_empty()) {
        processed = processed.replace("${HERMES_SESSION_ID}", session_id);
        processed = processed.replace("${HAKIMI_SESSION_ID}", session_id);
    }
    processed
}

pub fn expand_inline_shell(content: &str, cwd: Option<&Path>, timeout_secs: u64) -> String {
    if !content.contains("!`") {
        return content.to_string();
    }

    let mut output = String::with_capacity(content.len());
    let mut cursor = 0;
    while let Some(relative_start) = content[cursor..].find("!`") {
        let start = cursor + relative_start;
        let command_start = start + 2;
        output.push_str(&content[cursor..start]);

        let Some(relative_end) = content[command_start..].find('`') else {
            output.push_str(&content[start..]);
            return output;
        };
        let end = command_start + relative_end;
        let command = &content[command_start..end];
        if command.contains('\n') {
            output.push_str(&content[start..=end]);
        } else {
            output.push_str(&run_inline_shell(command.trim(), cwd, timeout_secs));
        }
        cursor = end + 1;
    }

    output.push_str(&content[cursor..]);
    output
}

pub fn run_inline_shell(command: &str, cwd: Option<&Path>, timeout_secs: u64) -> String {
    if command.is_empty() {
        return String::new();
    }

    let mut child = shell_command(command);
    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }
    child.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(err) => return format!("[inline-shell error: {err}]"),
    };

    let timeout = Duration::from_secs(timeout_secs.max(1));
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => match child.wait_with_output() {
                Ok(output) => return render_shell_output(output.stdout, output.stderr),
                Err(err) => return format!("[inline-shell error: {err}]"),
            },
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return format!(
                    "[inline-shell timeout after {}s: {command}]",
                    timeout.as_secs()
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(err) => return format!("[inline-shell error: {err}]"),
        }
    }
}

fn shell_command(command: &str) -> Command {
    if cfg!(windows) {
        let mut shell = Command::new("powershell");
        shell.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        shell
    } else {
        let mut shell = Command::new("sh");
        shell.args(["-c", command]);
        shell
    }
}

fn render_shell_output(stdout: Vec<u8>, stderr: Vec<u8>) -> String {
    let raw = if stdout.is_empty() { stderr } else { stdout };
    let text = String::from_utf8_lossy(&raw)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    truncate_chars(&text, INLINE_SHELL_MAX_OUTPUT)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...[truncated]");
    truncated
}

fn env_bool(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn substitutes_available_template_vars_and_leaves_missing_session() {
        let dir = TempDir::new().unwrap();
        let content =
            "Use ${HERMES_SKILL_DIR}; session=${HERMES_SESSION_ID}; h=${HAKIMI_SKILL_DIR}";

        let processed = substitute_template_vars(content, Some(dir.path()), None);

        assert!(processed.contains(&dir.path().display().to_string()));
        assert!(processed.contains("${HERMES_SESSION_ID}"));
        assert!(!processed.contains("${HERMES_SKILL_DIR}"));
        assert!(!processed.contains("${HAKIMI_SKILL_DIR}"));
    }

    #[test]
    fn substitutes_session_aliases() {
        let processed = substitute_template_vars(
            "${HERMES_SESSION_ID}/${HAKIMI_SESSION_ID}",
            None,
            Some("session-123"),
        );

        assert_eq!(processed, "session-123/session-123");
    }

    #[test]
    fn expands_inline_shell_when_enabled() {
        let processed = expand_inline_shell("today=!`echo ready`", None, 5);

        assert_eq!(processed, "today=ready");
    }

    #[test]
    fn leaves_multiline_inline_shell_unchanged() {
        let processed = expand_inline_shell("bad !`echo one\necho two`", None, 5);

        assert_eq!(processed, "bad !`echo one\necho two`");
    }

    #[test]
    fn preprocess_respects_disabled_inline_shell() {
        let options = SkillPreprocessOptions {
            template_vars: true,
            inline_shell: false,
            inline_shell_timeout_secs: 5,
            session_id: Some("abc".to_string()),
        };

        let processed =
            preprocess_skill_content("id=${HERMES_SESSION_ID}; !`echo no`", None, &options);

        assert_eq!(processed, "id=abc; !`echo no`");
    }
}
