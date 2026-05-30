use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SkillSyncReport {
    pub copied: Vec<String>,
    pub updated: Vec<String>,
    pub skipped: usize,
    pub user_modified: Vec<String>,
    pub cleaned: Vec<String>,
    pub total_bundled: usize,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone)]
struct BundledSkill {
    name: String,
    source_dir: PathBuf,
    relative_dir: PathBuf,
    content_hash: String,
}

#[derive(Debug, Clone)]
pub struct SkillSync {
    skills_dir: PathBuf,
    bundled_dir: PathBuf,
}

impl SkillSync {
    pub fn new(skills_dir: impl Into<PathBuf>, bundled_dir: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
            bundled_dir: bundled_dir.into(),
        }
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.skills_dir.join(".bundled_manifest")
    }

    pub fn sync(&self) -> Result<SkillSyncReport> {
        let manifest_path = self.manifest_path();
        if !self.bundled_dir.exists() {
            return Ok(SkillSyncReport {
                manifest_path,
                ..SkillSyncReport::default()
            });
        }

        std::fs::create_dir_all(&self.skills_dir)
            .with_context(|| format!("failed to create {}", self.skills_dir.display()))?;
        let mut manifest = read_manifest(&manifest_path);
        let bundled = discover_bundled_skills(&self.bundled_dir)?;
        let bundled_names = bundled
            .iter()
            .map(|skill| skill.name.clone())
            .collect::<BTreeSet<_>>();

        let mut report = SkillSyncReport {
            total_bundled: bundled.len(),
            manifest_path: manifest_path.clone(),
            ..SkillSyncReport::default()
        };

        for skill in bundled {
            let dest = self.skills_dir.join(&skill.relative_dir);
            match manifest.get(&skill.name).cloned() {
                None => {
                    if dest.exists() {
                        report.skipped += 1;
                        if hash_dir(&dest)? == skill.content_hash {
                            manifest.insert(skill.name, skill.content_hash);
                        }
                        continue;
                    }
                    copy_dir(&skill.source_dir, &dest)?;
                    manifest.insert(skill.name.clone(), skill.content_hash);
                    report.copied.push(skill.name);
                }
                Some(origin_hash) if dest.exists() => {
                    let user_hash = hash_dir(&dest)?;
                    if origin_hash.is_empty() {
                        manifest.insert(skill.name, user_hash);
                        report.skipped += 1;
                        continue;
                    }
                    if user_hash != origin_hash {
                        report.user_modified.push(skill.name);
                        continue;
                    }
                    if skill.content_hash != origin_hash {
                        replace_unmodified_dir(&skill.source_dir, &dest)?;
                        manifest.insert(skill.name.clone(), skill.content_hash);
                        report.updated.push(skill.name);
                    } else {
                        report.skipped += 1;
                    }
                }
                Some(_) => {
                    report.skipped += 1;
                }
            }
        }

        report.cleaned = manifest
            .keys()
            .filter(|name| !bundled_names.contains(*name))
            .cloned()
            .collect();
        for name in &report.cleaned {
            manifest.remove(name);
        }

        copy_category_descriptions(&self.bundled_dir, &self.skills_dir)?;
        write_manifest(&manifest_path, &manifest)?;
        report.copied.sort();
        report.updated.sort();
        report.user_modified.sort();
        Ok(report)
    }
}

fn discover_bundled_skills(root: &Path) -> Result<Vec<BundledSkill>> {
    let mut skills = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in read_dir_sorted(&dir)? {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            if should_skip_path(&path) || file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if path.join("SKILL.md").is_file() {
                    let name = read_skill_name(&path.join("SKILL.md"), &fallback_name(&path));
                    let relative_dir = safe_relative_dir(&path, root)?;
                    let content_hash = hash_dir(&path)?;
                    skills.push(BundledSkill {
                        name,
                        source_dir: path,
                        relative_dir,
                        content_hash,
                    });
                } else {
                    dirs.push(path);
                }
            }
        }
    }
    skills.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.relative_dir.cmp(&b.relative_dir))
    });
    Ok(skills)
}

fn read_manifest(path: &Path) -> BTreeMap<String, String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (name, hash) = trimmed
                .split_once(':')
                .map_or((trimmed, ""), |(name, hash)| (name.trim(), hash.trim()));
            (!name.is_empty()).then(|| (name.to_string(), hash.to_string()))
        })
        .collect()
}

