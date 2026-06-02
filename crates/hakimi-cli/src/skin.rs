//! Data-driven CLI skin management.
//!
//! The rendering layer can consume these values incrementally. This module
//! focuses on Hermes-compatible discovery, inheritance, and config switching.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use hakimi_config::HakimiConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinCommandResult {
    pub message: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinListItem {
    pub name: String,
    pub description: String,
    pub source: SkinSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinSource {
    BuiltIn,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkinConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    #[serde(default)]
    pub spinner: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub branding: BTreeMap<String, String>,
    #[serde(default = "default_tool_prefix")]
    pub tool_prefix: String,
    #[serde(default)]
    pub tool_emojis: BTreeMap<String, String>,
    #[serde(default)]
    pub banner_logo: String,
    #[serde(default)]
    pub banner_hero: String,
}

fn default_tool_prefix() -> String {
    "|".to_string()
}

impl SkinConfig {
    fn merged_with_default(mut self) -> Self {
        if self.name == "default" {
            return self;
        }

        let default = builtin_skin("default").expect("default skin must exist");
        for (key, value) in default.colors {
            self.colors.entry(key).or_insert(value);
        }
        for (key, value) in default.spinner {
            self.spinner.entry(key).or_insert(value);
        }
        for (key, value) in default.branding {
            self.branding.entry(key).or_insert(value);
        }
        for (key, value) in default.tool_emojis {
            self.tool_emojis.entry(key).or_insert(value);
        }
        if self.tool_prefix.is_empty() {
            self.tool_prefix = default.tool_prefix;
        }
        if self.description.is_empty() {
            self.description = "Custom skin".to_string();
        }
        self
    }

    pub fn branding(&self, key: &str) -> Option<&str> {
        self.branding.get(key).map(String::as_str)
    }

    pub fn color(&self, key: &str) -> Option<&str> {
        self.colors.get(key).map(String::as_str)
    }
}

pub fn skins_dir(home: &Path) -> PathBuf {
    home.join("skins")
}

pub fn skin_command_response(
    args: &[String],
    config: &mut HakimiConfig,
    home: &Path,
) -> SkinCommandResult {
    match try_skin_command_response(args, config, home) {
        Ok(result) => result,
        Err(err) => SkinCommandResult {
            message: format!("Skin error: {err}"),
            changed: false,
        },
    }
}

pub fn gateway_skin_response(command: Option<&str>, current_skin: &str, home: &Path) -> String {
    let args = split_skin_args(command.unwrap_or_default());
    let action = args.first().map(String::as_str).unwrap_or("current");
    match action {
        "" | "current" | "status" => match load_skin(current_skin, home) {
            Ok(skin) => render_current_skin(&skin),
            Err(err) => format!("Skin error: {err}"),
        },
        "list" | "ls" => render_skin_list(home).unwrap_or_else(|err| format!("Skin error: {err}")),
        "path" => format!("Skin directory: {}", skins_dir(home).display()),
        "inspect" | "show" => {
            let Some(name) = args.get(1) else {
                return "usage: /skin inspect <name>".to_string();
            };
            render_skin_inspect(name, home).unwrap_or_else(|err| format!("Skin error: {err}"))
        }
        "set" | "use" => {
            let Some(name) = args.get(1) else {
                return "usage: /skin set <name>".to_string();
            };
            match load_skin(name, home) {
                Ok(skin) => format!(
                    "Skin `{}` is available. Run `hakimi skin set {}` to persist it; the running gateway will use it after restart.",
                    skin.name, skin.name
                ),
                Err(err) => format!("Skin error: {err}"),
            }
        }
        name if args.len() == 1 => match load_skin(name, home) {
            Ok(skin) => format!(
                "Skin `{}` is available. Run `hakimi skin set {}` to persist it; the running gateway will use it after restart.",
                skin.name, skin.name
            ),
            Err(err) => format!("Skin error: {err}"),
        },
        _ => skin_usage(),
    }
}

fn try_skin_command_response(
    args: &[String],
    config: &mut HakimiConfig,
    home: &Path,
) -> Result<SkinCommandResult> {
    let action = args.first().map(String::as_str).unwrap_or("current");
    match action {
        "" | "current" | "status" => {
            let skin = load_skin(&config.display.skin, home)?;
            Ok(SkinCommandResult {
                message: render_current_skin(&skin),
                changed: false,
            })
        }
        "list" | "ls" => Ok(SkinCommandResult {
            message: render_skin_list(home)?,
            changed: false,
        }),
        "path" => Ok(SkinCommandResult {
            message: format!("Skin directory: {}", skins_dir(home).display()),
            changed: false,
        }),
        "inspect" | "show" => {
            let name = args
                .get(1)
                .ok_or_else(|| anyhow!("usage: hakimi skin inspect <name>"))?;
            Ok(SkinCommandResult {
                message: render_skin_inspect(name, home)?,
                changed: false,
            })
        }
        "set" | "use" => {
            let name = args
                .get(1)
                .ok_or_else(|| anyhow!("usage: hakimi skin set <name>"))?;
            set_skin(name, config, home)
        }
        name if args.len() == 1 => set_skin(name, config, home),
        _ => Ok(SkinCommandResult {
            message: skin_usage(),
            changed: false,
        }),
    }
}

fn set_skin(name: &str, config: &mut HakimiConfig, home: &Path) -> Result<SkinCommandResult> {
    let skin = load_skin(name, home)?;
    config.display.skin = skin.name.clone();
    Ok(SkinCommandResult {
        message: format!("Skin set to `{}` ({})", skin.name, skin.description.trim()),
        changed: true,
    })
}

pub fn load_skin(name: &str, home: &Path) -> Result<SkinConfig> {
    let normalized = validate_skin_name(name)?;
    if let Some(skin) = builtin_skin(&normalized) {
        return Ok(skin.merged_with_default());
    }

    let path = skins_dir(home).join(format!("{normalized}.yaml"));
    let contents =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut skin: SkinConfig = serde_yaml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if skin.name.trim().is_empty() {
        skin.name = normalized;
    } else {
        skin.name = validate_skin_name(&skin.name)?;
    }
    Ok(skin.merged_with_default())
}

pub fn list_skins(home: &Path) -> Result<Vec<SkinListItem>> {
    let mut items = builtin_skin_names()
        .into_iter()
        .filter_map(|name| {
            builtin_skin(name).map(|skin| SkinListItem {
                name: skin.name,
                description: skin.description,
                source: SkinSource::BuiltIn,
            })
        })
        .collect::<Vec<_>>();

    let dir = skins_dir(home);
    if dir.exists() {
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("yaml") {
                continue;
            }
            match skin_from_file(&path) {
                Ok(skin) => items.push(SkinListItem {
                    name: skin.name,
                    description: skin.description,
                    source: SkinSource::User,
                }),
                Err(err) => {
                    items.push(SkinListItem {
                        name: path
                            .file_stem()
                            .and_then(|value| value.to_str())
                            .unwrap_or("invalid")
                            .to_string(),
                        description: format!("invalid skin file: {err}"),
                        source: SkinSource::User,
                    });
                }
            }
        }
    }

    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.name.clone()));
    items.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(items)
}

