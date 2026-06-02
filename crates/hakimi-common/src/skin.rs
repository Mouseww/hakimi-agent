use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const DEFAULT_SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinRuntime {
    pub name: String,
    pub colors: BTreeMap<String, String>,
    pub branding: BTreeMap<String, String>,
    pub tool_prefix: String,
    pub tool_emojis: BTreeMap<String, String>,
    pub spinner: SkinSpinner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinSpinner {
    pub frames: Vec<String>,
    pub thinking_verbs: Vec<String>,
    pub wings: Vec<(String, String)>,
}

impl Default for SkinRuntime {
    fn default() -> Self {
        builtin_skin_runtime("default").expect("default skin runtime must exist")
    }
}

impl SkinRuntime {
    pub fn animation_len(&self) -> usize {
        [
            self.spinner.frames.len(),
            self.spinner.thinking_verbs.len(),
            self.spinner.wings.len(),
        ]
        .into_iter()
        .max()
        .unwrap_or(1)
        .max(1)
    }

    pub fn spinner_frame(&self, index: usize) -> String {
        let frame = indexed_or_default(&self.spinner.frames, index, "⠋");
        if let Some((left, right)) = indexed_pair(&self.spinner.wings, index) {
            format!("{left}{frame}{right}")
        } else {
            frame.to_string()
        }
    }

    pub fn thinking_label(&self, index: usize) -> String {
        let frame = self.spinner_frame(index);
        if let Some(verb) = indexed(&self.spinner.thinking_verbs, index) {
            format!("{frame} {verb}")
        } else {
            format!("{frame} Thinking...")
        }
    }

    pub fn color(&self, key: &str) -> Option<&str> {
        self.colors
            .get(key)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }

    pub fn branding(&self, key: &str) -> Option<&str> {
        self.branding
            .get(key)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }

    pub fn tool_emoji(&self, tool_name: &str) -> Option<&str> {
        self.tool_emojis
            .get(tool_name.trim())
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }
}

pub fn skins_dir(home: &Path) -> PathBuf {
    home.join("skins")
}

pub fn load_skin_runtime(name: &str, home: &Path) -> Result<SkinRuntime> {
    let normalized = validate_skin_name(name)?;
    if let Some(skin) = builtin_skin_runtime(&normalized) {
        return Ok(skin);
    }

    let path = skins_dir(home).join(format!("{normalized}.yaml"));
    let contents =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let raw: RawSkinRuntime = serde_yaml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let name = raw
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(validate_skin_name)
        .transpose()?
        .unwrap_or(normalized);

    let default = builtin_skin_runtime("default").expect("default skin runtime must exist");
    let mut colors = default.colors;
    for (key, value) in raw.colors {
        let value = value.trim();
        if !key.trim().is_empty() && !value.is_empty() {
            colors.insert(key.trim().to_string(), value.to_string());
        }
    }
    let mut branding = default.branding;
    for (key, value) in raw.branding {
        let value = value.trim();
        if !key.trim().is_empty() && !value.is_empty() {
            branding.insert(key.trim().to_string(), value.to_string());
        }
    }
    let tool_prefix = raw
        .tool_prefix
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(default.tool_prefix);
    let mut tool_emojis = default.tool_emojis;
    for (key, value) in raw.tool_emojis {
        let key = key.trim();
        let value = value.trim();
        if !key.is_empty() && !value.is_empty() {
            tool_emojis.insert(key.to_string(), value.to_string());
        }
    }

    Ok(SkinRuntime {
        name,
        colors,
        branding,
        tool_prefix,
        tool_emojis,
        spinner: raw.spinner.into_runtime_spinner(),
    })
}

fn builtin_skin_runtime(name: &str) -> Option<SkinRuntime> {
    let skin = match name {
        "default" => SkinRuntime {
            name: name.to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#5f875f".to_string()),
                ("banner_title".to_string(), "#87af87".to_string()),
                ("banner_dim".to_string(), "#6c6c6c".to_string()),
                ("banner_text".to_string(), "#e4e4e4".to_string()),
                ("ui_accent".to_string(), "#00ffff".to_string()),
                ("ui_label".to_string(), "#87af87".to_string()),
                ("ui_ok".to_string(), "#00ff00".to_string()),
                ("ui_error".to_string(), "#ff0000".to_string()),
                ("ui_warn".to_string(), "#ffff00".to_string()),
                ("prompt".to_string(), "#00ffff".to_string()),
                ("input_rule".to_string(), "#6c6c6c".to_string()),
                ("response_border".to_string(), "#6c6c6c".to_string()),
                ("status_bar_bg".to_string(), "#141428".to_string()),
                ("status_bar_text".to_string(), "#6c6c6c".to_string()),
                ("status_bar_strong".to_string(), "#87af87".to_string()),
                ("status_bar_dim".to_string(), "#6c6c6c".to_string()),
                ("status_bar_good".to_string(), "#87af87".to_string()),
                ("status_bar_warn".to_string(), "#ffff00".to_string()),
                ("status_bar_bad".to_string(), "#ffaf00".to_string()),
                ("status_bar_critical".to_string(), "#ff0000".to_string()),
                ("session_label".to_string(), "#87af87".to_string()),
                ("session_border".to_string(), "#6c6c6c".to_string()),
                ("selection_bg".to_string(), "#1e1e32".to_string()),
                ("completion_menu_bg".to_string(), "#19192d".to_string()),
                (
                    "completion_menu_current_bg".to_string(),
                    "#1e1e32".to_string(),
                ),
                ("completion_menu_meta_bg".to_string(), "#19192d".to_string()),
                (
                    "completion_menu_meta_current_bg".to_string(),
                    "#1e1e32".to_string(),
                ),
            ]),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Hakimi Agent".to_string()),
                ("prompt_symbol".to_string(), "⟩".to_string()),
                ("response_label".to_string(), " AI ".to_string()),
                ("help_header".to_string(), "Available Commands".to_string()),
            ]),
            tool_prefix: "┊".to_string(),
            tool_emojis: BTreeMap::new(),
            spinner: SkinSpinner {
                frames: DEFAULT_SPINNER_FRAMES
                    .iter()
                    .map(|value| value.to_string())
                    .collect(),
                thinking_verbs: Vec::new(),
                wings: Vec::new(),
            },
        },
        "ares" => SkinRuntime {
            name: name.to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#9f1c1c".to_string()),
                ("banner_title".to_string(), "#c7a96b".to_string()),
                ("banner_dim".to_string(), "#6e584b".to_string()),
                ("banner_text".to_string(), "#f1e6cf".to_string()),
                ("ui_accent".to_string(), "#dd4a3a".to_string()),
                ("ui_label".to_string(), "#c7a96b".to_string()),
                ("ui_ok".to_string(), "#7bc96f".to_string()),
                ("ui_error".to_string(), "#ef5350".to_string()),
                ("ui_warn".to_string(), "#ffa726".to_string()),
                ("prompt".to_string(), "#f1e6cf".to_string()),
                ("input_rule".to_string(), "#9f1c1c".to_string()),
                ("response_border".to_string(), "#c7a96b".to_string()),
                ("status_bar_bg".to_string(), "#2a1212".to_string()),
                ("status_bar_text".to_string(), "#f1e6cf".to_string()),
                ("status_bar_strong".to_string(), "#c7a96b".to_string()),
                ("status_bar_dim".to_string(), "#6e584b".to_string()),
                ("status_bar_good".to_string(), "#7bc96f".to_string()),
                ("status_bar_warn".to_string(), "#c7a96b".to_string()),
                ("status_bar_bad".to_string(), "#dd4a3a".to_string()),
                ("status_bar_critical".to_string(), "#ef5350".to_string()),
                ("session_label".to_string(), "#c7a96b".to_string()),
                ("session_border".to_string(), "#6e584b".to_string()),
                ("selection_bg".to_string(), "#4a1a1a".to_string()),
                ("completion_menu_bg".to_string(), "#2a1212".to_string()),
                (
                    "completion_menu_current_bg".to_string(),
                    "#4a1a1a".to_string(),
                ),
                ("completion_menu_meta_bg".to_string(), "#2a1212".to_string()),
                (
                    "completion_menu_meta_current_bg".to_string(),
                    "#4a1a1a".to_string(),
                ),
            ]),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Ares Agent".to_string()),
                ("prompt_symbol".to_string(), "⚔".to_string()),
                ("response_label".to_string(), " ⚔ Ares ".to_string()),
                ("help_header".to_string(), "Ares Commands".to_string()),
            ]),
            tool_prefix: "╎".to_string(),
            tool_emojis: BTreeMap::from([
                ("terminal".to_string(), "⚔".to_string()),
                ("bash".to_string(), "⚔".to_string()),
                ("read_file".to_string(), "⛨".to_string()),
                ("web_search".to_string(), "🔎".to_string()),
            ]),
            spinner: SkinSpinner {
                frames: ["(⚔)", "(⛨)", "(▲)", "(⌁)", "(<>)"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                thinking_verbs: [
                    "forging",
                    "marching",
                    "sizing the field",
                    "holding the line",
                    "hammering plans",
                    "tempering steel",
                    "plotting impact",
                    "raising the shield",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                wings: [("⟪⚔", "⚔⟫"), ("⟪▲", "▲⟫"), ("⟪╸", "╺⟫"), ("⟪⛨", "⛨⟫")]
                    .into_iter()
                    .map(|(left, right)| (left.to_string(), right.to_string()))
                    .collect(),
            },
        },
        "mono" => SkinRuntime {
            name: name.to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#808080".to_string()),
                ("banner_title".to_string(), "#ffffff".to_string()),
                ("banner_dim".to_string(), "#808080".to_string()),
                ("banner_text".to_string(), "#d0d0d0".to_string()),
                ("ui_accent".to_string(), "#ffffff".to_string()),
                ("ui_label".to_string(), "#d0d0d0".to_string()),
                ("ui_ok".to_string(), "#d0d0d0".to_string()),
                ("ui_error".to_string(), "#ffffff".to_string()),
                ("ui_warn".to_string(), "#d0d0d0".to_string()),
                ("prompt".to_string(), "#ffffff".to_string()),
                ("input_rule".to_string(), "#808080".to_string()),
                ("response_border".to_string(), "#d0d0d0".to_string()),
                ("status_bar_bg".to_string(), "#101010".to_string()),
                ("status_bar_text".to_string(), "#d0d0d0".to_string()),
                ("status_bar_strong".to_string(), "#ffffff".to_string()),
                ("status_bar_dim".to_string(), "#808080".to_string()),
                ("status_bar_good".to_string(), "#d0d0d0".to_string()),
                ("status_bar_warn".to_string(), "#ffffff".to_string()),
                ("status_bar_bad".to_string(), "#d0d0d0".to_string()),
                ("status_bar_critical".to_string(), "#ffffff".to_string()),
                ("session_label".to_string(), "#d0d0d0".to_string()),
                ("session_border".to_string(), "#808080".to_string()),
                ("selection_bg".to_string(), "#303030".to_string()),
            ]),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Hakimi Agent".to_string()),
                ("prompt_symbol".to_string(), ">".to_string()),
                ("response_label".to_string(), " AI ".to_string()),
                ("help_header".to_string(), "Available Commands".to_string()),
            ]),
            tool_prefix: "┊".to_string(),
            tool_emojis: BTreeMap::new(),
            spinner: SkinSpinner {
                frames: ["-", "\\", "|", "/"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                thinking_verbs: Vec::new(),
                wings: Vec::new(),
            },
        },
        "slate" => SkinRuntime {
            name: name.to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#5f87af".to_string()),
                ("banner_title".to_string(), "#87afd7".to_string()),
                ("banner_dim".to_string(), "#6c6c6c".to_string()),
                ("banner_text".to_string(), "#d7e5f5".to_string()),
                ("ui_accent".to_string(), "#5fd7ff".to_string()),
                ("ui_label".to_string(), "#87afd7".to_string()),
                ("prompt".to_string(), "#d7e5f5".to_string()),
                ("input_rule".to_string(), "#5f87af".to_string()),
                ("response_border".to_string(), "#87afd7".to_string()),
                ("status_bar_bg".to_string(), "#101a24".to_string()),
                ("status_bar_text".to_string(), "#d7e5f5".to_string()),
                ("status_bar_strong".to_string(), "#87afd7".to_string()),
                ("status_bar_dim".to_string(), "#6c6c6c".to_string()),
                ("status_bar_good".to_string(), "#63d0a6".to_string()),
                ("status_bar_warn".to_string(), "#87afd7".to_string()),
                ("status_bar_bad".to_string(), "#5fd7ff".to_string()),
                ("status_bar_critical".to_string(), "#ef5350".to_string()),
                ("session_label".to_string(), "#87afd7".to_string()),
                ("session_border".to_string(), "#6c6c6c".to_string()),
                ("selection_bg".to_string(), "#1e2a38".to_string()),
            ]),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Hakimi Agent".to_string()),
                ("prompt_symbol".to_string(), "›".to_string()),
                ("response_label".to_string(), " AI ".to_string()),
                ("help_header".to_string(), "Available Commands".to_string()),
            ]),
            tool_prefix: "┊".to_string(),
            tool_emojis: BTreeMap::new(),
            spinner: SkinSpinner {
                frames: ["◐", "◓", "◑", "◒"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                thinking_verbs: Vec::new(),
                wings: Vec::new(),
            },
        },
        "daylight" => SkinRuntime {
            name: name.to_string(),
            colors: BTreeMap::from([
                ("banner_border".to_string(), "#4f6f9f".to_string()),
                ("banner_title".to_string(), "#1f3f6f".to_string()),
                ("banner_dim".to_string(), "#6c6c6c".to_string()),
                ("banner_text".to_string(), "#1f2937".to_string()),
                ("ui_accent".to_string(), "#2f5f9f".to_string()),
                ("ui_label".to_string(), "#1f3f6f".to_string()),
                ("prompt".to_string(), "#1f3f6f".to_string()),
                ("input_rule".to_string(), "#4f6f9f".to_string()),
                ("response_border".to_string(), "#2f5f9f".to_string()),
                ("status_bar_bg".to_string(), "#e5e7eb".to_string()),
                ("status_bar_text".to_string(), "#1f2937".to_string()),
                ("status_bar_strong".to_string(), "#1f3f6f".to_string()),
                ("status_bar_dim".to_string(), "#6c6c6c".to_string()),
                ("status_bar_good".to_string(), "#2f7d5f".to_string()),
                ("status_bar_warn".to_string(), "#9a6a00".to_string()),
                ("status_bar_bad".to_string(), "#b45309".to_string()),
                ("status_bar_critical".to_string(), "#b91c1c".to_string()),
                ("session_label".to_string(), "#1f3f6f".to_string()),
                ("session_border".to_string(), "#6c6c6c".to_string()),
                ("selection_bg".to_string(), "#bfdbfe".to_string()),
            ]),
            branding: BTreeMap::from([
                ("agent_name".to_string(), "Hakimi Agent".to_string()),
                ("prompt_symbol".to_string(), "›".to_string()),
                ("response_label".to_string(), " AI ".to_string()),
                ("help_header".to_string(), "Available Commands".to_string()),
            ]),
            tool_prefix: "│".to_string(),
            tool_emojis: BTreeMap::new(),
            spinner: SkinSpinner {
                frames: ["·", "•", "●", "•"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                thinking_verbs: Vec::new(),
                wings: Vec::new(),
            },
        },
        _ => return None,
    };
    Some(skin)
}

