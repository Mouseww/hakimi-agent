//! Persona configuration and on-disk storage for multi-agent isolation.
//!
//! Each persona is an isolated agent within a single instance. Its definition
//! lives in `<home>/agents/<id>/persona.yaml`; the registry index (ordered list
//! plus default pointer) lives in `<home>/agents/registry.yaml`. Per-persona
//! isolated state (memory, skills, sessions, context) lives in sibling files
//! under the persona directory. The heavy shared resources (transport, tools,
//! knowledge) are NOT stored here; see [`crate::SharedRuntime`].
//!
//! Design: `docs/superpowers/specs/2026-06-22-multi-agent-isolation-and-webui-design.md`.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Persisted definition of a single persona (agent) within an instance.
///
/// Serialized to `agents/<id>/persona.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonaConfig {
    /// Stable slug identifying the persona (matches its directory name).
    pub id: String,
    /// Human-facing display name.
    #[serde(default)]
    pub name: String,
    /// Emoji or short avatar marker.
    #[serde(default)]
    pub avatar: String,
    /// Short description of the persona's role.
    #[serde(default)]
    pub description: String,
    /// Model id this persona uses (shares the instance provider/credentials).
    #[serde(default)]
    pub model: String,
    /// Optional reasoning-effort override (`low` | `medium` | `high`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// System prompt defining the persona's identity.
    #[serde(default)]
    pub system_prompt: String,
    /// Names of skills enabled for this persona (isolated skill set).
    #[serde(default)]
    pub enabled_skills: Vec<String>,
    /// Channel bindings (`platform:bot_id`); empty means WebUI-only.
    #[serde(default)]
    pub bindings: Vec<String>,
    /// Whether this persona is the fallback for unbound gateway messages.
    #[serde(default)]
    pub is_default: bool,
    /// Whether other personas may consult this one as a teammate (`team` tool).
    /// Defaults to `true` so teams work out of the box; toggle off to opt out.
    #[serde(default = "default_true")]
    pub addressable: bool,
}

fn default_true() -> bool {
    true
}

impl PersonaConfig {
    /// Create a minimal persona with the given id (name defaults to the id).
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let name = id.clone();
        Self {
            id,
            name,
            avatar: String::new(),
            description: String::new(),
            model: String::new(),
            reasoning_effort: None,
            system_prompt: String::new(),
            enabled_skills: Vec::new(),
            bindings: Vec::new(),
            is_default: false,
            addressable: true,
        }
    }
}

/// Index of all personas in an instance, persisted to `agents/registry.yaml`.
///
/// Source of truth for which personas exist and which is the default. The
/// channel-binding map is derived from each persona's `bindings` at load time,
/// not stored here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RegistryIndex {
    /// Id of the default persona (gateway fallback for unbound channels).
    #[serde(default)]
    pub default: String,
    /// Ordered list of persona ids.
    #[serde(default)]
    pub personas: Vec<String>,
}

/// Validate a persona id: `[a-z0-9][a-z0-9_-]{0,63}` (same shape as profiles).
pub fn validate_persona_id(id: &str) -> Result<()> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        bail!("Persona ids must match [a-z0-9][a-z0-9_-]{{0,63}}");
    }
}

/// Load a persona definition from `<persona_dir>/persona.yaml`.
pub fn load_persona(persona_dir: &Path) -> Result<PersonaConfig> {
    let path = persona_dir.join("persona.yaml");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading persona file {}", path.display()))?;
    let cfg: PersonaConfig = serde_yaml::from_str(&raw)
        .with_context(|| format!("parsing persona file {}", path.display()))?;
    validate_persona_id(&cfg.id)?;
    Ok(cfg)
}

/// Write a persona definition to `<persona_dir>/persona.yaml`, creating the
/// directory if needed.
pub fn save_persona(persona_dir: &Path, cfg: &PersonaConfig) -> Result<()> {
    validate_persona_id(&cfg.id)?;
    std::fs::create_dir_all(persona_dir)
        .with_context(|| format!("creating persona dir {}", persona_dir.display()))?;
    let path = persona_dir.join("persona.yaml");
    let yaml = serde_yaml::to_string(cfg).context("serializing persona")?;
    std::fs::write(&path, yaml)
        .with_context(|| format!("writing persona file {}", path.display()))?;
    Ok(())
}

/// Load the registry index from `<agents_dir>/registry.yaml`. Returns an empty
/// index when the file does not exist yet.
pub fn load_registry_index(agents_dir: &Path) -> Result<RegistryIndex> {
    let path = agents_dir.join("registry.yaml");
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_yaml::from_str(&raw)
            .with_context(|| format!("parsing registry index {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(RegistryIndex::default()),
        Err(err) => Err(err).with_context(|| format!("reading registry index {}", path.display())),
    }
}

/// Write the registry index to `<agents_dir>/registry.yaml`, creating the
/// directory if needed.
pub fn save_registry_index(agents_dir: &Path, index: &RegistryIndex) -> Result<()> {
    std::fs::create_dir_all(agents_dir)
        .with_context(|| format!("creating agents dir {}", agents_dir.display()))?;
    let path = agents_dir.join("registry.yaml");
    let yaml = serde_yaml::to_string(index).context("serializing registry index")?;
    std::fs::write(&path, yaml)
        .with_context(|| format!("writing registry index {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("hakimi-persona-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn persona_round_trips_through_yaml() {
        let dir = temp_dir();
        let persona_dir = dir.join("coder");
        let mut cfg = PersonaConfig::new("coder");
        cfg.name = "编码助手".to_string();
        cfg.model = "claude-opus-4-8".to_string();
        cfg.reasoning_effort = Some("high".to_string());
        cfg.system_prompt = "You are 编码助手.".to_string();
        cfg.enabled_skills = vec!["tdd".to_string(), "systematic-debugging".to_string()];
        cfg.bindings = vec!["telegram:devbot".to_string()];
        cfg.is_default = true;

        save_persona(&persona_dir, &cfg).unwrap();
        let loaded = load_persona(&persona_dir).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn registry_index_round_trips_and_defaults_to_empty() {
        let agents = temp_dir();
        // Missing file resolves to an empty index.
        assert_eq!(
            load_registry_index(&agents).unwrap(),
            RegistryIndex::default()
        );

        let index = RegistryIndex {
            default: "default".to_string(),
            personas: vec!["default".to_string(), "coder".to_string()],
        };
        save_registry_index(&agents, &index).unwrap();
        assert_eq!(load_registry_index(&agents).unwrap(), index);
    }

    #[test]
    fn addressable_defaults_true_when_absent() {
        // A persona.yaml written before the field existed must load as addressable.
        let dir = temp_dir();
        let persona_dir = dir.join("legacy");
        std::fs::create_dir_all(&persona_dir).unwrap();
        std::fs::write(
            persona_dir.join("persona.yaml"),
            "id: legacy\nname: Legacy\n",
        )
        .unwrap();

        let loaded = load_persona(&persona_dir).unwrap();
        assert!(loaded.addressable, "missing addressable must default to true");
    }

    #[test]
    fn new_persona_is_addressable_by_default() {
        assert!(PersonaConfig::new("coder").addressable);
    }

    #[test]
    fn invalid_persona_id_is_rejected() {
        assert!(validate_persona_id("coder").is_ok());
        assert!(validate_persona_id("coder-2").is_ok());
        assert!(validate_persona_id("default").is_ok());
        assert!(validate_persona_id("Coder").is_err());
        assert!(validate_persona_id("../escape").is_err());
        assert!(validate_persona_id("").is_err());
    }
}
