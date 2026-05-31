//! Multi-profile support for Hakimi Agent.
//!
//! Each profile has its own config, memory, sessions, and skills directory.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;

const ACTIVE_PROFILE_FILE: &str = "active_profile";

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
            active: read_active_profile(hakimi_home),
        }
    }

    /// Create a new profile.
    pub fn create(&self, name: &str, description: Option<&str>) -> anyhow::Result<PathBuf> {
        validate_profile_name(name)?;
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
        validate_profile_name(name)?;
        let profile_dir = self.profiles_dir.join(name);
        if !profile_dir.exists() {
            anyhow::bail!("Profile '{}' does not exist", name);
        }
        std::fs::remove_dir_all(&profile_dir)?;
        if self.active.as_deref() == Some(name) {
            let _ = std::fs::remove_file(active_profile_path_from_profiles_dir(&self.profiles_dir));
        }
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
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    /// Set the active profile.
    pub fn use_profile(&mut self, name: &str) -> anyhow::Result<PathBuf> {
        validate_profile_name(name)?;
        let profile_dir = self.profiles_dir.join(name);
        if !profile_dir.exists() {
            anyhow::bail!("Profile '{}' does not exist", name);
        }
        std::fs::create_dir_all(parent_hakimi_home(&self.profiles_dir))?;
        std::fs::write(
            active_profile_path_from_profiles_dir(&self.profiles_dir),
            name,
        )?;
        self.active = Some(name.to_string());
        info!(name = %name, "Switched to profile");
        Ok(profile_dir)
    }

    /// Clear the sticky profile and return to the default Hakimi home.
    pub fn use_default(&mut self) -> anyhow::Result<PathBuf> {
        let home = parent_hakimi_home(&self.profiles_dir);
        let _ = std::fs::remove_file(active_profile_path_from_profiles_dir(&self.profiles_dir));
        self.active = None;
        Ok(home.to_path_buf())
    }

    /// Get the active profile name.
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Get the profile directory for a given name.
    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_dir.join(name)
    }

    /// Get the default Hakimi home directory that owns this profile store.
    pub fn default_home(&self) -> PathBuf {
        parent_hakimi_home(&self.profiles_dir).to_path_buf()
    }

    /// Check if a profile exists.
    pub fn exists(&self, name: &str) -> bool {
        if validate_profile_name(name).is_err() {
            return false;
        }
        self.profiles_dir.join(name).exists()
    }
}

/// Validate the on-disk profile identifier.
pub fn validate_profile_name(name: &str) -> anyhow::Result<()> {
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
        anyhow::bail!("Profile names must match [a-z0-9][a-z0-9_-]{{0,63}}");
    }
}

/// Format a Hermes-style profile management response for CLI or gateway use.
pub fn profile_response(args: &[String], hakimi_home: &Path) -> String {
    let mut manager = ProfileManager::new(hakimi_home);
    match args.first().map(|s| s.as_str()).unwrap_or("list") {
        "help" | "-h" | "--help" => profile_usage(),
        "list" => profile_list_response(&manager),
        "current" | "status" => profile_current_response(&manager),
        "path" => profile_path_response(&manager, args.get(1).map(String::as_str)),
        "create" => {
            let Some(name) = args.get(1) else {
                return "Usage: profile create <name> [description]".to_string();
            };
            let description = args.get(2..).and_then(|parts| {
                let joined = parts.join(" ");
                (!joined.trim().is_empty()).then_some(joined)
            });
            match manager.create(name, description.as_deref()) {
                Ok(path) => format!("Profile `{name}` created at {}", path.display()),
                Err(err) => format!("Failed to create profile `{name}`: {err}"),
            }
        }
        "delete" | "remove" | "rm" => {
            let Some(name) = args.get(1) else {
                return "Usage: profile delete <name>".to_string();
            };
            match manager.delete(name) {
                Ok(()) => format!("Profile `{name}` deleted."),
                Err(err) => format!("Failed to delete profile `{name}`: {err}"),
            }
        }
        "use" | "switch" => {
            let Some(name) = args.get(1) else {
                return "Usage: profile use <name|default>".to_string();
            };
            if name == "default" {
                return match manager.use_default() {
                    Ok(path) => format!(
                        "Active profile cleared. Using default at {}",
                        path.display()
                    ),
                    Err(err) => format!("Failed to switch to default profile: {err}"),
                };
            }
            match manager.use_profile(name) {
                Ok(path) => format!("Active profile set to `{name}` at {}", path.display()),
                Err(err) => format!("Failed to use profile `{name}`: {err}"),
            }
        }
        command => format!(
            "Unknown profile command: `{command}`\n\n{}",
            profile_usage()
        ),
    }
}

