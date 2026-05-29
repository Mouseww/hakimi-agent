//! Rust-native Hakimi state backup and import helpers.
//!
//! Archives contain user state under a `.hakimi/` prefix and deliberately skip
//! binaries, transient files, sqlite sidecars, and symlinks.

use anyhow::{Context, Result, bail};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use rusqlite::{Connection, DatabaseName, OpenFlags};
use std::{
    ffi::OsStr,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
use tar::{Archive, Builder};

const EXCLUDED_DIRS: &[&str] = &[
    "bin",
    "backups",
    "state-snapshots",
    "checkpoints",
    "__pycache__",
    ".git",
    "node_modules",
    "target",
];

const EXCLUDED_NAMES: &[&str] = &["gateway.pid", "cron.pid"];

const EXCLUDED_SUFFIXES: &[&str] = &[".db-wal", ".db-shm", ".db-journal"];

const OVERWRITE_GUARD_FILES: &[&str] = &["config.yaml", ".env", "sessions.db"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSummary {
    pub path: PathBuf,
    pub file_count: usize,
    pub total_bytes: u64,
    pub skipped_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreSummary {
    pub archive: PathBuf,
    pub target_home: PathBuf,
    pub restored_count: usize,
    pub skipped_count: usize,
}

pub fn active_hakimi_home() -> PathBuf {
    std::env::var_os("HAKIMI_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".hakimi")))
        .unwrap_or_else(|| PathBuf::from(".hakimi"))
}

pub fn default_backup_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(format!(
        "hakimi-backup-{}.tar.gz",
        chrono::Local::now().format("%Y-%m-%d-%H%M%S")
    ))
}

pub fn backup_response(output: Option<&Path>) -> String {
    let hakimi_home = active_hakimi_home();
    let out_path = output
        .map(expand_backup_output_path)
        .unwrap_or_else(default_backup_path);

    match create_backup_archive(&hakimi_home, &out_path) {
        Ok(Some(summary)) => format!(
            "Backup complete: {}\n  Files: {}\n  Original: {}\n  Skipped: {}\nRestore with: hakimi import {} --force",
            summary.path.display(),
            summary.file_count,
            format_size(summary.total_bytes),
            summary.skipped_count,
            summary.path.display()
        ),
        Ok(None) => format!("No Hakimi state files found at {}.", hakimi_home.display()),
        Err(err) => format!("Failed to create backup: {err}"),
    }
}

pub fn import_response(archive: &Path, force: bool) -> String {
    let hakimi_home = active_hakimi_home();
    match restore_backup_archive(&hakimi_home, archive, force) {
        Ok(summary) => format!(
            "Import complete: {} file(s) restored to {} ({} skipped).",
            summary.restored_count,
            summary.target_home.display(),
            summary.skipped_count
        ),
        Err(err) => format!("Failed to import backup: {err}"),
    }
}

pub fn expand_backup_output_path(path: &Path) -> PathBuf {
    let path = expand_home(path);
    if path.is_dir() {
        path.join(
            default_backup_path()
                .file_name()
                .unwrap_or_else(|| OsStr::new("hakimi-backup.tar.gz")),
        )
    } else if path.extension().is_some() {
        path
    } else {
        path.with_extension("tar.gz")
    }
}

pub fn create_backup_archive(
    hakimi_home: &Path,
    output_path: &Path,
) -> Result<Option<BackupSummary>> {
    if !hakimi_home.is_dir() {
        return Ok(None);
    }
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut entries = Vec::new();
    let mut skipped_count = 0;
    collect_backup_entries(
        hakimi_home,
        Path::new(""),
        output_path,
        &mut entries,
        &mut skipped_count,
    )?;

    if entries.is_empty() {
        return Ok(None);
    }

    let file = fs::File::create(output_path)
        .with_context(|| format!("failed to create {}", output_path.display()))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = Builder::new(encoder);
    let temp = tempfile::tempdir()?;
    let mut total_bytes = 0;

    for (idx, (abs_path, rel_path)) in entries.iter().enumerate() {
        let archive_path = Path::new(".hakimi").join(rel_path);
        total_bytes += append_backup_file(&mut archive, abs_path, &archive_path, temp.path(), idx)
            .with_context(|| format!("failed to archive {}", rel_path.display()))?;
    }

    archive.finish()?;
    let encoder = archive.into_inner()?;
    encoder.finish()?;

    Ok(Some(BackupSummary {
        path: output_path.to_path_buf(),
        file_count: entries.len(),
        total_bytes,
        skipped_count,
    }))
}

pub fn restore_backup_archive(
    hakimi_home: &Path,
    archive_path: &Path,
    force: bool,
) -> Result<RestoreSummary> {
    if !archive_path.is_file() {
        bail!("backup archive not found: {}", archive_path.display());
    }

    if !force && would_overwrite_existing_state(hakimi_home) {
        bail!(
            "target {} already has Hakimi state; pass --force to overwrite",
            hakimi_home.display()
        );
    }

    fs::create_dir_all(hakimi_home)
        .with_context(|| format!("failed to create {}", hakimi_home.display()))?;

    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut restored_count = 0;
    let mut skipped_count = 0;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.into_owned();
        let Some(safe_path) = sanitize_archive_path(&raw_path) else {
            skipped_count += 1;
            continue;
        };
        let Some(restore_rel) = normalize_restore_path(&safe_path) else {
            skipped_count += 1;
            continue;
        };
        if should_exclude_restore_entry(&restore_rel) {
            skipped_count += 1;
            continue;
        }

        let target = hakimi_home.join(&restore_rel);
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if !entry_type.is_file() {
            skipped_count += 1;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&target)?;
        restored_count += 1;
    }

    Ok(RestoreSummary {
        archive: archive_path.to_path_buf(),
        target_home: hakimi_home.to_path_buf(),
        restored_count,
        skipped_count,
    })
}

fn collect_backup_entries(
    dir: &Path,
    rel_dir: &Path,
    output_path: &Path,
    entries: &mut Vec<(PathBuf, PathBuf)>,
    skipped_count: &mut usize,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let rel_path = rel_dir.join(file_name);
        let abs_path = entry.path();

        if should_exclude_backup_entry(&rel_path) || points_to_output(&abs_path, output_path) {
            *skipped_count += 1;
            continue;
        }

        let meta = fs::symlink_metadata(&abs_path)?;
        if meta.file_type().is_symlink() {
            *skipped_count += 1;
            continue;
        }
        if meta.is_dir() {
            collect_backup_entries(&abs_path, &rel_path, output_path, entries, skipped_count)?;
        } else if meta.is_file() {
            entries.push((abs_path, rel_path));
        } else {
            *skipped_count += 1;
        }
    }

    Ok(())
}

fn append_backup_file<W: Write>(
    archive: &mut Builder<W>,
    abs_path: &Path,
    archive_path: &Path,
    temp_dir: &Path,
    index: usize,
) -> Result<u64> {
    if is_sqlite_database(abs_path) {
        let snapshot_path = temp_dir.join(format!("sqlite-{index}.db"));
        if sqlite_backup(abs_path, &snapshot_path).is_ok() {
            archive.append_path_with_name(&snapshot_path, archive_path)?;
            return Ok(snapshot_path.metadata()?.len());
        }
    }

    archive.append_path_with_name(abs_path, archive_path)?;
    Ok(abs_path.metadata()?.len())
}

fn sqlite_backup(src: &Path, dst: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open sqlite database {}", src.display()))?;
    conn.backup(DatabaseName::Main, dst, None)
        .with_context(|| format!("failed to snapshot sqlite database {}", src.display()))?;
    Ok(())
}

fn should_exclude_backup_entry(rel_path: &Path) -> bool {
    path_has_excluded_dir(rel_path) || name_is_excluded(rel_path) || suffix_is_excluded(rel_path)
}

fn should_exclude_restore_entry(rel_path: &Path) -> bool {
    path_has_excluded_dir(rel_path) || name_is_excluded(rel_path) || suffix_is_excluded(rel_path)
}

fn path_has_excluded_dir(rel_path: &Path) -> bool {
    rel_path.components().any(|component| {
        let Component::Normal(part) = component else {
            return false;
        };
        EXCLUDED_DIRS
            .iter()
            .any(|excluded| part == OsStr::new(excluded))
    })
}

fn name_is_excluded(rel_path: &Path) -> bool {
    rel_path.file_name().is_some_and(|name| {
        EXCLUDED_NAMES
            .iter()
            .any(|excluded| name == OsStr::new(excluded))
    })
}

fn suffix_is_excluded(rel_path: &Path) -> bool {
    let Some(name) = rel_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    EXCLUDED_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

fn points_to_output(abs_path: &Path, output_path: &Path) -> bool {
    match (abs_path.canonicalize(), output_path.canonicalize()) {
        (Ok(abs), Ok(out)) => abs == out,
        _ => false,
    }
}

fn is_sqlite_database(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("db"))
}

fn sanitize_archive_path(path: &Path) -> Option<PathBuf> {
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return None,
        }
    }
    (!safe.as_os_str().is_empty()).then_some(safe)
}

