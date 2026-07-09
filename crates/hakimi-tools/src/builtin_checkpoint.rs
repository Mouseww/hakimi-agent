//! Checkpoint manager — transparent shadow-git snapshots before file mutations.
//!
//! The checkpoint store intentionally lives outside the user's project git
//! directory. Git is used as a content-addressed snapshot engine through
//! `GIT_DIR`, `GIT_WORK_TREE`, and a per-project `GIT_INDEX_FILE`.

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn};

use crate::Tool;

const STORE_DIR: &str = "store";
const INDEXES_DIR: &str = "indexes";
const PROJECTS_DIR: &str = "projects";
const REFS_PREFIX: &str = "refs/hakimi";
const DEFAULT_LIST_LIMIT: usize = 20;
const MAX_LIST_LIMIT: usize = 100;

const DEFAULT_EXCLUDES: &[&str] = &[
    ".git/",
    ".hg/",
    ".svn/",
    ".worktrees/",
    ".hakimi-checkpoints/",
    "target/",
    "node_modules/",
    "dist/",
    "build/",
    ".next/",
    ".nuxt/",
    "__pycache__/",
    ".cache/",
    ".pytest_cache/",
    ".mypy_cache/",
    ".ruff_cache/",
    ".venv/",
    "venv/",
    "env/",
    ".env",
    ".env.*",
    "*.log",
    "*.zip",
    "*.tar",
    "*.tar.gz",
    "*.tgz",
    "*.7z",
    "*.rar",
    "*.mp4",
    "*.mov",
    "*.mkv",
    "*.webm",
    "*.exe",
    "*.dll",
    "*.dylib",
    "*.so",
    "*.o",
    "*.a",
];

/// Built-in tool for creating and managing filesystem checkpoints.
pub struct CheckpointTool;

#[async_trait]
impl Tool for CheckpointTool {
    fn name(&self) -> &str {
        "checkpoint"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Create and manage filesystem checkpoints in a shared shadow-git store \
         under ~/.hakimi/checkpoints without touching the project .git directory."
    }

    fn emoji(&self) -> &str {
        "\u{1f4be}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "rollback", "diff", "status"],
                    "description": "Action to perform: create, list, rollback, diff, or status."
                },
                "checkpoint_id": {
                    "type": "string",
                    "description": "Checkpoint commit id, required for rollback and diff."
                },
                "label": {
                    "type": "string",
                    "description": "Optional label for a created checkpoint."
                },
                "path": {
                    "type": "string",
                    "description": "Optional relative file path for rollback or diff."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "description": "Maximum checkpoints to list."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: action".into()))?;

        match action {
            "create" => {
                let label = args.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let store = CheckpointStore::for_workdir(Path::new(&ctx.workdir))?;
                store.create(label).map(|body| body.to_string())
            }
            "list" => {
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| usize::try_from(v).ok())
                    .unwrap_or(DEFAULT_LIST_LIMIT)
                    .clamp(1, MAX_LIST_LIMIT);
                let store = CheckpointStore::for_workdir(Path::new(&ctx.workdir))?;
                store.list(limit).map(|body| body.to_string())
            }
            "rollback" => {
                let checkpoint_id = required_checkpoint_id(args)?;
                let path = optional_relative_path(args, "path")?;
                let store = CheckpointStore::for_workdir(Path::new(&ctx.workdir))?;
                store
                    .rollback(checkpoint_id, path.as_deref())
                    .map(|body| body.to_string())
            }
            "diff" => {
                let checkpoint_id = required_checkpoint_id(args)?;
                let path = optional_relative_path(args, "path")?;
                let store = CheckpointStore::for_workdir(Path::new(&ctx.workdir))?;
                store
                    .diff(checkpoint_id, path.as_deref())
                    .map(|body| body.to_string())
            }
            "status" => {
                let store = CheckpointStore::for_workdir(Path::new(&ctx.workdir))?;
                store.status().map(|body| body.to_string())
            }
            _ => Err(HakimiError::ToolSimple(format!(
                "Unknown checkpoint action: '{action}'. Valid actions: create, list, rollback, diff, status"
            ))),
        }
    }
}