/// Parse a raw slash-command tail and format a profile response.
pub fn profile_response_from_raw(raw: Option<&str>, hakimi_home: &Path) -> String {
    let args = raw
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect::<Vec<_>>();
    profile_response(&args, hakimi_home)
}

fn profile_list_response(manager: &ProfileManager) -> String {
    match manager.list() {
        Ok(profiles) if profiles.is_empty() => {
            "No named profiles found. Use `profile create <name>` to create one.".to_string()
        }
        Ok(profiles) => {
            let active = manager.active();
            let mut out = String::from("Profiles:\n");
            for profile in profiles {
                let marker = if active == Some(profile.name.as_str()) {
                    "*"
                } else {
                    "-"
                };
                let description = profile
                    .description
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!(" - {value}"))
                    .unwrap_or_default();
                out.push_str(&format!("{marker} `{}`{description}\n", profile.name));
            }
            if active.is_none() {
                out.push_str("\nActive: `default`\n");
            }
            out.trim_end().to_string()
        }
        Err(err) => format!("Failed to list profiles: {err}"),
    }
}

fn profile_current_response(manager: &ProfileManager) -> String {
    if let Some(active) = manager.active() {
        format!(
            "Active profile: `{active}`\nPath: {}",
            manager.profile_dir(active).display()
        )
    } else {
        format!(
            "Active profile: `default`\nPath: {}",
            manager.default_home().display()
        )
    }
}

fn profile_path_response(manager: &ProfileManager, name: Option<&str>) -> String {
    match name.or_else(|| manager.active()) {
        None | Some("default") => manager.default_home().display().to_string(),
        Some(profile) if manager.exists(profile) => {
            manager.profile_dir(profile).display().to_string()
        }
        Some(profile) => format!("Profile `{profile}` does not exist."),
    }
}

fn profile_usage() -> String {
    "Usage: profile <list|current|path|create|use|delete>\n\
     Examples:\n\
     - profile list\n\
     - profile create coder Coding workspace\n\
     - profile use coder\n\
     - profile use default\n\
     - profile delete coder"
        .to_string()
}

fn read_active_profile(hakimi_home: &Path) -> Option<String> {
    std::fs::read_to_string(hakimi_home.join(ACTIVE_PROFILE_FILE))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| validate_profile_name(value).is_ok())
}

fn active_profile_path_from_profiles_dir(profiles_dir: &Path) -> PathBuf {
    parent_hakimi_home(profiles_dir).join(ACTIVE_PROFILE_FILE)
}

