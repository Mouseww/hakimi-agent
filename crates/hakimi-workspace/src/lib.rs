//! Path-jailed workspace filesystem for Hakimi Studio.
//!
//! All relative paths are resolved under a fixed root. Parent-dir escapes (`..`)
//! and absolute paths that leave the root are rejected.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;
use tracing::debug;

/// Maximum bytes returned by a single `read` (prevents huge binary dumps).
pub const DEFAULT_MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("path escape denied: {0}")]
    PathEscape(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("is a directory: {0}")]
    IsDirectory(String),
    #[error("is a file: {0}")]
    IsFile(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("file too large ({size} > {max})")]
    TooLarge { size: u64, max: u64 },
    #[error("git/worktree error: {0}")]
    Git(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    /// Relative path from workspace root.
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    #[serde(default)]
    pub git_status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    max_read_bytes: u64,
}

impl Workspace {
    /// Open a workspace rooted at `root` (created if missing).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Ok(Self {
            root,
            max_read_bytes: DEFAULT_MAX_READ_BYTES,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn with_max_read_bytes(mut self, max: u64) -> Self {
        self.max_read_bytes = max;
        self
    }

    /// Resolve a client-relative path under the jail. Empty path → root.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf> {
        let trimmed = relative.trim().trim_start_matches('/');
        if trimmed.is_empty() {
            return Ok(self.root.clone());
        }

        for component in Path::new(trimmed).components() {
            match component {
                Component::ParentDir => {
                    return Err(WorkspaceError::PathEscape(relative.into()));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(WorkspaceError::PathEscape(relative.into()));
                }
                Component::CurDir | Component::Normal(_) => {}
            }
        }

        let joined = self.root.join(trimmed);
        // If the path exists, canonicalize and verify containment.
        if joined.exists() {
            let canon = std::fs::canonicalize(&joined)?;
            if !canon.starts_with(&self.root) {
                return Err(WorkspaceError::PathEscape(relative.into()));
            }
            return Ok(canon);
        }