/// Render checkpoint slash-command output for gateway and other text surfaces.
pub fn checkpoint_response(raw: Option<&str>, workdir: &Path) -> String {
    let mut parts = raw.unwrap_or("list").split_whitespace();
    let action = parts.next().unwrap_or("list");
    let store = match CheckpointStore::for_workdir(workdir) {
        Ok(store) => store,
        Err(err) => return format!("Failed to initialize checkpoints: {err}"),
    };

    let result = match action {
        "list" | "ls" => store.list(DEFAULT_LIST_LIMIT),
        "status" => store.status(),
        "create" | "new" => {
            let label = parts.collect::<Vec<_>>().join(" ");
            store.create(&label)
        }
        "restore" | "rollback" => {
            let Some(id) = parts.next() else {
                return "Usage: /checkpoints restore <id> [relative-path]".to_string();
            };
            let path = parts.next();
            store.rollback(id, path)
        }
        "diff" => {
            let Some(id) = parts.next() else {
                return "Usage: /checkpoints diff <id> [relative-path]".to_string();
            };
            let path = parts.next();
            store.diff(id, path)
        }
        _ => {
            return "Usage: /checkpoints <list|status|create [label]|diff <id> [path]|restore <id> [path]>".to_string();
        }
    };

    match result {
        Ok(value) => format_checkpoint_value(&value),
        Err(err) => format!("Checkpoint command failed: {err}"),
    }
}