fn write_manifest(path: &Path, manifest: &BTreeMap<String, String>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut body = String::new();
    for (name, hash) in manifest {
        body.push_str(name);
        body.push(':');
        body.push_str(hash);
        body.push('\n');
    }
    let tmp = path.with_extension(format!(
        "tmp-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, body).with_context(|| format!("failed to write {}", tmp.display()))?;
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("failed to replace {}", path.display()))?;
    }
    std::fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to replace {} with {}",
            path.display(),
            tmp.display()
        )
    })
}

fn read_skill_name(skill_md: &Path, fallback: &str) -> String {
    let Ok(raw) = std::fs::read_to_string(skill_md) else {
        return fallback.to_string();
    };
    let mut in_frontmatter = false;
    for line in raw.lines().take(80) {
        let trimmed = line.trim();
        if trimmed == "---" {
            if in_frontmatter {
                break;
            }
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter && let Some(value) = trimmed.strip_prefix("name:") {
            let name = value.trim().trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    fallback.to_string()
}

fn hash_dir(dir: &Path) -> Result<String> {
    let mut hash = 0xcbf29ce484222325u64;
    for file in list_files(dir)? {
        let rel = file
            .strip_prefix(dir)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| file.to_string_lossy().replace('\\', "/"));
        update_hash(&mut hash, rel.as_bytes());
        update_hash(&mut hash, &[0]);
        update_hash(
            &mut hash,
            &std::fs::read(&file).with_context(|| format!("failed to read {}", file.display()))?,
        );
        update_hash(&mut hash, &[0]);
    }
    Ok(format!("fnv64:{hash:016x}"))
}

fn update_hash(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        bail!("destination already exists: {}", dest.display());
    }
    for file in list_files(source)? {
        let rel = file
            .strip_prefix(source)
            .with_context(|| format!("failed to relativize {}", file.display()))?;
        let target = dest.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::copy(&file, &target).with_context(|| {
            format!("failed to copy {} to {}", file.display(), target.display())
        })?;
    }
    Ok(())
}

fn replace_unmodified_dir(source: &Path, dest: &Path) -> Result<()> {
    let backup = dest.with_extension("sync-backup");
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .with_context(|| format!("failed to remove {}", backup.display()))?;
    }
    std::fs::rename(dest, &backup)
        .with_context(|| format!("failed to move {} to {}", dest.display(), backup.display()))?;
    match copy_dir(source, dest) {
        Ok(()) => {
            std::fs::remove_dir_all(&backup)
                .with_context(|| format!("failed to remove {}", backup.display()))?;
            Ok(())
        }
        Err(err) => {
            if !dest.exists() {
                let _ = std::fs::rename(&backup, dest);
            }
            Err(err)
        }
    }
}

fn list_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(current) = dirs.pop() {
        for entry in read_dir_sorted(&current)? {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            if should_skip_path(&path) || file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                dirs.push(path);
            } else if file_type.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<std::fs::DirEntry>> {
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.path());
    Ok(entries)
}

fn safe_relative_dir(path: &Path, root: &Path) -> Result<PathBuf> {
    let rel = path
        .strip_prefix(root)
        .with_context(|| format!("{} is outside {}", path.display(), root.display()))?;
    if rel.components().any(|component| {
        matches!(
            component,
            std::path::Component::Prefix(_)
                | std::path::Component::RootDir
                | std::path::Component::ParentDir
        )
    }) {
        bail!("unsafe bundled skill path: {}", rel.display());
    }
    Ok(rel.to_path_buf())
}

fn fallback_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("skill")
        .to_string()
}

fn should_skip_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git"
                    | ".hg"
                    | ".svn"
                    | ".hub"
                    | ".usage.json"
                    | ".bundled_manifest"
                    | "__pycache__"
                    | "node_modules"
                    | "target"
            )
        })
}

