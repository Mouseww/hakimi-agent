use std::path::{Path, PathBuf};

use hakimi_skills::{Skill, SkillHub, SkillHubEntry, SkillHubInstallOptions};
use serde_json::json;

#[derive(Debug, Clone)]
struct SkillCliOptions {
    index_path: Option<PathBuf>,
    limit: usize,
    json: bool,
    force: bool,
    trust_community: bool,
    category: Option<String>,
}

impl Default for SkillCliOptions {
    fn default() -> Self {
        Self {
            index_path: None,
            limit: 20,
            json: false,
            force: false,
            trust_community: false,
            category: None,
        }
    }
}

pub fn skills_response(args: &[String]) -> String {
    skills_response_for_dir(args, &default_skills_dir())
}

pub fn gateway_skills_response(raw: Option<&str>, loaded_skills: &[Skill]) -> String {
    let args: Vec<String> = raw
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect();
    if args.is_empty() || args.first().is_some_and(|arg| arg == "loaded") {
        return loaded_skills_response(loaded_skills);
    }

    if args
        .first()
        .is_some_and(|arg| arg == "help" || arg == "--help")
    {
        return gateway_skills_help();
    }

    let hub_args = if args.first().is_some_and(|arg| arg == "hub") {
        &args[1..]
    } else {
        &args[..]
    };
    match hub_args.first().map(String::as_str) {
        Some("browse" | "search" | "inspect" | "install" | "list" | "path" | "help") => {
            skills_response_for_dir(hub_args, &default_skills_dir())
        }
        _ => gateway_skills_help(),
    }
}

pub(crate) fn skills_response_for_dir(args: &[String], skills_dir: &Path) -> String {
    let Some(command) = args.first().map(|arg| arg.as_str()) else {
        return skills_help_response();
    };
    let (options, rest) = match parse_skill_cli_options(&args[1..]) {
        Ok(parsed) => parsed,
        Err(err) => return format!("Error: {err}\n{}", skills_help_response()),
    };
    let hub = hub_for_options(skills_dir, &options);

    match command {
        "browse" | "ls-remote" => match hub.browse(options.limit) {
            Ok(entries) => render_skill_entries(&entries, &hub, options.json),
            Err(err) => format!("Error: {err}"),
        },
        "search" => {
            let query = rest.join(" ");
            if query.trim().is_empty() {
                return "Usage: hakimi skills search <query> [--limit N] [--json]".to_string();
            }
            match hub.search(&query, options.limit) {
                Ok(entries) => render_skill_entries(&entries, &hub, options.json),
                Err(err) => format!("Error: {err}"),
            }
        }
        "inspect" => {
            let Some(identifier) = rest.first() else {
                return "Usage: hakimi skills inspect <identifier-or-name>".to_string();
            };
            match hub.inspect(identifier) {
                Ok(entry) => render_skill_inspect(&entry, options.json),
                Err(err) => format!("Error: {err}"),
            }
        }
        "install" => {
            let Some(identifier) = rest.first() else {
                return "Usage: hakimi skills install <identifier-or-name> [--category NAME] [--force] [--trust-community]".to_string();
            };
            let install_options = SkillHubInstallOptions {
                category: options.category,
                force: options.force,
                allow_community: options.trust_community,
            };
            match hub.install(identifier, install_options) {
                Ok(install) => format!(
                    "Installed skill `{}` from `{}` at {}\nTrust: `{}`\nHash: `{}`\nReload Hakimi to load the new skill into the active runtime.",
                    install.name,
                    install.identifier,
                    install.install_path.display(),
                    install.trust_level,
                    install.content_hash
                ),
                Err(err) => format!("Error: {err}"),
            }
        }
        "list" => match hub.installed() {
            Ok(installed) if options.json => serde_json::to_string_pretty(&installed)
                .unwrap_or_else(|err| format!(r#"{{"error":"{err}"}}"#)),
            Ok(installed) if installed.is_empty() => {
                format!(
                    "No hub-installed skills recorded in `{}`.",
                    hub.skills_dir().display()
                )
            }
            Ok(installed) => {
                let mut lines = vec![format!(
                    "Skills Hub installs in `{}`:",
                    hub.skills_dir().display()
                )];
                for skill in installed {
                    lines.push(format!(
                        "- `{}` [{}:{}] `{}` -> {}",
                        skill.name,
                        skill.source,
                        skill.trust_level,
                        skill.identifier,
                        skill.install_path
                    ));
                }
                lines.join("\n")
            }
            Err(err) => format!("Error: {err}"),
        },
        "path" => format!(
            "Skills directory: `{}`\nHub index: `{}`",
            hub.skills_dir().display(),
            hub.index_path().display()
        ),
        "help" | "-h" | "--help" => skills_help_response(),
        other => format!(
            "Unknown skills command `{other}`.\n{}",
            skills_help_response()
        ),
    }
}

fn default_skills_dir() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".hakimi").join("skills"))
        .unwrap_or_else(|| PathBuf::from(".hakimi").join("skills"))
}

