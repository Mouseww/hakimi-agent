//! Multi-profile support for Hakimi Agent.
//!
//! Each profile has its own config, memory, sessions, and skills directory.

use anyhow::{Context, Result, bail};
use flate2::{Compression, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};
use tar::Builder;
use tracing::info;

const ACTIVE_PROFILE_FILE: &str = "active_profile";
const PROFILE_CONFIG_FILES: &[&str] = &["config.yaml", ".env", "SOUL.md"];
const PROFILE_MEMORY_FILES: &[&str] = &["memory/memory.md", "memory/user.md"];
const PROFILE_DIRS: &[&str] = &["memory", "sessions", "skills"];
const CLONE_ROOT_EXCLUDES: &[&str] = &["profiles", "bin", "node_modules", "target", ".git"];
const EXPORT_ROOT_EXCLUDES: &[&str] = &["profiles", "bin", "node_modules", "target", ".git"];
const RUNTIME_NAMES: &[&str] = &[
    "active_profile",
    "gateway.pid",
    "cron.pid",
    "processes.json",
    "gateway_state.json",
];
const CREDENTIAL_NAMES: &[&str] = &[".env", "auth.json", "credentials.json", "bws_cache.json"];
const TRANSIENT_DIRS: &[&str] = &[
    "__pycache__",
    "logs",
    "checkpoints",
    "image_cache",
    "audio_cache",
    "document_cache",
    "browser_screenshots",
    "sandboxes",
];
const TRANSIENT_SUFFIXES: &[&str] = &[
    ".db-wal",
    ".db-shm",
    ".db-journal",
    ".sock",
    ".tmp",
    ".pyc",
    ".pyo",
];

/// How a new profile should be seeded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProfileCloneMode {
    /// Create an empty profile with the standard directories.
    #[default]
    Empty,
    /// Copy config, identity, memory, and skills from a source profile.
    Config,
    /// Copy the full source profile tree, excluding sibling profiles and runtime files.
    Full,
}

/// Options for creating a profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileCreateOptions {
    /// Optional description written to profile metadata.
    pub description: Option<String>,
    /// Optional source profile. Defaults to the active profile, or default.
    pub clone_from: Option<String>,
    /// Clone mode.
    pub clone_mode: ProfileCloneMode,
}

