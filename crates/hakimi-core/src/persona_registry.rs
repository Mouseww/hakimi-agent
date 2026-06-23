//! Persona registry: the routing brain for multi-agent isolation.
//!
//! This layer manages the set of [`PersonaConfig`]s in an instance and resolves
//! an inbound channel (`platform:bot_id`) to the persona that owns it, falling
//! back to the default persona when nothing matches. It is a pure
//! config-and-routing layer; constructing the live per-persona [`AIAgent`]
//! (shared runtime + isolated model/prompt/context/skills) is layered on top in
//! a later step.
//!
//! Design: `docs/superpowers/specs/2026-06-22-multi-agent-isolation-and-webui-design.md`.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::persona::{
    PersonaConfig, RegistryIndex, load_persona, load_registry_index, save_persona,
    save_registry_index, validate_persona_id,
};

/// Id used for the auto-seeded default persona when no registry exists yet.
pub const DEFAULT_PERSONA_ID: &str = "default";

/// In-memory registry of personas plus the derived channel-binding index.
#[derive(Debug, Clone)]
pub struct PersonaRegistry {
    agents_dir: PathBuf,
    personas: HashMap<String, PersonaConfig>,
    /// Stable display/iteration order of persona ids.
    order: Vec<String>,
    default_id: String,
    /// Derived map: `"platform:bot_id"` -> persona id.
    binding_index: HashMap<String, String>,
}

impl PersonaRegistry {
    /// Load the registry from `<agents_dir>`. When no registry exists yet, seed
    /// a single in-memory default persona (not written to disk until persisted).
    pub fn load(agents_dir: impl Into<PathBuf>) -> Result<Self> {
        let agents_dir = agents_dir.into();
        let index = load_registry_index(&agents_dir)?;

        let mut personas = HashMap::new();
        let mut order = Vec::new();
        for id in &index.personas {
            let cfg = load_persona(&agents_dir.join(id))?;
            if cfg.id != *id {
                bail!(
                    "persona id mismatch: registry lists '{id}' but persona.yaml has '{}'",
                    cfg.id
                );
            }
            order.push(id.clone());
            personas.insert(id.clone(), cfg);
        }

        if personas.is_empty() {
            // Fresh instance: seed an in-memory default persona.
            let mut def = PersonaConfig::new(DEFAULT_PERSONA_ID);
            def.is_default = true;
            order.push(def.id.clone());
            personas.insert(def.id.clone(), def);
        }

        let default_id = resolve_default_id(&index.default, &personas, &order);

        let mut registry = Self {
            agents_dir,
            personas,
            order,
            default_id,
            binding_index: HashMap::new(),
        };
        registry.rebuild_binding_index();
        Ok(registry)
    }

    /// Personas in stable order.
    pub fn list(&self) -> Vec<&PersonaConfig> {
        self.order
            .iter()
            .filter_map(|id| self.personas.get(id))
            .collect()
    }

    /// Look up a persona by id.
    pub fn get(&self, id: &str) -> Option<&PersonaConfig> {
        self.personas.get(id)
    }

    /// Id of the default (fallback) persona.
    pub fn default_id(&self) -> &str {
        &self.default_id
    }

    /// The default persona config (always present).
    pub fn default_persona(&self) -> &PersonaConfig {
        self.personas
            .get(&self.default_id)
            .expect("default persona must exist")
    }

    /// Root directory backing this registry (`<home>/agents`).
    pub fn agents_dir(&self) -> &std::path::Path {
        &self.agents_dir
    }

    /// Resolve an inbound channel to the owning persona, falling back to the
    /// default persona when no binding matches.
    pub fn resolve_for_channel(&self, platform: &str, bot_id: &str) -> &PersonaConfig {
        let key = channel_key(platform, bot_id);
        self.binding_index
            .get(&key)
            .and_then(|id| self.personas.get(id))
            .unwrap_or_else(|| self.default_persona())
    }

    /// The full binding map (`platform:bot_id` -> persona id), for the WebUI
    /// bindings overview.
    pub fn bindings(&self) -> &HashMap<String, String> {
        &self.binding_index
    }

    /// Create a new persona and persist it. Errors if the id already exists.
    pub fn create(&mut self, cfg: PersonaConfig) -> Result<()> {
        validate_persona_id(&cfg.id)?;
        if self.personas.contains_key(&cfg.id) {
            bail!("persona '{}' already exists", cfg.id);
        }
        let id = cfg.id.clone();
        self.apply_default_flag(&cfg);
        self.order.push(id.clone());
        self.personas.insert(id, cfg);
        self.rebuild_binding_index();
        self.persist()
    }

    /// Replace an existing persona's config and persist it.
    pub fn update(&mut self, cfg: PersonaConfig) -> Result<()> {
        validate_persona_id(&cfg.id)?;
        if !self.personas.contains_key(&cfg.id) {
            bail!("persona '{}' does not exist", cfg.id);
        }
        self.apply_default_flag(&cfg);
        self.personas.insert(cfg.id.clone(), cfg);
        self.rebuild_binding_index();
        self.persist()
    }

