use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct SkillUsageStore {
    path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillUsageRecord {
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub use_count: u64,
    #[serde(default)]
    pub view_count: u64,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub last_viewed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillUsageSnapshot {
    pub name: String,
    #[serde(flatten)]
    pub record: SkillUsageRecord,
}

impl SkillUsageStore {
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            path: skills_dir.into().join(".usage.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn report(&self) -> Vec<SkillUsageSnapshot> {
        self.read_map()
            .into_iter()
            .map(|(name, record)| SkillUsageSnapshot { name, record })
            .collect()
    }

    pub fn record_use(&self, skill_name: &str) -> Result<()> {
        let name = skill_name.trim();
        if name.is_empty() {
            return Ok(());
        }

        self.mutate(|records, now| {
            bump_record_use(records, name, now);
        })
    }

    pub fn record_uses(&self, skill_names: &[String]) -> Result<()> {
        let names = unique_names(skill_names);
        if names.is_empty() {
            return Ok(());
        }

        self.mutate(|records, now| {
            for name in names {
                bump_record_use(records, &name, now);
            }
        })
    }

    pub fn record_view(&self, skill_name: &str) -> Result<()> {
        let name = skill_name.trim();
        if name.is_empty() {
            return Ok(());
        }

        self.mutate(|records, now| {
            let record = records
                .entry(name.to_string())
                .or_insert_with(|| new_record(now));
            ensure_created_at(record, now);
            record.view_count = record.view_count.saturating_add(1);
            record.last_viewed_at = Some(now.to_string());
        })
    }

    pub fn bump_use(&self, skill_name: &str) {
        if let Err(err) = self.record_use(skill_name) {
            debug!(skill = %skill_name, error = %err, "failed to record skill use");
        }
    }

    pub fn bump_uses(&self, skill_names: &[String]) {
        if let Err(err) = self.record_uses(skill_names) {
            debug!(error = %err, "failed to record skill uses");
        }
    }

    pub fn bump_view(&self, skill_name: &str) {
        if let Err(err) = self.record_view(skill_name) {
            debug!(skill = %skill_name, error = %err, "failed to record skill view");
        }
    }

    fn mutate<F>(&self, mutator: F) -> Result<()>
    where
        F: FnOnce(&mut BTreeMap<String, SkillUsageRecord>, &str),
    {
        let mut records = self.read_map();
        let now = now_rfc3339();
        mutator(&mut records, &now);
        self.write_map(&records)
    }

    fn read_map(&self) -> BTreeMap<String, SkillUsageRecord> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return BTreeMap::new(),
            Err(err) => {
                debug!(path = %self.path.display(), error = %err, "failed to read skill usage sidecar");
                return BTreeMap::new();
            }
        };

        match serde_json::from_str(&raw) {
            Ok(records) => records,
            Err(err) => {
                debug!(path = %self.path.display(), error = %err, "failed to parse skill usage sidecar");
                BTreeMap::new()
            }
        }
    }

    fn write_map(&self, records: &BTreeMap<String, SkillUsageRecord>) -> Result<()> {
        let parent = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create skills directory: {}", parent.display()))?;

        let mut tmp = NamedTempFile::new_in(parent)
            .with_context(|| format!("failed to create usage temp file in {}", parent.display()))?;
        serde_json::to_writer_pretty(&mut tmp, records)
            .context("failed to serialize skill usage")?;
        tmp.write_all(b"\n")
            .context("failed to finalize skill usage JSON")?;
        tmp.flush().context("failed to flush skill usage JSON")?;
        tmp.persist(&self.path)
            .map_err(|err| err.error)
            .with_context(|| format!("failed to persist skill usage: {}", self.path.display()))?;
        Ok(())
    }
}

fn new_record(now: &str) -> SkillUsageRecord {
    SkillUsageRecord {
        created_at: Some(now.to_string()),
        ..SkillUsageRecord::default()
    }
}

fn ensure_created_at(record: &mut SkillUsageRecord, now: &str) {
    if record.created_at.is_none() {
        record.created_at = Some(now.to_string());
    }
}

fn bump_record_use(records: &mut BTreeMap<String, SkillUsageRecord>, name: &str, now: &str) {
    let record = records
        .entry(name.to_string())
        .or_insert_with(|| new_record(now));
    ensure_created_at(record, now);
    record.use_count = record.use_count.saturating_add(1);
    record.last_used_at = Some(now.to_string());
}

fn unique_names(skill_names: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    for name in skill_names {
        let name = name.trim();
        if !name.is_empty() {
            seen.insert(name.to_string());
        }
    }
    seen.into_iter().collect()
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn record_use_creates_sidecar_and_increments() {
        let tmp = TempDir::new().unwrap();
        let usage = SkillUsageStore::new(tmp.path());

        usage.record_use("release-check").unwrap();
        usage.record_use("release-check").unwrap();

        let report = usage.report();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].name, "release-check");
        assert_eq!(report[0].record.use_count, 2);
        assert!(report[0].record.last_used_at.is_some());
        assert!(usage.path().exists());
    }

    #[test]
    fn record_uses_deduplicates_empty_and_repeated_names() {
        let tmp = TempDir::new().unwrap();
        let usage = SkillUsageStore::new(tmp.path());

        usage
            .record_uses(&[
                "rust".to_string(),
                " ".to_string(),
                "rust".to_string(),
                "release".to_string(),
            ])
            .unwrap();

        let report = usage.report();
        assert_eq!(report.len(), 2);
        assert_eq!(report[0].name, "release");
        assert_eq!(report[1].name, "rust");
        assert_eq!(report[1].record.use_count, 1);
    }

    #[test]
    fn record_view_preserves_use_fields() {
        let tmp = TempDir::new().unwrap();
        let usage = SkillUsageStore::new(tmp.path());

        usage.record_use("docs").unwrap();
        usage.record_view("docs").unwrap();

        let report = usage.report();
        assert_eq!(report[0].record.use_count, 1);
        assert_eq!(report[0].record.view_count, 1);
        assert!(report[0].record.last_viewed_at.is_some());
    }

    #[test]
    fn corrupt_usage_file_recovers_as_empty_report() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".usage.json");
        std::fs::write(path, "{not-json").unwrap();

        let usage = SkillUsageStore::new(tmp.path());

        assert!(usage.report().is_empty());
    }
}