fn parent_hakimi_home(profiles_dir: &Path) -> &Path {
    profiles_dir.parent().unwrap_or(profiles_dir)
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
    fn test_rejects_path_traversal_profile_name() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());

        assert!(manager.create("../escape", None).is_err());
        assert!(manager.create("BadName", None).is_err());
        assert!(manager.create("work.profile", None).is_err());
        assert!(manager.create("default", None).is_err());
    }

    #[test]
    fn test_use_profile_persists_active_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manager = ProfileManager::new(tmp.path());

        manager.create("work", None).unwrap();
        manager.use_profile("work").unwrap();

        let fresh = ProfileManager::new(tmp.path());
        assert_eq!(fresh.active(), Some("work"));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("active_profile")).unwrap(),
            "work"
        );
    }

    #[test]
    fn test_use_default_clears_active_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manager = ProfileManager::new(tmp.path());

        manager.create("work", None).unwrap();
        manager.use_profile("work").unwrap();
        manager.use_default().unwrap();

        let fresh = ProfileManager::new(tmp.path());
        assert!(fresh.active().is_none());
        assert!(!tmp.path().join("active_profile").exists());
    }

    #[test]
    fn test_profile_response_create_use_list_and_path() {
        let tmp = tempfile::tempdir().unwrap();

        let create = profile_response(
            &[
                "create".to_string(),
                "coder".to_string(),
                "Coding".to_string(),
                "workspace".to_string(),
            ],
            tmp.path(),
        );
        assert!(create.contains("Profile `coder` created"));

        let use_profile = profile_response(&["use".to_string(), "coder".to_string()], tmp.path());
        assert!(use_profile.contains("Active profile set to `coder`"));

        let list = profile_response(&["list".to_string()], tmp.path());
        assert!(list.contains("* `coder` - Coding workspace"));

        let path = profile_response_from_raw(Some("path coder"), tmp.path());
        assert!(path.ends_with("profiles\\coder") || path.ends_with("profiles/coder"));
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

    #[test]
    fn test_list_empty_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());

        // List should return an empty vec when the profiles dir doesn't exist yet
        let profiles = manager.list().unwrap();
        assert!(profiles.is_empty());
        assert_eq!(profiles.len(), 0);

        // Create the profiles dir but don't add any profiles
        fs::create_dir_all(tmp.path().join("profiles")).unwrap();
        let profiles = manager.list().unwrap();
        assert!(profiles.is_empty());
        assert_eq!(profiles.len(), 0);

        // Add one profile and list should return exactly one
        manager.create("alpha", Some("first")).unwrap();
        let profiles = manager.list().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "alpha");
    }

    #[test]
    fn test_use_nonexistent_profile_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manager = ProfileManager::new(tmp.path());

        // Trying to use a profile when no profiles exist should fail
        let result = manager.use_profile("ghost");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("ghost"));
        assert!(err_msg.contains("does not exist"));

        // Active profile should remain None after a failed use
        assert!(manager.active().is_none());

        // Create one profile, then try to use a different one
        manager.create("real", None).unwrap();
        let result = manager.use_profile("fake");
        assert!(result.is_err());
        assert!(manager.active().is_none());

        // The real profile should still be usable
        let dir = manager.use_profile("real").unwrap();
        assert!(dir.exists());
        assert_eq!(manager.active(), Some("real"));
    }

    #[test]
    fn test_get_active_profile_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manager = ProfileManager::new(tmp.path());

        // profile_dir returns the expected path for any name
        let expected = tmp.path().join("profiles").join("myprofile");
        assert_eq!(manager.profile_dir("myprofile"), expected);

        // No active profile initially
        assert!(manager.active().is_none());

        // Create and activate a profile, then verify the dir
        manager.create("myprofile", Some("test profile")).unwrap();
        let dir = manager.use_profile("myprofile").unwrap();

        // The dir returned by use_profile should match profile_dir
        assert_eq!(dir, manager.profile_dir("myprofile"));

        // The dir should exist and contain expected subdirs
        assert!(dir.exists());
        assert!(dir.join("memory").exists());
        assert!(dir.join("sessions").exists());
        assert!(dir.join("skills").exists());
        assert!(dir.join("profile.yaml").exists());

        // active() should report the correct name
        assert_eq!(manager.active(), Some("myprofile"));

        // Switch to a second profile and verify the dir changes
        manager.create("other", None).unwrap();
        let dir2 = manager.use_profile("other").unwrap();
        assert_eq!(dir2, manager.profile_dir("other"));
        assert_ne!(dir, dir2);
        assert_eq!(manager.active(), Some("other"));
    }
}
