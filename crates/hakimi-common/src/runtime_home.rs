use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

const ACTIVE_PROFILE_FILE: &str = "active_profile";

/// Resolved Hakimi home paths for the default runtime or an isolated profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHome {
    root_home: PathBuf,
    active_profile: Option<String>,
    home: PathBuf,
}

impl RuntimeHome {
    /// Resolve from the platform default `~/.hakimi` root.
    pub fn resolve_default(explicit_profile: Option<&str>) -> Result<Self> {
        Self::resolve(default_hakimi_home(), explicit_profile)
    }

    /// Resolve from a known root, honoring an explicit profile before sticky state.
    pub fn resolve(root_home: impl Into<PathBuf>, explicit_profile: Option<&str>) -> Result<Self> {
        let root_home = root_home.into();
        let explicit = explicit_profile.and_then(non_empty_trimmed);
        let selected = match explicit {
            Some("default") => None,
            Some(profile) => {
                validate_runtime_profile_name(profile)?;
                Some(profile.to_string())
            }
            None => read_sticky_profile(&root_home)?,
        };
        let home = selected
            .as_deref()
            .map(|profile| root_home.join("profiles").join(profile))
            .unwrap_or_else(|| root_home.clone());
        Ok(Self {
            root_home,
            active_profile: selected,
            home,
        })
    }

    pub fn root_home(&self) -> &Path {
        &self.root_home
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn active_profile(&self) -> Option<&str> {
        self.active_profile.as_deref()
    }

    pub fn is_profile(&self) -> bool {
        self.active_profile.is_some()
    }

    pub fn config_path(&self) -> PathBuf {
        self.home.join("config.yaml")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.home.join("skills")
    }

    pub fn memory_dir(&self) -> PathBuf {
        self.home.join("memory")
    }

    pub fn knowledge_path(&self) -> PathBuf {
        self.home.join("knowledge.json")
    }

    pub fn sessions_db_path(&self) -> PathBuf {
        self.home.join("sessions.db")
    }

    pub fn cron_db_path(&self) -> PathBuf {
        self.home.join("cron.db")
    }

    pub fn trajectories_dir(&self) -> PathBuf {
        self.home.join("trajectories")
    }
}

pub fn default_hakimi_home() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".hakimi"))
        .unwrap_or_else(|| PathBuf::from(".hakimi"))
}

/// Return the active runtime home, honoring `HAKIMI_HOME` when the launcher
/// selected a profile-scoped home for this process.
pub fn effective_hakimi_home() -> PathBuf {
    std::env::var_os("HAKIMI_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_hakimi_home)
}

pub fn validate_runtime_profile_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name != "default"
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        bail!("Profile names must match [a-z0-9][a-z0-9_-]{{0,63}}");
    }
}

fn read_sticky_profile(root_home: &Path) -> Result<Option<String>> {
    let path = root_home.join(ACTIVE_PROFILE_FILE);
    let raw = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let Some(profile) = non_empty_trimmed(&raw) else {
        return Ok(None);
    };
    validate_runtime_profile_name(profile)?;
    Ok(Some(profile.to_string()))
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_profile_maps_to_profile_home() {
        let runtime = RuntimeHome::resolve(PathBuf::from("/tmp/hakimi"), Some("research")).unwrap();

        assert_eq!(runtime.active_profile(), Some("research"));
        assert_eq!(runtime.home(), Path::new("/tmp/hakimi/profiles/research"));
        assert_eq!(
            runtime.config_path(),
            PathBuf::from("/tmp/hakimi/profiles/research/config.yaml")
        );
        assert_eq!(
            runtime.skills_dir(),
            PathBuf::from("/tmp/hakimi/profiles/research/skills")
        );
    }

    #[test]
    fn explicit_default_ignores_sticky_profile() {
        let tmp = tempfile_dir();
        std::fs::write(tmp.join(ACTIVE_PROFILE_FILE), "coder\n").unwrap();

        let runtime = RuntimeHome::resolve(&tmp, Some("default")).unwrap();

        assert_eq!(runtime.active_profile(), None);
        assert_eq!(runtime.home(), tmp.as_path());
    }

    #[test]
    fn sticky_profile_maps_to_profile_home() {
        let tmp = tempfile_dir();
        std::fs::write(tmp.join(ACTIVE_PROFILE_FILE), "ops\n").unwrap();

        let runtime = RuntimeHome::resolve(&tmp, None).unwrap();

        assert_eq!(runtime.active_profile(), Some("ops"));
        assert_eq!(runtime.home(), tmp.join("profiles/ops").as_path());
        assert_eq!(runtime.cron_db_path(), tmp.join("profiles/ops/cron.db"));
        assert_eq!(
            runtime.knowledge_path(),
            tmp.join("profiles/ops/knowledge.json")
        );
    }

    #[test]
    fn invalid_sticky_profile_is_rejected() {
        let tmp = tempfile_dir();
        std::fs::write(tmp.join(ACTIVE_PROFILE_FILE), "../config\n").unwrap();

        let err = RuntimeHome::resolve(&tmp, None).unwrap_err();

        assert!(err.to_string().contains("Profile names must match"));
    }

    fn tempfile_dir() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("hakimi-runtime-home-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