fn skin_from_file(path: &Path) -> Result<SkinConfig> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut skin: SkinConfig = serde_yaml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if skin.name.trim().is_empty() {
        let fallback = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("skin filename has no stem"))?;
        skin.name = fallback.to_string();
    }
    skin.name = validate_skin_name(&skin.name)?;
    Ok(skin.merged_with_default())
}

fn render_skin_list(home: &Path) -> Result<String> {
    let items = list_skins(home)?;
    let mut lines = vec!["Available skins:".to_string()];
    for item in items {
        let source = match item.source {
            SkinSource::BuiltIn => "built-in",
            SkinSource::User => "user",
        };
        lines.push(format!(
            "- {} [{}] - {}",
            item.name, source, item.description
        ));
    }
    lines.push(format!("User skins path: {}", skins_dir(home).display()));
    Ok(lines.join("\n"))
}

fn render_current_skin(skin: &SkinConfig) -> String {
    format!(
        "Current skin: `{}`\nDescription: {}\nAgent name: {}\nPrompt symbol: {}\nTool prefix: {}",
        skin.name,
        skin.description,
        skin.branding("agent_name").unwrap_or("Hakimi Agent"),
        skin.branding("prompt_symbol").unwrap_or(">"),
        skin.tool_prefix
    )
}

