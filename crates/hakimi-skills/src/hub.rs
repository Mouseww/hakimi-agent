use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::safety::scan_skill_text;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SkillHubIndex {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub skills: Vec<SkillHubEntry>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SkillHubEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub identifier: String,
    #[serde(default = "default_trust_level")]
    pub trust_level: String,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

impl SkillHubEntry {
    fn normalized(mut self) -> Self {
        self.name = normalize_skill_name_lossy(&self.name);
        self.source = normalize_label(&self.source).unwrap_or_else(default_source);
        self.trust_level =
            normalize_trust_level(&self.trust_level).unwrap_or_else(default_trust_level);
        self.identifier = normalize_identifier(&self.identifier)
            .unwrap_or_else(|| format!("{}/{}", self.source, self.name));
        self.category = self
            .category
            .and_then(|category| normalize_category_lossy(&category));
        self.tags = self
            .tags
            .into_iter()
            .filter_map(|tag| normalize_label(&tag))
            .collect();
        self
    }

    pub fn matches_query(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }
        let searchable = format!(
            "{} {} {} {} {}",
            self.name,
            self.description,
            self.source,
            self.identifier,
            self.tags.join(" ")
        )
        .to_ascii_lowercase();
        query
            .split_whitespace()
            .all(|word| searchable.contains(word))
    }

    pub fn trust_rank(&self) -> u8 {
        trust_rank(&self.trust_level)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledSkill {
    pub name: String,
    pub source: String,
    pub identifier: String,
    pub trust_level: String,
    pub install_path: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillHubInstallOptions {
    pub category: Option<String>,
    pub force: bool,
    pub allow_community: bool,
}

#[derive(Debug, Clone)]
pub struct SkillHubInstall {
    pub name: String,
    pub identifier: String,
    pub trust_level: String,
    pub install_path: PathBuf,
    pub content_hash: String,
}

#[derive(Debug, Clone)]
pub struct SkillHub {
    skills_dir: PathBuf,
    index_path: PathBuf,
}

impl SkillHub {
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        let skills_dir = skills_dir.into();
        let index_path = default_index_path(&skills_dir);
        Self {
            skills_dir,
            index_path,
        }
    }

    pub fn with_index_path(skills_dir: impl Into<PathBuf>, index_path: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
            index_path: index_path.into(),
        }
    }

    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    pub fn index_path(&self) -> &Path {
        &self.index_path
    }

    pub fn load_index(&self) -> Result<SkillHubIndex> {
        if !self.index_path.exists() {
            return Ok(SkillHubIndex::default());
        }
        let raw = std::fs::read_to_string(&self.index_path).with_context(|| {
            format!(
                "failed to read skills hub index: {}",
                self.index_path.display()
            )
        })?;
        let mut index: SkillHubIndex = serde_json::from_str(&raw).with_context(|| {
            format!(
                "failed to parse skills hub index: {}",
                self.index_path.display()
            )
        })?;
        index.skills = index
            .skills
            .into_iter()
            .map(SkillHubEntry::normalized)
            .filter(|entry| !entry.name.is_empty())
            .collect();
        Ok(index)
    }

    pub fn browse(&self, limit: usize) -> Result<Vec<SkillHubEntry>> {
        let mut entries = self.load_index()?.skills;
        sort_entries(&mut entries);
        entries.truncate(limit.max(1));
        Ok(entries)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SkillHubEntry>> {
        let mut entries: Vec<_> = self
            .load_index()?
            .skills
            .into_iter()
            .filter(|entry| entry.matches_query(query))
            .collect();
        sort_entries(&mut entries);
        entries.truncate(limit.max(1));
        Ok(entries)
    }

    pub fn inspect(&self, identifier_or_name: &str) -> Result<SkillHubEntry> {
        let index = self.load_index()?;
        resolve_entry(&index.skills, identifier_or_name).cloned()
    }

    pub fn install(
        &self,
        identifier_or_name: &str,
        options: SkillHubInstallOptions,
    ) -> Result<SkillHubInstall> {
        let entry = self.inspect(identifier_or_name)?;
        if entry.trust_level == "community" && !options.allow_community {
            bail!(
                "community skill `{}` requires explicit --trust-community before install",
                entry.identifier
            );
        }

        let skill_name = validate_skill_name(&entry.name)?;
        let category = options
            .category
            .or(entry.category.clone())
            .map(|category| validate_category(category.as_str()))
            .transpose()?;
        let files = normalize_bundle_files(&entry)?;
        let skill_md = files
            .get("SKILL.md")
            .ok_or_else(|| anyhow::anyhow!("skill `{}` has no SKILL.md file", entry.identifier))?;
        let safety = scan_skill_text(skill_md);
        if !safety.is_allowed() {
            bail!(
                "skill safety scan blocked `{}` ({})",
                entry.identifier,
                safety.summary()
            );
        }

        let install_rel = match category {
            Some(category) => format!("{category}/{skill_name}"),
            None => skill_name.clone(),
        };
        validate_relative_path(&install_rel, "install path", true)?;
        let install_dir = join_validated_relative(&self.skills_dir, &install_rel);
        ensure_safe_install_parent(&self.skills_dir, &install_rel)?;

        if install_dir.exists() {
            if !options.force {
                bail!(
                    "skill `{}` already exists at {}; use --force to replace it",
                    skill_name,
                    install_dir.display()
                );
            }
            remove_existing_install_dir(&self.skills_dir, &install_dir)?;
        }

        std::fs::create_dir_all(&install_dir)
            .with_context(|| format!("failed to create {}", install_dir.display()))?;
        for (rel_path, content) in &files {
            let target = join_validated_relative(&install_dir, rel_path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::write(&target, content)
                .with_context(|| format!("failed to write {}", target.display()))?;
        }

        let content_hash = content_hash(&files);
        record_lock_install(
            &self.lock_path(),
            &skill_name,
            &entry,
            &install_rel,
            &content_hash,
            files.keys().cloned().collect(),
        )?;
        append_audit_log(
            &self.audit_log_path(),
            "INSTALL",
            &skill_name,
            &entry.source,
            &entry.trust_level,
            &content_hash,
        )?;

        Ok(SkillHubInstall {
            name: skill_name,
            identifier: entry.identifier,
            trust_level: entry.trust_level,
            install_path: install_dir,
            content_hash,
        })
    }

    pub fn installed(&self) -> Result<Vec<InstalledSkill>> {
        read_installed_lock(&self.lock_path())
    }

    fn lock_path(&self) -> PathBuf {
        self.skills_dir.join(".hub").join("lock.json")
    }

    fn audit_log_path(&self) -> PathBuf {
        self.skills_dir.join(".hub").join("audit.log")
    }
}

pub fn default_index_path(skills_dir: &Path) -> PathBuf {
    skills_dir.join(".hub").join("index.json")
}

fn default_source() -> String {
    "local-index".to_string()
}

fn default_trust_level() -> String {
    "community".to_string()
}

fn sort_entries(entries: &mut [SkillHubEntry]) {
    entries.sort_by(|a, b| {
        b.trust_rank()
            .cmp(&a.trust_rank())
            .then_with(|| (a.source != "official").cmp(&(b.source != "official")))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
}

fn resolve_entry<'a>(
    entries: &'a [SkillHubEntry],
    identifier_or_name: &str,
) -> Result<&'a SkillHubEntry> {
    let query = identifier_or_name.trim();
    if query.is_empty() {
        bail!("skill identifier must not be empty");
    }
    if let Some(entry) = entries.iter().find(|entry| entry.identifier == query) {
        return Ok(entry);
    }

    let matches: Vec<_> = entries
        .iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case(query))
        .collect();
    match matches.as_slice() {
        [entry] => Ok(*entry),
        [] => bail!("skill `{query}` was not found in the hub index"),
        _ => bail!("skill name `{query}` is ambiguous; use a full identifier"),
    }
}

fn trust_rank(trust_level: &str) -> u8 {
    match trust_level {
        "builtin" => 3,
        "trusted" => 2,
        "community" => 1,
        _ => 0,
    }
}

fn normalize_identifier(value: &str) -> Option<String> {
    let trimmed = value.trim().replace('\\', "/");
    if trimmed.is_empty() || trimmed.contains("..") {
        return None;
    }
    let normalized = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/' | ':'))
        .collect::<String>();
    (!normalized.is_empty()).then_some(normalized)
}

fn normalize_label(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut last_dash = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let out = out.trim_matches('-');
    (!out.is_empty()).then(|| out.chars().take(120).collect())
}

fn normalize_trust_level(value: &str) -> Option<String> {
    match normalize_label(value)?.as_str() {
        "builtin" | "official" => Some("builtin".to_string()),
        "trusted" => Some("trusted".to_string()),
        "community" => Some("community".to_string()),
        _ => Some("community".to_string()),
    }
}

fn normalize_skill_name_lossy(value: &str) -> String {
    normalize_label(value).unwrap_or_default()
}

fn normalize_category_lossy(value: &str) -> Option<String> {
    validate_category(value).ok()
}

fn validate_skill_name(value: &str) -> Result<String> {
    let normalized = normalize_skill_name_lossy(value);
    if normalized.is_empty()
        || normalized.starts_with('.')
        || normalized.contains('/')
        || normalized.contains('\\')
        || normalized == "skill"
        || normalized == "readme"
    {
        bail!("invalid skill name `{value}`");
    }
    Ok(normalized)
}

fn validate_category(value: &str) -> Result<String> {
    validate_relative_path(value, "category", true)
}

fn validate_relative_path(value: &str, field: &str, allow_nested: bool) -> Result<String> {
    let normalized = value.trim().replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') {
        bail!("unsafe {field}: `{value}`");
    }
    let parts: Vec<_> = normalized
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if parts.is_empty()
        || parts.iter().any(|part| {
            *part == ".."
                || part.starts_with('.')
                || part.contains(':')
                || !part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        })
    {
        bail!("unsafe {field}: `{value}`");
    }
    if !allow_nested && parts.len() != 1 {
        bail!("unsafe {field}: `{value}`");
    }
    Ok(parts.join("/"))
}

fn join_validated_relative(root: &Path, rel_path: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for part in rel_path.split('/') {
        path.push(part);
    }
    path
}

fn normalize_bundle_files(entry: &SkillHubEntry) -> Result<BTreeMap<String, String>> {
    if entry.files.is_empty() {
        bail!("skill `{}` has no files", entry.identifier);
    }
    let mut files = BTreeMap::new();
    for (rel_path, content) in &entry.files {
        let rel_path = validate_relative_path(rel_path, "bundle file path", true)?;
        files.insert(rel_path, content.clone());
    }
    Ok(files)
}

fn ensure_safe_install_parent(skills_dir: &Path, install_rel: &str) -> Result<()> {
    let mut current = skills_dir.to_path_buf();
    let parts = install_rel.split('/').collect::<Vec<_>>();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        current.push(part);
        if let Ok(metadata) = std::fs::symlink_metadata(&current)
            && metadata.file_type().is_symlink()
        {
            bail!(
                "refusing to install through symlinked path {}",
                current.display()
            );
        }
    }
    Ok(())
}