#[derive(Debug, Default, Deserialize)]
struct RawSkinRuntime {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
    #[serde(default)]
    branding: BTreeMap<String, String>,
    #[serde(default)]
    tool_prefix: Option<String>,
    #[serde(default)]
    tool_emojis: BTreeMap<String, String>,
    #[serde(default)]
    spinner: RawSkinSpinner,
}

#[derive(Debug, Default, Deserialize)]
struct RawSkinSpinner {
    #[serde(default)]
    frames: Vec<String>,
    #[serde(default)]
    thinking_faces: Vec<String>,
    #[serde(default)]
    waiting_faces: Vec<String>,
    #[serde(default)]
    thinking_verbs: Vec<String>,
    #[serde(default)]
    wings: Vec<Vec<String>>,
}

impl RawSkinSpinner {
    fn into_runtime_spinner(self) -> SkinSpinner {
        let mut frames = non_empty_strings(self.frames);
        if frames.is_empty() {
            frames = non_empty_strings(self.thinking_faces);
        }
        if frames.is_empty() {
            frames = non_empty_strings(self.waiting_faces);
        }
        if frames.is_empty() {
            frames = DEFAULT_SPINNER_FRAMES
                .iter()
                .map(|value| value.to_string())
                .collect();
        }

        SkinSpinner {
            frames,
            thinking_verbs: non_empty_strings(self.thinking_verbs),
            wings: self
                .wings
                .into_iter()
                .filter_map(|pair| {
                    let left = pair.first()?.trim();
                    let right = pair.get(1)?.trim();
                    if left.is_empty() || right.is_empty() {
                        None
                    } else {
                        Some((left.to_string(), right.to_string()))
                    }
                })
                .collect(),
        }
    }
}