fn format_checkpoint_value(value: &JsonValue) -> String {
    match value.get("status").and_then(|v| v.as_str()) {
        Some("created") => format!(
            "Checkpoint created: `{}`\n{}",
            value
                .get("short_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
            value.get("message").and_then(|v| v.as_str()).unwrap_or("")
        ),
        Some("rolled_back") => format!(
            "Checkpoint restored from `{}`{}.",
            value
                .get("short_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
            value
                .get("path")
                .and_then(|v| v.as_str())
                .map(|p| format!(" for `{p}`"))
                .unwrap_or_default()
        ),
        Some("status") => format!(
            "Checkpoint store: `{}`\nProject: `{}`\nCheckpoints: {}\nStore size: {} bytes",
            value
                .get("base")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
            value
                .get("project_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
            value
                .get("checkpoint_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            value
                .get("store_size_bytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        ),
        _ if value.get("checkpoints").is_some() => {
            let Some(items) = value.get("checkpoints").and_then(|v| v.as_array()) else {
                return value.to_string();
            };
            if items.is_empty() {
                return "No checkpoints found.".to_string();
            }
            let mut lines = vec!["Recent checkpoints:".to_string()];
            for item in items {
                lines.push(format!(
                    "- `{}` {} {}",
                    item.get("short_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    item.get("timestamp").and_then(|v| v.as_str()).unwrap_or(""),
                    item.get("message").and_then(|v| v.as_str()).unwrap_or("")
                ));
            }
            lines.push("Use `/checkpoints diff <id>` or `/checkpoints restore <id>`.".to_string());
            lines.join("\n")
        }
        _ if value.get("diff").is_some() => {
            let diff = value.get("diff").and_then(|v| v.as_str()).unwrap_or("");
            if diff.trim().is_empty() {
                "No changes since that checkpoint.".to_string()
            } else {
                format!("```diff\n{diff}\n```")
            }
        }
        _ => value.to_string(),
    }
}

#[derive(Debug, Clone)]
struct CheckpointStore {
    base: PathBuf,
    store: PathBuf,
    workdir: PathBuf,
    project_id: String,
    index: PathBuf,
    project_ref: String,
}

impl CheckpointStore {
    fn for_workdir(workdir: &Path) -> Result<Self> {
        let base = checkpoint_base_dir();
        Self::with_base(base, workdir)
    }

    fn with_base(base: PathBuf, workdir: &Path) -> Result<Self> {
        let workdir = workdir
            .canonicalize()
            .map_err(|e| HakimiError::ToolSimple(format!("invalid checkpoint workdir: {e}")))?;
        if !workdir.is_dir() {
            return Err(HakimiError::ToolSimple(format!(
                "checkpoint workdir is not a directory: {}",
                workdir.display()
            )));
        }
        let project_id = project_id(&workdir);
        let store = base.join(STORE_DIR);
        let index = store.join(INDEXES_DIR).join(format!("{project_id}.index"));
        let project_ref = format!("{REFS_PREFIX}/{project_id}");
        let this = Self {
            base,
            store,
            workdir,
            project_id,
            index,
            project_ref,
        };
        this.ensure_store()?;
        Ok(this)
    }

    fn create(&self, label: &str) -> Result<JsonValue> {
        self.register_project()?;
        match self.current_tip() {
            Ok(parent) => self.git_ok([OsString::from("read-tree"), OsString::from(parent)])?,
            Err(_) => self.git_ok(["read-tree", "--empty"])?,
        }
        self.git_ok(["add", "-A", "--", "."])?;
        let tree = self.git_stdout(["write-tree"])?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let trimmed_label = label.trim();
        let message = if trimmed_label.is_empty() {
            format!("checkpoint: {timestamp}")
        } else {
            format!("checkpoint: {trimmed_label} ({timestamp})")
        };

        let parent = self.current_tip().ok();
        let mut args = vec![
            OsString::from("commit-tree"),
            OsString::from("--no-gpg-sign"),
            OsString::from(tree.trim()),
            OsString::from("-m"),
            OsString::from(&message),
        ];
        if let Some(parent) = parent.as_deref() {
            args.push(OsString::from("-p"));
            args.push(OsString::from(parent));
        }
        let hash = self.git_stdout(args)?;
        let hash = hash.trim().to_string();
        self.git_ok([
            OsString::from("update-ref"),
            OsString::from(&self.project_ref),
            OsString::from(&hash),
        ])?;

        info!(checkpoint_id = %hash, label = %trimmed_label, "Checkpoint created");
        Ok(json!({
            "status": "created",
            "checkpoint_id": &hash,
            "short_id": short_id(&hash),
            "label": trimmed_label,
            "message": message,
            "timestamp": timestamp,
            "store": self.store.display().to_string(),
            "project_id": &self.project_id,
        }))
    }

    fn list(&self, limit: usize) -> Result<JsonValue> {
        if self.current_tip().is_err() {
            return Ok(json!({
                "checkpoints": [],
                "count": 0,
                "project_id": &self.project_id,
                "message": "No checkpoints exist yet. Use action='create' to create one."
            }));
        }

        let output = self.git_stdout([
            OsString::from("log"),
            OsString::from(format!("--max-count={limit}")),
            OsString::from("--format=%H%x1f%h%x1f%cI%x1f%s"),
            OsString::from(&self.project_ref),
        ])?;

        let checkpoints: Vec<JsonValue> = output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\u{1f}').collect();
                if parts.len() == 4 {
                    Some(json!({
                        "id": parts[0],
                        "short_id": parts[1],
                        "timestamp": parts[2],
                        "message": parts[3],
                    }))
                } else {
                    None
                }
            })
            .collect();
        let count = checkpoints.len();

        Ok(json!({
            "checkpoints": checkpoints,
            "count": count,
            "project_id": &self.project_id,
            "store": self.store.display().to_string(),
        }))
    }

    fn rollback(&self, checkpoint_id: &str, path: Option<&str>) -> Result<JsonValue> {
        let commit = self.resolve_project_commit(checkpoint_id)?;
        let path = validate_optional_path(path, &self.workdir)?;
        let mut args = vec![
            OsString::from("checkout"),
            OsString::from(&commit),
            OsString::from("--"),
        ];
        if let Some(path) = path.as_deref() {
            args.push(OsString::from(path));
        } else {
            args.push(OsString::from("."));
        }
        self.git_ok(args)?;

        info!(checkpoint_id = %commit, path = ?path, "Rolled back to checkpoint");
        Ok(json!({
            "status": "rolled_back",
            "checkpoint_id": &commit,
            "short_id": short_id(&commit),
            "path": path,
            "message": format!("Successfully rolled back to checkpoint {}", short_id(&commit)),
        }))
    }

    fn diff(&self, checkpoint_id: &str, path: Option<&str>) -> Result<JsonValue> {
        let commit = self.resolve_project_commit(checkpoint_id)?;
        let path = validate_optional_path(path, &self.workdir)?;
        let mut args = vec![
            OsString::from("diff"),
            OsString::from(&commit),
            OsString::from("--"),
        ];
        if let Some(path) = path.as_deref() {
            args.push(OsString::from(path));
        } else {
            args.push(OsString::from("."));
        }
        let output = self.git_stdout(args)?;
        Ok(json!({
            "checkpoint_id": &commit,
            "short_id": short_id(&commit),
            "path": path,
            "diff": output,
            "has_changes": !output.trim().is_empty(),
        }))
    }

    fn status(&self) -> Result<JsonValue> {
        let count = if self.current_tip().is_ok() {
            self.git_stdout([
                OsString::from("rev-list"),
                OsString::from("--count"),
                OsString::from(&self.project_ref),
            ])
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0)
        } else {
            0
        };
        Ok(json!({
            "status": "status",
            "base": self.base.display().to_string(),
            "store": self.store.display().to_string(),
            "workdir": self.workdir.display().to_string(),
            "project_id": &self.project_id,
            "project_ref": &self.project_ref,
            "checkpoint_count": count,
            "store_size_bytes": dir_size_bytes(&self.base),
        }))
    }

    fn ensure_store(&self) -> Result<()> {
        fs::create_dir_all(&self.base).map_err(HakimiError::Io)?;
        fs::create_dir_all(self.store.join(INDEXES_DIR)).map_err(HakimiError::Io)?;
        fs::create_dir_all(self.store.join(PROJECTS_DIR)).map_err(HakimiError::Io)?;
        if !self.store.join("HEAD").exists() {
            fs::create_dir_all(&self.store).map_err(HakimiError::Io)?;
            raw_git_ok(
                [
                    OsString::from("init"),
                    OsString::from("--bare"),
                    self.store.clone().into_os_string(),
                ],
                &self.base,
            )?;
            raw_git_ok(
                [
                    OsString::from("--git-dir"),
                    self.store.clone().into_os_string(),
                    OsString::from("config"),
                    OsString::from("user.email"),
                    OsString::from("hakimi@checkpoint.local"),
                ],
                &self.base,
            )?;
            raw_git_ok(
                [
                    OsString::from("--git-dir"),
                    self.store.clone().into_os_string(),
                    OsString::from("config"),
                    OsString::from("user.name"),
                    OsString::from("Hakimi Checkpoint"),
                ],
                &self.base,
            )?;
            raw_git_ok(
                [
                    OsString::from("--git-dir"),
                    self.store.clone().into_os_string(),
                    OsString::from("config"),
                    OsString::from("commit.gpgsign"),
                    OsString::from("false"),
                ],
                &self.base,
            )?;
            raw_git_ok(
                [
                    OsString::from("--git-dir"),
                    self.store.clone().into_os_string(),
                    OsString::from("config"),
                    OsString::from("tag.gpgSign"),
                    OsString::from("false"),
                ],
                &self.base,
            )?;
            raw_git_ok(
                [
                    OsString::from("--git-dir"),
                    self.store.clone().into_os_string(),
                    OsString::from("config"),
                    OsString::from("gc.auto"),
                    OsString::from("0"),
                ],
                &self.base,
            )?;
        }
        let info_dir = self.store.join("info");
        fs::create_dir_all(&info_dir).map_err(HakimiError::Io)?;
        let exclude_path = info_dir.join("exclude");
        if !exclude_path.exists() {
            fs::write(exclude_path, format!("{}\n", DEFAULT_EXCLUDES.join("\n")))
                .map_err(HakimiError::Io)?;
        }
        Ok(())
    }

    fn register_project(&self) -> Result<()> {
        let path = self
            .store
            .join(PROJECTS_DIR)
            .join(format!("{}.json", self.project_id));
        let now = chrono::Utc::now().to_rfc3339();
        let body = json!({
            "workdir": self.workdir.display().to_string(),
            "project_id": &self.project_id,
            "last_touch": now,
        });
        fs::write(path, body.to_string()).map_err(HakimiError::Io)
    }

    fn current_tip(&self) -> Result<String> {
        self.git_stdout([
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from(&self.project_ref),
        ])
        .map(|value| value.trim().to_string())
    }

    fn resolve_project_commit(&self, checkpoint_id: &str) -> Result<String> {
        validate_checkpoint_id(checkpoint_id)?;
        let commit = self.git_stdout([
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from(format!("{checkpoint_id}^{{commit}}")),
        ])?;
        let commit = commit.trim().to_string();
        let output = self.git_status([
            OsString::from("merge-base"),
            OsString::from("--is-ancestor"),
            OsString::from(&commit),
            OsString::from(&self.project_ref),
        ])?;
        if !output.status.success() {
            return Err(HakimiError::ToolSimple(format!(
                "checkpoint {} does not belong to this workdir",
                short_id(&commit)
            )));
        }
        Ok(commit)
    }

    fn git_ok<I, S>(&self, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.git_status(args)?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        warn!(stderr = %stderr, "checkpoint git command failed");
        Err(HakimiError::ToolSimple(format!(
            "checkpoint git command failed: {stderr}"
        )))
    }

    fn git_stdout<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.git_status(args)?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(HakimiError::ToolSimple(format!(
            "checkpoint git command failed: {stderr}"
        )))
    }

    fn git_status<I, S>(&self, args: I) -> Result<std::process::Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new("git");
        command
            .current_dir(&self.workdir)
            .args(args)
            .env("GIT_DIR", &self.store)
            .env("GIT_WORK_TREE", &self.workdir)
            .env("GIT_INDEX_FILE", &self.index)
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_SYSTEM", null_device())
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
            .output()
            .map_err(|e| HakimiError::Other(format!("Failed to run git: {e}")))
    }
}

fn raw_git_ok<I, S>(args: I, cwd: &Path) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|e| HakimiError::Other(format!("Failed to run git: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(HakimiError::ToolSimple(format!(
        "checkpoint git init failed: {stderr}"
    )))
}