fn remove_existing_install_dir(skills_dir: &Path, install_dir: &Path) -> Result<()> {
    if install_dir == skills_dir {
        bail!("refusing to remove skills root");
    }
    let metadata = std::fs::symlink_metadata(install_dir)
        .with_context(|| format!("failed to inspect {}", install_dir.display()))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "refusing to replace symlinked skill {}",
            install_dir.display()
        );
    }
    if !metadata.is_dir() {
        bail!(
            "refusing to replace non-directory {}",
            install_dir.display()
        );
    }
    std::fs::remove_dir_all(install_dir)
        .with_context(|| format!("failed to remove {}", install_dir.display()))
}

fn record_lock_install(
    lock_path: &Path,
    name: &str,
    entry: &SkillHubEntry,
    install_path: &str,
    content_hash: &str,
    files: Vec<String>,
) -> Result<()> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut root = match std::fs::read_to_string(lock_path) {
        Ok(raw) => serde_json::from_str::<JsonValue>(&raw).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    };
    if !root.is_object() {
        root = json!({});
    }
    let object = root.as_object_mut().expect("root object");
    object.entry("version").or_insert(json!(1));
    if !object.get("installed").is_some_and(JsonValue::is_object) {
        object.insert("installed".to_string(), json!({}));
    }
    let installed = object
        .get_mut("installed")
        .and_then(JsonValue::as_object_mut)
        .expect("installed object");
    installed.insert(
        name.to_string(),
        json!({
            "source": entry.source.as_str(),
            "identifier": entry.identifier.as_str(),
            "trust_level": entry.trust_level.as_str(),
            "repo": entry.repo.as_deref(),
            "created_by": entry.created_by.as_deref(),
            "install_path": install_path,
            "content_hash": content_hash,
            "files": files,
            "installed_at": unix_timestamp_string(),
        }),
    );
    let rendered = serde_json::to_string_pretty(&root)?;
    std::fs::write(lock_path, rendered + "\n")
        .with_context(|| format!("failed to write {}", lock_path.display()))
}

