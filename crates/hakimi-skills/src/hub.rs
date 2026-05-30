use std::collections::BTreeMap;
use std::env;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::safety::scan_skill_text;

const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
const GITHUB_API_BASE: &str = "https://api.github.com/repos";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SkillHubIndex {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub skills: Vec<SkillHubEntry>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SkillHubSources {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub sources: Vec<SkillHubSource>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SkillHubSource {
    pub name: String,
    pub location: String,
    #[serde(default = "default_trust_level")]
    pub trust_level: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillHubSourceRefresh {
    pub name: String,
    pub location: String,
    pub cached_path: PathBuf,
    pub skills: usize,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubSourceSpec {
    owner: String,
    repo: String,
    path: String,
    ref_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubContentEntry {
    name: String,
    path: String,
    #[serde(rename = "type")]
    kind: String,
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

    fn with_source_defaults(mut self, source: &SkillHubSource) -> Self {
        let source_name = normalize_label(&source.name).unwrap_or_else(default_source);
        if self.source.trim().is_empty() || self.source == "local-index" {
            self.source = source_name;
        }
        if self.trust_level.trim().is_empty() || self.trust_level == "community" {
            self.trust_level = source.trust_level.clone();
        }
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
        let mut index = read_index_file(&self.index_path)?;
        for cached_path in self.cached_index_paths()? {
            let cached = read_index_file(&cached_path)?;
            index.skills.extend(cached.skills);
        }
        index.skills = normalize_and_dedupe_entries(index.skills);
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

    pub fn sources(&self) -> Result<Vec<SkillHubSource>> {
        read_sources_file(&self.sources_path())
    }

    pub fn add_source(
        &self,
        name: &str,
        location: &str,
        trust_level: Option<&str>,
    ) -> Result<SkillHubSource> {
        let source = normalize_source(name, location, trust_level)?;
        let mut sources = self.sources()?;
        sources.retain(|existing| existing.name != source.name);
        sources.push(source.clone());
        sources.sort_by(|a, b| a.name.cmp(&b.name));
        write_sources_file(&self.sources_path(), &sources)?;
        Ok(source)
    }

    pub fn remove_source(&self, name: &str) -> Result<bool> {
        let normalized =
            normalize_label(name).ok_or_else(|| anyhow::anyhow!("invalid source name"))?;
        let mut sources = self.sources()?;
        let before = sources.len();
        sources.retain(|source| source.name != normalized);
        if sources.len() == before {
            return Ok(false);
        }
        write_sources_file(&self.sources_path(), &sources)?;
        let cache_path = self.cached_source_path(&normalized);
        if cache_path.exists() {
            std::fs::remove_file(&cache_path)
                .with_context(|| format!("failed to remove {}", cache_path.display()))?;
        }
        Ok(true)
    }

    pub fn refresh_sources(&self) -> Result<Vec<SkillHubSourceRefresh>> {
        let mut reports = Vec::new();
        for source in self.sources()? {
            let index = load_source_index(&source)?;
            let skills = index.skills.len();
            let cache_path = self.cached_source_path(&source.name);
            ensure_hub_ignore(&self.skills_dir)?;
            if let Some(parent) = cache_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let rendered = serde_json::to_string_pretty(&index)?;
            std::fs::write(&cache_path, rendered + "\n")
                .with_context(|| format!("failed to write {}", cache_path.display()))?;
            reports.push(SkillHubSourceRefresh {
                name: source.name,
                location: source.location,
                cached_path: cache_path,
                skills,
                status: "refreshed".to_string(),
            });
        }
        Ok(reports)
    }

    fn lock_path(&self) -> PathBuf {
        self.skills_dir.join(".hub").join("lock.json")
    }

    fn audit_log_path(&self) -> PathBuf {
        self.skills_dir.join(".hub").join("audit.log")
    }

    pub fn sources_path(&self) -> PathBuf {
        self.skills_dir.join(".hub").join("sources.json")
    }

    pub fn index_cache_dir(&self) -> PathBuf {
        self.skills_dir.join(".hub").join("index-cache")
    }

    fn cached_source_path(&self, name: &str) -> PathBuf {
        self.index_cache_dir().join(format!("{name}.json"))
    }

    fn cached_index_paths(&self) -> Result<Vec<PathBuf>> {
        let dir = self.index_cache_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let entry = entry.with_context(|| format!("failed to read {}", dir.display()))?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
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

fn read_index_file(path: &Path) -> Result<SkillHubIndex> {
    if !path.exists() {
        return Ok(SkillHubIndex::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read skills hub index: {}", path.display()))?;
    parse_index_json(&raw, path)
}

fn parse_index_json(raw: &str, path: &Path) -> Result<SkillHubIndex> {
    let mut index: SkillHubIndex = serde_json::from_str(raw)
        .with_context(|| format!("failed to parse skills hub index: {}", path.display()))?;
    index.skills = normalize_and_dedupe_entries(index.skills);
    Ok(index)
}

fn normalize_and_dedupe_entries(entries: Vec<SkillHubEntry>) -> Vec<SkillHubEntry> {
    let mut deduped: BTreeMap<String, SkillHubEntry> = BTreeMap::new();
    for entry in entries.into_iter().map(SkillHubEntry::normalized) {
        if entry.name.is_empty() {
            continue;
        }
        let key = entry.identifier.clone();
        match deduped.get(&key) {
            Some(existing) if existing.trust_rank() >= entry.trust_rank() => {}
            _ => {
                deduped.insert(key, entry);
            }
        }
    }
    deduped.into_values().collect()
}

fn read_sources_file(path: &Path) -> Result<Vec<SkillHubSource>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read skills hub sources: {}", path.display()))?;
    let parsed: SkillHubSources = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse skills hub sources: {}", path.display()))?;
    parsed
        .sources
        .into_iter()
        .map(|source| normalize_source(&source.name, &source.location, Some(&source.trust_level)))
        .collect()
}

fn write_sources_file(path: &Path, sources: &[SkillHubSource]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let rendered = serde_json::to_string_pretty(&SkillHubSources {
        version: 1,
        sources: sources.to_vec(),
    })?;
    std::fs::write(path, rendered + "\n")
        .with_context(|| format!("failed to write skills hub sources: {}", path.display()))
}

fn ensure_hub_ignore(skills_dir: &Path) -> Result<()> {
    let hub_dir = skills_dir.join(".hub");
    std::fs::create_dir_all(&hub_dir)
        .with_context(|| format!("failed to create {}", hub_dir.display()))?;
    let ignore_path = hub_dir.join(".ignore");
    if ignore_path.exists() {
        return Ok(());
    }
    std::fs::write(
        &ignore_path,
        "# Exclude hub internals and untrusted catalog cache from search tools\n*\n",
    )
    .with_context(|| format!("failed to write {}", ignore_path.display()))
}

fn normalize_source(
    name: &str,
    location: &str,
    trust_level: Option<&str>,
) -> Result<SkillHubSource> {
    let name = normalize_label(name).ok_or_else(|| anyhow::anyhow!("invalid source name"))?;
    let location = location.trim();
    if location.is_empty() {
        bail!("source `{name}` location must not be empty");
    }
    if let Some(index_url) = well_known_index_url(location)? {
        validate_index_url(&index_url)?;
    } else if let Some(spec) = parse_github_source_location(location)? {
        validate_github_source_spec(&spec)?;
    } else if looks_like_url(location) {
        validate_index_url(location)?;
    }
    let trust_level = trust_level
        .and_then(normalize_trust_level)
        .unwrap_or_else(default_trust_level);
    Ok(SkillHubSource {
        name,
        location: location.to_string(),
        trust_level,
    })
}

fn load_source_index(source: &SkillHubSource) -> Result<SkillHubIndex> {
    if let Some(index_url) = well_known_index_url(&source.location)? {
        let raw = fetch_https_index(&index_url)?;
        parse_source_index(&raw, source)
    } else if let Some(spec) = parse_github_source_location(&source.location)? {
        fetch_github_source_index(&spec, source)
    } else {
        let raw = read_source_index(source)?;
        parse_source_index(&raw, source)
    }
}

fn read_source_index(source: &SkillHubSource) -> Result<String> {
    if looks_like_url(&source.location) {
        fetch_https_index(&source.location)
    } else {
        let path = PathBuf::from(&source.location);
        std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read skills hub source `{}`", source.name))
    }
}

fn parse_source_index(raw: &str, source: &SkillHubSource) -> Result<SkillHubIndex> {
    let mut index: SkillHubIndex = serde_json::from_str(raw)
        .with_context(|| format!("failed to parse skills hub source `{}`", source.name))?;
    if index.version == 0 {
        index.version = 1;
    }
    index.skills = normalize_and_dedupe_entries(
        index
            .skills
            .into_iter()
            .map(|entry| entry.with_source_defaults(source))
            .collect(),
    );
    Ok(index)
}

fn looks_like_url(location: &str) -> bool {
    let lower = location.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn well_known_index_url(location: &str) -> Result<Option<String>> {
    let Some(raw) = location.trim().strip_prefix("well-known:") else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("well-known skills source must include a domain or index URL");
    }
    let mut url = if looks_like_url(raw) {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    if url.ends_with("/index.json") {
        return Ok(Some(url));
    }
    url = url.trim_end_matches('/').to_string();
    Ok(Some(format!("{url}/.well-known/skills/index.json")))
}

fn parse_github_source_location(location: &str) -> Result<Option<GithubSourceSpec>> {
    let location = location.trim();
    if let Some(rest) = location.strip_prefix("github:") {
        let parts = safe_github_path_parts(rest)?;
        if parts.len() < 2 {
            bail!("github skills source must be github:owner/repo[/path]");
        }
        return Ok(Some(GithubSourceSpec {
            owner: parts[0].clone(),
            repo: parts[1].clone(),
            path: parts[2..].join("/"),
            ref_name: None,
        }));
    }

    if !location.starts_with("https://github.com/") {
        return Ok(None);
    }

    let parsed = reqwest::Url::parse(location)
        .with_context(|| format!("invalid GitHub skills source URL `{location}`"))?;
    if parsed.host_str() != Some("github.com") {
        return Ok(None);
    }
    let parts = parsed
        .path_segments()
        .map(|segments| segments.map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    if parts.len() < 2 {
        bail!("github skills source URL must include owner and repo");
    }
    let owner = parts[0].clone();
    let repo = parts[1].clone();
    let (ref_name, path) = if parts.get(2).is_some_and(|part| part == "tree") {
        let Some(branch) = parts.get(3) else {
            bail!("github skills source tree URL must include a branch");
        };
        (Some(branch.clone()), parts[4..].join("/"))
    } else {
        (None, parts[2..].join("/"))
    };
    let spec = GithubSourceSpec {
        owner,
        repo,
        path,
        ref_name,
    };
    validate_github_source_spec(&spec)?;
    Ok(Some(spec))
}

fn safe_github_path_parts(value: &str) -> Result<Vec<String>> {
    let normalized = value.trim().replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') {
        bail!("unsafe github skills source path");
    }
    let mut parts = Vec::new();
    for part in normalized.split('/').filter(|part| !part.is_empty()) {
        if part == "."
            || part == ".."
            || part.contains(':')
            || !part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        {
            bail!("unsafe github skills source path");
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

fn validate_github_source_spec(spec: &GithubSourceSpec) -> Result<()> {
    safe_github_path_parts(&format!("{}/{}", spec.owner, spec.repo))?;
    if !spec.path.is_empty() {
        safe_github_path_parts(&spec.path)?;
    }
    if let Some(ref_name) = &spec.ref_name {
        safe_github_path_parts(ref_name)?;
    }
    Ok(())
}

fn fetch_github_source_index(
    spec: &GithubSourceSpec,
    source: &SkillHubSource,
) -> Result<SkillHubIndex> {
    let client = github_client()?;
    let entries = fetch_github_contents(&client, spec, &spec.path)?;
    let skill_paths = github_skill_paths(&entries, &spec.path);
    let mut skills = Vec::new();

    for skill_path in skill_paths {
        let Some(skill_md) = fetch_github_skill_md(&client, spec, &skill_path)? else {
            continue;
        };
        skills.push(github_skill_entry(spec, source, &skill_path, &skill_md)?);
    }

    Ok(SkillHubIndex {
        version: 1,
        skills: normalize_and_dedupe_entries(skills),
    })
}

fn github_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent("hakimi-skills-hub")
        .build()
        .context("failed to build GitHub Skills Hub client")
}

fn fetch_github_contents(
    client: &reqwest::blocking::Client,
    spec: &GithubSourceSpec,
    path: &str,
) -> Result<Vec<GithubContentEntry>> {
    let url = github_contents_url(spec, path);
    let response = github_get(client, &url, "application/vnd.github+json")?
        .send()
        .with_context(|| format!("failed to fetch GitHub skills source `{url}`"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("GitHub skills source `{url}` returned HTTP {status}");
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_INDEX_BYTES)
    {
        bail!("GitHub skills source `{url}` is larger than {MAX_INDEX_BYTES} bytes");
    }
    let text = response
        .text()
        .with_context(|| format!("failed to read GitHub skills source `{url}`"))?;
    if text.len() as u64 > MAX_INDEX_BYTES {
        bail!("GitHub skills source `{url}` is larger than {MAX_INDEX_BYTES} bytes");
    }
    parse_github_contents_json(&text, &url)
}

fn parse_github_contents_json(raw: &str, url: &str) -> Result<Vec<GithubContentEntry>> {
    let value: JsonValue = serde_json::from_str(raw)
        .with_context(|| format!("failed to parse GitHub skills source `{url}`"))?;
    match value {
        JsonValue::Array(_) => serde_json::from_value(value)
            .with_context(|| format!("failed to parse GitHub skills source `{url}`")),
        JsonValue::Object(_) => {
            let entry: GithubContentEntry = serde_json::from_value(value)
                .with_context(|| format!("failed to parse GitHub skills source `{url}`"))?;
            Ok(vec![entry])
        }
        _ => bail!("GitHub skills source `{url}` returned unexpected JSON"),
    }
}

fn fetch_github_skill_md(
    client: &reqwest::blocking::Client,
    spec: &GithubSourceSpec,
    skill_path: &str,
) -> Result<Option<String>> {
    let path = if skill_path.is_empty() {
        "SKILL.md".to_string()
    } else {
        format!("{}/SKILL.md", skill_path.trim_matches('/'))
    };
    let url = github_contents_url(spec, &path);
    let response = github_get(client, &url, "application/vnd.github.v3.raw")?
        .send()
        .with_context(|| format!("failed to fetch GitHub skill `{url}`"))?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        bail!("GitHub skill `{url}` returned HTTP {status}");
    }
    let text = response
        .text()
        .with_context(|| format!("failed to read GitHub skill `{url}`"))?;
    if text.len() as u64 > MAX_INDEX_BYTES {
        bail!("GitHub skill `{url}` is larger than {MAX_INDEX_BYTES} bytes");
    }
    Ok(Some(text))
}

fn github_get(
    client: &reqwest::blocking::Client,
    url: &str,
    accept: &str,
) -> Result<reqwest::blocking::RequestBuilder> {
    validate_index_url(url)?;
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT, accept)
        .header(reqwest::header::USER_AGENT, "hakimi-skills-hub");
    if let Some(token) = github_token_from_env() {
        request = request.bearer_auth(token);
    }
    Ok(request)
}

fn github_token_from_env() -> Option<String> {
    env::var("GH_TOKEN")
        .ok()
        .or_else(|| env::var("GITHUB_TOKEN").ok())
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn github_contents_url(spec: &GithubSourceSpec, path: &str) -> String {
    let mut url = format!("{}/{}/{}/contents", GITHUB_API_BASE, spec.owner, spec.repo);
    let path = path.trim_matches('/');
    if !path.is_empty() {
        url.push('/');
        url.push_str(path);
    }
    if let Some(ref_name) = &spec.ref_name {
        url.push_str("?ref=");
        url.push_str(ref_name);
    }
    url
}

fn github_skill_paths(entries: &[GithubContentEntry], base_path: &str) -> Vec<String> {
    let mut paths = Vec::new();
    if entries
        .iter()
        .any(|entry| entry.kind == "file" && entry.name.eq_ignore_ascii_case("SKILL.md"))
    {
        paths.push(base_path.trim_matches('/').to_string());
    }
    paths.extend(
        entries
            .iter()
            .filter(|entry| entry.kind == "dir")
            .map(|entry| entry.path.trim_matches('/').to_string()),
    );
    paths.sort();
    paths.dedup();
    paths
}

fn github_skill_entry(
    spec: &GithubSourceSpec,
    source: &SkillHubSource,
    skill_path: &str,
    skill_md: &str,
) -> Result<SkillHubEntry> {
    let fallback_name = skill_path
        .trim_matches('/')
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(&spec.repo);
    let name = frontmatter_string(skill_md, "name").unwrap_or_else(|| fallback_name.to_string());
    let description = frontmatter_string(skill_md, "description")
        .or_else(|| first_markdown_heading(skill_md))
        .unwrap_or_default();
    let tags = frontmatter_string_list(skill_md, "tags");
    let identifier_path = skill_path.trim_matches('/');
    let identifier = if identifier_path.is_empty() {
        format!("github:{}/{}", spec.owner, spec.repo)
    } else {
        format!("github:{}/{}/{}", spec.owner, spec.repo, identifier_path)
    };
    let mut files = BTreeMap::new();
    files.insert("SKILL.md".to_string(), skill_md.to_string());

    Ok(SkillHubEntry {
        name,
        description,
        source: source.name.clone(),
        identifier,
        trust_level: source.trust_level.clone(),
        repo: Some(format!("{}/{}", spec.owner, spec.repo)),
        category: None,
        tags,
        created_by: None,
        files,
    }
    .normalized())
}

fn frontmatter_string(skill_md: &str, key: &str) -> Option<String> {
    let value = frontmatter_value(skill_md)?;
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn frontmatter_string_list(skill_md: &str, key: &str) -> Vec<String> {
    let Some(value) = frontmatter_value(skill_md).and_then(|value| value.get(key).cloned()) else {
        return Vec::new();
    };
    match value {
        JsonValue::Array(items) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        JsonValue::String(item) => vec![item],
        _ => Vec::new(),
    }
}

fn frontmatter_value(skill_md: &str) -> Option<JsonValue> {
    let body = skill_md.strip_prefix("---")?;
    let end = body.find("\n---")?;
    let frontmatter = &body[..end];
    serde_yaml::from_str(frontmatter).ok()
}

fn first_markdown_heading(skill_md: &str) -> Option<String> {
    skill_md.lines().find_map(|line| {
        let heading = line.trim().strip_prefix("# ")?;
        let heading = heading.trim();
        (!heading.is_empty()).then(|| heading.to_string())
    })
}

fn fetch_https_index(location: &str) -> Result<String> {
    validate_index_url(location)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("hakimi-skills-hub")
        .build()
        .context("failed to build Skills Hub HTTP client")?;
    let response = client
        .get(location)
        .send()
        .with_context(|| format!("failed to fetch skills hub source `{location}`"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("skills hub source `{location}` returned HTTP {status}");
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_INDEX_BYTES)
    {
        bail!("skills hub source `{location}` is larger than {MAX_INDEX_BYTES} bytes");
    }
    let text = response
        .text()
        .with_context(|| format!("failed to read skills hub source `{location}`"))?;
    if text.len() as u64 > MAX_INDEX_BYTES {
        bail!("skills hub source `{location}` is larger than {MAX_INDEX_BYTES} bytes");
    }
    Ok(text)
}

fn validate_index_url(location: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(location.trim())
        .with_context(|| format!("invalid remote skills hub source URL `{location}`"))?;
    if parsed.scheme() != "https" {
        bail!("remote skills hub sources must use https:// URLs");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("remote skills hub source host is not allowed");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("remote skills hub source host is not allowed"))?;
    let host_lower = host.to_ascii_lowercase();
    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
    {
        bail!("remote skills hub source host is not allowed");
    }
    if let Ok(ip) = host_lower.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(ip) => {
                if ip.is_loopback()
                    || ip.is_private()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.octets()[0] == 0
                {
                    bail!("remote skills hub source IP is not allowed");
                }
            }
            IpAddr::V6(ip) => {
                let first = ip.segments()[0];
                let unique_local = (first & 0xfe00) == 0xfc00;
                let link_local = (first & 0xffc0) == 0xfe80;
                if ip.is_loopback() || ip.is_unspecified() || unique_local || link_local {
                    bail!("remote skills hub source IP is not allowed");
                }
            }
        }
    }
    Ok(())
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

    #[test]
    fn load_index_merges_cached_sources_and_prefers_trust() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        let index = skills_dir.join(".hub/index.json");
        std::fs::create_dir_all(index.parent().unwrap()).unwrap();
        std::fs::write(
            &index,
            r##"{
  "skills": [
    {"name":"release-check","description":"Local","source":"local","identifier":"same/id","trust_level":"community","files":{"SKILL.md":"# Local"}}
  ]
}"##,
        )
        .unwrap();
        let cache = skills_dir.join(".hub/index-cache/trusted.json");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(
            &cache,
            r##"{
  "skills": [
    {"name":"release-check","description":"Trusted","source":"trusted-source","identifier":"same/id","trust_level":"trusted","files":{"SKILL.md":"# Trusted"}},
    {"name":"lint-helper","description":"Lint","source":"trusted-source","identifier":"trusted/lint","trust_level":"trusted","files":{"SKILL.md":"# Lint"}}
  ]
}"##,
        )
        .unwrap();
        let hub = SkillHub::new(skills_dir.clone());

        let loaded = hub.load_index().unwrap();

        assert_eq!(loaded.skills.len(), 2);
        let release = loaded
            .skills
            .iter()
            .find(|entry| entry.identifier == "same/id")
            .unwrap();
        assert_eq!(release.description, "Trusted");
        assert_eq!(release.trust_level, "trusted");
    }

    #[test]
    fn refresh_file_source_caches_index_with_source_defaults() {
        let tmp = TempDir::new().unwrap();
        let source_index = write_index(
            tmp.path(),
            r##"{
  "skills": [
    {"name":"ops-runbook","description":"Ops","files":{"SKILL.md":"# Ops"}}
  ]
}"##,
        );
        let hub = SkillHub::new(tmp.path().join("skills"));
        let source_location = source_index.display().to_string();
        hub.add_source("official-pack", &source_location, Some("trusted"))
            .unwrap();

        let report = hub.refresh_sources().unwrap();
        let loaded = hub.load_index().unwrap();

        assert_eq!(report[0].skills, 1);
        assert!(report[0].cached_path.exists());
        assert_eq!(loaded.skills[0].source, "official-pack");
        assert_eq!(loaded.skills[0].trust_level, "trusted");
    }

    #[test]
    fn remote_sources_require_safe_https_urls() {
        let tmp = TempDir::new().unwrap();
        let hub = SkillHub::new(tmp.path().join("skills"));

        let http = hub
            .add_source("local", "http://example.com/index.json", None)
            .unwrap_err();
        let localhost = hub
            .add_source("loopback", "https://127.0.0.1/index.json", None)
            .unwrap_err();
        let credential = hub
            .add_source(
                "credentialed",
                "https://user:secret@example.com/index.json",
                None,
            )
            .unwrap_err();

        assert!(http.to_string().contains("https://"));
        assert!(localhost.to_string().contains("not allowed"));
        assert!(credential.to_string().contains("not allowed"));
    }

    #[test]
    fn well_known_sources_expand_to_index_url() {
        let domain = well_known_index_url("well-known:skills.example.com").unwrap();
        let full = well_known_index_url("well-known:https://skills.example.com/catalog/index.json")
            .unwrap();

        assert_eq!(
            domain.as_deref(),
            Some("https://skills.example.com/.well-known/skills/index.json")
        );
        assert_eq!(
            full.as_deref(),
            Some("https://skills.example.com/catalog/index.json")
        );
    }

    #[test]
    fn github_sources_validate_safe_repo_paths() {
        let spec = parse_github_source_location(
            "https://github.com/NousResearch/hermes-agent/tree/main/skills/software",
        )
        .unwrap()
        .unwrap();

        assert_eq!(spec.owner, "NousResearch");
        assert_eq!(spec.repo, "hermes-agent");
        assert_eq!(spec.ref_name.as_deref(), Some("main"));
        assert_eq!(spec.path, "skills/software");

        let err = parse_github_source_location("github:owner/repo/../secrets")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unsafe github"));
    }

    #[test]
    fn github_skill_entry_extracts_frontmatter_metadata() {
        let source =
            normalize_source("github-live", "github:owner/repo/skills", Some("trusted")).unwrap();
        let spec = GithubSourceSpec {
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            path: "skills".to_string(),
            ref_name: None,
        };

        let entry = github_skill_entry(
            &spec,
            &source,
            "skills/review",
            "---\nname: code-review\ndescription: Review Rust changes\ntags:\n  - rust\n---\n# Review\nBody.",
        )
        .unwrap();

        assert_eq!(entry.name, "code-review");
        assert_eq!(entry.description, "Review Rust changes");
        assert_eq!(entry.source, "github-live");
        assert_eq!(entry.identifier, "github:owner/repo/skills/review");
        assert_eq!(entry.trust_level, "trusted");
        assert_eq!(entry.repo.as_deref(), Some("owner/repo"));
        assert_eq!(entry.tags, vec!["rust"]);
        assert!(entry.files.contains_key("SKILL.md"));
    }

    #[test]
    fn remove_source_deletes_cached_index() {
        let tmp = TempDir::new().unwrap();
        let hub = SkillHub::new(tmp.path().join("skills"));
        let source_index = write_index(
            tmp.path(),
            r##"{"skills":[{"name":"one","files":{"SKILL.md":"# One"}}]}"##,
        );
        let source_location = source_index.display().to_string();
        hub.add_source("cache-me", &source_location, None).unwrap();
        hub.refresh_sources().unwrap();
        let cache = tmp.path().join("skills/.hub/index-cache/cache-me.json");
        assert!(cache.exists());

        assert!(hub.remove_source("cache-me").unwrap());

        assert!(!cache.exists());
        assert!(hub.sources().unwrap().is_empty());
    }
}