fn checkpoint_base_dir() -> PathBuf {
    std::env::var_os("HAKIMI_CHECKPOINT_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HAKIMI_HOME").map(|home| PathBuf::from(home).join("checkpoints"))
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(".hakimi").join("checkpoints")))
        .unwrap_or_else(|| PathBuf::from(".hakimi").join("checkpoints"))
}

fn required_checkpoint_id(args: &JsonValue) -> Result<&str> {
    args.get("checkpoint_id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| HakimiError::ToolSimple("checkpoint_id is required".into()))
}

fn optional_relative_path(args: &JsonValue, key: &str) -> Result<Option<String>> {
    match args.get(key).and_then(|v| v.as_str()) {
        Some(value) if !value.trim().is_empty() => Ok(Some(value.trim().to_string())),
        _ => Ok(None),
    }
}

fn validate_checkpoint_id(value: &str) -> Result<()> {
    let value = value.trim();
    if !(4..=64).contains(&value.len()) || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(HakimiError::ToolSimple(
            "checkpoint_id must be a 4-64 character hex commit id".into(),
        ));
    }
    Ok(())
}

fn validate_optional_path(path: Option<&str>, workdir: &Path) -> Result<Option<String>> {
    let Some(path) = path.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(None);
    };
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(HakimiError::ToolSimple(
            "checkpoint path must be relative to the workdir".into(),
        ));
    }
    let resolved = workdir.join(candidate);
    let normalized = normalize_path_for_prefix(&resolved)?;
    if !normalized.starts_with(workdir) {
        return Err(HakimiError::ToolSimple(
            "checkpoint path escapes the workdir".into(),
        ));
    }
    Ok(Some(path.replace('\\', "/")))
}