fn copy_category_descriptions(source: &Path, skills_dir: &Path) -> Result<()> {
    for file in list_files(source)? {
        if file.file_name().and_then(|name| name.to_str()) != Some("DESCRIPTION.md") {
            continue;
        }
        let rel = file
            .strip_prefix(source)
            .with_context(|| format!("failed to relativize {}", file.display()))?;
        let target = skills_dir.join(rel);
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::copy(&file, &target).with_context(|| {
            format!("failed to copy {} to {}", file.display(), target.display())
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(root: &Path, rel: &str, name: &str, body: &str) {
        let dir = root.join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\n---\n{body}"),
        )
        .unwrap();
    }

    #[test]
    fn fresh_sync_copies_bundled_skills_and_manifest() {
        let tmp = TempDir::new().unwrap();
        let bundled = tmp.path().join("bundled");
        let skills = tmp.path().join("skills");
        write_skill(&bundled, "coding/release", "release-check", "# Release");
        std::fs::write(bundled.join("coding").join("DESCRIPTION.md"), "Coding").unwrap();

        let report = SkillSync::new(&skills, &bundled).sync().unwrap();

        assert_eq!(report.copied, vec!["release-check"]);
        assert!(skills.join("coding/release/SKILL.md").exists());
        assert!(skills.join("coding/DESCRIPTION.md").exists());
        assert!(
            std::fs::read_to_string(skills.join(".bundled_manifest"))
                .unwrap()
                .contains("release-check:fnv64:")
        );
    }

    #[test]
    fn sync_updates_unmodified_user_copy() {
        let tmp = TempDir::new().unwrap();
        let bundled = tmp.path().join("bundled");
        let skills = tmp.path().join("skills");
        write_skill(&bundled, "ops/release", "release-check", "# v1");
        let sync = SkillSync::new(&skills, &bundled);
        sync.sync().unwrap();
        std::fs::write(
            bundled.join("ops/release/SKILL.md"),
            "---\nname: release-check\n---\n# v2",
        )
        .unwrap();

        let report = sync.sync().unwrap();

        assert_eq!(report.updated, vec!["release-check"]);
        assert!(
            std::fs::read_to_string(skills.join("ops/release/SKILL.md"))
                .unwrap()
                .contains("# v2")
        );
    }

    #[test]
    fn sync_preserves_user_modified_skill() {
        let tmp = TempDir::new().unwrap();
        let bundled = tmp.path().join("bundled");
        let skills = tmp.path().join("skills");
        write_skill(&bundled, "ops/release", "release-check", "# v1");
        let sync = SkillSync::new(&skills, &bundled);
        sync.sync().unwrap();
        std::fs::write(skills.join("ops/release/SKILL.md"), "# user edit").unwrap();
        std::fs::write(
            bundled.join("ops/release/SKILL.md"),
            "---\nname: release-check\n---\n# v2",
        )
        .unwrap();

        let report = sync.sync().unwrap();

        assert_eq!(report.user_modified, vec!["release-check"]);
        assert_eq!(
            std::fs::read_to_string(skills.join("ops/release/SKILL.md")).unwrap(),
            "# user edit"
        );
    }

    #[test]
    fn sync_respects_deleted_user_skill() {
        let tmp = TempDir::new().unwrap();
        let bundled = tmp.path().join("bundled");
        let skills = tmp.path().join("skills");
        write_skill(&bundled, "ops/release", "release-check", "# v1");
        let sync = SkillSync::new(&skills, &bundled);
        sync.sync().unwrap();
        std::fs::remove_dir_all(skills.join("ops/release")).unwrap();

        let report = sync.sync().unwrap();

        assert_eq!(report.skipped, 1);
        assert!(!skills.join("ops/release").exists());
    }

    #[test]
    fn sync_migrates_v1_manifest_without_overwrite() {
        let tmp = TempDir::new().unwrap();
        let bundled = tmp.path().join("bundled");
        let skills = tmp.path().join("skills");
        write_skill(&bundled, "ops/release", "release-check", "# bundled");
        write_skill(&skills, "ops/release", "release-check", "# user");
        std::fs::write(skills.join(".bundled_manifest"), "release-check\n").unwrap();

        let report = SkillSync::new(&skills, &bundled).sync().unwrap();

        assert_eq!(report.skipped, 1);
        assert!(
            std::fs::read_to_string(skills.join("ops/release/SKILL.md"))
                .unwrap()
                .contains("# user")
        );
        assert!(
            std::fs::read_to_string(skills.join(".bundled_manifest"))
                .unwrap()
                .contains("release-check:fnv64:")
        );
    }

    #[test]
    fn sync_cleans_manifest_for_removed_bundled_skill() {
        let tmp = TempDir::new().unwrap();
        let bundled = tmp.path().join("bundled");
        let skills = tmp.path().join("skills");
        write_skill(&bundled, "ops/release", "release-check", "# v1");
        let sync = SkillSync::new(&skills, &bundled);
        sync.sync().unwrap();
        std::fs::remove_dir_all(bundled.join("ops/release")).unwrap();

        let report = sync.sync().unwrap();

        assert_eq!(report.cleaned, vec!["release-check"]);
        assert!(
            !std::fs::read_to_string(skills.join(".bundled_manifest"))
                .unwrap()
                .contains("release-check")
        );
    }
}