fn non_empty_strings(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn indexed_or_default<'a>(values: &'a [String], index: usize, default: &'a str) -> &'a str {
    indexed(values, index).unwrap_or(default)
}

fn indexed(values: &[String], index: usize) -> Option<&str> {
    if values.is_empty() {
        None
    } else {
        values.get(index % values.len()).map(String::as_str)
    }
}

fn indexed_pair(values: &[(String, String)], index: usize) -> Option<(&str, &str)> {
    if values.is_empty() {
        None
    } else {
        values
            .get(index % values.len())
            .map(|(left, right)| (left.as_str(), right.as_str()))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home() -> PathBuf {
        let path = std::env::temp_dir().join(format!("hakimi-skin-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_user_skin(home: &Path, name: &str, yaml: &str) {
        let dir = skins_dir(home);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.yaml")), yaml).unwrap();
    }

    #[test]
    fn runtime_skin_reads_hermes_spinner_schema() {
        let home = temp_home();
        write_user_skin(
            &home,
            "glacier",
            r#"
name: glacier
spinner:
  thinking_faces: ["(g)", "(*)"]
  thinking_verbs: ["cutting ice", "checking snow"]
  wings:
    - ["<", ">"]
"#,
        );

        let skin = load_skin_runtime("glacier", &home).unwrap();

        assert_eq!(skin.name, "glacier");
        assert_eq!(skin.animation_len(), 2);
        assert_eq!(skin.color("banner_title"), Some("#87af87"));
        assert_eq!(skin.branding("agent_name"), Some("Hakimi Agent"));
        assert_eq!(skin.tool_prefix, "┊");
        assert!(skin.tool_emoji("terminal").is_none());
        assert_eq!(skin.spinner_frame(0), "<(g)>");
        assert_eq!(skin.thinking_label(1), "<(*)> checking snow");

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn runtime_skin_rejects_path_traversal() {
        let home = temp_home();

        let err = load_skin_runtime("../config", &home).unwrap_err();

        assert!(err.to_string().contains("invalid skin name"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn built_in_ares_exposes_themed_spinner() {
        let home = temp_home();

        let skin = load_skin_runtime("ares", &home).unwrap();

        assert!(skin.animation_len() > 1);
        assert_eq!(skin.color("status_bar_bg"), Some("#2a1212"));
        assert_eq!(skin.branding("agent_name"), Some("Ares Agent"));
        assert_eq!(skin.tool_emoji("terminal"), Some("⚔"));
        assert!(skin.spinner_frame(0).contains("(⚔)"));
        assert!(skin.thinking_label(0).contains("forging"));
        assert_ne!(skin.spinner_frame(0), "⠋");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn runtime_skin_reads_hermes_tui_colors_and_branding() {
        let home = temp_home();
        write_user_skin(
            &home,
            "glacier",
            r##"
name: glacier
colors:
  status_bar_bg: "#112233"
  status_bar_text: "#ccddee"
  response_border: "#445566"
branding:
  agent_name: Glacier Agent
  prompt_symbol: =>
"##,
        );

        let skin = load_skin_runtime("glacier", &home).unwrap();

        assert_eq!(skin.color("status_bar_bg"), Some("#112233"));
        assert_eq!(skin.color("status_bar_text"), Some("#ccddee"));
        assert_eq!(skin.color("response_border"), Some("#445566"));
        assert_eq!(skin.branding("agent_name"), Some("Glacier Agent"));
        assert_eq!(skin.branding("prompt_symbol"), Some("=>"));
        assert_eq!(skin.color("session_label"), Some("#87af87"));
        assert_eq!(skin.branding("help_header"), Some("Available Commands"));
        assert_eq!(skin.color("banner_title"), Some("#87af87"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn runtime_skin_reads_hermes_tool_prefix() {
        let home = temp_home();
        write_user_skin(
            &home,
            "glacier",
            r#"
name: glacier
tool_prefix: ">>"
"#,
        );

        let skin = load_skin_runtime("glacier", &home).unwrap();

        assert_eq!(skin.tool_prefix, ">>");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn runtime_skin_reads_hermes_tool_emojis() {
        let home = temp_home();
        write_user_skin(
            &home,
            "glacier",
            r#"
name: glacier
tool_emojis:
  terminal: "⚔"
  web_search: "🔮"
  empty: ""
"#,
        );

        let skin = load_skin_runtime("glacier", &home).unwrap();

        assert_eq!(skin.tool_emoji("terminal"), Some("⚔"));
        assert_eq!(skin.tool_emoji("web_search"), Some("🔮"));
        assert!(skin.tool_emoji("empty").is_none());
        assert!(skin.tool_emoji("missing").is_none());
        let _ = fs::remove_dir_all(home);
    }
}