fn normalize_path_for_prefix(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(HakimiError::ToolSimple(
                        "path traversal is not allowed".into(),
                    ));
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

fn project_id(workdir: &Path) -> String {
    let path = workdir.to_string_lossy();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn short_id(value: &str) -> &str {
    &value[..value.len().min(8)]
}

fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

fn dir_size_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total = total.saturating_add(dir_size_bytes(&path));
        } else {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store_for(tmp: &tempfile::TempDir, workdir: &Path) -> CheckpointStore {
        CheckpointStore::with_base(tmp.path().join("checkpoints"), workdir).unwrap()
    }

    #[test]
    fn test_tool_metadata() {
        let tool = CheckpointTool;
        assert_eq!(tool.name(), "checkpoint");
        assert_eq!(tool.toolset(), "file");
        assert!(!tool.description().is_empty());
        assert!(!tool.emoji().is_empty());
    }

    #[test]
    fn schema_has_valid_actions_enum() {
        let tool = CheckpointTool;
        let schema = tool.schema();
        let actions = schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum should be an array");
        let action_strs: Vec<&str> = actions.iter().map(|v| v.as_str().unwrap()).collect();
        for action in ["create", "list", "rollback", "diff", "status"] {
            assert!(action_strs.contains(&action), "missing action {action}");
        }
    }

    #[test]
    fn schema_required_field_is_action() {
        let tool = CheckpointTool;
        let schema = tool.schema();
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required, &vec![json!("action")]);
    }

    #[test]
    fn schema_exposes_optional_file_path() {
        let tool = CheckpointTool;
        let schema = tool.schema();
        assert_eq!(schema["properties"]["path"]["type"], "string");
    }

    #[test]
    fn validates_checkpoint_ids() {
        assert!(validate_checkpoint_id("abc123").is_ok());
        assert!(validate_checkpoint_id("--patch").is_err());
        assert!(validate_checkpoint_id("../abc").is_err());
    }

    #[test]
    fn validates_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().canonicalize().unwrap();
        assert_eq!(
            validate_optional_path(Some("src/main.rs"), &workdir).unwrap(),
            Some("src/main.rs".into())
        );
        assert!(validate_optional_path(Some("../escape"), &workdir).is_err());
        assert!(validate_optional_path(Some("/tmp/file"), &workdir).is_err());
    }

    #[test]
    fn project_id_is_stable_and_path_sensitive() {
        let a = PathBuf::from("/tmp/project-a");
        let b = PathBuf::from("/tmp/project-b");
        assert_eq!(project_id(&a), project_id(&a));
        assert_ne!(project_id(&a), project_id(&b));
    }

    #[test]
    fn short_id_handles_short_values() {
        assert_eq!(short_id("abcdef"), "abcdef");
        assert_eq!(short_id("abcdef123"), "abcdef12");
    }

    #[test]
    fn format_empty_checkpoint_list() {
        let body = json!({ "checkpoints": [], "count": 0 });
        assert_eq!(format_checkpoint_value(&body), "No checkpoints found.");
    }

    #[test]
    fn list_checkpoints_empty_store() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);

        let body = store.list(10).unwrap();
        assert_eq!(body["count"], 0);
        assert!(body["checkpoints"].as_array().unwrap().is_empty());
    }

    #[test]
    fn status_reports_zero_before_create() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);

        let body = store.status().unwrap();
        assert_eq!(body["checkpoint_count"], 0);
        assert_eq!(body["project_id"].as_str().unwrap(), store.project_id);
    }

    #[test]
    fn create_checkpoint_does_not_create_project_git() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(workdir.join("hello.txt"), "checkpoint content").unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);

        let body = store.create("before-refactor").unwrap();
        assert_eq!(body["status"], "created");
        assert!(!body["checkpoint_id"].as_str().unwrap().is_empty());
        assert!(store.store.join("HEAD").exists());
        assert!(!workdir.join(".git").exists());
    }

    #[test]
    fn create_checkpoint_with_label() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(workdir.join("labeled.txt"), "labeled content").unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);

        let body = store.create("before-refactor").unwrap();
        assert_eq!(body["label"], "before-refactor");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("before-refactor")
        );
    }

    #[test]
    fn list_checkpoints_after_create() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(workdir.join("file.txt"), "v1").unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);
        store.create("v1").unwrap();

        let body = store.list(10).unwrap();
        assert_eq!(body["count"], 1);
        assert!(
            body["checkpoints"][0]["message"]
                .as_str()
                .unwrap()
                .contains("v1")
        );
    }

    #[test]
    fn create_multiple_checkpoints_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(workdir.join("file.txt"), "v1").unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);

        store.create("v1").unwrap();
        fs::write(workdir.join("file.txt"), "v2").unwrap();
        store.create("v2").unwrap();

        let body = store.list(10).unwrap();
        assert_eq!(body["count"], 2);
        let messages: Vec<&str> = body["checkpoints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cp| cp["message"].as_str().unwrap())
            .collect();
        assert!(messages.iter().any(|m| m.contains("v1")));
        assert!(messages.iter().any(|m| m.contains("v2")));
    }

    #[test]
    fn diff_checkpoint_detects_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(workdir.join("file.txt"), "original").unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);
        let created = store.create("baseline").unwrap();
        let cp_id = created["checkpoint_id"].as_str().unwrap().to_string();
        fs::write(workdir.join("file.txt"), "modified").unwrap();

        let diff = store.diff(&cp_id, None).unwrap();
        assert_eq!(diff["has_changes"], true);
        let diff_text = diff["diff"].as_str().unwrap();
        assert!(diff_text.contains("original"));
        assert!(diff_text.contains("modified"));
    }

    #[test]
    fn diff_checkpoint_can_target_single_file() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(workdir.join("file.txt"), "original").unwrap();
        fs::write(workdir.join("other.txt"), "unchanged").unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);
        let created = store.create("baseline").unwrap();
        let cp_id = created["checkpoint_id"].as_str().unwrap().to_string();
        fs::write(workdir.join("file.txt"), "modified").unwrap();

        let diff = store.diff(&cp_id, Some("file.txt")).unwrap();
        let diff_text = diff["diff"].as_str().unwrap();
        assert!(diff_text.contains("file.txt"));
        assert!(!diff_text.contains("other.txt"));
    }

    #[test]
    fn diff_rejects_unknown_checkpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);
        assert!(store.diff("deadbeef", None).is_err());
    }

    #[test]
    fn rollback_can_restore_single_file() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(workdir.join("file.txt"), "original").unwrap();
        fs::write(workdir.join("other.txt"), "keep").unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);
        let created = store.create("baseline").unwrap();
        let cp_id = created["checkpoint_id"].as_str().unwrap().to_string();
        fs::write(workdir.join("file.txt"), "modified").unwrap();
        fs::write(workdir.join("other.txt"), "changed").unwrap();

        store.rollback(&cp_id, Some("file.txt")).unwrap();
        assert_eq!(
            fs::read_to_string(workdir.join("file.txt")).unwrap(),
            "original"
        );
        assert_eq!(
            fs::read_to_string(workdir.join("other.txt")).unwrap(),
            "changed"
        );
    }

    #[test]
    fn rollback_rejects_unknown_checkpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        fs::create_dir_all(&workdir).unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = store_for(&store_root, &workdir);
        assert!(store.rollback("deadbeef", None).is_err());
    }

    #[tokio::test]
    async fn test_missing_action_fails() {
        let tool = CheckpointTool;
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            workdir: tmp.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let result = tool.execute(&json!({}), &ctx).await;
        assert!(result.is_err(), "missing action should fail");
    }

    #[tokio::test]
    async fn test_rollback_missing_id() {
        let tool = CheckpointTool;
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            workdir: tmp.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let result = tool.execute(&json!({"action": "rollback"}), &ctx).await;
        assert!(result.is_err());
    }
}