/// Summary for a profile export archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileExportSummary {
    pub path: PathBuf,
    pub profile: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub skipped_count: usize,
}

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
    pub fn create(&self, name: &str, description: Option<&str>) -> Result<PathBuf> {
        self.create_with_options(
            name,
            ProfileCreateOptions {
                description: description.map(String::from),
                ..Default::default()
            },
        )
    }

    /// Create a new profile with clone/export parity options.
    pub fn create_with_options(
        &self,
        name: &str,
        options: ProfileCreateOptions,
    ) -> Result<PathBuf> {
        validate_profile_name(name)?;
        let profile_dir = self.profiles_dir.join(name);
        if profile_dir.exists() {
            bail!("Profile '{}' already exists", name);
        }

        let clone_source = match options.clone_mode {
            ProfileCloneMode::Empty => None,
            ProfileCloneMode::Config | ProfileCloneMode::Full => {
                Some(self.source_profile_dir(options.clone_from.as_deref())?.1)
            }
        };

        match options.clone_mode {
            ProfileCloneMode::Empty => create_profile_dirs(&profile_dir)?,
            ProfileCloneMode::Config => {
                create_profile_dirs(&profile_dir)?;
                let Some(source_dir) = clone_source.as_deref() else {
                    bail!("clone source was not resolved");
                };
                clone_config_files(&source_dir, &profile_dir)?;
            }
            ProfileCloneMode::Full => {
                let Some(source_dir) = clone_source.as_deref() else {
                    bail!("clone source was not resolved");
                };
                copy_dir_filtered(&source_dir, &profile_dir, CopyMode::CloneAll, true)?;
                strip_runtime_files(&profile_dir)?;
            }
        }

        let meta = ProfileMeta {
            name: name.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            description: options.description,
        };

        let meta_path = profile_dir.join("profile.yaml");
        let yaml = serde_yaml::to_string(&meta)?;
        fs::write(meta_path, yaml)?;

        info!(name = %name, "Profile created");
        Ok(profile_dir)
    }

    /// Delete a profile.
    pub fn delete(&self, name: &str) -> Result<()> {
        validate_profile_name(name)?;
        let profile_dir = self.profiles_dir.join(name);
        if !profile_dir.exists() {
            bail!("Profile '{}' does not exist", name);
        }
        fs::remove_dir_all(&profile_dir)?;
        if self.active.as_deref() == Some(name) {
            let _ = fs::remove_file(active_profile_path_from_profiles_dir(&self.profiles_dir));
        }
        info!(name = %name, "Profile deleted");
        Ok(())
    }

    /// List all profiles.
    pub fn list(&self) -> Result<Vec<ProfileMeta>> {
        if !self.profiles_dir.exists() {
            return Ok(Vec::new());
        }

        let mut profiles = Vec::new();
        for entry in fs::read_dir(&self.profiles_dir)? {
            let entry = entry?;
            let meta_path = entry.path().join("profile.yaml");
            if meta_path.exists() {
                let content = fs::read_to_string(&meta_path)?;
                if let Ok(meta) = serde_yaml::from_str::<ProfileMeta>(&content) {
                    profiles.push(meta);
                }
            }
        }
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    /// Set the active profile.
    pub fn use_profile(&mut self, name: &str) -> Result<PathBuf> {
        validate_profile_name(name)?;
        let profile_dir = self.profiles_dir.join(name);
        if !profile_dir.exists() {
            bail!("Profile '{}' does not exist", name);
        }
        fs::create_dir_all(parent_hakimi_home(&self.profiles_dir))?;
        fs::write(
            active_profile_path_from_profiles_dir(&self.profiles_dir),
            name,
        )?;
        self.active = Some(name.to_string());
        info!(name = %name, "Switched to profile");
        Ok(profile_dir)
    }

    /// Clear the sticky profile and return to the default Hakimi home.
    pub fn use_default(&mut self) -> Result<PathBuf> {
        let home = parent_hakimi_home(&self.profiles_dir);
        let _ = fs::remove_file(active_profile_path_from_profiles_dir(&self.profiles_dir));
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

    /// Export a profile to a tar.gz archive, excluding credentials and runtime files.
    pub fn export(&self, name: &str, output: Option<&Path>) -> Result<ProfileExportSummary> {
        let (profile, profile_dir) = self.existing_profile_dir(name)?;
        let out_path = expand_profile_export_output(output, &profile);
        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut entries = Vec::new();
        let mut skipped_count = 0;
        collect_profile_entries(
            &profile_dir,
            Path::new(""),
            CopyMode::Export,
            true,
            &out_path,
            &mut entries,
            &mut skipped_count,
        )?;
        if entries.is_empty() {
            bail!("Profile '{}' has no exportable files", profile);
        }

        let file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create {}", out_path.display()))?;
        let encoder = GzEncoder::new(file, Compression::default());
        let mut archive = Builder::new(encoder);
        let mut total_bytes = 0;

        for (abs_path, rel_path) in &entries {
            let archive_path = Path::new(&profile).join(rel_path);
            archive
                .append_path_with_name(abs_path, &archive_path)
                .with_context(|| format!("failed to archive {}", rel_path.display()))?;
            total_bytes += abs_path.metadata()?.len();
        }

        archive.finish()?;
        let encoder = archive.into_inner()?;
        encoder.finish()?;

        Ok(ProfileExportSummary {
            path: out_path,
            profile,
            file_count: entries.len(),
            total_bytes,
            skipped_count,
        })
    }

    fn source_profile_dir(&self, source: Option<&str>) -> Result<(String, PathBuf)> {
        match source.or(self.active.as_deref()) {
            None | Some("default") => {
                let home = self.default_home();
                ensure_profile_source_exists("default", &home)?;
                Ok(("default".to_string(), home))
            }
            Some(profile) => self.existing_profile_dir(profile),
        }
    }

    fn existing_profile_dir(&self, name: &str) -> Result<(String, PathBuf)> {
        if name == "default" {
            let home = self.default_home();
            ensure_profile_source_exists("default", &home)?;
            return Ok(("default".to_string(), home));
        }
        validate_profile_name(name)?;
        let profile_dir = self.profile_dir(name);
        ensure_profile_source_exists(name, &profile_dir)?;
        Ok((name.to_string(), profile_dir))
    }
}

/// Validate the on-disk profile identifier.
pub fn validate_profile_name(name: &str) -> Result<()> {
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

/// Format a Hermes-style profile management response for CLI or gateway use.
pub fn profile_response(args: &[String], hakimi_home: &Path) -> String {
    let mut manager = ProfileManager::new(hakimi_home);
    match args.first().map(|s| s.as_str()).unwrap_or("list") {
        "help" | "-h" | "--help" => profile_usage(),
        "list" => profile_list_response(&manager),
        "current" | "status" => profile_current_response(&manager),
        "path" => profile_path_response(&manager, args.get(1).map(String::as_str)),
        "create" => {
            let (name, options) = match parse_profile_create_args(&args[1..]) {
                Ok(parsed) => parsed,
                Err(err) => return err.to_string(),
            };
            match manager.create_with_options(&name, options) {
                Ok(path) => format!("Profile `{name}` created at {}", path.display()),
                Err(err) => format!("Failed to create profile `{name}`: {err}"),
            }
        }
        "export" => {
            let (name, output) = match parse_profile_export_args(&args[1..]) {
                Ok(parsed) => parsed,
                Err(err) => return err.to_string(),
            };
            match manager.export(&name, output.as_deref()) {
                Ok(summary) => format!(
                    "Profile `{}` exported to {}\n  Files: {}\n  Original: {} bytes\n  Skipped: {}",
                    summary.profile,
                    summary.path.display(),
                    summary.file_count,
                    summary.total_bytes,
                    summary.skipped_count
                ),
                Err(err) => format!("Failed to export profile `{name}`: {err}"),
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
    "Usage: profile <list|current|path|create|export|use|delete>\n\
     Examples:\n\
     - profile list\n\
     - profile create coder Coding workspace\n\
     - profile create review --clone=default Review workspace\n\
     - profile create fullcopy --clone-all --from coder\n\
     - profile export coder ./coder.tar.gz\n\
     - profile use coder\n\
     - profile use default\n\
     - profile delete coder"
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyMode {
    CloneAll,
    Export,
}

fn parse_profile_create_args(args: &[String]) -> Result<(String, ProfileCreateOptions)> {
    let Some(name) = args.first() else {
        bail!("Usage: profile create <name> [--clone[=source]|--clone-all[=source]] [description]");
    };

    let mut options = ProfileCreateOptions::default();
    let mut description_parts = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let token = &args[index];
        match token.as_str() {
            "--clone" => {
                set_clone_mode(&mut options, ProfileCloneMode::Config)?;
            }
            "--clone-all" => {
                set_clone_mode(&mut options, ProfileCloneMode::Full)?;
            }
            "--from" | "--clone-from" => {
                index += 1;
                let Some(source) = args.get(index) else {
                    bail!("{token} requires a source profile");
                };
                options.clone_from = Some(source.clone());
            }
            "--description" => {
                let rest = args[index + 1..].join(" ");
                if rest.trim().is_empty() {
                    bail!("--description requires text");
                }
                description_parts.push(rest);
                break;
            }
            value if value.starts_with("--clone=") => {
                set_clone_mode(&mut options, ProfileCloneMode::Config)?;
                options.clone_from = Some(value.trim_start_matches("--clone=").to_string());
            }
            value if value.starts_with("--clone-all=") => {
                set_clone_mode(&mut options, ProfileCloneMode::Full)?;
                options.clone_from = Some(value.trim_start_matches("--clone-all=").to_string());
            }
            value if value.starts_with('-') => bail!("Unknown profile create option: {value}"),
            value => description_parts.push(value.to_string()),
        }
        index += 1;
    }

    if !description_parts.is_empty() {
        options.description = Some(description_parts.join(" "));
    }

    Ok((name.clone(), options))
}

fn set_clone_mode(options: &mut ProfileCreateOptions, mode: ProfileCloneMode) -> Result<()> {
    if options.clone_mode != ProfileCloneMode::Empty && options.clone_mode != mode {
        bail!("--clone and --clone-all are mutually exclusive");
    }
    options.clone_mode = mode;
    Ok(())
}

fn parse_profile_export_args(args: &[String]) -> Result<(String, Option<PathBuf>)> {
    let Some(name) = args.first() else {
        bail!("Usage: profile export <name|default> [output|--output <path>]");
    };

    let mut output = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "-o" | "--output" => {
                index += 1;
                let Some(path) = args.get(index) else {
                    bail!("{} requires an output path", args[index - 1]);
                };
                output = Some(PathBuf::from(path));
            }
            value if value.starts_with('-') => bail!("Unknown profile export option: {value}"),
            value if output.is_none() => output = Some(PathBuf::from(value)),
            value => bail!("Unexpected profile export argument: {value}"),
        }
        index += 1;
    }

    Ok((name.clone(), output))
}

fn create_profile_dirs(profile_dir: &Path) -> Result<()> {
    fs::create_dir_all(profile_dir)?;
    for dir in PROFILE_DIRS {
        fs::create_dir_all(profile_dir.join(dir))?;
    }
    Ok(())
}

fn clone_config_files(source_dir: &Path, profile_dir: &Path) -> Result<()> {
    for file in PROFILE_CONFIG_FILES {
        copy_profile_file_if_present(source_dir, profile_dir, Path::new(file))?;
    }
    for file in PROFILE_MEMORY_FILES {
        copy_profile_file_if_present(source_dir, profile_dir, Path::new(file))?;
    }

    let source_skills = source_dir.join("skills");
    if source_skills.is_dir() {
        copy_dir_filtered(
            &source_skills,
            &profile_dir.join("skills"),
            CopyMode::CloneAll,
            false,
        )?;
    }

    Ok(())
}

fn copy_profile_file_if_present(
    source_dir: &Path,
    profile_dir: &Path,
    rel_path: &Path,
) -> Result<()> {
    let source = source_dir.join(rel_path);
    if !source.is_file() {
        return Ok(());
    }
    let target = profile_dir.join(rel_path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&source, &target).with_context(|| format!("failed to copy {}", rel_path.display()))?;
    Ok(())
}

fn copy_dir_filtered(
    source_dir: &Path,
    target_dir: &Path,
    mode: CopyMode,
    root_level: bool,
) -> Result<()> {
    let mut entries = Vec::new();
    let mut skipped_count = 0;
    collect_profile_entries(
        source_dir,
        Path::new(""),
        mode,
        root_level,
        Path::new(""),
        &mut entries,
        &mut skipped_count,
    )?;

    for (source, rel_path) in entries {
        let target = target_dir.join(&rel_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, &target)
            .with_context(|| format!("failed to copy {}", rel_path.display()))?;
    }

    Ok(())
}

fn collect_profile_entries(
    dir: &Path,
    rel_dir: &Path,
    mode: CopyMode,
    root_level: bool,
    output_path: &Path,
    entries: &mut Vec<(PathBuf, PathBuf)>,
    skipped_count: &mut usize,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let rel_path = rel_dir.join(&file_name);
        let abs_path = entry.path();
        let is_root_entry = root_level && rel_dir.as_os_str().is_empty();

        if should_skip_profile_entry(&rel_path, mode, is_root_entry)
            || points_to_output(&abs_path, output_path)
        {
            *skipped_count += 1;
            continue;
        }

        let meta = fs::symlink_metadata(&abs_path)?;
        if meta.file_type().is_symlink() {
            *skipped_count += 1;
            continue;
        }
        if meta.is_dir() {
            collect_profile_entries(
                &abs_path,
                &rel_path,
                mode,
                root_level,
                output_path,
                entries,
                skipped_count,
            )?;
        } else if meta.is_file() {
            entries.push((abs_path, rel_path));
        } else {
            *skipped_count += 1;
        }
    }

    Ok(())
}

fn should_skip_profile_entry(rel_path: &Path, mode: CopyMode, root_entry: bool) -> bool {
    has_unsafe_component(rel_path)
        || name_in(rel_path, RUNTIME_NAMES)
        || path_has_dir(rel_path, TRANSIENT_DIRS)
        || suffix_in(rel_path, TRANSIENT_SUFFIXES)
        || (root_entry
            && match mode {
                CopyMode::CloneAll => name_in(rel_path, CLONE_ROOT_EXCLUDES),
                CopyMode::Export => name_in(rel_path, EXPORT_ROOT_EXCLUDES),
            })
        || (mode == CopyMode::Export && name_in(rel_path, CREDENTIAL_NAMES))
}

fn strip_runtime_files(profile_dir: &Path) -> Result<()> {
    for name in RUNTIME_NAMES {
        let path = profile_dir.join(name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

fn ensure_profile_source_exists(name: &str, path: &Path) -> Result<()> {
    if path.is_dir() {
        Ok(())
    } else {
        bail!("Profile '{}' does not exist at {}", name, path.display())
    }
}

fn expand_profile_export_output(output: Option<&Path>, profile: &str) -> PathBuf {
    let default_name = format!(
        "hakimi-profile-{profile}-{}.tar.gz",
        chrono::Local::now().format("%Y-%m-%d-%H%M%S")
    );
    let raw = output.map(expand_home).unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(&default_name)
    });

    if raw.is_dir() {
        raw.join(default_name)
    } else if raw.extension().is_some() {
        raw
    } else {
        raw.with_extension("tar.gz")
    }
}

fn expand_home(path: &Path) -> PathBuf {
    let raw = path.as_os_str().to_string_lossy();
    if raw == "~" {
        return dirs::home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

fn points_to_output(abs_path: &Path, output_path: &Path) -> bool {
    if output_path.as_os_str().is_empty() {
        return false;
    }
    match (abs_path.canonicalize(), output_path.canonicalize()) {
        (Ok(abs), Ok(out)) => abs == out,
        _ => false,
    }
}

fn has_unsafe_component(rel_path: &Path) -> bool {
    rel_path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    })
}

fn path_has_dir(rel_path: &Path, names: &[&str]) -> bool {
    rel_path.components().any(|component| {
        let Component::Normal(part) = component else {
            return false;
        };
        names.iter().any(|name| part == OsStr::new(name))
    })
}

fn name_in(rel_path: &Path, names: &[&str]) -> bool {
    rel_path
        .file_name()
        .is_some_and(|name| names.iter().any(|expected| name == OsStr::new(expected)))
}

fn suffix_in(rel_path: &Path, suffixes: &[&str]) -> bool {
    let Some(name) = rel_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    suffixes.iter().any(|suffix| name.ends_with(suffix))
}

fn read_active_profile(hakimi_home: &Path) -> Option<String> {
    fs::read_to_string(hakimi_home.join(ACTIVE_PROFILE_FILE))
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

    #[test]
    fn test_clone_profile_copies_config_identity_memory_and_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());

        fs::create_dir_all(tmp.path().join("memory")).unwrap();
        fs::create_dir_all(tmp.path().join("sessions")).unwrap();
        fs::create_dir_all(tmp.path().join("skills/writer")).unwrap();
        fs::write(tmp.path().join("config.yaml"), "model: source\n").unwrap();
        fs::write(tmp.path().join(".env"), "SECRET=source\n").unwrap();
        fs::write(tmp.path().join("SOUL.md"), "source soul\n").unwrap();
        fs::write(tmp.path().join("memory/memory.md"), "agent memory\n").unwrap();
        fs::write(tmp.path().join("memory/user.md"), "user memory\n").unwrap();
        fs::write(tmp.path().join("skills/writer/SKILL.md"), "skill body\n").unwrap();
        fs::write(tmp.path().join("sessions/session.json"), "{}\n").unwrap();

        let dir = manager
            .create_with_options(
                "clone",
                ProfileCreateOptions {
                    clone_from: Some("default".to_string()),
                    clone_mode: ProfileCloneMode::Config,
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("config.yaml")).unwrap(),
            "model: source\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join("memory/memory.md")).unwrap(),
            "agent memory\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join("skills/writer/SKILL.md")).unwrap(),
            "skill body\n"
        );
        assert!(!dir.join("sessions/session.json").exists());
    }

    #[test]
    fn test_clone_all_copies_state_but_excludes_runtime_and_siblings() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());

        fs::create_dir_all(tmp.path().join("profiles/sibling")).unwrap();
        fs::create_dir_all(tmp.path().join("bin")).unwrap();
        fs::create_dir_all(tmp.path().join("logs")).unwrap();
        fs::create_dir_all(tmp.path().join("sessions")).unwrap();
        fs::write(tmp.path().join("config.yaml"), "model: source\n").unwrap();
        fs::write(tmp.path().join("sessions/session.json"), "{}\n").unwrap();
        fs::write(tmp.path().join("active_profile"), "old\n").unwrap();
        fs::write(tmp.path().join("gateway.pid"), "123\n").unwrap();
        fs::write(
            tmp.path().join("profiles/sibling/profile.yaml"),
            "name: sibling\n",
        )
        .unwrap();
        fs::write(tmp.path().join("bin/hakimi"), "binary\n").unwrap();
        fs::write(tmp.path().join("logs/gateway.log"), "log\n").unwrap();

        let dir = manager
            .create_with_options(
                "full",
                ProfileCreateOptions {
                    clone_from: Some("default".to_string()),
                    clone_mode: ProfileCloneMode::Full,
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("config.yaml")).unwrap(),
            "model: source\n"
        );
        assert!(dir.join("sessions/session.json").exists());
        assert!(!dir.join("active_profile").exists());
        assert!(!dir.join("gateway.pid").exists());
        assert!(!dir.join("profiles/sibling/profile.yaml").exists());
        assert!(!dir.join("bin/hakimi").exists());
        assert!(!dir.join("logs/gateway.log").exists());
    }

    #[test]
    fn test_clone_missing_source_does_not_leave_target_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());

        let result = manager.create_with_options(
            "copy",
            ProfileCreateOptions {
                clone_from: Some("missing".to_string()),
                clone_mode: ProfileCloneMode::Config,
                ..Default::default()
            },
        );

        assert!(result.is_err());
        assert!(!tmp.path().join("profiles/copy").exists());
    }

    #[test]
    fn test_export_profile_excludes_credentials_runtime_and_caches() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(tmp.path());
        let profile_dir = manager.create("work", Some("Work")).unwrap();

        fs::create_dir_all(profile_dir.join("memory")).unwrap();
        fs::create_dir_all(profile_dir.join("logs")).unwrap();
        fs::write(profile_dir.join("config.yaml"), "model: work\n").unwrap();
        fs::write(profile_dir.join(".env"), "SECRET=work\n").unwrap();
        fs::write(profile_dir.join("auth.json"), "{}\n").unwrap();
        fs::write(profile_dir.join("gateway.pid"), "123\n").unwrap();
        fs::write(profile_dir.join("memory/memory.md"), "notes\n").unwrap();
        fs::write(profile_dir.join("logs/gateway.log"), "log\n").unwrap();

        let out = tmp.path().join("work-export.tar.gz");
        let summary = manager.export("work", Some(&out)).unwrap();
        let names = names_in_archive(&summary.path);

        assert!(names.contains(&"work/profile.yaml".to_string()));
        assert!(names.contains(&"work/config.yaml".to_string()));
        assert!(names.contains(&"work/memory/memory.md".to_string()));
        assert!(!names.contains(&"work/.env".to_string()));
        assert!(!names.contains(&"work/auth.json".to_string()));
        assert!(!names.contains(&"work/gateway.pid".to_string()));
        assert!(!names.contains(&"work/logs/gateway.log".to_string()));
        assert!(summary.skipped_count >= 4);
    }

    #[test]
    fn test_profile_response_create_clone_and_export() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("config.yaml"), "model: default\n").unwrap();

        let create = profile_response(
            &[
                "create".to_string(),
                "coder".to_string(),
                "--clone=default".to_string(),
                "Coding".to_string(),
            ],
            tmp.path(),
        );
        assert!(create.contains("Profile `coder` created"));
        assert_eq!(
            fs::read_to_string(tmp.path().join("profiles/coder/config.yaml")).unwrap(),
            "model: default\n"
        );

        let out = tmp.path().join("coder.tar.gz");
        let export = profile_response(
            &[
                "export".to_string(),
                "coder".to_string(),
                out.display().to_string(),
            ],
            tmp.path(),
        );
        assert!(export.contains("Profile `coder` exported"));
        assert!(out.exists());
    }

    fn names_in_archive(path: &Path) -> Vec<String> {
        let file = fs::File::open(path).unwrap();
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }
}
