//! Multi-profile support for Hakimi Agent.
//!
//! Each profile has its own config, memory, sessions, and skills directory.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Profile metadata stored in the profile directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMeta {
    /// Profile name.
    pub name: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Description.
    pub description: Option<String>,
}

/// Manager for Hakimi profiles.
pub struct ProfileManager {
    /// Base directory for profiles (~/.hakimi/profiles/).
    profiles_dir: PathBuf,
    /// Currently active profile name.
    active: Option<String>,
}

impl ProfileManager {
    /// Create a new profile manager.
    pub fn new(hakimi_home: &Path) -> Self {
        let profiles_dir = hakimi_home.join("profiles");
        Self {
            profiles_dir,
            active: None,
        }
    }

    /// Create a new profile.
    pub fn create(&self, name: &str, description: Option<&str>) -> anyhow::Result<PathBuf> {
        let profile_dir = self.profiles_dir.join(name);
        if profile_dir.exists() {
            anyhow::bail!("Profile '{}' already exists", name);
        }

        std::fs::create_dir_all(&profile_dir)?;
        std::fs::create_dir_all(profile_dir.join("memory"))?;
        std::fs::create_dir_all(profile_dir.join("sessions"))?;
        std::fs::create_dir_all(profile_dir.join("skills"))?;

        let meta = ProfileMeta {
            name: name.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            description: description.map(String::from),
        };

        let meta_path = profile_dir.join("profile.yaml");
        let yaml = serde_yaml::to_string(&meta)?;
        std::fs::write(meta_path, yaml)?;

        info!(name = %name, "Profile created");
        Ok(profile_dir)
    }

    /// Delete a profile.
    pub fn delete(&self, name: &str) -> anyhow::Result<()> {
        let profile_dir = self.profiles_dir.join(name);
        if !profile_dir.exists() {
            anyhow::bail!("Profile '{}' does not exist", name);
        }
        std::fs::remove_dir_all(&profile_dir)?;
        info!(name = %name, "Profile deleted");
        Ok(())
    }

    /// List all profiles.
    pub fn list(&self) -> anyhow::Result<Vec<ProfileMeta>> {
        if !self.profiles_dir.exists() {
            return Ok(Vec::new());
        }

        let mut profiles = Vec::new();
        for entry in std::fs::read_dir(&self.profiles_dir)? {
            let entry = entry?;
            let meta_path = entry.path().join("profile.yaml");
            if meta_path.exists() {
                let content = std::fs::read_to_string(&meta_path)?;
                if let Ok(meta) = serde_yaml::from_str::<ProfileMeta>(&content) {
                    profiles.push(meta);
                }
            }
        }
        Ok(profiles)
    }

    /// Set the active profile.
    pub fn use_profile(&mut self, name: &str) -> anyhow::Result<PathBuf> {
        let profile_dir = self.profiles_dir.join(name);
        if !profile_dir.exists() {
            anyhow::bail!("Profile '{}' does not exist", name);
        }
        self.active = Some(name.to_string());
        info!(name = %name, "Switched to profile");
        Ok(profile_dir)
    }

    /// Get the active profile name.
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Get the profile directory for a given name.
    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_dir.join(name)
    }

    /// Check if a profile exists.
    pub fn exists(&self, name: &str) -> bool {
        self.profiles_dir.join(name).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_create_and_list_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());

        manager.create("work", Some("Work profile")).unwrap();
        manager.create("personal", None).unwrap();

        let profiles = manager.list().unwrap();
        assert_eq!(profiles.len(), 2);
    }

    #[test]
    fn test_delete_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());

        manager.create("test", None).unwrap();
        assert!(manager.exists("test"));

        manager.delete("test").unwrap();
        assert!(!manager.exists("test"));
    }

    #[test]
    fn test_use_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manager = ProfileManager::new(tmp.path());

        manager.create("work", None).unwrap();
        let dir = manager.use_profile("work").unwrap();
        assert!(dir.exists());
        assert_eq!(manager.active(), Some("work"));
    }

    #[test]
    fn test_use_nonexistent_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manager = ProfileManager::new(tmp.path());
        assert!(manager.use_profile("nonexistent").is_err());
    }

    #[test]
    fn test_create_duplicate_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());
        manager.create("test", None).unwrap();
        assert!(manager.create("test", None).is_err());
    }

    #[test]
    fn test_list_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());
        let profiles = manager.list().unwrap();
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());
        assert!(!manager.exists("test"));
        manager.create("test", None).unwrap();
        assert!(manager.exists("test"));
    }
}