fn render_skin_inspect(name: &str, home: &Path) -> Result<String> {
    let skin = load_skin(name, home)?;
    let color_keys = skin.colors.keys().cloned().collect::<Vec<_>>().join(", ");
    let branding_keys = skin.branding.keys().cloned().collect::<Vec<_>>().join(", ");
    Ok(format!(
        "Skin `{}`\nDescription: {}\nTool prefix: {}\nColors: {}\nBranding: {}",
        skin.name, skin.description, skin.tool_prefix, color_keys, branding_keys
    ))
}

fn split_skin_args(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_string).collect()
}

fn skin_usage() -> String {
    "usage: hakimi skin [current|list|inspect <name>|set <name>|path]".to_string()
}

fn normalize_skin_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('_', "-")
}

fn validate_skin_name(name: &str) -> Result<String> {
    let normalized = normalize_skin_name(name);
    if normalized.is_empty()
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        bail!("invalid skin name `{}`", name.trim());
    }
    Ok(normalized)
}

fn builtin_skin_names() -> Vec<&'static str> {
    vec!["default", "ares", "mono", "slate", "daylight"]
}

fn builtin_skin(name: &str) -> Option<SkinConfig> {
    let skin = match name {
        "default" => SkinConfig {
            name: "default".to_string(),
            description: "Classic Hakimi terminal skin".to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#5f875f".to_string()),
                ("banner_title".to_string(), "#87af87".to_string()),
                ("banner_accent".to_string(), "#afd7af".to_string()),
                ("banner_dim".to_string(), "#6c6c6c".to_string()),
                ("banner_text".to_string(), "#e4e4e4".to_string()),
                ("prompt".to_string(), "#afd7af".to_string()),
                ("response_border".to_string(), "#87af87".to_string()),
                ("status_bar_bg".to_string(), "#1c1c1c".to_string()),
                ("status_bar_text".to_string(), "#d0d0d0".to_string()),
            ]),
            spinner: BTreeMap::new(),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Hakimi Agent".to_string()),
                (
                    "welcome".to_string(),
                    "Welcome to Hakimi Agent. Type /help for commands.".to_string(),
                ),
                ("goodbye".to_string(), "Goodbye.".to_string()),
                ("response_label".to_string(), " Hakimi ".to_string()),
                ("prompt_symbol".to_string(), ">".to_string()),
                ("help_header".to_string(), "Available Commands".to_string()),
            ]),
            tool_prefix: "|".to_string(),
            tool_emojis: BTreeMap::new(),
            banner_logo: String::new(),
            banner_hero: String::new(),
        },
        "ares" => SkinConfig {
            name: "ares".to_string(),
            description: "Crimson and bronze high-contrast skin".to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#9f1c1c".to_string()),
                ("banner_title".to_string(), "#c7a96b".to_string()),
                ("banner_accent".to_string(), "#dd4a3a".to_string()),
                ("prompt".to_string(), "#f1e6cf".to_string()),
                ("response_border".to_string(), "#c7a96b".to_string()),
            ]),
            spinner: BTreeMap::from([(
                "thinking_verbs".to_string(),
                serde_json::json!(["forging", "planning", "holding the line"]),
            )]),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Ares Agent".to_string()),
                ("response_label".to_string(), " Ares ".to_string()),
                ("prompt_symbol".to_string(), ">>".to_string()),
                ("help_header".to_string(), "Ares Commands".to_string()),
            ]),
            tool_prefix: "::".to_string(),
            tool_emojis: BTreeMap::new(),
            banner_logo: String::new(),
            banner_hero: String::new(),
        },
        "mono" => SkinConfig {
            name: "mono".to_string(),
            description: "Low-noise grayscale skin".to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#808080".to_string()),
                ("banner_title".to_string(), "#ffffff".to_string()),
                ("banner_accent".to_string(), "#d0d0d0".to_string()),
                ("prompt".to_string(), "#ffffff".to_string()),
            ]),
            spinner: BTreeMap::new(),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Hakimi Mono".to_string()),
                ("prompt_symbol".to_string(), "$".to_string()),
            ]),
            tool_prefix: ">".to_string(),
            tool_emojis: BTreeMap::new(),
            banner_logo: String::new(),
            banner_hero: String::new(),
        },
        "slate" => SkinConfig {
            name: "slate".to_string(),
            description: "Cool developer-focused dark skin".to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#5f87af".to_string()),
                ("banner_title".to_string(), "#87afd7".to_string()),
                ("banner_accent".to_string(), "#5fd7ff".to_string()),
                ("prompt".to_string(), "#d7eaff".to_string()),
            ]),
            spinner: BTreeMap::new(),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Hakimi Slate".to_string()),
                ("prompt_symbol".to_string(), "=>".to_string()),
            ]),
            tool_prefix: "||".to_string(),
            tool_emojis: BTreeMap::new(),
            banner_logo: String::new(),
            banner_hero: String::new(),
        },
        "daylight" => SkinConfig {
            name: "daylight".to_string(),
            description: "Light terminal skin with dark text accents".to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#4f6f9f".to_string()),
                ("banner_title".to_string(), "#1f3f6f".to_string()),
                ("banner_accent".to_string(), "#2f5f9f".to_string()),
                ("prompt".to_string(), "#1f3f6f".to_string()),
            ]),
            spinner: BTreeMap::new(),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Hakimi Daylight".to_string()),
                ("prompt_symbol".to_string(), ">".to_string()),
            ]),
            tool_prefix: "|".to_string(),
            tool_emojis: BTreeMap::new(),
            banner_logo: String::new(),
            banner_hero: String::new(),
        },
        _ => return None,
    };
    Some(skin)
}

