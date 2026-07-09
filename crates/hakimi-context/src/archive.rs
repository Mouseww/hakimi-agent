// Memory archive functionality for Hakimi Agent
//
// This module provides functionality to archive old memory entries to reduce
// the size of active memory files while preserving historical data.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Statistics from an archive operation
#[derive(Debug, Clone)]
pub struct ArchiveStats {
    pub entries_archived: usize,
    pub bytes_archived: usize,
    pub archive_path: PathBuf,
    pub duration: Duration,
}

impl ArchiveStats {
    pub fn empty() -> Self {
        Self {
            entries_archived: 0,
            bytes_archived: 0,
            archive_path: PathBuf::new(),
            duration: Duration::from_secs(0),
        }
    }
}

/// A single memory entry with parsed timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub timestamp: DateTime<Utc>,
    pub content: String,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Information about an archived month
#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    pub year_month: String,
    pub entry_count: usize,
    pub size_bytes: u64,
    pub path: PathBuf,
}

/// Memory archive manager
pub struct MemoryArchive {
    memory_path: PathBuf,
    archive_dir: PathBuf,
}

impl MemoryArchive {
    /// Create a new archive manager
    pub fn new(memory_dir: &Path) -> Self {
        Self {
            memory_path: memory_dir.join("memory.md"),
            archive_dir: memory_dir.join("archive"),
        }
    }

    /// Archive memory entries before a given date
    pub fn archive_before(&self, cutoff_date: DateTime<Utc>) -> Result<ArchiveStats, String> {
        let start = Instant::now();
        info!(
            "Starting archive of memory entries before {}",
            cutoff_date.format("%Y-%m-%d")
        );

        // 1. Parse memory entries
        let entries = self.parse_memory_entries()?;

        // 2. Split into archive and keep
        let (to_archive, to_keep): (Vec<_>, Vec<_>) =
            entries.into_iter().partition(|e| e.timestamp < cutoff_date);

        if to_archive.is_empty() {
            info!("No entries to archive");
            return Ok(ArchiveStats::empty());
        }

        // 3. Backup original file
        self.backup_memory_file()?;

        // 4. Group by month
        let grouped = self.group_by_month(&to_archive);

        // 5. Write archive files
        let mut total_bytes = 0;
        for (year_month, entries) in &grouped {
            let bytes = self.write_archive(year_month, entries)?;
            total_bytes += bytes;
        }

        // 6. Update memory.md with index
        self.update_memory_with_index(&to_keep, &grouped)?;

        let duration = start.elapsed();
        let stats = ArchiveStats {
            entries_archived: to_archive.len(),
            bytes_archived: total_bytes,
            archive_path: self.archive_dir.clone(),
            duration,
        };

        info!(
            entries = stats.entries_archived,
            bytes = stats.bytes_archived,
            duration_ms = duration.as_millis(),
            "Archive completed"
        );

        Ok(stats)
    }

    /// Parse memory entries from memory.md
    fn parse_memory_entries(&self) -> Result<Vec<MemoryEntry>, String> {
        if !self.memory_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.memory_path)
            .map_err(|e| format!("Failed to read memory.md: {}", e))?;

        let mut entries = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('>') {
                continue;
            }

