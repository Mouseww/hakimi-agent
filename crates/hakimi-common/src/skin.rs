use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const DEFAULT_SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinRuntime {
    pub name: String,
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

    Ok(SkinRuntime {
        name,
        spinner: raw.spinner.into_runtime_spinner(),
    })
}

fn builtin_skin_runtime(name: &str) -> Option<SkinRuntime> {
    let spinner = match name {
        "default" => SkinSpinner {
            frames: DEFAULT_SPINNER_FRAMES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            thinking_verbs: Vec::new(),
            wings: Vec::new(),
        },
        "ares" => SkinSpinner {
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
        "mono" => SkinSpinner {
            frames: ["-", "\\", "|", "/"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            thinking_verbs: Vec::new(),
            wings: Vec::new(),
        },
        "slate" => SkinSpinner {
            frames: ["◐", "◓", "◑", "◒"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            thinking_verbs: Vec::new(),
            wings: Vec::new(),
        },
        "daylight" => SkinSpinner {
            frames: ["·", "•", "●", "•"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            thinking_verbs: Vec::new(),
            wings: Vec::new(),
        },
        _ => return None,
    };
    Some(SkinRuntime {
        name: name.to_string(),
        spinner,
    })
}

#[derive(Debug, Default, Deserialize)]
struct RawSkinRuntime {
    #[serde(default)]
    name: Option<String>,
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
        assert!(skin.spinner_frame(0).contains("(⚔)"));
        assert!(skin.thinking_label(0).contains("forging"));
        assert_ne!(skin.spinner_frame(0), "⠋");
        let _ = fs::remove_dir_all(home);
    }
}