    /// Delete a persona and persist. The default persona cannot be deleted.
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if id == self.default_id {
            bail!("cannot delete the default persona '{id}'");
        }
        if self.personas.remove(id).is_none() {
            bail!("persona '{id}' does not exist");
        }
        self.order.retain(|existing| existing != id);
        self.rebuild_binding_index();
        self.persist()
    }

    /// When a persona is marked `is_default`, demote any previous default so
    /// exactly one default exists, and update `default_id`.
    fn apply_default_flag(&mut self, cfg: &PersonaConfig) {
        if cfg.is_default {
            for (id, existing) in self.personas.iter_mut() {
                if id != &cfg.id {
                    existing.is_default = false;
                }
            }
            self.default_id = cfg.id.clone();
        }
    }

    fn rebuild_binding_index(&mut self) {
        let mut index = HashMap::new();
        for id in &self.order {
            if let Some(cfg) = self.personas.get(id) {
                for binding in &cfg.bindings {
                    index.insert(binding.trim().to_string(), id.clone());
                }
            }
        }
        self.binding_index = index;
    }

    /// Persist the registry index plus every persona's `persona.yaml`.
    pub fn persist(&self) -> Result<()> {
        for id in &self.order {
            if let Some(cfg) = self.personas.get(id) {
                save_persona(&self.agents_dir.join(id), cfg)?;
            }
        }
        let index = RegistryIndex {
            default: self.default_id.clone(),
            personas: self.order.clone(),
        };
        save_registry_index(&self.agents_dir, &index)
    }
}

fn channel_key(platform: &str, bot_id: &str) -> String {
    format!("{}:{}", platform.trim(), bot_id.trim())
}

/// Pick the default persona id: prefer the index's `default` when it names an
/// existing persona, then any persona flagged `is_default`, then the first.
fn resolve_default_id(
    index_default: &str,
    personas: &HashMap<String, PersonaConfig>,
    order: &[String],
) -> String {
    if !index_default.is_empty() && personas.contains_key(index_default) {
        return index_default.to_string();
    }
    if let Some(flagged) = order
        .iter()
        .find(|id| personas.get(*id).is_some_and(|c| c.is_default))
    {
        return flagged.clone();
    }
    order
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_PERSONA_ID.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("hakimi-registry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path.join("agents")
    }

    fn persona_with_bindings(id: &str, bindings: &[&str]) -> PersonaConfig {
        let mut cfg = PersonaConfig::new(id);
        cfg.bindings = bindings.iter().map(|b| b.to_string()).collect();
        cfg
    }

    #[test]
    fn agents_dir_exposes_backing_path() {
        let dir = temp_dir();
        let reg = PersonaRegistry::load(dir.clone()).unwrap();
        assert_eq!(reg.agents_dir(), dir.as_path());
    }

    #[test]
    fn load_empty_seeds_default() {
        let reg = PersonaRegistry::load(temp_dir()).unwrap();
        assert_eq!(reg.default_id(), DEFAULT_PERSONA_ID);
        assert_eq!(reg.list().len(), 1);
        // Unbound channel falls back to default.
        assert_eq!(
            reg.resolve_for_channel("telegram", "anybot").id,
            DEFAULT_PERSONA_ID
        );
    }

    #[test]
    fn resolve_uses_binding_then_default() {
        let mut reg = PersonaRegistry::load(temp_dir()).unwrap();
        reg.create(persona_with_bindings("coder", &["telegram:devbot"]))
            .unwrap();
        reg.create(persona_with_bindings("writer", &["telegram:writebot"]))
            .unwrap();

        assert_eq!(reg.resolve_for_channel("telegram", "devbot").id, "coder");
        assert_eq!(reg.resolve_for_channel("telegram", "writebot").id, "writer");
        // Unbound -> default.
        assert_eq!(
            reg.resolve_for_channel("slack", "support").id,
            DEFAULT_PERSONA_ID
        );
    }

    #[test]
    fn create_get_update_delete() {
        let mut reg = PersonaRegistry::load(temp_dir()).unwrap();
        reg.create(PersonaConfig::new("coder")).unwrap();
        assert!(reg.get("coder").is_some());

        // Duplicate create rejected.
        assert!(reg.create(PersonaConfig::new("coder")).is_err());

        let mut updated = PersonaConfig::new("coder");
        updated.model = "claude-opus-4-8".to_string();
        reg.update(updated).unwrap();
        assert_eq!(reg.get("coder").unwrap().model, "claude-opus-4-8");

        reg.delete("coder").unwrap();
        assert!(reg.get("coder").is_none());
        // Default persona cannot be deleted.
        assert!(reg.delete(DEFAULT_PERSONA_ID).is_err());
    }

    #[test]
    fn marking_default_demotes_previous_and_persists() {
        let dir = temp_dir();
        {
            let mut reg = PersonaRegistry::load(&dir).unwrap();
            let mut coder = PersonaConfig::new("coder");
            coder.is_default = true;
            reg.create(coder).unwrap();
            assert_eq!(reg.default_id(), "coder");
            assert!(!reg.get(DEFAULT_PERSONA_ID).unwrap().is_default);
        }
        // Reload from disk: persisted state round-trips.
        let reg = PersonaRegistry::load(&dir).unwrap();
        assert_eq!(reg.default_id(), "coder");
        assert!(reg.get("coder").is_some());
        assert!(reg.get(DEFAULT_PERSONA_ID).is_some());
    }
}