            // Try to parse timestamp: [YYYY-MM-DD HH:MM:SS] or [YYYY-MM-DD HH:MM UTC]
            if let Some(entry) = self.parse_entry_line(line)? {
                entries.push(entry);
            }
        }

        debug!("Parsed {} memory entries", entries.len());
        Ok(entries)
    }

    /// Parse a single line into a MemoryEntry
    fn parse_entry_line(&self, line: &str) -> Result<Option<MemoryEntry>, String> {
        // Look for [timestamp] at the start
        if !line.starts_with('[') {
            return Ok(None);
        }

        let close_bracket = match line.find(']') {
            Some(idx) => idx,
            None => return Ok(None),
        };

        let timestamp_str = &line[1..close_bracket];
        let content = line[close_bracket + 1..].trim();

        if content.is_empty() {
            return Ok(None);
        }

        // Try multiple timestamp formats
        let timestamp = self
            .parse_timestamp(timestamp_str)
            .ok_or_else(|| format!("Failed to parse timestamp: {}", timestamp_str))?;

        Ok(Some(MemoryEntry {
            timestamp,
            content: content.to_string(),
            metadata: HashMap::new(),
        }))
    }

    /// Parse timestamp in various formats
    fn parse_timestamp(&self, s: &str) -> Option<DateTime<Utc>> {
        use chrono::NaiveDateTime;

        // Try: YYYY-MM-DD HH:MM:SS (assume UTC)
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
        }

        // Try: YYYY-MM-DD HH:MM UTC
        if s.ends_with("UTC") {
            let trimmed = s.trim_end_matches("UTC").trim();
            if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M") {
                return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
            }
        }

        // Try: Session ended: YYYY-MM-DD HH:MM UTC
        if s.contains("Session ended:") {
            if let Some(date_part) = s.split("Session ended:").nth(1) {
                return self.parse_timestamp(date_part.trim());
            }
        }

        None
    }


    /// Group entries by year-month
    fn group_by_month(&self, entries: &[MemoryEntry]) -> HashMap<String, Vec<MemoryEntry>> {
        let mut grouped: HashMap<String, Vec<MemoryEntry>> = HashMap::new();

        for entry in entries {
            let key = entry.timestamp.format("%Y-%m").to_string();
            grouped
                .entry(key)
                .or_insert_with(Vec::new)
                .push(entry.clone());
        }

        debug!("Grouped entries into {} months", grouped.len());
        grouped
    }

    /// Write archive file for a given month
    fn write_archive(&self, year_month: &str, entries: &[MemoryEntry]) -> Result<usize, String> {
        let archive_month_dir = self.archive_dir.join(year_month);
        fs::create_dir_all(&archive_month_dir)
            .map_err(|e| format!("Failed to create archive directory: {}", e))?;

        let archive_file = archive_month_dir.join("memory_archived.md");
        let mut content = String::new();

        content.push_str(&format!("# 归档记忆 - {}\n\n", year_month));
        content.push_str(&format!(
            "> 归档时间: {}\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));
        content.push_str(&format!("> 记忆条数: {}\n\n", entries.len()));
        content.push_str("---\n\n");

        for entry in entries {
            content.push_str(&format!(
                "[{}] {}\n",
                entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
                entry.content
            ));
        }

        let bytes = content.len();
        fs::write(&archive_file, &content)
            .map_err(|e| format!("Failed to write archive file: {}", e))?;

        info!(
            year_month = year_month,
            entries = entries.len(),
            bytes = bytes,
            "Wrote archive file"
        );

        Ok(bytes)
    }

    /// Update memory.md with remaining entries and archive index
    fn update_memory_with_index(
        &self,
        active_entries: &[MemoryEntry],
        archived: &HashMap<String, Vec<MemoryEntry>>,
    ) -> Result<(), String> {
        let mut content = String::new();

        // Write active entries
        if !active_entries.is_empty() {
            content.push_str("# 活跃记忆\n\n");
            for entry in active_entries {
                content.push_str(&format!(
                    "[{}] {}\n",
                    entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
                    entry.content
                ));
            }
        }

        // Write archive index
        if !archived.is_empty() {
            content.push_str("\n---\n\n## 已归档记忆\n\n");
            content.push_str("> 旧记忆已移动到归档目录，可通过 `hakimi memory restore` 恢复\n\n");

            let mut months: Vec<_> = archived.keys().collect();
            months.sort();
            months.reverse(); // Most recent first

            for month in months {
                let count = archived[month].len();
                content.push_str(&format!(
                    "- **{}**: {} 条记忆 → `archive/{}/memory_archived.md`\n",
                    month, count, month
                ));
            }
        }

        fs::write(&self.memory_path, content)
            .map_err(|e| format!("Failed to write updated memory.md: {}", e))?;

        Ok(())
    }

    /// Backup memory.md before archiving
    fn backup_memory_file(&self) -> Result<(), String> {
        if !self.memory_path.exists() {
            return Ok(());
        }

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let backup_path = self
            .memory_path
            .with_file_name(format!("memory.md.backup.{}", timestamp));

        fs::copy(&self.memory_path, &backup_path)
            .map_err(|e| format!("Failed to backup memory.md: {}", e))?;

        debug!("Created backup at {}", backup_path.display());
        Ok(())
    }

    /// List all archives
    pub fn list_archives(&self) -> Result<Vec<ArchiveInfo>, String> {
        if !self.archive_dir.exists() {
            return Ok(Vec::new());
        }

        let mut archives = Vec::new();

        for entry in fs::read_dir(&self.archive_dir)
            .map_err(|e| format!("Failed to read archive directory: {}", e))?
        {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let year_month = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let archive_file = path.join("memory_archived.md");
            if !archive_file.exists() {
                continue;
            }

            let metadata = fs::metadata(&archive_file)
                .map_err(|e| format!("Failed to read archive metadata: {}", e))?;

            let size_bytes = metadata.len();

            // Count entries by parsing file
            let content = fs::read_to_string(&archive_file)
                .map_err(|e| format!("Failed to read archive file: {}", e))?;
            let entry_count = content.lines().filter(|l| l.starts_with('[')).count();

            archives.push(ArchiveInfo {
                year_month,
                entry_count,
                size_bytes,
                path: archive_file,
            });
        }

        archives.sort_by(|a, b| b.year_month.cmp(&a.year_month));
        Ok(archives)
    }

    /// Restore an archived month back to memory.md
    pub fn restore_archive(&self, year_month: &str) -> Result<(), String> {
        let archive_file = self.archive_dir.join(year_month).join("memory_archived.md");

        if !archive_file.exists() {
            return Err(format!("Archive not found: {}", year_month));
        }

        info!("Restoring archive: {}", year_month);

        // 1. Backup current memory.md
        self.backup_memory_file()?;

        // 2. Read archived content
        let archived_content = fs::read_to_string(&archive_file)
            .map_err(|e| format!("Failed to read archive: {}", e))?;

        // Extract just the entries (skip header)
        let entries: String = archived_content
            .lines()
            .filter(|l| l.starts_with('['))
            .map(|l| format!("{}\n", l))
            .collect();

        // 3. Read current memory.md
        let mut current_content = fs::read_to_string(&self.memory_path).unwrap_or_default();

        // Remove archive index if present
        if let Some(idx) = current_content.find("## 已归档记忆") {
            current_content.truncate(idx);
        }

        // 4. Append archived entries
        if !current_content.is_empty() && !current_content.ends_with('\n') {
            current_content.push('\n');
        }
        current_content.push_str(&entries);

        // 5. Write back
        fs::write(&self.memory_path, current_content)
            .map_err(|e| format!("Failed to write memory.md: {}", e))?;

        // 6. Delete archive file
        fs::remove_file(&archive_file)
            .map_err(|e| format!("Failed to remove archive file: {}", e))?;

        info!("Restore completed: {}", year_month);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};
    use tempfile::TempDir;

    #[test]
    fn test_parse_entry_line() {
        let temp_dir = TempDir::new().unwrap();
        let archive = MemoryArchive::new(temp_dir.path());

        let line = "[2026-01-15 10:00:00] 测试记忆内容";
        let entry = archive.parse_entry_line(line).unwrap().unwrap();

        assert_eq!(entry.content, "测试记忆内容");
        assert_eq!(entry.timestamp.year(), 2026);
        assert_eq!(entry.timestamp.month(), 1);
        assert_eq!(entry.timestamp.day(), 15);
    }

    #[test]
    fn test_parse_entry_line_with_utc() {
        let temp_dir = TempDir::new().unwrap();
        let archive = MemoryArchive::new(temp_dir.path());

        let line = "[2026-06-20 15:30 UTC] 另一个测试";
        let entry = archive.parse_entry_line(line).unwrap().unwrap();

        assert_eq!(entry.content, "另一个测试");
        assert_eq!(entry.timestamp.month(), 6);
    }

    #[test]
    fn test_group_by_month() {
        let temp_dir = TempDir::new().unwrap();
        let archive = MemoryArchive::new(temp_dir.path());

        let entries = vec![
            MemoryEntry {
                timestamp: Utc.with_ymd_and_hms(2026, 1, 15, 10, 0, 0).unwrap(),
                content: "记忆 1".into(),
                metadata: HashMap::new(),
            },
            MemoryEntry {
                timestamp: Utc.with_ymd_and_hms(2026, 1, 20, 10, 0, 0).unwrap(),
                content: "记忆 2".into(),
                metadata: HashMap::new(),
            },
            MemoryEntry {
                timestamp: Utc.with_ymd_and_hms(2026, 2, 5, 10, 0, 0).unwrap(),
                content: "记忆 3".into(),
                metadata: HashMap::new(),
            },
        ];

        let grouped = archive.group_by_month(&entries);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped.get("2026-01").unwrap().len(), 2);
        assert_eq!(grouped.get("2026-02").unwrap().len(), 1);
    }

    #[test]
    fn test_archive_before() {
        let temp_dir = TempDir::new().unwrap();
        let archive = MemoryArchive::new(temp_dir.path());

        // Create test memory content
        let content = r#"[2026-01-15 10:00:00] 旧记忆 1
[2026-01-20 10:00:00] 旧记忆 2
[2026-06-20 15:30:00] 新记忆 1
[2026-07-01 08:00:00] 新记忆 2
"#;
        fs::write(&archive.memory_path, content).unwrap();

        // Archive entries before 2026-06-01
        let cutoff = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let stats = archive.archive_before(cutoff).unwrap();

        assert_eq!(stats.entries_archived, 2);
        assert!(stats.bytes_archived > 0);

        // Check archive file exists
        let archive_file = temp_dir.path().join("archive/2026-01/memory_archived.md");
        assert!(archive_file.exists());

        let archived = fs::read_to_string(&archive_file).unwrap();
        assert!(archived.contains("旧记忆 1"));
        assert!(archived.contains("旧记忆 2"));

        // Check memory.md has active entries and index
        let active = fs::read_to_string(&archive.memory_path).unwrap();
        assert!(active.contains("新记忆 1"));
        assert!(active.contains("新记忆 2"));
        assert!(active.contains("已归档记忆"));
        assert!(active.contains("2026-01"));
    }

    #[test]
    fn test_list_archives() {
        let temp_dir = TempDir::new().unwrap();
        let archive = MemoryArchive::new(temp_dir.path());

        // Create test archives
        let archive_dir = temp_dir.path().join("archive/2026-01");
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(
            archive_dir.join("memory_archived.md"),
            "[2026-01-15 10:00:00] 测试\n",
        )
        .unwrap();

        let archives = archive.list_archives().unwrap();
        assert_eq!(archives.len(), 1);
        assert_eq!(archives[0].year_month, "2026-01");
        assert_eq!(archives[0].entry_count, 1);
    }

    #[test]
    fn test_restore_archive() {
        let temp_dir = TempDir::new().unwrap();
        let archive = MemoryArchive::new(temp_dir.path());

        // Create an archive
        let archive_dir = temp_dir.path().join("archive/2026-01");
        fs::create_dir_all(&archive_dir).unwrap();
        let archived_content = "# 归档记忆 - 2026-01\n\n[2026-01-15 10:00:00] 归档的记忆\n";
        fs::write(archive_dir.join("memory_archived.md"), archived_content).unwrap();

        // Create current memory
        let current = "[2026-06-20 15:30:00] 当前记忆\n";
        fs::write(&archive.memory_path, current).unwrap();

        // Restore
        archive.restore_archive("2026-01").unwrap();

        // Check content
        let content = fs::read_to_string(&archive.memory_path).unwrap();
        assert!(content.contains("当前记忆"));
        assert!(content.contains("归档的记忆"));

        // Archive file should be deleted
        assert!(!archive_dir.join("memory_archived.md").exists());
    }
}