fn hub_for_options(skills_dir: &Path, options: &SkillCliOptions) -> SkillHub {
    match &options.index_path {
        Some(index_path) => SkillHub::with_index_path(skills_dir, index_path),
        None => SkillHub::new(skills_dir),
    }
}

fn parse_skill_cli_options(args: &[String]) -> Result<(SkillCliOptions, Vec<String>), String> {
    let mut options = SkillCliOptions::default();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--index" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--index requires a path".to_string());
                };
                options.index_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--limit" | "--size" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--limit requires a number".to_string());
                };
                options.limit = value
                    .parse::<usize>()
                    .map_err(|_| "--limit must be a positive integer".to_string())?
                    .clamp(1, 100);
                i += 2;
            }
            "--json" => {
                options.json = true;
                i += 1;
            }
            "--force" => {
                options.force = true;
                i += 1;
            }
            "--trust-community" | "--allow-community" => {
                options.trust_community = true;
                i += 1;
            }
            "--category" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--category requires a value".to_string());
                };
                options.category = Some(value.clone());
                i += 2;
            }
            arg if arg.starts_with("--") => return Err(format!("unknown option `{arg}`")),
            _ => {
                rest.push(args[i].clone());
                i += 1;
            }
        }
    }
    Ok((options, rest))
}