        // For not-yet-existing paths (write/create): check parent containment.
        if let Some(parent) = joined.parent() {
            if parent.exists() {
                let parent_canon = std::fs::canonicalize(parent)?;
                if !parent_canon.starts_with(&self.root) {
                    return Err(WorkspaceError::PathEscape(relative.into()));
                }
            } else if !self.root.starts_with(&self.root) {
                // unreachable guard
            }
        }
        // Lexical check: joined must start with root after normalize.
        let normalized = normalize_lexical(&joined);
        if !normalized.starts_with(&self.root) {
            return Err(WorkspaceError::PathEscape(relative.into()));
        }
        Ok(joined)
    }

    pub async fn list(&self, relative: &str) -> Result<Vec<DirEntry>> {
        let path = self.resolve(relative)?;
        if !path.exists() {
            return Err(WorkspaceError::NotFound(relative.into()));
        }
        if !path.is_dir() {
            return Err(WorkspaceError::IsFile(relative.into()));
        }

        let mut entries = Vec::new();
        let mut rd = fs::read_dir(&path).await?;
        while let Some(entry) = rd.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            let meta = entry.metadata().await?;
            let is_dir = meta.is_dir();
            let size = if is_dir { 0 } else { meta.len() };
            let rel = if relative.trim().is_empty() {
                name.clone()
            } else {
                format!(
                    "{}/{}",
                    relative
                        .trim()
                        .trim_start_matches('/')
                        .trim_end_matches('/'),
                    name
                )
            };
            entries.push(DirEntry {
                name,
                path: rel,
                is_dir,
                size,
                git_status: None,
            });
        }
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        Ok(entries)
    }

    pub async fn read(&self, relative: &str) -> Result<String> {
        let path = self.resolve(relative)?;
        if !path.exists() {
            return Err(WorkspaceError::NotFound(relative.into()));
        }
        if path.is_dir() {
            return Err(WorkspaceError::IsDirectory(relative.into()));
        }
        let meta = fs::metadata(&path).await?;
        if meta.len() > self.max_read_bytes {
            return Err(WorkspaceError::TooLarge {
                size: meta.len(),
                max: self.max_read_bytes,
            });
        }
        let bytes = fs::read(&path).await?;
        // Lossy UTF-8 for binary-ish files; Studio can still display.
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub async fn write(&self, relative: &str, content: &str) -> Result<()> {
        let path = self.resolve(relative)?;
        if path.is_dir() {
            return Err(WorkspaceError::IsDirectory(relative.into()));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&path, content.as_bytes()).await?;
        debug!(path = %path.display(), bytes = content.len(), "workspace write");
        Ok(())
    }

    pub async fn create(&self, relative: &str, is_dir: bool) -> Result<()> {
        let path = self.resolve(relative)?;
        if is_dir {
            fs::create_dir_all(&path).await?;
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }
            if !path.exists() {
                fs::write(&path, b"").await?;
            }
        }
        Ok(())
    }

    pub async fn delete(&self, relative: &str, recursive: bool) -> Result<()> {
        let path = self.resolve(relative)?;
        if !path.exists() {
            return Err(WorkspaceError::NotFound(relative.into()));
        }
        if path == self.root {
            return Err(WorkspaceError::Other("cannot delete workspace root".into()));
        }
        if path.is_dir() {
            if recursive {
                fs::remove_dir_all(&path).await?;
            } else {
                fs::remove_dir(&path).await?;
            }
        } else {
            fs::remove_file(&path).await?;
        }
        Ok(())
    }

    /// Simple substring search across text files under `relative` (max depth 8).
    pub async fn grep(&self, relative: &str, pattern: &str, limit: usize) -> Result<Vec<GrepHit>> {
        let path = self.resolve(relative)?;
        let mut hits = Vec::new();
        grep_walk(&path, &self.root, pattern, limit, 0, &mut hits).await?;
        Ok(hits)
    }

    // -----------------------------------------------------------------------
    // Worktree isolation (Phase 3.5 — default strategy for sub-agents)
    // -----------------------------------------------------------------------

    /// Relative directory under the workspace where agent worktrees live.
    pub const WORKTREE_DIR: &'static str = ".worktrees";

    /// Whether worktree isolation is recommended/default for parallel agents.
    pub fn worktree_isolation_default() -> bool {
        true
    }

    /// Path (relative to workspace root) where file checkpoints are stored.
    pub const CHECKPOINT_DIR: &'static str = ".hakimi/checkpoints";

    /// Create a named file snapshot of selected relative paths (or all tracked text files
    /// under `paths` when empty → snapshot `paths` roots).
    ///
    /// Snapshots live under `.hakimi/checkpoints/<id>/` with a `manifest.json` and
    /// content files preserving relative paths.
    pub async fn create_checkpoint(
        &self,
        label: Option<&str>,
        paths: &[String],
    ) -> Result<CheckpointInfo> {
        let id = format!(
            "cp-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let rel_root = format!("{}/{}", Self::CHECKPOINT_DIR, id);
        let abs_root = self.resolve(&rel_root)?;
        fs::create_dir_all(&abs_root).await?;

        let mut files: Vec<String> = Vec::new();
        let targets: Vec<String> = if paths.is_empty() {
            // Default: top-level entries (non-hidden).
            let entries = self.list("").await.unwrap_or_default();
            entries
                .into_iter()
                .filter(|e| !e.name.starts_with('.'))
                .map(|e| e.path)
                .collect()
        } else {
            paths.to_vec()
        };

        for p in targets {
            if let Err(e) = self.snapshot_path_into(&p, &abs_root, &mut files).await {
                debug!(path = %p, error = %e, "checkpoint skip path");
            }
        }

        let info = CheckpointInfo {
            id: id.clone(),
            label: label.map(|s| s.to_string()),
            created_at: chrono_like_now(),
            files: files.clone(),
            path: rel_root.clone(),
        };
        let manifest = serde_json::to_string_pretty(&info)
            .map_err(|e| WorkspaceError::Other(e.to_string()))?;
        fs::write(abs_root.join("manifest.json"), manifest).await?;
        Ok(info)
    }

    async fn snapshot_path_into(
        &self,
        relative: &str,
        checkpoint_root: &Path,
        files: &mut Vec<String>,
    ) -> Result<()> {
        let src = self.resolve(relative)?;
        if !src.exists() {
            return Err(WorkspaceError::NotFound(relative.into()));
        }
        if src.is_dir() {
            let mut rd = fs::read_dir(&src).await?;
            while let Some(entry) = rd.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
                let child_rel = if relative.is_empty() {
                    name
                } else {
                    format!("{}/{}", relative.trim_end_matches('/'), name)
                };
                // Cap depth / size via recursion on list.
                Box::pin(self.snapshot_path_into(&child_rel, checkpoint_root, files)).await?;
            }
        } else {
            let meta = fs::metadata(&src).await?;
            if meta.len() > self.max_read_bytes {
                return Ok(());
            }
            let dest = checkpoint_root.join("files").join(relative);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&src, &dest).await?;
            files.push(relative.to_string());
        }
        Ok(())
    }

    /// List available checkpoints.
    pub async fn list_checkpoints(&self) -> Result<Vec<CheckpointInfo>> {
        let parent = match self.resolve(Self::CHECKPOINT_DIR) {
            Ok(p) if p.is_dir() => p,
            _ => return Ok(Vec::new()),
        };
        let mut out = Vec::new();
        let mut rd = fs::read_dir(&parent).await?;
        while let Some(entry) = rd.next_entry().await? {
            if !entry.metadata().await?.is_dir() {
                continue;
            }
            let manifest = entry.path().join("manifest.json");
            if let Ok(text) = fs::read_to_string(&manifest).await {
                if let Ok(info) = serde_json::from_str::<CheckpointInfo>(&text) {
                    out.push(info);
                    continue;
                }
            }
            let name = entry.file_name().to_string_lossy().to_string();
            out.push(CheckpointInfo {
                id: name.clone(),
                label: None,
                created_at: String::new(),
                files: Vec::new(),
                path: format!("{}/{}", Self::CHECKPOINT_DIR, name),
            });
        }
        out.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(out)
    }

    /// Restore files from a checkpoint (overwrites current workspace files).
    pub async fn restore_checkpoint(&self, checkpoint_id: &str) -> Result<CheckpointInfo> {
        let rel = format!("{}/{}", Self::CHECKPOINT_DIR, checkpoint_id);
        let abs = self.resolve(&rel)?;
        let manifest_path = abs.join("manifest.json");
        let info: CheckpointInfo = if manifest_path.exists() {
            let text = fs::read_to_string(&manifest_path).await?;
            serde_json::from_str(&text).map_err(|e| WorkspaceError::Other(e.to_string()))?
        } else {
            return Err(WorkspaceError::NotFound(rel));
        };
        let files_root = abs.join("files");
        for f in &info.files {
            let src = files_root.join(f);
            if !src.exists() {
                continue;
            }
            let dest = self.resolve(f)?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&src, &dest).await?;
        }
        Ok(info)
    }

    /// Path (relative to workspace root) for a named agent worktree.
    pub fn worktree_relative_path(agent_id: &str) -> String {
        let safe: String = agent_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}/{}", Self::WORKTREE_DIR, safe)
    }

    /// Create (or ensure) a git worktree for `agent_id` under `.worktrees/<id>`.
    ///
    /// Requires the workspace root to be a git repository. Uses
    /// `git worktree add` with a new branch `hakimi/<agent_id>` when possible.
    /// Returns the absolute path of the worktree.
    pub async fn ensure_worktree(&self, agent_id: &str) -> Result<PathBuf> {
        let rel = Self::worktree_relative_path(agent_id);
        let abs = self.resolve(&rel)?;
        if abs.is_dir() {
            return Ok(abs);
        }
        // Ensure parent .worktrees exists inside jail.
        let parent_rel = Self::WORKTREE_DIR;
        let parent = self.resolve(parent_rel)?;
        fs::create_dir_all(&parent).await?;

        let branch = format!(
            "hakimi/{}",
            agent_id.replace(
                |c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_',
                "_"
            )
        );
        let agent_owned = agent_id.to_string();
        let root = self.root.clone();
        let abs_clone = abs.clone();
        let output = tokio::task::spawn_blocking(move || {
            // Prefer linked worktree; fall back to plain dir if not a git repo.
            let status = std::process::Command::new("git")
                .args(["rev-parse", "--is-inside-work-tree"])
                .current_dir(&root)
                .output();
            match status {
                Ok(o) if o.status.success() => {
                    // Remove branch if leftover, ignore errors.
                    let _ = std::process::Command::new("git")
                        .args(["branch", "-D", &branch])
                        .current_dir(&root)
                        .output();
                    std::process::Command::new("git")
                        .args([
                            "worktree",
                            "add",
                            "-b",
                            &branch,
                            abs_clone.to_str().unwrap_or(".worktrees/agent"),
                            "HEAD",
                        ])
                        .current_dir(&root)
                        .output()
                }
                _ => {
                    // Not a git repo: create isolated directory copy marker.
                    std::fs::create_dir_all(&abs_clone)?;
                    std::fs::write(
                        abs_clone.join(".hakimi-worktree"),
                        format!("agent={agent_owned}\nisolation=dir\n"),
                    )?;
                    Ok(std::process::Output {
                        status: std::process::ExitStatus::default(),
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    })
                }
            }
        })
        .await
        .map_err(|e| WorkspaceError::Other(e.to_string()))?
        .map_err(|e| WorkspaceError::Git(e.to_string()))?;

        if !output.status.success() && !abs.is_dir() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            // Fallback: plain directory isolation.
            fs::create_dir_all(&abs).await?;
            fs::write(
                abs.join(".hakimi-worktree"),
                format!("agent={agent_id}\nisolation=dir-fallback\nerr={err}\n"),
            )
            .await?;
        }
        Ok(abs)
    }

    /// List agent worktrees under `.worktrees/`.
    pub async fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        let parent = match self.resolve(Self::WORKTREE_DIR) {
            Ok(p) if p.is_dir() => p,
            _ => return Ok(Vec::new()),
        };
        let mut out = Vec::new();
        let mut rd = fs::read_dir(&parent).await?;
        while let Some(entry) = rd.next_entry().await? {
            let meta = entry.metadata().await?;
            if !meta.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            let rel = path
                .strip_prefix(&self.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            out.push(WorktreeInfo {
                agent_id: name,
                path: rel,
                is_git_worktree: path.join(".git").exists()
                    || path.join(".hakimi-worktree").exists(),
            });
        }
        out.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
        Ok(out)
    }

    /// Remove a worktree (git worktree remove or directory delete).
    pub async fn remove_worktree(&self, agent_id: &str) -> Result<()> {
        let rel = Self::worktree_relative_path(agent_id);
        let abs = self.resolve(&rel)?;
        if !abs.exists() {
            return Err(WorkspaceError::NotFound(rel));
        }
        let root = self.root.clone();
        let abs_clone = abs.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = std::process::Command::new("git")
                .args([
                    "worktree",
                    "remove",
                    "--force",
                    abs_clone.to_str().unwrap_or(""),
                ])
                .current_dir(&root)
                .output();
        })
        .await;
        if abs.exists() {
            if abs.is_dir() {
                fs::remove_dir_all(&abs).await?;
            } else {
                fs::remove_file(&abs).await?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub agent_id: String,
    /// Relative path from workspace root.
    pub path: String,
    pub is_git_worktree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointInfo {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub created_at: String,
    pub files: Vec<String>,
    /// Relative path of the checkpoint directory under workspace root.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepHit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

fn chrono_like_now() -> String {
    // RFC3339-ish without chrono dep: use system clock UTC approx via epoch.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

async fn grep_walk(
    dir: &Path,
    root: &Path,
    pattern: &str,
    limit: usize,
    depth: usize,
    hits: &mut Vec<GrepHit>,
) -> Result<()> {
    if hits.len() >= limit || depth > 8 {
        return Ok(());
    }
    if !dir.is_dir() {
        return Ok(());
    }
    let mut rd = fs::read_dir(dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        if hits.len() >= limit {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        let path = entry.path();
        let meta = entry.metadata().await?;
        if meta.is_dir() {
            Box::pin(grep_walk(&path, root, pattern, limit, depth + 1, hits)).await?;
        } else if meta.len() <= DEFAULT_MAX_READ_BYTES {
            if let Ok(text) = fs::read_to_string(&path).await {
                for (i, line) in text.lines().enumerate() {
                    if line.contains(pattern) {
                        let rel = path
                            .strip_prefix(root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();
                        hits.push(GrepHit {
                            path: rel,
                            line: i + 1,
                            text: line.chars().take(200).collect(),
                        });
                        if hits.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn rejects_parent_escape() {
        let dir = tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        assert!(matches!(
            ws.resolve("../etc/passwd"),
            Err(WorkspaceError::PathEscape(_))
        ));
        assert!(matches!(
            ws.resolve("foo/../../etc"),
            Err(WorkspaceError::PathEscape(_))
        ));
    }

    #[tokio::test]
    async fn list_read_write_roundtrip() {
        let dir = tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        ws.write("src/hello.txt", "hi studio").await.unwrap();
        let entries = ws.list("").await.unwrap();
        assert!(entries.iter().any(|e| e.name == "src" && e.is_dir));
        let nested = ws.list("src").await.unwrap();
        assert!(nested.iter().any(|e| e.name == "hello.txt"));
        let content = ws.read("src/hello.txt").await.unwrap();
        assert_eq!(content, "hi studio");
    }

    #[tokio::test]
    async fn grep_finds_line() {
        let dir = tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        ws.write("a.rs", "fn main() {\n    println!(\"studio\");\n}\n")
            .await
            .unwrap();
        let hits = ws.grep("", "studio", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
    }

    #[tokio::test]
    async fn worktree_dir_isolation_without_git() {
        let dir = tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        assert!(Workspace::worktree_isolation_default());
        let path = ws.ensure_worktree("agent-1").await.unwrap();
        assert!(path.is_dir());
        let list = ws.list_worktrees().await.unwrap();
        assert!(list.iter().any(|w| w.agent_id == "agent-1"));
        ws.remove_worktree("agent-1").await.unwrap();
        let list2 = ws.list_worktrees().await.unwrap();
        assert!(!list2.iter().any(|w| w.agent_id == "agent-1"));
    }

    #[tokio::test]
    async fn checkpoint_create_and_restore() {
        let dir = tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        ws.write("note.txt", "v1").await.unwrap();
        let cp = ws
            .create_checkpoint(Some("before"), &["note.txt".into()])
            .await
            .unwrap();
        assert!(!cp.id.is_empty());
        ws.write("note.txt", "v2").await.unwrap();
        assert_eq!(ws.read("note.txt").await.unwrap(), "v2");
        ws.restore_checkpoint(&cp.id).await.unwrap();
        assert_eq!(ws.read("note.txt").await.unwrap(), "v1");
        let listed = ws.list_checkpoints().await.unwrap();
        assert!(listed.iter().any(|c| c.id == cp.id));
    }
}