pub fn ensure_known_skin(name: &str, home: &Path) -> Result<()> {
    if load_skin(name, home).is_ok() {
        Ok(())
    } else {
        bail!("unknown skin `{}`", name.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_user_skin(home: &Path, name: &str, yaml: &str) {
        let dir = skins_dir(home);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.yaml")), yaml).unwrap();
    }

    #[test]
    fn list_skins_includes_builtin_and_user_yaml() {
        let temp = tempfile::tempdir().unwrap();
        write_user_skin(
            temp.path(),
            "glacier",
            r##"
name: glacier
description: Crisp user skin
colors:
  banner_title: "#abcdef"
"##,
        );

        let skins = list_skins(temp.path()).unwrap();

        assert!(
            skins
                .iter()
                .any(|skin| skin.name == "default" && skin.source == SkinSource::BuiltIn)
        );
        assert!(
            skins
                .iter()
                .any(|skin| skin.name == "glacier" && skin.source == SkinSource::User)
        );
    }

    #[test]
    fn load_user_skin_inherits_default_branding_and_colors() {
        let temp = tempfile::tempdir().unwrap();
        write_user_skin(
            temp.path(),
            "glacier",
            r#"
name: glacier
branding:
  agent_name: Glacier Agent
"#,
        );

        let skin = load_skin("glacier", temp.path()).unwrap();

        assert_eq!(skin.branding("agent_name"), Some("Glacier Agent"));
        assert_eq!(skin.branding("prompt_symbol"), Some(">"));
        assert!(skin.color("banner_title").is_some());
    }

    #[test]
    fn skin_command_set_updates_display_config() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = HakimiConfig::default();

        let result = skin_command_response(
            &["set".to_string(), "mono".to_string()],
            &mut config,
            temp.path(),
        );

        assert!(result.changed);
        assert_eq!(config.display.skin, "mono");
        assert!(result.message.contains("Skin set to `mono`"));
    }

    #[test]
    fn skin_command_rejects_unknown_skin_without_mutating_config() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = HakimiConfig::default();

        let result = skin_command_response(
            &["set".to_string(), "missing".to_string()],
            &mut config,
            temp.path(),
        );

        assert!(!result.changed);
        assert_eq!(config.display.skin, "default");
        assert!(result.message.contains("Skin error"));
    }

    #[test]
    fn load_skin_rejects_path_traversal_names() {
        let temp = tempfile::tempdir().unwrap();

        let err = load_skin("../config", temp.path()).unwrap_err();

        assert!(err.to_string().contains("invalid skin name"));
    }

    #[test]
    fn gateway_skin_response_does_not_persist_runtime_set_requests() {
        let temp = tempfile::tempdir().unwrap();

        let response = gateway_skin_response(Some("set ares"), "default", temp.path());

        assert!(response.contains("hakimi skin set ares"));
        assert!(response.contains("after restart"));
    }
}