fn render_skill_entries(entries: &[SkillHubEntry], hub: &SkillHub, as_json: bool) -> String {
    if as_json {
        let payload = entries.iter().map(skill_entry_json).collect::<Vec<_>>();
        return serde_json::to_string_pretty(&payload)
            .unwrap_or_else(|err| format!(r#"{{"error":"{err}"}}"#));
    }
    if entries.is_empty() {
        return format!(
            "No skills found in hub index `{}`.",
            hub.index_path().display()
        );
    }

    let mut lines = vec![format!(
        "Skills Hub results from `{}`:",
        hub.index_path().display()
    )];
    for entry in entries {
        lines.push(format!(
            "- `{}` [{}:{}] - {}",
            entry.name,
            entry.source,
            entry.trust_level,
            empty_dash(&entry.description)
        ));
        lines.push(format!("  id: `{}`", entry.identifier));
        if !entry.tags.is_empty() {
            lines.push(format!("  tags: {}", entry.tags.join(", ")));
        }
    }
    lines.join("\n")
}

fn render_skill_inspect(entry: &SkillHubEntry, as_json: bool) -> String {
    if as_json {
        return serde_json::to_string_pretty(&skill_entry_json(entry))
            .unwrap_or_else(|err| format!(r#"{{"error":"{err}"}}"#));
    }
    [
        format!("Skill: `{}`", entry.name),
        format!("Identifier: `{}`", entry.identifier),
        format!("Source: `{}`", entry.source),
        format!("Trust: `{}`", entry.trust_level),
        format!("Description: {}", empty_dash(&entry.description)),
        format!(
            "Tags: {}",
            if entry.tags.is_empty() {
                "-".to_string()
            } else {
                entry.tags.join(", ")
            }
        ),
        format!("Files: {}", entry.files.len()),
    ]
    .join("\n")
}

fn skill_entry_json(entry: &SkillHubEntry) -> serde_json::Value {
    json!({
        "name": entry.name,
        "description": entry.description,
        "source": entry.source,
        "identifier": entry.identifier,
        "trust_level": entry.trust_level,
        "repo": entry.repo,
        "category": entry.category,
        "tags": entry.tags,
        "files": entry.files.keys().collect::<Vec<_>>(),
    })
}

fn empty_dash(value: &str) -> &str {
    if value.trim().is_empty() { "-" } else { value }
}

fn loaded_skills_response(loaded_skills: &[Skill]) -> String {
    if loaded_skills.is_empty() {
        return "Loaded Skills: none".to_string();
    }
    let mut msg = "Loaded Skills:\n".to_string();
    for skill in loaded_skills {
        msg.push_str(&format!(
            "- `{}`: {} [{}]\n",
            skill.name,
            skill.description,
            skill.provenance_label()
        ));
    }
    msg
}

fn skills_help_response() -> String {
    [
        "Usage: hakimi skills <command>",
        "",
        "Commands:",
        "- browse [--limit N] [--json] [--index PATH] - list skills from the local hub index",
        "- search <query> [--limit N] [--json] [--index PATH] - search the local hub index",
        "- inspect <identifier-or-name> [--json] [--index PATH] - preview metadata without installing",
        "- install <identifier-or-name> [--category NAME] [--force] [--trust-community] [--index PATH] - install a scanned skill",
        "- list [--json] - list hub-installed skills recorded in .hub/lock.json",
        "- path - show the skills directory and index path",
        "",
        "Community skills require --trust-community so non-interactive installs cannot silently cross trust boundaries.",
    ]
    .join("\n")
}

fn gateway_skills_help() -> String {
    [
        "Skills commands:",
        "- `/skills` or `/skills loaded` - list currently loaded runtime skills",
        "- `/skills browse` - browse the local Skills Hub index",
        "- `/skills search <query>` - search hub skills",
        "- `/skills inspect <identifier>` - preview hub skill metadata",
        "- `/skills install <identifier> [--trust-community]` - install a scanned skill for the next reload",
        "- `/skills hub list` - list hub-installed skills",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_index(dir: &Path) -> PathBuf {
        let path = dir.join("index.json");
        std::fs::write(
            &path,
            r##"{
  "skills": [
    {"name":"release-check","description":"Release workflow","source":"official","identifier":"official/release-check","trust_level":"builtin","tags":["release"],"files":{"SKILL.md":"# Release"}},
    {"name":"rust-helper","description":"Rust support","source":"github","identifier":"owner/repo/rust-helper","trust_level":"community","tags":["rust"],"files":{"SKILL.md":"# Rust"}}
  ]
}"##,
        )
        .unwrap();
        path
    }

    #[test]
    fn search_renders_json_without_file_contents() {
        let tmp = TempDir::new().unwrap();
        let index = write_index(tmp.path());
        let args = vec![
            "search".to_string(),
            "rust".to_string(),
            "--index".to_string(),
            index.display().to_string(),
            "--json".to_string(),
        ];

        let response = skills_response_for_dir(&args, &tmp.path().join("skills"));

        assert!(response.contains("owner/repo/rust-helper"));
        assert!(!response.contains("# Rust"));
    }

    #[test]
    fn install_community_requires_explicit_flag() {
        let tmp = TempDir::new().unwrap();
        let index = write_index(tmp.path());
        let args = vec![
            "install".to_string(),
            "rust-helper".to_string(),
            "--index".to_string(),
            index.display().to_string(),
        ];

        let response = skills_response_for_dir(&args, &tmp.path().join("skills"));

        assert!(response.contains("--trust-community"));
    }

    #[test]
    fn gateway_defaults_to_loaded_skills() {
        let mut skill = Skill::new("release-check", "# Release");
        skill.description = "Release workflow".to_string();

        let response = gateway_skills_response(None, &[skill]);

        assert!(response.contains("Loaded Skills"));
        assert!(response.contains("release-check"));
    }
}