fn read_installed_lock(lock_path: &Path) -> Result<Vec<InstalledSkill>> {
    let raw = match std::fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(_) => return Ok(Vec::new()),
    };
    let value: JsonValue = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", lock_path.display()))?;
    let Some(installed) = value.get("installed").and_then(JsonValue::as_object) else {
        return Ok(Vec::new());
    };

    let mut items = Vec::new();
    for (name, entry) in installed {
        let source = entry
            .get("source")
            .and_then(JsonValue::as_str)
            .unwrap_or("local-index");
        let identifier = entry
            .get("identifier")
            .and_then(JsonValue::as_str)
            .unwrap_or(name);
        let trust_level = entry
            .get("trust_level")
            .and_then(JsonValue::as_str)
            .unwrap_or("community");
        let install_path = entry
            .get("install_path")
            .and_then(JsonValue::as_str)
            .unwrap_or(name);
        let content_hash = entry
            .get("content_hash")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        items.push(InstalledSkill {
            name: name.clone(),
            source: source.to_string(),
            identifier: identifier.to_string(),
            trust_level: trust_level.to_string(),
            install_path: install_path.to_string(),
            content_hash: content_hash.to_string(),
        });
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

fn append_audit_log(
    path: &Path,
    action: &str,
    skill_name: &str,
    source: &str,
    trust_level: &str,
    content_hash: &str,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let line = format!(
        "{}\t{}\t{}\t{}:{}\t{}\n",
        unix_timestamp_string(),
        action,
        skill_name,
        source,
        trust_level,
        content_hash
    );
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    file.write_all(line.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))
}