fn normalize_restore_path(path: &Path) -> Option<PathBuf> {
    if path.components().next().is_some_and(
        |component| matches!(component, Component::Normal(part) if part == OsStr::new(".hakimi")),
    ) {
        let mut rest = PathBuf::new();
        for component in path.components().skip(1) {
            if let Component::Normal(part) = component {
                rest.push(part);
            }
        }
        (!rest.as_os_str().is_empty()).then_some(rest)
    } else {
        Some(path.to_path_buf())
    }
}

fn would_overwrite_existing_state(hakimi_home: &Path) -> bool {
    OVERWRITE_GUARD_FILES
        .iter()
        .any(|rel| hakimi_home.join(rel).exists())
}

fn expand_home(path: &Path) -> PathBuf {
    let raw = path.as_os_str().to_string_lossy();
    if raw == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    path.to_path_buf()
}

fn format_size(nbytes: u64) -> String {
    let mut value = nbytes as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if value < 1024.0 {
            return if unit == "B" {
                format!("{nbytes} B")
            } else {
                format!("{value:.1} {unit}")
            };
        }
        value /= 1024.0;
    }
    format!("{value:.1} TB")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tar::Header;

    fn names_in_archive(path: &Path) -> Vec<String> {
        let file = fs::File::open(path).unwrap();
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);
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

    fn append_bytes(builder: &mut Builder<GzEncoder<fs::File>>, path: &str, bytes: &[u8]) {
        let mut header = Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder.append(&header, Cursor::new(bytes)).unwrap();
    }

    #[test]
    fn backup_excludes_runtime_and_sidecar_paths() {
        assert!(should_exclude_backup_entry(Path::new("bin/hakimi")));
        assert!(should_exclude_backup_entry(Path::new(
            "checkpoints/abc/state.json"
        )));
        assert!(should_exclude_backup_entry(Path::new("sessions.db-wal")));
        assert!(should_exclude_backup_entry(Path::new("gateway.pid")));
        assert!(!should_exclude_backup_entry(Path::new("config.yaml")));
    }

    #[test]
    fn backup_includes_user_state_and_excludes_binary_state() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".hakimi");
        fs::create_dir_all(home.join("memory")).unwrap();
        fs::create_dir_all(home.join("bin")).unwrap();
        fs::create_dir_all(home.join("checkpoints/abc")).unwrap();
        fs::write(home.join("config.yaml"), "model: test\n").unwrap();
        fs::write(home.join(".env"), "TOKEN=secret\n").unwrap();
        fs::write(home.join("memory/notes.md"), "remember\n").unwrap();
        fs::write(home.join("bin/hakimi"), "binary").unwrap();
        fs::write(home.join("sessions.db-wal"), "sidecar").unwrap();
        fs::write(home.join("checkpoints/abc/state.json"), "{}").unwrap();

        let out = temp.path().join("backup.tar.gz");
        let summary = create_backup_archive(&home, &out).unwrap().unwrap();
        assert_eq!(summary.file_count, 3);

        let names = names_in_archive(&out);
        assert!(names.contains(&".hakimi/config.yaml".to_string()));
        assert!(names.contains(&".hakimi/.env".to_string()));
        assert!(names.contains(&".hakimi/memory/notes.md".to_string()));
        assert!(!names.iter().any(|name| name.contains("/bin/")));
        assert!(!names.iter().any(|name| name.ends_with(".db-wal")));
    }

    #[test]
    fn backup_skips_symlinked_files() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".hakimi");
        fs::create_dir_all(home.join("skills")).unwrap();
        fs::write(temp.path().join("outside.txt"), "outside secret\n").unwrap();
        let link = home.join("skills/outside.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(temp.path().join("outside.txt"), &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(temp.path().join("outside.txt"), &link).unwrap();
        fs::write(home.join("config.yaml"), "model: test\n").unwrap();

        let out = temp.path().join("backup.tar.gz");
        let summary = create_backup_archive(&home, &out).unwrap().unwrap();
        assert_eq!(summary.file_count, 1);
        assert!(
            !names_in_archive(&out)
                .iter()
                .any(|name| name.contains("outside.txt"))
        );
    }

    #[test]
    fn import_refuses_existing_state_without_force() {
        let temp = tempfile::tempdir().unwrap();
        let source_home = temp.path().join("source/.hakimi");
        fs::create_dir_all(&source_home).unwrap();
        fs::write(source_home.join("config.yaml"), "model: backup\n").unwrap();
        let out = temp.path().join("backup.tar.gz");
        create_backup_archive(&source_home, &out).unwrap().unwrap();

        let target_home = temp.path().join("target/.hakimi");
        fs::create_dir_all(&target_home).unwrap();
        fs::write(target_home.join("config.yaml"), "model: existing\n").unwrap();

        let err = restore_backup_archive(&target_home, &out, false).unwrap_err();
        assert!(err.to_string().contains("--force"));
    }

    #[test]
    fn import_restores_hakimi_prefixed_archive() {
        let temp = tempfile::tempdir().unwrap();
        let source_home = temp.path().join("source/.hakimi");
        fs::create_dir_all(source_home.join("memory")).unwrap();
        fs::write(source_home.join("config.yaml"), "model: backup\n").unwrap();
        fs::write(source_home.join("memory/notes.md"), "remember\n").unwrap();
        let out = temp.path().join("backup.tar.gz");
        create_backup_archive(&source_home, &out).unwrap().unwrap();

        let target_home = temp.path().join("target/.hakimi");
        let summary = restore_backup_archive(&target_home, &out, true).unwrap();
        assert_eq!(summary.restored_count, 2);
        assert_eq!(
            fs::read_to_string(target_home.join("config.yaml")).unwrap(),
            "model: backup\n"
        );
        assert_eq!(
            fs::read_to_string(target_home.join("memory/notes.md")).unwrap(),
            "remember\n"
        );
    }

    #[test]
    fn import_rejects_unsafe_archive_paths_before_unpack() {
        assert!(sanitize_archive_path(Path::new("../evil.txt")).is_none());
        assert!(sanitize_archive_path(Path::new("/tmp/evil.txt")).is_none());
        assert!(sanitize_archive_path(Path::new(".hakimi/config.yaml")).is_some());
    }

    #[test]
    fn import_skips_binary_entries_even_when_archive_contains_them() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("bin.tar.gz");
        let file = fs::File::create(&out).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);
        append_bytes(&mut builder, ".hakimi/config.yaml", b"model: ok\n");
        append_bytes(&mut builder, ".hakimi/bin/hakimi", b"old binary\n");
        builder.finish().unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();

        let target_home = temp.path().join("target/.hakimi");
        let summary = restore_backup_archive(&target_home, &out, true).unwrap();
        assert_eq!(summary.restored_count, 1);
        assert_eq!(summary.skipped_count, 1);
        assert!(!target_home.join("bin/hakimi").exists());
    }
}
