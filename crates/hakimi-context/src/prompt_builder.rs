use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, warn};

/// Platform-specific formatting hints.
fn build_platform_hints() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert(
        "telegram",
        "You are communicating via Telegram. Telegram supports markdown: \
         **bold**, *italic*, ~~strikethrough~~, ||spoiler||, `inline code`, \
         ```code blocks```, [links](url), and ## headers. \
         There is NO table syntax — use bullet lists or key: value pairs. \
         You can send files and images natively.",
    );
    m.insert(
        "discord",
        "You are communicating via Discord. Discord supports a subset of markdown: \
         **bold**, *italic*, ~~strikethrough~~, `inline code`, ```code blocks```, \
         [links](url), and >>> block quotes. \
         Use Discord-specific formatting: <@user_id> to mention users, \
         <#channel_id> to mention channels.",
    );
    m.insert(
        "slack",
        "You are communicating via Slack. Slack uses its own mrkdwn format: \
         *bold*, _italic_, ~strikethrough~, `inline code`, ```code blocks```, \
         <url|text> links. Use <!here>, <!channel>, <@user_id> for mentions.",
    );
    m
}

/// Build a complete system prompt from component parts.
pub fn build_system_prompt(
    identity: &str,
    platform: &str,
    skills: &str,
    memory: &str,
    env_hints: &str,
) -> String {
    let mut parts = Vec::new();

    // Identity / persona
    if !identity.is_empty() {
        parts.push(identity.to_string());
    }

    // Environment hints
    if !env_hints.is_empty() {
        parts.push(format!("## Environment\n{env_hints}"));
    }

    // Platform hints
    let platform_hints = build_platform_hints();
    if let Some(hint) = platform_hints.get(platform) {
        parts.push(format!("## Platform\n{hint}"));
    }

    // Skills
    if !skills.is_empty() {
        parts.push(format!("## Skills\n{skills}"));
    }

    // Memory
    if !memory.is_empty() {
        parts.push(format!("## Memory\n{memory}"));
    }

    parts.join("\n\n")
}

/// Scan a directory for SKILL.md files and build a skills prompt.
///
/// Recursively walks `skills_dir` looking for files named `SKILL.md`.
/// Each found file's contents are included under a heading derived from
/// the parent directory name.
pub fn build_skills_prompt(skills_dir: &str) -> String {
    let dir = Path::new(skills_dir);
    if !dir.exists() {
        debug!(path = skills_dir, "Skills directory does not exist");
        return String::new();
    }

    let mut skills = Vec::new();
    collect_skills(dir, &mut skills);

    if skills.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for (name, content) in &skills {
        out.push_str(&format!("### {name}\n{content}\n\n"));
    }

    debug!(count = skills.len(), "Loaded skills");
    out
}

/// Recursively collect SKILL.md files from a directory.
fn collect_skills(dir: &Path, out: &mut Vec<(String, String)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warn!(path = %dir.display(), error = %e, "Failed to read skills directory");
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect_skills(&path, out);
            continue;
        }

        if path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
            let name = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unnamed")
                .to_string();

            match std::fs::read_to_string(&path) {
                Ok(content) => out.push((name, content)),
                Err(e) => warn!(path = %path.display(), error = %e, "Failed to read SKILL.md"),
            }
        }
    }
}

/// Load context files (AGENTS.md, CLAUDE.md, .cursorrules) from the given directory.
///
/// Walks up from `cwd` to the filesystem root, collecting these files along the way.
/// Earlier (closer to root) files appear first in the output.
pub fn build_context_files_prompt(cwd: &str) -> String {
    let context_filenames = &["AGENTS.md", "CLAUDE.md", ".cursorrules"];
    let mut collected: Vec<(String, String)> = Vec::new();

    let mut dir = Some(Path::new(cwd).to_path_buf());

    // Walk from cwd up to root collecting context files.
    while let Some(current) = dir {
        for name in context_filenames {
            let path = current.join(name);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let label = format!("{} ({})", name, current.display());
                    collected.push((label, content));
                }
            }
        }
        dir = current.parent().map(|p| p.to_path_buf());
    }

    if collected.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for (label, content) in &collected {
        out.push_str(&format!("### {label}\n{content}\n\n"));
    }

    debug!(count = collected.len(), "Loaded context files");
    out
}

/// Build environment hints describing the current runtime environment.
pub fn build_environment_hints(platform: &str, os: &str, home: &str, cwd: &str) -> String {
    let mut parts = Vec::new();

    parts.push(format!("Platform: {platform}"));
    parts.push(format!("OS: {os}"));
    parts.push(format!("Home directory: {home}"));
    parts.push(format!("Working directory: {cwd}"));

    // Add any environment-specific paths
    if let Ok(path) = std::env::var("PATH") {
        parts.push(format!("PATH: {path}"));
    }

    if let Ok(shell) = std::env::var("SHELL") {
        parts.push(format!("Shell: {shell}"));
    }

    parts.join("\n")
}