fn content_hash(files: &BTreeMap<String, String>) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for (path, content) in files {
        fn update(hash: &mut u64, bytes: &[u8]) {
            for byte in bytes {
                *hash ^= u64::from(*byte);
                *hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        update(&mut hash, path.as_bytes());
        update(&mut hash, &[0]);
        update(&mut hash, content.as_bytes());
        update(&mut hash, &[0]);
    }
    format!("fnv64:{hash:016x}")
}

fn unix_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_index(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("index.json");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn search_sorts_by_trust_then_name() {
        let tmp = TempDir::new().unwrap();
        let index = write_index(
            tmp.path(),
            r##"{
  "skills": [
    {"name":"z-community","description":"Rust help","source":"github","trust_level":"community","files":{"SKILL.md":"# Z"}},
    {"name":"a-official","description":"Rust help","source":"official","trust_level":"builtin","files":{"SKILL.md":"# A"}}
  ]
}"##,
        );
        let hub = SkillHub::with_index_path(tmp.path().join("skills"), index);

        let results = hub.search("rust", 10).unwrap();

        assert_eq!(results[0].name, "a-official");
        assert_eq!(results[1].name, "z-community");
    }

    #[test]
    fn install_blocks_community_without_explicit_trust() {
        let tmp = TempDir::new().unwrap();
        let index = write_index(
            tmp.path(),
            r##"{
  "skills": [
    {"name":"community-helper","description":"Help","source":"github","identifier":"owner/repo/community-helper","trust_level":"community","files":{"SKILL.md":"---\nname: community-helper\n---\n# Help"}}
  ]
}"##,
        );
        let hub = SkillHub::with_index_path(tmp.path().join("skills"), index);

        let err = hub
            .install("community-helper", SkillHubInstallOptions::default())
            .unwrap_err();

        assert!(err.to_string().contains("--trust-community"));
    }

    #[test]
    fn install_writes_skill_and_lock_metadata() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        let index = write_index(
            tmp.path(),
            r##"{
  "skills": [
    {
      "name":"release-check",
      "description":"Release checklist",
      "source":"official",
      "identifier":"official/release-check",
      "trust_level":"builtin",
      "repo":"NousResearch/hermes-agent",
      "category":"software",
      "files":{"SKILL.md":"---\nname: release-check\n---\n# Release\n- Check CI"}
    }
  ]
}"##,
        );
        let hub = SkillHub::with_index_path(skills_dir.clone(), index);

        let installed = hub
            .install("release-check", SkillHubInstallOptions::default())
            .unwrap();

        assert!(installed.install_path.join("SKILL.md").exists());
        assert_eq!(installed.trust_level, "builtin");
        let lock_items = hub.installed().unwrap();
        assert_eq!(lock_items.len(), 1);
        assert_eq!(lock_items[0].source, "official");
        assert_eq!(lock_items[0].install_path, "software/release-check");
        assert!(skills_dir.join(".hub").join("audit.log").exists());
    }

    #[test]
    fn install_rejects_traversal_files() {
        let tmp = TempDir::new().unwrap();
        let index = write_index(
            tmp.path(),
            r##"{
  "skills": [
    {"name":"bad","description":"Bad","source":"official","trust_level":"builtin","files":{"SKILL.md":"# Bad","../escape.txt":"no"}}
  ]
}"##,
        );
        let hub = SkillHub::with_index_path(tmp.path().join("skills"), index);

        let err = hub
            .install("bad", SkillHubInstallOptions::default())
            .unwrap_err();

        assert!(err.to_string().contains("bundle file path"));
    }
}
