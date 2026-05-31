use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

use crate::Tool;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const DEFAULT_LOG_TAIL_BYTES: usize = 16 * 1024;
const MAX_LOG_TAIL_BYTES: usize = 128 * 1024;
const DEFAULT_BOARD: &str = "default";
const DEFAULT_NOTIFY_EVENT_KINDS: &[&str] =
    &["completed", "blocked", "gave_up", "crashed", "timed_out"];
const VALID_STATUSES: &[&str] = &[
    "triage", "todo", "ready", "running", "blocked", "review", "done", "archived",
];

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS kanban_tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT,
    assignee TEXT,
    profile TEXT,
    status TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    blocked_reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    heartbeat_at INTEGER
);

CREATE TABLE IF NOT EXISTS kanban_comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    author TEXT,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(task_id) REFERENCES kanban_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS kanban_links (
    parent_id TEXT NOT NULL,
    child_id TEXT NOT NULL,
    relation TEXT NOT NULL DEFAULT 'blocks',
    created_at INTEGER NOT NULL,
    PRIMARY KEY(parent_id, child_id),
    FOREIGN KEY(parent_id) REFERENCES kanban_tasks(id) ON DELETE CASCADE,
    FOREIGN KEY(child_id) REFERENCES kanban_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS kanban_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    actor TEXT,
    note TEXT,
    payload TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(task_id) REFERENCES kanban_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS kanban_notify_subs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    platform TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    thread_id TEXT,
    notifier_profile TEXT,
    last_event_id INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(task_id) REFERENCES kanban_tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_kanban_tasks_status ON kanban_tasks(status);
CREATE INDEX IF NOT EXISTS idx_kanban_tasks_assignee ON kanban_tasks(assignee);
CREATE INDEX IF NOT EXISTS idx_kanban_comments_task ON kanban_comments(task_id, created_at);
CREATE INDEX IF NOT EXISTS idx_kanban_links_child ON kanban_links(child_id);
CREATE INDEX IF NOT EXISTS idx_kanban_events_task ON kanban_events(task_id, id);
CREATE INDEX IF NOT EXISTS idx_kanban_events_kind ON kanban_events(kind, created_at);
CREATE INDEX IF NOT EXISTS idx_kanban_notify_task ON kanban_notify_subs(task_id);
CREATE INDEX IF NOT EXISTS idx_kanban_notify_profile ON kanban_notify_subs(notifier_profile, last_event_id);
"#;

#[derive(Debug, Clone, Serialize)]
pub struct KanbanTask {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub assignee: Option<String>,
    pub profile: Option<String>,
    pub status: String,
    pub priority: i64,
    pub blocked_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub heartbeat_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KanbanComment {
    pub id: i64,
    pub task_id: String,
    pub author: Option<String>,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KanbanLink {
    pub parent_id: String,
    pub child_id: String,
    pub relation: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KanbanEvent {
    pub id: i64,
    pub task_id: String,
    pub kind: String,
    pub actor: Option<String>,
    pub note: Option<String>,
    pub payload: Option<JsonValue>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KanbanNotifySub {
    pub id: i64,
    pub task_id: String,
    pub platform: String,
    pub chat_id: String,
    pub thread_id: Option<String>,
    pub notifier_profile: Option<String>,
    pub last_event_id: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KanbanNotification {
    pub subscription: KanbanNotifySub,
    pub event: KanbanEvent,
    pub terminal: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct KanbanDiagnostic {
    pub kind: String,
    pub severity: String,
    pub task_id: String,
    pub title: String,
    pub detail: String,
    pub actions: Vec<String>,
    pub data: JsonValue,
}

#[derive(Debug, Clone, Serialize)]
pub struct KanbanWorkerLog {
    pub task_id: String,
    pub profile: Option<String>,
    pub body: String,
    pub created_at: i64,
    pub path: String,
}

#[derive(Debug, Clone)]
struct CreateTask {
    title: String,
    body: Option<String>,
    assignee: Option<String>,
    status: String,
    priority: i64,
}

#[derive(Debug, Clone)]
struct NotifyTarget {
    task_id: String,
    platform: String,
    chat_id: String,
    thread_id: Option<String>,
    notifier_profile: Option<String>,
}

pub struct KanbanStore {
    path: PathBuf,
}

impl Default for KanbanStore {
    fn default() -> Self {
        Self::new(default_kanban_db_path())
    }
}

impl KanbanStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn for_board(board: Option<&str>) -> Result<Self> {
        Ok(Self::new(resolve_kanban_db_path(board)?))
    }

    fn connect(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(HakimiError::Io)?;
        }
        let conn = Connection::open(&self.path).map_err(db_err)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(db_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_err)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(db_err)?;
        conn.execute_batch(SCHEMA).map_err(db_err)?;
        migrate_schema(&conn)?;
        Ok(conn)
    }

    fn create_task(&self, input: CreateTask) -> Result<KanbanTask> {
        validate_status(&input.status)?;
        let title = input.title.trim();
        if title.is_empty() {
            return Err(HakimiError::Tool("kanban task title is required".into()));
        }
        let body = normalize_optional(input.body);
        let assignee = normalize_profile_arg(input.assignee.as_deref())?;
        let profile = assignee.clone();
        let status = input.status;
        let priority = input.priority;
        let raw_id = Uuid::new_v4().simple().to_string();
        let id = format!("kb-{}", &raw_id[..10]);
        let now = now_epoch();
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO kanban_tasks
             (id, title, body, assignee, profile, status, priority, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?7)",
            params![
                id,
                title,
                body.as_deref(),
                assignee.as_deref(),
                status.as_str(),
                priority,
                now
            ],
        )
        .map_err(db_err)?;
        self.record_event(
            &id,
            "created",
            Some("hakimi"),
            None,
            Some(json!({
                "status": status,
                "assignee": assignee,
                "profile": profile,
                "priority": priority,
            })),
        )?;
        self.get_task_required(&id)
    }

    fn get_task(&self, id: &str) -> Result<Option<KanbanTask>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT * FROM kanban_tasks WHERE id = ?1",
            params![id],
            KanbanTask::from_row,
        )
        .optional()
        .map_err(db_err)
    }

    fn get_task_required(&self, id: &str) -> Result<KanbanTask> {
        self.get_task(id)?
            .ok_or_else(|| HakimiError::Tool(format!("kanban task not found: {id}")))
    }

    fn list_tasks(
        &self,
        status: Option<&str>,
        assignee: Option<&str>,
        limit: usize,
    ) -> Result<Vec<KanbanTask>> {
        if let Some(status) = status {
            validate_status(status)?;
        }
        let limit = limit.clamp(1, MAX_LIMIT);
        let conn = self.connect()?;
        let mut clauses = vec!["status != 'archived'".to_string()];
        let mut values = Vec::new();
        if let Some(status) = status {
            clauses.push("status = ?".to_string());
            values.push(status.to_string());
        }
        if let Some(assignee) = assignee.and_then(non_empty_str) {
            clauses.push("assignee = ?".to_string());
            values.push(assignee.to_string());
        }
        let sql = format!(
            "SELECT * FROM kanban_tasks WHERE {} \
             ORDER BY priority DESC, created_at ASC LIMIT {}",
            clauses.join(" AND "),
            limit
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(
                params_from_iter(values.iter().map(String::as_str)),
                KanbanTask::from_row,
            )
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    fn comments(&self, task_id: &str) -> Result<Vec<KanbanComment>> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT * FROM kanban_comments WHERE task_id = ?1 ORDER BY created_at ASC, id ASC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![task_id], KanbanComment::from_row)
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    fn parents(&self, task_id: &str) -> Result<Vec<KanbanLink>> {
        self.links_for(task_id, "child_id")
    }

    fn children(&self, task_id: &str) -> Result<Vec<KanbanLink>> {
        self.links_for(task_id, "parent_id")
    }

    fn links_for(&self, task_id: &str, column: &str) -> Result<Vec<KanbanLink>> {
        let conn = self.connect()?;
        let sql = format!("SELECT * FROM kanban_links WHERE {column} = ?1 ORDER BY created_at ASC");
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params![task_id], KanbanLink::from_row)
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    fn add_comment(
        &self,
        task_id: &str,
        body: &str,
        author: Option<&str>,
    ) -> Result<KanbanComment> {
        self.get_task_required(task_id)?;
        let body = body.trim();
        if body.is_empty() {
            return Err(HakimiError::Tool("kanban comment body is required".into()));
        }
        let now = now_epoch();
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO kanban_comments (task_id, author, body, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![task_id, author.and_then(non_empty_str), body, now],
        )
        .map_err(db_err)?;
        let id = conn.last_insert_rowid();
        conn.query_row(
            "SELECT * FROM kanban_comments WHERE id = ?1",
            params![id],
            KanbanComment::from_row,
        )
        .map_err(db_err)
        .and_then(|comment| {
            self.record_event(
                task_id,
                "commented",
                author,
                Some(body),
                Some(json!({"comment_id": comment.id})),
            )?;
            Ok(comment)
        })
    }

    fn complete_task(&self, task_id: &str, summary: Option<&str>) -> Result<KanbanTask> {
        self.update_status(task_id, "done", None, Some(now_epoch()))?;
        if let Some(summary) = summary.and_then(non_empty_str) {
            self.add_comment(task_id, summary, Some("hakimi"))?;
        }
        self.get_task_required(task_id)
    }

    fn block_task(&self, task_id: &str, reason: &str) -> Result<KanbanTask> {
        let reason = non_empty_str(reason)
            .ok_or_else(|| HakimiError::Tool("block reason is required".into()))?;
        self.update_status(task_id, "blocked", Some(reason), None)?;
        self.get_task_required(task_id)
    }

    fn unblock_task(&self, task_id: &str, status: Option<&str>) -> Result<KanbanTask> {
        let next = status.unwrap_or("ready");
        validate_status(next)?;
        if matches!(next, "blocked" | "done" | "archived") {
            return Err(HakimiError::Tool(
                "unblock status must be triage, todo, ready, running, or review".into(),
            ));
        }
        self.update_status(task_id, next, None, None)?;
        self.get_task_required(task_id)
    }

    fn heartbeat_task(&self, task_id: &str, note: Option<&str>) -> Result<KanbanTask> {
        self.get_task_required(task_id)?;
        let now = now_epoch();
        let conn = self.connect()?;
        conn.execute(
            "UPDATE kanban_tasks SET heartbeat_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )
        .map_err(db_err)?;
        if let Some(note) = note.and_then(non_empty_str) {
            self.add_comment(task_id, note, Some("heartbeat"))?;
        }
        self.record_event(task_id, "heartbeat", Some("hakimi"), note, None)?;
        self.get_task_required(task_id)
    }

    fn assign_task(
        &self,
        task_id: &str,
        profile: Option<&str>,
        actor: Option<&str>,
    ) -> Result<KanbanTask> {
        let previous = self.get_task_required(task_id)?;
        let profile = normalize_profile_arg(profile)?;
        let now = now_epoch();
        let conn = self.connect()?;
        conn.execute(
            "UPDATE kanban_tasks
                SET assignee = ?1,
                    profile = ?1,
                    updated_at = ?2
              WHERE id = ?3",
            params![profile.as_deref(), now, task_id],
        )
        .map_err(db_err)?;
        self.record_event(
            task_id,
            "assigned",
            actor.or(Some("hakimi")),
            None,
            Some(json!({
                "previous_assignee": previous.assignee,
                "previous_profile": previous.profile,
                "assignee": profile,
            })),
        )?;
        self.get_task_required(task_id)
    }

    fn append_worker_log(
        &self,
        task_id: &str,
        body: &str,
        profile: Option<&str>,
    ) -> Result<KanbanWorkerLog> {
        let task = self.get_task_required(task_id)?;
        let body = non_empty_str(body)
            .ok_or_else(|| HakimiError::Tool("kanban worker log body is required".into()))?;
        let profile = normalize_profile_arg(profile)?
            .or(task.profile.clone())
            .or(task.assignee.clone());
        let path = self.worker_log_path(task_id)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(HakimiError::Io)?;
        }
        let now = now_epoch();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(HakimiError::Io)?;
        writeln!(
            file,
            "--- ts={} profile={} ---",
            now,
            profile.as_deref().unwrap_or("-")
        )
        .map_err(HakimiError::Io)?;
        writeln!(file, "{body}").map_err(HakimiError::Io)?;
        self.record_event(
            task_id,
            "worker_log",
            profile.as_deref().or(Some("hakimi")),
            Some(first_line(body)),
            Some(json!({"path": path.display().to_string()})),
        )?;
        Ok(KanbanWorkerLog {
            task_id: task_id.to_string(),
            profile,
            body: body.to_string(),
            created_at: now,
            path: path.display().to_string(),
        })
    }

    fn read_worker_log(&self, task_id: &str, tail_bytes: usize) -> Result<Option<KanbanWorkerLog>> {
        let task = self.get_task_required(task_id)?;
        let path = self.worker_log_path(task_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let tail_bytes = tail_bytes.clamp(1, MAX_LOG_TAIL_BYTES);
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(HakimiError::Io)?;
        let len = file.metadata().map_err(HakimiError::Io)?.len();
        let tail = u64::try_from(tail_bytes).unwrap_or(MAX_LOG_TAIL_BYTES as u64);
        if len > tail {
            file.seek(SeekFrom::Start(len - tail))
                .map_err(HakimiError::Io)?;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(HakimiError::Io)?;
        Ok(Some(KanbanWorkerLog {
            task_id: task_id.to_string(),
            profile: task.profile.or(task.assignee),
            body: String::from_utf8_lossy(&bytes).to_string(),
            created_at: now_epoch(),
            path: path.display().to_string(),
        }))
    }

    fn worker_log_path(&self, task_id: &str) -> Result<PathBuf> {
        validate_log_task_id(task_id)?;
        let root = self
            .path
            .parent()
            .map(|parent| parent.join("worker-logs"))
            .unwrap_or_else(|| PathBuf::from("worker-logs"));
        Ok(root.join(format!("{task_id}.log")))
    }

    fn link_tasks(
        &self,
        parent_id: &str,
        child_id: &str,
        relation: Option<&str>,
    ) -> Result<KanbanLink> {
        if parent_id == child_id {
            return Err(HakimiError::Tool(
                "kanban task cannot link to itself".into(),
            ));
        }
        self.get_task_required(parent_id)?;
        self.get_task_required(child_id)?;
        if self.reaches(child_id, parent_id)? {
            return Err(HakimiError::Tool(
                "kanban link would create a dependency cycle".into(),
            ));
        }
        let relation = relation.and_then(non_empty_str).unwrap_or("blocks");
        let now = now_epoch();
        let conn = self.connect()?;
        conn.execute(
            "INSERT OR REPLACE INTO kanban_links (parent_id, child_id, relation, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![parent_id, child_id, relation, now],
        )
        .map_err(db_err)?;
        let link = KanbanLink {
            parent_id: parent_id.to_string(),
            child_id: child_id.to_string(),
            relation: relation.to_string(),
            created_at: now,
        };
        self.record_event(
            parent_id,
            "linked_child",
            Some("hakimi"),
            None,
            Some(json!({
                "child_id": child_id,
                "relation": link.relation.as_str(),
            })),
        )?;
        self.record_event(
            child_id,
            "linked_parent",
            Some("hakimi"),
            None,
            Some(json!({
                "parent_id": parent_id,
                "relation": link.relation.as_str(),
            })),
        )?;
        Ok(link)
    }

    fn update_status(
        &self,
        task_id: &str,
        status: &str,
        blocked_reason: Option<&str>,
        completed_at: Option<i64>,
    ) -> Result<()> {
        self.get_task_required(task_id)?;
        validate_status(status)?;
        let now = now_epoch();
        let conn = self.connect()?;
        conn.execute(
            "UPDATE kanban_tasks
                SET status = ?1,
                    blocked_reason = ?2,
                    completed_at = ?3,
                    updated_at = ?4
              WHERE id = ?5",
            params![status, blocked_reason, completed_at, now, task_id],
        )
        .map_err(db_err)?;
        let kind = match status {
            "blocked" => "blocked",
            "done" => "completed",
            "ready" => "unblocked",
            _ => "status_changed",
        };
        self.record_event(
            task_id,
            kind,
            Some("hakimi"),
            blocked_reason,
            Some(json!({"status": status})),
        )?;
        Ok(())
    }

    fn record_event(
        &self,
        task_id: &str,
        kind: &str,
        actor: Option<&str>,
        note: Option<&str>,
        payload: Option<JsonValue>,
    ) -> Result<KanbanEvent> {
        let kind = non_empty_str(kind)
            .ok_or_else(|| HakimiError::Tool("kanban event kind is required".into()))?;
        let now = now_epoch();
        let payload = payload
            .map(|value| serde_json::to_string(&value))
            .transpose()
            .map_err(|err| HakimiError::Tool(format!("kanban event payload error: {err}")))?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO kanban_events (task_id, kind, actor, note, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                task_id,
                kind,
                actor.and_then(non_empty_str),
                note.and_then(non_empty_str),
                payload,
                now
            ],
        )
        .map_err(db_err)?;
        let id = conn.last_insert_rowid();
        conn.query_row(
            "SELECT * FROM kanban_events WHERE id = ?1",
            params![id],
            KanbanEvent::from_row,
        )
        .map_err(db_err)
    }

    fn events(&self, task_id: &str, limit: usize) -> Result<Vec<KanbanEvent>> {
        self.get_task_required(task_id)?;
        let limit = limit.clamp(1, MAX_LIMIT) as i64;
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT * FROM kanban_events
                 WHERE task_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![task_id, limit], KanbanEvent::from_row)
            .map_err(db_err)?;
        let mut events = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        events.reverse();
        Ok(events)
    }

    fn add_notify_sub(&self, target: NotifyTarget) -> Result<KanbanNotifySub> {
        self.get_task_required(&target.task_id)?;
        let platform = require_notify_text(&target.platform, "platform")?;
        let chat_id = require_notify_text(&target.chat_id, "chat_id")?;
        let thread_id = normalize_notify_text(target.thread_id.as_deref())?;
        let notifier_profile = normalize_profile_arg(target.notifier_profile.as_deref())?;
        let now = now_epoch();
        let conn = self.connect()?;
        if let Some(existing) =
            self.get_notify_sub(&target.task_id, platform, chat_id, thread_id.as_deref())?
        {
            conn.execute(
                "UPDATE kanban_notify_subs
                    SET notifier_profile = COALESCE(?1, notifier_profile),
                        updated_at = ?2
                  WHERE id = ?3",
                params![notifier_profile.as_deref(), now, existing.id],
            )
            .map_err(db_err)?;
        } else {
            conn.execute(
                "INSERT INTO kanban_notify_subs
                    (task_id, platform, chat_id, thread_id, notifier_profile, last_event_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
                params![
                    target.task_id.as_str(),
                    platform,
                    chat_id,
                    thread_id.as_deref(),
                    notifier_profile.as_deref(),
                    now
                ],
            )
            .map_err(db_err)?;
        }
        self.get_notify_sub(&target.task_id, platform, chat_id, thread_id.as_deref())?
            .ok_or_else(|| {
                HakimiError::Tool("kanban notification subscription was not saved".into())
            })
    }

    fn list_notify_subs(&self, task_id: Option<&str>) -> Result<Vec<KanbanNotifySub>> {
        let conn = self.connect()?;
        if let Some(task_id) = task_id.and_then(non_empty_str) {
            self.get_task_required(task_id)?;
            let mut stmt = conn
                .prepare(
                    "SELECT * FROM kanban_notify_subs
                     WHERE task_id = ?1
                     ORDER BY created_at ASC, id ASC",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map(params![task_id], KanbanNotifySub::from_row)
                .map_err(db_err)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT * FROM kanban_notify_subs
                     ORDER BY task_id ASC, created_at ASC, id ASC",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], KanbanNotifySub::from_row)
                .map_err(db_err)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)
        }
    }

    fn remove_notify_sub(&self, target: NotifyTarget) -> Result<JsonValue> {
        let platform = require_notify_text(&target.platform, "platform")?;
        let chat_id = require_notify_text(&target.chat_id, "chat_id")?;
        let thread_id = normalize_notify_text(target.thread_id.as_deref())?;
        let conn = self.connect()?;
        let removed = conn
            .execute(
                "DELETE FROM kanban_notify_subs
                 WHERE task_id = ?1
                   AND platform = ?2
                   AND chat_id = ?3
                   AND COALESCE(thread_id, '') = ?4",
                params![
                    target.task_id.as_str(),
                    platform,
                    chat_id,
                    thread_id.as_deref().unwrap_or("")
                ],
            )
            .map_err(db_err)?;
        Ok(json!({"removed": removed > 0}))
    }

    fn claim_notify_events(
        &self,
        notifier_profile: Option<&str>,
        kinds: &[String],
        limit: usize,
    ) -> Result<Vec<KanbanNotification>> {
        let profile = normalize_profile_arg(notifier_profile)?;
        let kinds = normalize_event_kinds(kinds)?;
        let limit = limit.clamp(1, MAX_LIMIT);
        let mut notifications = Vec::new();
        for sub in self.list_notify_subs(None)? {
            if let Some(profile) = profile.as_deref()
                && sub.notifier_profile.as_deref() != Some(profile)
            {
                continue;
            }
            let claimed = self.claim_for_sub(&sub, &kinds, limit - notifications.len())?;
            notifications.extend(claimed);
            if notifications.len() >= limit {
                break;
            }
        }
        Ok(notifications)
    }

    fn claim_for_sub(
        &self,
        sub: &KanbanNotifySub,
        kinds: &[String],
        remaining: usize,
    ) -> Result<Vec<KanbanNotification>> {
        if remaining == 0 {
            return Ok(Vec::new());
        }
        let conn = self.connect()?;
        let placeholders = vec!["?"; kinds.len()].join(",");
        let clauses = [
            "task_id = ?".to_string(),
            "id > ?".to_string(),
            format!("kind IN ({placeholders})"),
        ];
        let mut values = vec![sub.task_id.clone(), sub.last_event_id.to_string()];
        values.extend(kinds.iter().cloned());
        let sql = format!(
            "SELECT * FROM kanban_events WHERE {} ORDER BY id ASC LIMIT {}",
            clauses.join(" AND "),
            remaining
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(
                params_from_iter(values.iter().map(String::as_str)),
                KanbanEvent::from_row,
            )
            .map_err(db_err)?;
        let events = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        drop(stmt);
        if events.is_empty() {
            return Ok(Vec::new());
        }

        let new_cursor = events
            .last()
            .map(|event| event.id)
            .unwrap_or(sub.last_event_id);
        let updated = conn
            .execute(
                "UPDATE kanban_notify_subs
                SET last_event_id = ?1,
                    updated_at = ?2
              WHERE id = ?3 AND last_event_id = ?4",
                params![new_cursor, now_epoch(), sub.id, sub.last_event_id],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Ok(Vec::new());
        }
        let task = self.get_task_required(&sub.task_id)?;
        let task_terminal = matches!(task.status.as_str(), "done" | "archived");
        if task_terminal {
            conn.execute(
                "DELETE FROM kanban_notify_subs WHERE id = ?1",
                params![sub.id],
            )
            .map_err(db_err)?;
        }
        Ok(events
            .into_iter()
            .map(|event| KanbanNotification {
                subscription: sub.clone(),
                event,
                terminal: task_terminal,
            })
            .collect())
    }

    fn get_notify_sub(
        &self,
        task_id: &str,
        platform: &str,
        chat_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<KanbanNotifySub>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT * FROM kanban_notify_subs
             WHERE task_id = ?1
               AND platform = ?2
               AND chat_id = ?3
               AND COALESCE(thread_id, '') = ?4",
            params![task_id, platform, chat_id, thread_id.unwrap_or("")],
            KanbanNotifySub::from_row,
        )
        .optional()
        .map_err(db_err)
    }

    fn diagnostics(&self, task_id: Option<&str>) -> Result<Vec<KanbanDiagnostic>> {
        let tasks = if let Some(task_id) = task_id.and_then(non_empty_str) {
            vec![self.get_task_required(task_id)?]
        } else {
            self.list_tasks(None, None, MAX_LIMIT)?
        };
        let mut diagnostics = Vec::new();
        for task in tasks {
            diagnostics.extend(self.task_diagnostics(&task)?);
        }
        diagnostics.sort_by_key(|diag| severity_rank(&diag.severity));
        Ok(diagnostics)
    }

    fn task_diagnostics(&self, task: &KanbanTask) -> Result<Vec<KanbanDiagnostic>> {
        let events = self.events(&task.id, MAX_LIMIT)?;
        let mut diagnostics = Vec::new();
        if task.status == "blocked"
            && no_event_after(&events, "commented", task.updated_at)
            && no_event_after(&events, "unblocked", task.updated_at)
        {
            diagnostics.push(KanbanDiagnostic {
                kind: "stuck_blocked".to_string(),
                severity: "warning".to_string(),
                task_id: task.id.clone(),
                title: "Task is blocked without follow-up".to_string(),
                detail: task
                    .blocked_reason
                    .clone()
                    .unwrap_or_else(|| "No block reason recorded.".to_string()),
                actions: vec![
                    format!("/kanban comment {} <update>", task.id),
                    format!("/kanban unblock {}", task.id),
                ],
                data: json!({"blocked_at": task.updated_at}),
            });
        }
        let recent_blocked = events
            .iter()
            .filter(|event| event.kind == "blocked")
            .count();
        let recent_unblocked = events
            .iter()
            .filter(|event| event.kind == "unblocked")
            .count();
        if recent_blocked >= 2 && recent_unblocked >= 1 && task.status != "done" {
            diagnostics.push(KanbanDiagnostic {
                kind: "block_cycle".to_string(),
                severity: "error".to_string(),
                task_id: task.id.clone(),
                title: "Task repeatedly cycles through blocked".to_string(),
                detail: "Repeated block/unblock transitions suggest the recovery action is not addressing the underlying blocker.".to_string(),
                actions: vec![format!("/kanban events {}", task.id)],
                data: json!({
                    "blocked_events": recent_blocked,
                    "unblocked_events": recent_unblocked,
                }),
            });
        }
        if task.status == "running" && task.heartbeat_at.is_none() {
            diagnostics.push(KanbanDiagnostic {
                kind: "missing_heartbeat".to_string(),
                severity: "warning".to_string(),
                task_id: task.id.clone(),
                title: "Running task has no heartbeat".to_string(),
                detail: "Workers should emit heartbeats so dispatchers and operators can distinguish live work from stalled claims.".to_string(),
                actions: vec![format!("/kanban heartbeat {} <note>", task.id)],
                data: json!({"status": task.status}),
            });
        }
        if matches!(task.status.as_str(), "todo" | "ready" | "running")
            && task
                .profile
                .as_deref()
                .or(task.assignee.as_deref())
                .is_none()
        {
            diagnostics.push(KanbanDiagnostic {
                kind: "unassigned_routable_task".to_string(),
                severity: "warning".to_string(),
                task_id: task.id.clone(),
                title: "Task has no profile assignee".to_string(),
                detail: "Dispatcher-style Kanban work needs an assignee/profile before a worker can pick it up.".to_string(),
                actions: vec![format!("/kanban assign {} <profile>", task.id)],
                data: json!({"status": task.status}),
            });
        }
        if task.status == "running"
            && !self
                .worker_log_path(&task.id)
                .map(|path| path.exists())
                .unwrap_or(false)
        {
            diagnostics.push(KanbanDiagnostic {
                kind: "missing_worker_log".to_string(),
                severity: "info".to_string(),
                task_id: task.id.clone(),
                title: "Running task has no worker log yet".to_string(),
                detail: "Worker logs make retries and operator review easier; append durable progress with kanban_worker_log or /kanban worker-log.".to_string(),
                actions: vec![format!("/kanban worker-log {} <note>", task.id)],
                data: json!({"status": task.status}),
            });
        }
        Ok(diagnostics)
    }

    fn reaches(&self, from: &str, target: &str) -> Result<bool> {
        let conn = self.connect()?;
        let mut stack = vec![from.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let mut stmt = conn
                .prepare("SELECT child_id FROM kanban_links WHERE parent_id = ?1")
                .map_err(db_err)?;
            let children = stmt
                .query_map(params![id], |row| row.get::<_, String>(0))
                .map_err(db_err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?;
            for child in children {
                if child == target {
                    return Ok(true);
                }
                stack.push(child);
            }
        }
        Ok(false)
    }

    fn stats(&self) -> Result<JsonValue> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare("SELECT status, COUNT(*) AS n FROM kanban_tasks GROUP BY status")
            .map_err(db_err)?;
        let counts = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        let by_status = counts
            .into_iter()
            .map(|(status, count)| (status, json!(count)))
            .collect::<serde_json::Map<_, _>>();
        let by_assignee = self.assignee_counts(&conn)?;
        Ok(json!({
            "db_path": self.path.display().to_string(),
            "by_status": by_status,
            "by_assignee": by_assignee,
        }))
    }

    fn assignee_counts(&self, conn: &Connection) -> Result<JsonValue> {
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(profile, assignee) AS profile, status, COUNT(*) AS n
                 FROM kanban_tasks
                 WHERE status != 'archived' AND COALESCE(profile, assignee) IS NOT NULL
                 GROUP BY COALESCE(profile, assignee), status",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(db_err)?;
        let mut by_assignee = serde_json::Map::new();
        for row in rows {
            let (profile, status, count) = row.map_err(db_err)?;
            let entry = by_assignee.entry(profile).or_insert_with(|| json!({}));
            if let JsonValue::Object(counts) = entry {
                counts.insert(status, json!(count));
            }
        }
        Ok(JsonValue::Object(by_assignee))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KanbanBoardMetadata {
    slug: String,
    name: String,
    description: Option<String>,
    created_at: i64,
}

impl KanbanBoardMetadata {
    fn new(slug: String, name: Option<&str>, description: Option<&str>) -> Self {
        Self {
            name: name
                .and_then(non_empty_str)
                .map(str::to_string)
                .unwrap_or_else(|| default_board_name(&slug)),
            slug,
            description: description.and_then(non_empty_str).map(str::to_string),
            created_at: now_epoch(),
        }
    }
}

impl KanbanTask {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            title: row.get("title")?,
            body: row.get("body")?,
            assignee: row.get("assignee")?,
            profile: row.get("profile")?,
            status: row.get("status")?,
            priority: row.get("priority")?,
            blocked_reason: row.get("blocked_reason")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            completed_at: row.get("completed_at")?,
            heartbeat_at: row.get("heartbeat_at")?,
        })
    }
}

impl KanbanComment {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            author: row.get("author")?,
            body: row.get("body")?,
            created_at: row.get("created_at")?,
        })
    }
}

impl KanbanLink {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            parent_id: row.get("parent_id")?,
            child_id: row.get("child_id")?,
            relation: row.get("relation")?,
            created_at: row.get("created_at")?,
        })
    }
}

impl KanbanEvent {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let payload: Option<String> = row.get("payload")?;
        Ok(Self {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            kind: row.get("kind")?,
            actor: row.get("actor")?,
            note: row.get("note")?,
            payload: payload.and_then(|raw| serde_json::from_str(&raw).ok()),
            created_at: row.get("created_at")?,
        })
    }
}

impl KanbanNotifySub {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            platform: row.get("platform")?,
            chat_id: row.get("chat_id")?,
            thread_id: row.get("thread_id")?,
            notifier_profile: row.get("notifier_profile")?,
            last_event_id: row.get("last_event_id")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

#[derive(Clone, Copy)]
enum KanbanToolKind {
    Show,
    List,
    Create,
    Complete,
    Block,
    Unblock,
    Comment,
    Heartbeat,
    Link,
    Events,
    Diagnostics,
    Assign,
    WorkerLog,
    NotifySubscribe,
    NotifyList,
    NotifyUnsubscribe,
    NotifyClaim,
}

pub struct KanbanTool {
    kind: KanbanToolKind,
}

impl KanbanTool {
    fn new(kind: KanbanToolKind) -> Self {
        Self { kind }
    }
}

pub fn kanban_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(KanbanTool::new(KanbanToolKind::Show)),
        Arc::new(KanbanTool::new(KanbanToolKind::List)),
        Arc::new(KanbanTool::new(KanbanToolKind::Create)),
        Arc::new(KanbanTool::new(KanbanToolKind::Complete)),
        Arc::new(KanbanTool::new(KanbanToolKind::Block)),
        Arc::new(KanbanTool::new(KanbanToolKind::Unblock)),
        Arc::new(KanbanTool::new(KanbanToolKind::Comment)),
        Arc::new(KanbanTool::new(KanbanToolKind::Heartbeat)),
        Arc::new(KanbanTool::new(KanbanToolKind::Link)),
        Arc::new(KanbanTool::new(KanbanToolKind::Events)),
        Arc::new(KanbanTool::new(KanbanToolKind::Diagnostics)),
        Arc::new(KanbanTool::new(KanbanToolKind::Assign)),
        Arc::new(KanbanTool::new(KanbanToolKind::WorkerLog)),
        Arc::new(KanbanTool::new(KanbanToolKind::NotifySubscribe)),
        Arc::new(KanbanTool::new(KanbanToolKind::NotifyList)),
        Arc::new(KanbanTool::new(KanbanToolKind::NotifyUnsubscribe)),
        Arc::new(KanbanTool::new(KanbanToolKind::NotifyClaim)),
    ]
}

#[async_trait]
impl Tool for KanbanTool {
    fn name(&self) -> &str {
        match self.kind {
            KanbanToolKind::Show => "kanban_show",
            KanbanToolKind::List => "kanban_list",
            KanbanToolKind::Create => "kanban_create",
            KanbanToolKind::Complete => "kanban_complete",
            KanbanToolKind::Block => "kanban_block",
            KanbanToolKind::Unblock => "kanban_unblock",
            KanbanToolKind::Comment => "kanban_comment",
            KanbanToolKind::Heartbeat => "kanban_heartbeat",
            KanbanToolKind::Link => "kanban_link",
            KanbanToolKind::Events => "kanban_events",
            KanbanToolKind::Diagnostics => "kanban_diagnostics",
            KanbanToolKind::Assign => "kanban_assign",
            KanbanToolKind::WorkerLog => "kanban_worker_log",
            KanbanToolKind::NotifySubscribe => "kanban_notify_subscribe",
            KanbanToolKind::NotifyList => "kanban_notify_list",
            KanbanToolKind::NotifyUnsubscribe => "kanban_notify_unsubscribe",
            KanbanToolKind::NotifyClaim => "kanban_notify_claim",
        }
    }

    fn toolset(&self) -> &str {
        "kanban"
    }

    fn description(&self) -> &str {
        match self.kind {
            KanbanToolKind::Show => "Show a Kanban task with comments and dependency links.",
            KanbanToolKind::List => "List durable Kanban tasks with status and assignee filters.",
            KanbanToolKind::Create => "Create a durable SQLite-backed Kanban task.",
            KanbanToolKind::Complete => {
                "Mark a Kanban task done and optionally append a handoff summary."
            }
            KanbanToolKind::Block => "Mark a Kanban task blocked with a human-readable reason.",
            KanbanToolKind::Unblock => "Move a blocked Kanban task back to an active status.",
            KanbanToolKind::Comment => "Append a durable comment to a Kanban task.",
            KanbanToolKind::Heartbeat => "Record worker liveness for a long-running Kanban task.",
            KanbanToolKind::Link => "Create a parent-child dependency link between Kanban tasks.",
            KanbanToolKind::Events => "List the durable event trail for a Kanban task.",
            KanbanToolKind::Diagnostics => {
                "List active Hermes-style Kanban diagnostics for one task or the board."
            }
            KanbanToolKind::Assign => "Assign or unassign a Kanban task to a profile.",
            KanbanToolKind::WorkerLog => "Append or read a durable worker log for a Kanban task.",
            KanbanToolKind::NotifySubscribe => {
                "Subscribe a gateway/chat target to Kanban task notification events."
            }
            KanbanToolKind::NotifyList => {
                "List Kanban notification subscriptions for one task or the whole board."
            }
            KanbanToolKind::NotifyUnsubscribe => "Remove a Kanban task notification subscription.",
            KanbanToolKind::NotifyClaim => {
                "Claim unread Kanban notification events and advance subscription cursors."
            }
        }
    }

    fn emoji(&self) -> &str {
        match self.kind {
            KanbanToolKind::Complete => "\u{2714}",
            KanbanToolKind::Block => "\u{23f8}",
            KanbanToolKind::Unblock => "\u{25b6}",
            KanbanToolKind::Comment => "\u{1f4ac}",
            KanbanToolKind::Heartbeat => "\u{1f493}",
            KanbanToolKind::Link => "\u{1f517}",
            KanbanToolKind::Events => "\u{1f4dc}",
            KanbanToolKind::Diagnostics => "\u{26a0}",
            KanbanToolKind::Assign => "\u{1f464}",
            KanbanToolKind::WorkerLog => "\u{1f4dd}",
            KanbanToolKind::NotifySubscribe
            | KanbanToolKind::NotifyList
            | KanbanToolKind::NotifyUnsubscribe
            | KanbanToolKind::NotifyClaim => "\u{1f514}",
            _ => "\u{1f4cb}",
        }
    }

    fn schema(&self) -> JsonValue {
        match self.kind {
            KanbanToolKind::Show => json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "board": board_schema_prop()
                },
                "required": ["task_id"]
            }),
            KanbanToolKind::List => json!({
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": VALID_STATUSES},
                    "assignee": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT},
                    "board": board_schema_prop()
                }
            }),
            KanbanToolKind::Create => json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "body": {"type": "string"},
                    "assignee": {"type": "string"},
                    "status": {"type": "string", "enum": VALID_STATUSES},
                    "priority": {"type": "integer"},
                    "board": board_schema_prop()
                },
                "required": ["title"]
            }),
            KanbanToolKind::Complete => task_id_note_schema("summary"),
            KanbanToolKind::Block => json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "reason": {"type": "string"},
                    "board": board_schema_prop()
                },
                "required": ["task_id", "reason"]
            }),
            KanbanToolKind::Unblock => json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "status": {"type": "string", "enum": ["triage", "todo", "ready", "running", "review"]},
                    "board": board_schema_prop()
                },
                "required": ["task_id"]
            }),
            KanbanToolKind::Comment => task_id_note_schema("body"),
            KanbanToolKind::Heartbeat => task_id_note_schema("note"),
            KanbanToolKind::Link => json!({
                "type": "object",
                "properties": {
                    "parent_id": {"type": "string"},
                    "child_id": {"type": "string"},
                    "relation": {"type": "string"},
                    "board": board_schema_prop()
                },
                "required": ["parent_id", "child_id"]
            }),
            KanbanToolKind::Events => json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT},
                    "board": board_schema_prop()
                },
                "required": ["task_id"]
            }),
            KanbanToolKind::Diagnostics => json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "board": board_schema_prop()
                }
            }),
            KanbanToolKind::Assign => json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "profile": {"type": "string", "description": "Profile name, or none/-/null to unassign."},
                    "board": board_schema_prop()
                },
                "required": ["task_id"]
            }),
            KanbanToolKind::WorkerLog => json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "body": {"type": "string", "description": "When present, append this durable worker log entry. Omit to read the latest log tail."},
                    "profile": {"type": "string"},
                    "tail_bytes": {"type": "integer", "minimum": 1, "maximum": MAX_LOG_TAIL_BYTES},
                    "board": board_schema_prop()
                },
                "required": ["task_id"]
            }),
            KanbanToolKind::NotifySubscribe => notify_target_schema(true),
            KanbanToolKind::NotifyList => json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "board": board_schema_prop()
                }
            }),
            KanbanToolKind::NotifyUnsubscribe => notify_target_schema(false),
            KanbanToolKind::NotifyClaim => json!({
                "type": "object",
                "properties": {
                    "notifier_profile": {"type": "string"},
                    "kinds": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Event kinds to claim. Defaults to completed, blocked, gave_up, crashed, timed_out."
                    },
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT},
                    "board": board_schema_prop()
                }
            }),
        }
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(64 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let store = KanbanStore::for_board(None)?;
        execute_kanban_tool(self.kind, args, &store)
    }
}

pub fn kanban_response(raw: Option<&str>) -> String {
    match KanbanStore::for_board(None) {
        Ok(store) => kanban_response_with_store(raw, &store),
        Err(err) => format!("Warning: {err}"),
    }
}

fn kanban_response_with_store(raw: Option<&str>, store: &KanbanStore) -> String {
    let rest = raw.unwrap_or_default().trim();
    if rest.is_empty() || matches!(rest, "help" | "-h" | "--help" | "?") {
        return kanban_help();
    }

    let (board, rest) = match extract_leading_board(rest) {
        Ok(parsed) => parsed,
        Err(err) => return format!("Warning: {err}"),
    };
    let board_store = match board
        .as_deref()
        .map(|slug| KanbanStore::for_board(Some(slug)))
        .transpose()
    {
        Ok(store) => store,
        Err(err) => return format!("Warning: {err}"),
    };
    let store = board_store.as_ref().unwrap_or(store);

    let mut parts = rest.split_whitespace();
    let command = parts.next().unwrap_or_default();
    let response = match command {
        "boards" => kanban_boards_response(parts.collect::<Vec<_>>()),
        "list" | "ls" => {
            let status = parts.next();
            json_result(store.list_tasks(status, None, DEFAULT_LIMIT))
        }
        "show" => match parts.next() {
            Some(task_id) => show_task_json(store, task_id),
            None => Err(HakimiError::Tool("usage: /kanban show <task_id>".into())),
        },
        "create" => {
            let title = parts.collect::<Vec<_>>().join(" ");
            json_result(store.create_task(CreateTask {
                title,
                body: None,
                assignee: None,
                status: "todo".to_string(),
                priority: 0,
            }))
        }
        "complete" => match parts.next() {
            Some(task_id) => {
                let summary = parts.collect::<Vec<_>>().join(" ");
                json_result(store.complete_task(task_id, non_empty_str(&summary)))
            }
            None => Err(HakimiError::Tool(
                "usage: /kanban complete <task_id> [summary]".into(),
            )),
        },
        "block" => match parts.next() {
            Some(task_id) => {
                let reason = parts.collect::<Vec<_>>().join(" ");
                json_result(store.block_task(task_id, &reason))
            }
            None => Err(HakimiError::Tool(
                "usage: /kanban block <task_id> <reason>".into(),
            )),
        },
        "unblock" => match parts.next() {
            Some(task_id) => json_result(store.unblock_task(task_id, None)),
            None => Err(HakimiError::Tool("usage: /kanban unblock <task_id>".into())),
        },
        "comment" => match parts.next() {
            Some(task_id) => {
                let body = parts.collect::<Vec<_>>().join(" ");
                json_result(store.add_comment(task_id, &body, Some("gateway")))
            }
            None => Err(HakimiError::Tool(
                "usage: /kanban comment <task_id> <body>".into(),
            )),
        },
        "heartbeat" => match parts.next() {
            Some(task_id) => {
                let note = parts.collect::<Vec<_>>().join(" ");
                json_result(store.heartbeat_task(task_id, non_empty_str(&note)))
            }
            None => Err(HakimiError::Tool(
                "usage: /kanban heartbeat <task_id> [note]".into(),
            )),
        },
        "link" => match (parts.next(), parts.next()) {
            (Some(parent_id), Some(child_id)) => {
                let relation = parts.next();
                json_result(store.link_tasks(parent_id, child_id, relation))
            }
            _ => Err(HakimiError::Tool(
                "usage: /kanban link <parent_id> <child_id> [relation]".into(),
            )),
        },
        "events" => match parts.next() {
            Some(task_id) => json_result(store.events(task_id, DEFAULT_LIMIT)),
            None => Err(HakimiError::Tool("usage: /kanban events <task_id>".into())),
        },
        "notify-subscribe" | "subscribe" => match (parts.next(), parts.next(), parts.next()) {
            (Some(task_id), Some(platform), Some(chat_id)) => {
                let thread_id = parts.next().map(str::to_string);
                let notifier_profile = parts.next().map(str::to_string);
                json_result(store.add_notify_sub(NotifyTarget {
                    task_id: task_id.to_string(),
                    platform: platform.to_string(),
                    chat_id: chat_id.to_string(),
                    thread_id,
                    notifier_profile,
                }))
            }
            _ => Err(HakimiError::Tool(
                "usage: /kanban notify-subscribe <task_id> <platform> <chat_id> [thread_id] [notifier_profile]".into(),
            )),
        },
        "notify-list" | "subscriptions" => {
            let task_id = parts.next();
            json_result(store.list_notify_subs(task_id))
        }
        "notify-unsubscribe" | "unsubscribe" => match (parts.next(), parts.next(), parts.next()) {
            (Some(task_id), Some(platform), Some(chat_id)) => {
                let thread_id = parts.next().map(str::to_string);
                json_result(store.remove_notify_sub(NotifyTarget {
                    task_id: task_id.to_string(),
                    platform: platform.to_string(),
                    chat_id: chat_id.to_string(),
                    thread_id,
                    notifier_profile: None,
                }))
            }
            _ => Err(HakimiError::Tool(
                "usage: /kanban notify-unsubscribe <task_id> <platform> <chat_id> [thread_id]".into(),
            )),
        },
        "notify-claim" => {
            let notifier_profile = parts.next();
            json_result(store.claim_notify_events(notifier_profile, &[], DEFAULT_LIMIT))
        }
        "assign" => match parts.next() {
            Some(task_id) => {
                let profile = parts.next();
                json_result(store.assign_task(task_id, profile, Some("gateway")))
            }
            None => Err(HakimiError::Tool(
                "usage: /kanban assign <task_id> [profile|none]".into(),
            )),
        },
        "worker-log" | "log" => match parts.next() {
            Some(task_id) => {
                let body = parts.collect::<Vec<_>>().join(" ");
                if let Some(body) = non_empty_str(&body) {
                    json_result(store.append_worker_log(task_id, body, Some("gateway")))
                } else {
                    match store.read_worker_log(task_id, DEFAULT_LOG_TAIL_BYTES) {
                        Ok(Some(log)) => Ok(json!(log).to_string()),
                        Ok(None) => Ok(json!({
                            "task_id": task_id,
                            "body": "",
                            "missing": true,
                        })
                        .to_string()),
                        Err(err) => Err(err),
                    }
                }
            }
            None => Err(HakimiError::Tool(
                "usage: /kanban worker-log <task_id> [body]".into(),
            )),
        },
        "diagnostics" | "diag" => {
            let task_id = parts.next();
            json_result(store.diagnostics(task_id))
        }
        "stats" => store.stats().map(|v| v.to_string()),
        _ => Err(HakimiError::Tool(format!(
            "unknown /kanban command: {command}; run /kanban help"
        ))),
    };

    match response {
        Ok(body) => body,
        Err(err) => format!("Warning: {err}"),
    }
}

fn execute_kanban_tool(
    kind: KanbanToolKind,
    args: &JsonValue,
    store: &KanbanStore,
) -> Result<String> {
    let board = args.get("board").and_then(JsonValue::as_str);
    let board_store = board
        .and_then(non_empty_str)
        .map(|slug| KanbanStore::for_board(Some(slug)))
        .transpose()?;
    let store = board_store.as_ref().unwrap_or(store);

    match kind {
        KanbanToolKind::Show => {
            let task_id = require_str(args, "task_id")?;
            show_task_json(store, task_id)
        }
        KanbanToolKind::List => {
            let status = args.get("status").and_then(JsonValue::as_str);
            let assignee = args.get("assignee").and_then(JsonValue::as_str);
            let limit = args
                .get("limit")
                .and_then(JsonValue::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(DEFAULT_LIMIT);
            json_result(store.list_tasks(status, assignee, limit))
        }
        KanbanToolKind::Create => {
            let title = require_str(args, "title")?.to_string();
            let status = args
                .get("status")
                .and_then(JsonValue::as_str)
                .unwrap_or("todo")
                .to_string();
            let priority = args
                .get("priority")
                .and_then(JsonValue::as_i64)
                .unwrap_or(0);
            json_result(
                store.create_task(CreateTask {
                    title,
                    body: args
                        .get("body")
                        .and_then(JsonValue::as_str)
                        .map(str::to_string),
                    assignee: args
                        .get("assignee")
                        .and_then(JsonValue::as_str)
                        .map(str::to_string),
                    status,
                    priority,
                }),
            )
        }
        KanbanToolKind::Complete => {
            let task_id = require_str(args, "task_id")?;
            let summary = args.get("summary").and_then(JsonValue::as_str);
            json_result(store.complete_task(task_id, summary))
        }
        KanbanToolKind::Block => {
            let task_id = require_str(args, "task_id")?;
            let reason = require_str(args, "reason")?;
            json_result(store.block_task(task_id, reason))
        }
        KanbanToolKind::Unblock => {
            let task_id = require_str(args, "task_id")?;
            let status = args.get("status").and_then(JsonValue::as_str);
            json_result(store.unblock_task(task_id, status))
        }
        KanbanToolKind::Comment => {
            let task_id = require_str(args, "task_id")?;
            let body = require_str(args, "body")?;
            json_result(store.add_comment(task_id, body, Some("agent")))
        }
        KanbanToolKind::Heartbeat => {
            let task_id = require_str(args, "task_id")?;
            let note = args.get("note").and_then(JsonValue::as_str);
            json_result(store.heartbeat_task(task_id, note))
        }
        KanbanToolKind::Link => {
            let parent_id = require_str(args, "parent_id")?;
            let child_id = require_str(args, "child_id")?;
            let relation = args.get("relation").and_then(JsonValue::as_str);
            json_result(store.link_tasks(parent_id, child_id, relation))
        }
        KanbanToolKind::Events => {
            let task_id = require_str(args, "task_id")?;
            let limit = args
                .get("limit")
                .and_then(JsonValue::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(DEFAULT_LIMIT);
            json_result(store.events(task_id, limit))
        }
        KanbanToolKind::Diagnostics => {
            let task_id = args.get("task_id").and_then(JsonValue::as_str);
            json_result(store.diagnostics(task_id))
        }
        KanbanToolKind::Assign => {
            let task_id = require_str(args, "task_id")?;
            let profile = args.get("profile").and_then(JsonValue::as_str);
            json_result(store.assign_task(task_id, profile, Some("agent")))
        }
        KanbanToolKind::WorkerLog => {
            let task_id = require_str(args, "task_id")?;
            if let Some(body) = args.get("body").and_then(JsonValue::as_str) {
                let profile = args.get("profile").and_then(JsonValue::as_str);
                json_result(store.append_worker_log(task_id, body, profile))
            } else {
                let tail_bytes = args
                    .get("tail_bytes")
                    .and_then(JsonValue::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(DEFAULT_LOG_TAIL_BYTES);
                match store.read_worker_log(task_id, tail_bytes)? {
                    Some(log) => Ok(json!(log).to_string()),
                    None => Ok(json!({
                        "task_id": task_id,
                        "body": "",
                        "missing": true,
                    })
                    .to_string()),
                }
            }
        }
        KanbanToolKind::NotifySubscribe => {
            json_result(store.add_notify_sub(notify_target_from_args(args, true)?))
        }
        KanbanToolKind::NotifyList => {
            let task_id = args.get("task_id").and_then(JsonValue::as_str);
            json_result(store.list_notify_subs(task_id))
        }
        KanbanToolKind::NotifyUnsubscribe => {
            json_result(store.remove_notify_sub(notify_target_from_args(args, false)?))
        }
        KanbanToolKind::NotifyClaim => {
            let profile = args.get("notifier_profile").and_then(JsonValue::as_str);
            let kinds = args
                .get("kinds")
                .and_then(JsonValue::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(JsonValue::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let limit = args
                .get("limit")
                .and_then(JsonValue::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(DEFAULT_LIMIT);
            json_result(store.claim_notify_events(profile, &kinds, limit))
        }
    }
}

fn show_task_json(store: &KanbanStore, task_id: &str) -> Result<String> {
    let task = store.get_task_required(task_id)?;
    Ok(json!({
        "task": task,
        "comments": store.comments(task_id)?,
        "parents": store.parents(task_id)?,
        "children": store.children(task_id)?,
        "events": store.events(task_id, DEFAULT_LIMIT)?,
        "diagnostics": store.diagnostics(Some(task_id))?,
    })
    .to_string())
}

fn json_result<T: Serialize>(result: Result<T>) -> Result<String> {
    result.map(|value| json!(value).to_string())
}

fn require_str<'a>(args: &'a JsonValue, name: &str) -> Result<&'a str> {
    args.get(name)
        .and_then(JsonValue::as_str)
        .and_then(non_empty_str)
        .ok_or_else(|| HakimiError::Tool(format!("missing required parameter: {name}")))
}

fn task_id_note_schema(note_name: &str) -> JsonValue {
    let mut properties = serde_json::Map::new();
    properties.insert("task_id".to_string(), json!({"type": "string"}));
    properties.insert(note_name.to_string(), json!({"type": "string"}));
    properties.insert("board".to_string(), board_schema_prop());
    json!({
        "type": "object",
        "properties": properties,
        "required": ["task_id"]
    })
}

fn notify_target_schema(include_profile: bool) -> JsonValue {
    let mut properties = serde_json::Map::new();
    properties.insert("task_id".to_string(), json!({"type": "string"}));
    properties.insert("platform".to_string(), json!({"type": "string"}));
    properties.insert("chat_id".to_string(), json!({"type": "string"}));
    properties.insert("thread_id".to_string(), json!({"type": "string"}));
    if include_profile {
        properties.insert("notifier_profile".to_string(), json!({"type": "string"}));
    }
    properties.insert("board".to_string(), board_schema_prop());
    json!({
        "type": "object",
        "properties": properties,
        "required": ["task_id", "platform", "chat_id"]
    })
}

fn notify_target_from_args(args: &JsonValue, include_profile: bool) -> Result<NotifyTarget> {
    Ok(NotifyTarget {
        task_id: require_str(args, "task_id")?.to_string(),
        platform: require_str(args, "platform")?.to_string(),
        chat_id: require_str(args, "chat_id")?.to_string(),
        thread_id: args
            .get("thread_id")
            .and_then(JsonValue::as_str)
            .map(str::to_string),
        notifier_profile: include_profile
            .then(|| {
                args.get("notifier_profile")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string)
            })
            .flatten(),
    })
}

fn board_schema_prop() -> JsonValue {
    json!({
        "type": "string",
        "description": "Optional Kanban board slug. Defaults to HAKIMI_KANBAN_BOARD/HERMES_KANBAN_BOARD, the current board file, then default."
    })
}

fn kanban_boards_response(args: Vec<&str>) -> Result<String> {
    let command = args.first().copied().unwrap_or("help");
    match command {
        "help" | "-h" | "--help" | "?" => Ok(kanban_boards_help()),
        "list" | "ls" => board_list_json(),
        "show" | "current" => {
            let slug = current_board_slug()?;
            board_summary_json(&slug)
        }
        "create" | "new" => {
            let slug = args.get(1).copied().ok_or_else(|| {
                HakimiError::Tool("usage: /kanban boards create <slug> [name]".into())
            })?;
            let name = args.get(2..).map(|rest| rest.join(" "));
            let meta = create_board(slug, name.as_deref(), None)?;
            Ok(json!(meta).to_string())
        }
        "switch" | "use" => {
            let slug = args
                .get(1)
                .copied()
                .ok_or_else(|| HakimiError::Tool("usage: /kanban boards switch <slug>".into()))?;
            switch_board(slug)?;
            board_summary_json(&normalize_board_slug(slug)?)
        }
        _ => Err(HakimiError::Tool(format!(
            "unknown /kanban boards command: {command}; run /kanban boards help"
        ))),
    }
}

fn kanban_boards_help() -> String {
    [
        "**/kanban boards** - manage isolated Kanban boards.",
        "",
        "Common subcommands:",
        "  `list`",
        "  `show`",
        "  `create <slug> [name]`",
        "  `switch <slug>`",
    ]
    .join("\n")
}

fn kanban_help() -> String {
    [
        "**/kanban** - manage the local SQLite task board.",
        "",
        "Common subcommands:",
        "  `--board <slug> <subcommand>`",
        "  `boards list|show|create|switch`",
        "  `list [status]`",
        "  `show <id>`",
        "  `create <title>`",
        "  `comment <id> <body>`",
        "  `complete <id> [summary]`",
        "  `block <id> <reason>`",
        "  `unblock <id>`",
        "  `assign <id> [profile|none]`",
        "  `heartbeat <id> [note]`",
        "  `worker-log <id> [body]`",
        "  `link <parent_id> <child_id> [relation]`",
        "  `events <id>`",
        "  `notify-subscribe <id> <platform> <chat_id> [thread_id] [profile]`",
        "  `notify-list [id]`",
        "  `notify-unsubscribe <id> <platform> <chat_id> [thread_id]`",
        "  `notify-claim [profile]`",
        "  `diagnostics [id]`",
        "  `stats`",
    ]
    .join("\n")
}

fn validate_status(status: &str) -> Result<()> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(HakimiError::Tool(format!(
            "invalid kanban status: {status}; expected one of {}",
            VALID_STATUSES.join(", ")
        )))
    }
}

fn migrate_schema(conn: &Connection) -> Result<()> {
    let columns = kanban_task_columns(conn)?;
    if !columns.iter().any(|column| column == "profile") {
        conn.execute("ALTER TABLE kanban_tasks ADD COLUMN profile TEXT", [])
            .map_err(db_err)?;
        conn.execute(
            "UPDATE kanban_tasks SET profile = assignee WHERE profile IS NULL AND assignee IS NOT NULL",
            [],
        )
        .map_err(db_err)?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_kanban_tasks_profile ON kanban_tasks(profile)",
        [],
    )
    .map_err(db_err)?;
    Ok(())
}

fn kanban_task_columns(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(kanban_tasks)")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>("name"))
        .map_err(db_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_err)
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn normalize_profile_arg(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value.and_then(non_empty_str) else {
        return Ok(None);
    };
    let normalized = value.trim();
    if matches!(
        normalized.to_ascii_lowercase().as_str(),
        "none" | "null" | "-"
    ) {
        return Ok(None);
    }
    if normalized.len() > 64
        || normalized.starts_with(['-', '_', '.'])
        || normalized.contains("..")
        || !normalized
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(HakimiError::Tool(format!(
            "invalid kanban profile: {normalized}; use 1-64 letters, digits, dash, underscore, or dot"
        )));
    }
    Ok(Some(normalized.to_string()))
}

fn require_notify_text<'a>(value: &'a str, name: &str) -> Result<&'a str> {
    let trimmed = non_empty_str(value)
        .ok_or_else(|| HakimiError::Tool(format!("kanban notification {name} is required")))?;
    if trimmed.len() > 256 || trimmed.chars().any(char::is_control) {
        return Err(HakimiError::Tool(format!(
            "invalid kanban notification {name}: use 1-256 non-control characters"
        )));
    }
    Ok(trimmed)
}

fn normalize_notify_text(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value.and_then(non_empty_str) else {
        return Ok(None);
    };
    if value.len() > 256 || value.chars().any(char::is_control) {
        return Err(HakimiError::Tool(
            "invalid kanban notification thread_id: use 1-256 non-control characters".into(),
        ));
    }
    Ok(Some(value.to_string()))
}

fn normalize_event_kinds(kinds: &[String]) -> Result<Vec<String>> {
    if kinds.is_empty() {
        return Ok(DEFAULT_NOTIFY_EVENT_KINDS
            .iter()
            .map(|kind| (*kind).to_string())
            .collect());
    }
    let mut normalized = Vec::new();
    for kind in kinds {
        let kind = non_empty_str(kind).ok_or_else(|| {
            HakimiError::Tool("kanban notification event kind is required".into())
        })?;
        if kind.len() > 64
            || !kind
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(HakimiError::Tool(format!(
                "invalid kanban notification event kind: {kind}; use lowercase letters, digits, or underscore"
            )));
        }
        if !normalized.iter().any(|existing| existing == kind) {
            normalized.push(kind.to_string());
        }
    }
    Ok(normalized)
}

fn non_empty_str(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value).trim()
}

fn validate_log_task_id(task_id: &str) -> Result<()> {
    if task_id.is_empty()
        || task_id.len() > 128
        || task_id.starts_with(['-', '_', '.'])
        || task_id.contains("..")
        || !task_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(HakimiError::Tool(format!(
            "invalid kanban task id for worker log path: {task_id}"
        )));
    }
    Ok(())
}

fn severity_rank(severity: &str) -> usize {
    match severity {
        "critical" => 0,
        "error" => 1,
        "warning" => 2,
        _ => 3,
    }
}

fn no_event_after(events: &[KanbanEvent], kind: &str, timestamp: i64) -> bool {
    !events
        .iter()
        .any(|event| event.kind == kind && event.created_at > timestamp)
}

fn extract_leading_board(rest: &str) -> Result<(Option<String>, String)> {
    let mut parts = rest.split_whitespace();
    let Some(first) = parts.next() else {
        return Ok((None, String::new()));
    };
    if let Some(slug) = first.strip_prefix("--board=") {
        let board = normalize_board_slug(slug)?;
        return Ok((Some(board), parts.collect::<Vec<_>>().join(" ")));
    }
    if first == "--board" {
        let slug = parts
            .next()
            .ok_or_else(|| HakimiError::Tool("--board requires a board slug".into()))?;
        let board = normalize_board_slug(slug)?;
        return Ok((Some(board), parts.collect::<Vec<_>>().join(" ")));
    }
    Ok((None, rest.to_string()))
}

fn default_kanban_db_path() -> PathBuf {
    std::env::var("HAKIMI_KANBAN_DB")
        .or_else(|_| std::env::var("HERMES_KANBAN_DB"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| kanban_home().join("kanban.db"))
}

fn resolve_kanban_db_path(board: Option<&str>) -> Result<PathBuf> {
    if let Some(board) = board.and_then(non_empty_str) {
        let slug = normalize_board_slug(board)?;
        if slug == DEFAULT_BOARD {
            return Ok(default_kanban_db_path());
        }
        return Ok(board_dir(&slug).join("kanban.db"));
    }

    if std::env::var("HAKIMI_KANBAN_DB").is_ok() || std::env::var("HERMES_KANBAN_DB").is_ok() {
        return Ok(default_kanban_db_path());
    }
    if let Some(slug) = env_board_slug() {
        return resolve_kanban_db_path(Some(&slug));
    }
    let current = current_board_slug()?;
    resolve_kanban_db_path(Some(&current))
}

fn kanban_home() -> PathBuf {
    std::env::var("HAKIMI_KANBAN_HOME")
        .or_else(|_| std::env::var("HERMES_KANBAN_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".hakimi")
        })
}

fn boards_root() -> PathBuf {
    kanban_home().join("kanban").join("boards")
}

fn board_dir(slug: &str) -> PathBuf {
    boards_root().join(slug)
}

fn current_board_path() -> PathBuf {
    kanban_home().join("kanban").join("current")
}

fn current_board_slug() -> Result<String> {
    if let Some(slug) = env_board_slug() {
        return normalize_board_slug(&slug);
    }
    if let Some(slug) = current_board_file_slug() {
        return Ok(slug);
    }
    Ok(DEFAULT_BOARD.to_string())
}

fn env_board_slug() -> Option<String> {
    let raw = std::env::var("HAKIMI_KANBAN_BOARD")
        .ok()
        .or_else(|| std::env::var("HERMES_KANBAN_BOARD").ok())?;
    non_empty_str(&raw).map(str::to_string)
}

fn current_board_file_slug() -> Option<String> {
    let raw = std::fs::read_to_string(current_board_path()).ok()?;
    let slug = normalize_board_slug(non_empty_str(&raw)?).ok()?;
    board_exists(&slug).then_some(slug)
}

fn board_exists(slug: &str) -> bool {
    slug == DEFAULT_BOARD
        || board_dir(slug).join("board.json").exists()
        || board_dir(slug).join("kanban.db").exists()
}

fn normalize_board_slug(slug: &str) -> Result<String> {
    let slug = slug.trim().to_ascii_lowercase();
    if slug.is_empty() {
        return Err(HakimiError::Tool("kanban board slug is required".into()));
    }
    if slug.len() > 64
        || !slug
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        || matches!(slug.as_bytes().first(), Some(b'-' | b'_'))
    {
        return Err(HakimiError::Tool(format!(
            "invalid kanban board slug: {slug}; use 1-64 lowercase letters, digits, hyphen, or underscore"
        )));
    }
    Ok(slug)
}

fn default_board_name(slug: &str) -> String {
    slug.replace(['-', '_'], " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_board_metadata(slug: &str) -> KanbanBoardMetadata {
    let path = board_dir(slug).join("board.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| KanbanBoardMetadata::new(slug.to_string(), None, None))
}

fn write_board_metadata(meta: &KanbanBoardMetadata) -> Result<()> {
    let dir = board_dir(&meta.slug);
    std::fs::create_dir_all(&dir).map_err(HakimiError::Io)?;
    let body = serde_json::to_string_pretty(meta)
        .map_err(|err| HakimiError::Tool(format!("kanban board metadata error: {err}")))?;
    std::fs::write(dir.join("board.json"), body + "\n").map_err(HakimiError::Io)
}

fn create_board(
    slug: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<KanbanBoardMetadata> {
    let slug = normalize_board_slug(slug)?;
    if board_exists(&slug) {
        return Ok(read_board_metadata(&slug));
    }
    let meta = KanbanBoardMetadata::new(slug, name, description);
    write_board_metadata(&meta)?;
    let _ = KanbanStore::for_board(Some(&meta.slug))?.stats()?;
    Ok(meta)
}

fn switch_board(slug: &str) -> Result<()> {
    let slug = normalize_board_slug(slug)?;
    if !board_exists(&slug) {
        return Err(HakimiError::Tool(format!("kanban board not found: {slug}")));
    }
    let path = current_board_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(HakimiError::Io)?;
    }
    std::fs::write(path, format!("{slug}\n")).map_err(HakimiError::Io)
}

fn board_list_json() -> Result<String> {
    let current = current_board_slug()?;
    let mut slugs = vec![DEFAULT_BOARD.to_string()];
    let root = boards_root();
    if root.exists() {
        for entry in std::fs::read_dir(root).map_err(HakimiError::Io)? {
            let entry = entry.map_err(HakimiError::Io)?;
            if !entry.file_type().map_err(HakimiError::Io)?.is_dir() {
                continue;
            }
            let slug = entry.file_name().to_string_lossy().to_string();
            if normalize_board_slug(&slug).is_ok() && slug != DEFAULT_BOARD {
                slugs.push(slug);
            }
        }
    }
    slugs.sort();
    slugs.dedup();
    let boards = slugs
        .into_iter()
        .map(|slug| board_summary(&slug, &current))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "current": current,
        "boards": boards,
    })
    .to_string())
}

fn board_summary_json(slug: &str) -> Result<String> {
    let current = current_board_slug()?;
    Ok(board_summary(slug, &current)?.to_string())
}

fn board_summary(slug: &str, current: &str) -> Result<JsonValue> {
    let slug = normalize_board_slug(slug)?;
    let meta = read_board_metadata(&slug);
    let store = KanbanStore::for_board(Some(&slug))?;
    let stats = store.stats()?;
    Ok(json!({
        "slug": slug,
        "name": meta.name,
        "description": meta.description,
        "current": meta.slug == current,
        "db_path": stats["db_path"],
        "by_status": stats["by_status"],
    }))
}

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

fn db_err(err: rusqlite::Error) -> HakimiError {
    HakimiError::Tool(format!("kanban sqlite error: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn store() -> (tempfile::TempDir, KanbanStore) {
        let dir = tempdir().unwrap();
        let db = dir.path().join("kanban.db");
        (dir, KanbanStore::new(db))
    }

    fn create(store: &KanbanStore, title: &str) -> KanbanTask {
        store
            .create_task(CreateTask {
                title: title.to_string(),
                body: Some("body".to_string()),
                assignee: Some("worker".to_string()),
                status: "todo".to_string(),
                priority: 1,
            })
            .unwrap()
    }

    #[test]
    fn creates_lists_and_shows_tasks() {
        let (_dir, store) = store();
        let task = create(&store, "Write spec");

        let listed = store.list_tasks(Some("todo"), Some("worker"), 10).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, task.id);

        let shown = show_task_json(&store, &task.id).unwrap();
        let value: JsonValue = serde_json::from_str(&shown).unwrap();
        assert_eq!(value["task"]["title"], "Write spec");
        assert!(value["comments"].as_array().unwrap().is_empty());
    }

    #[test]
    fn rejects_empty_titles_and_invalid_statuses() {
        let (_dir, store) = store();
        assert!(
            store
                .create_task(CreateTask {
                    title: " ".to_string(),
                    body: None,
                    assignee: None,
                    status: "todo".to_string(),
                    priority: 0,
                })
                .is_err()
        );
        assert!(validate_status("unknown").is_err());
    }

    #[test]
    fn comments_are_durable() {
        let (_dir, store) = store();
        let task = create(&store, "Investigate");
        let comment = store
            .add_comment(&task.id, "Finding recorded", Some("agent"))
            .unwrap();

        assert_eq!(comment.body, "Finding recorded");
        assert_eq!(store.comments(&task.id).unwrap().len(), 1);
    }

    #[test]
    fn complete_adds_summary_comment() {
        let (_dir, store) = store();
        let task = create(&store, "Ship");
        let completed = store.complete_task(&task.id, Some("Done")).unwrap();

        assert_eq!(completed.status, "done");
        assert!(completed.completed_at.is_some());
        assert_eq!(store.comments(&task.id).unwrap()[0].body, "Done");
    }

    #[test]
    fn block_and_unblock_transitions() {
        let (_dir, store) = store();
        let task = create(&store, "Ask human");
        let blocked = store.block_task(&task.id, "Need decision").unwrap();
        assert_eq!(blocked.status, "blocked");
        assert_eq!(blocked.blocked_reason.as_deref(), Some("Need decision"));

        let ready = store.unblock_task(&task.id, None).unwrap();
        assert_eq!(ready.status, "ready");
        assert!(ready.blocked_reason.is_none());
    }

    #[test]
    fn heartbeat_updates_liveness_and_note() {
        let (_dir, store) = store();
        let task = create(&store, "Long task");
        let beat = store
            .heartbeat_task(&task.id, Some("Still running"))
            .unwrap();

        assert!(beat.heartbeat_at.is_some());
        assert_eq!(
            store.comments(&task.id).unwrap()[0].author.as_deref(),
            Some("heartbeat")
        );
    }

    #[test]
    fn links_tasks_and_rejects_cycles() {
        let (_dir, store) = store();
        let parent = create(&store, "Parent");
        let child = create(&store, "Child");

        store.link_tasks(&parent.id, &child.id, None).unwrap();
        assert_eq!(store.children(&parent.id).unwrap().len(), 1);
        assert!(store.link_tasks(&child.id, &parent.id, None).is_err());
        assert!(store.link_tasks(&parent.id, &parent.id, None).is_err());
    }

    #[test]
    fn list_limit_is_clamped() {
        let (_dir, store) = store();
        for index in 0..3 {
            create(&store, &format!("Task {index}"));
        }
        assert_eq!(store.list_tasks(None, None, 0).unwrap().len(), 1);
    }

    #[test]
    fn exposes_kanban_tools_with_notifications() {
        let names = kanban_tools()
            .iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 17);
        assert!(names.contains(&"kanban_create".to_string()));
        assert!(names.contains(&"kanban_heartbeat".to_string()));
        assert!(names.contains(&"kanban_link".to_string()));
        assert!(names.contains(&"kanban_events".to_string()));
        assert!(names.contains(&"kanban_diagnostics".to_string()));
        assert!(names.contains(&"kanban_assign".to_string()));
        assert!(names.contains(&"kanban_worker_log".to_string()));
        assert!(names.contains(&"kanban_notify_subscribe".to_string()));
        assert!(names.contains(&"kanban_notify_list".to_string()));
        assert!(names.contains(&"kanban_notify_unsubscribe".to_string()));
        assert!(names.contains(&"kanban_notify_claim".to_string()));
    }

    #[test]
    fn tool_schema_has_required_create_title() {
        let tool = KanbanTool::new(KanbanToolKind::Create);
        let required = tool.schema()["required"].as_array().unwrap().clone();
        assert!(required.contains(&JsonValue::String("title".to_string())));
        assert_eq!(tool.toolset(), "kanban");
    }

    #[test]
    fn tool_execute_create_and_list() {
        let (_dir, store) = store();
        let created = execute_kanban_tool(
            KanbanToolKind::Create,
            &json!({"title": "Via tool", "assignee": "worker"}),
            &store,
        )
        .unwrap();
        let value: JsonValue = serde_json::from_str(&created).unwrap();
        assert_eq!(value["title"], "Via tool");

        let listed =
            execute_kanban_tool(KanbanToolKind::List, &json!({"assignee": "worker"}), &store)
                .unwrap();
        assert!(listed.contains("Via tool"));
    }

    #[test]
    fn assign_updates_profile_and_stats() {
        let (_dir, store) = store();
        let task = create(&store, "Route work");

        let assigned = store
            .assign_task(&task.id, Some("reviewer-1"), Some("test"))
            .unwrap();
        assert_eq!(assigned.assignee.as_deref(), Some("reviewer-1"));
        assert_eq!(assigned.profile.as_deref(), Some("reviewer-1"));

        let stats = store.stats().unwrap();
        assert_eq!(stats["by_assignee"]["reviewer-1"]["todo"], 1);
        let events = store.events(&task.id, DEFAULT_LIMIT).unwrap();
        assert!(events.iter().any(|event| event.kind == "assigned"));

        let unassigned = execute_kanban_tool(
            KanbanToolKind::Assign,
            &json!({"task_id": task.id, "profile": "none"}),
            &store,
        )
        .unwrap();
        let unassigned: JsonValue = serde_json::from_str(&unassigned).unwrap();
        assert!(unassigned["profile"].is_null());
    }

    #[test]
    fn worker_log_appends_reads_and_clears_diagnostics() {
        let (_dir, store) = store();
        let task = store
            .create_task(CreateTask {
                title: "Worker task".to_string(),
                body: None,
                assignee: Some("worker".to_string()),
                status: "running".to_string(),
                priority: 0,
            })
            .unwrap();

        let diagnostics = store.diagnostics(Some(&task.id)).unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.kind == "missing_worker_log")
        );

        let written = execute_kanban_tool(
            KanbanToolKind::WorkerLog,
            &json!({"task_id": task.id, "body": "started first pass"}),
            &store,
        )
        .unwrap();
        let written: JsonValue = serde_json::from_str(&written).unwrap();
        assert_eq!(written["profile"], "worker");

        let read = execute_kanban_tool(
            KanbanToolKind::WorkerLog,
            &json!({"task_id": task.id, "tail_bytes": 1024}),
            &store,
        )
        .unwrap();
        assert!(read.contains("started first pass"));

        let diagnostics = store.diagnostics(Some(&task.id)).unwrap();
        assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.kind == "missing_worker_log")
        );
    }

    #[test]
    fn notify_subscribe_list_and_unsubscribe_are_durable() {
        let (_dir, store) = store();
        let task = create(&store, "Notify me");

        let sub = store
            .add_notify_sub(NotifyTarget {
                task_id: task.id.clone(),
                platform: "telegram".to_string(),
                chat_id: "chat-1".to_string(),
                thread_id: Some("thread-1".to_string()),
                notifier_profile: Some("default".to_string()),
            })
            .unwrap();

        assert_eq!(sub.task_id, task.id);
        assert_eq!(sub.platform, "telegram");
        assert_eq!(sub.chat_id, "chat-1");
        assert_eq!(sub.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(store.list_notify_subs(Some(&task.id)).unwrap().len(), 1);

        let removed = store
            .remove_notify_sub(NotifyTarget {
                task_id: task.id.clone(),
                platform: "telegram".to_string(),
                chat_id: "chat-1".to_string(),
                thread_id: Some("thread-1".to_string()),
                notifier_profile: None,
            })
            .unwrap();
        assert!(removed["removed"].as_bool().unwrap());
        assert!(store.list_notify_subs(Some(&task.id)).unwrap().is_empty());
    }

    #[test]
    fn notify_claim_advances_cursor_and_keeps_blocked_subscription() {
        let (_dir, store) = store();
        let task = create(&store, "Blocked notification");
        store
            .add_notify_sub(NotifyTarget {
                task_id: task.id.clone(),
                platform: "telegram".to_string(),
                chat_id: "chat-1".to_string(),
                thread_id: None,
                notifier_profile: Some("default".to_string()),
            })
            .unwrap();

        store.block_task(&task.id, "Need input").unwrap();
        let claimed = store
            .claim_notify_events(Some("default"), &[], DEFAULT_LIMIT)
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].event.kind, "blocked");
        assert!(!claimed[0].terminal);

        let subs = store.list_notify_subs(Some(&task.id)).unwrap();
        assert_eq!(subs.len(), 1);
        assert!(subs[0].last_event_id >= claimed[0].event.id);

        let claimed_again = store
            .claim_notify_events(Some("default"), &[], DEFAULT_LIMIT)
            .unwrap();
        assert!(claimed_again.is_empty());
    }

    #[test]
    fn notify_claim_final_status_removes_subscription() {
        let (_dir, store) = store();
        let task = create(&store, "Done notification");
        execute_kanban_tool(
            KanbanToolKind::NotifySubscribe,
            &json!({
                "task_id": task.id,
                "platform": "telegram",
                "chat_id": "chat-1",
                "notifier_profile": "default"
            }),
            &store,
        )
        .unwrap();

        store.complete_task(&task.id, Some("Finished")).unwrap();
        let claimed = execute_kanban_tool(
            KanbanToolKind::NotifyClaim,
            &json!({"notifier_profile": "default"}),
            &store,
        )
        .unwrap();
        let claimed: JsonValue = serde_json::from_str(&claimed).unwrap();
        assert_eq!(claimed.as_array().unwrap().len(), 1);
        assert_eq!(claimed[0]["event"]["kind"], "completed");
        assert!(claimed[0]["terminal"].as_bool().unwrap());
        assert!(store.list_notify_subs(Some(&task.id)).unwrap().is_empty());
    }

    #[test]
    fn notify_claim_filters_by_notifier_profile() {
        let (_dir, store) = store();
        let default_task = create(&store, "Default owned");
        let other_task = create(&store, "Other owned");

        for (task_id, profile) in [(&default_task.id, "default"), (&other_task.id, "other")] {
            store
                .add_notify_sub(NotifyTarget {
                    task_id: task_id.to_string(),
                    platform: "telegram".to_string(),
                    chat_id: format!("chat-{profile}"),
                    thread_id: None,
                    notifier_profile: Some(profile.to_string()),
                })
                .unwrap();
        }

        store
            .block_task(&default_task.id, "Default blocker")
            .unwrap();
        store.block_task(&other_task.id, "Other blocker").unwrap();
        let claimed = store
            .claim_notify_events(Some("default"), &[], DEFAULT_LIMIT)
            .unwrap();

        assert_eq!(claimed.len(), 1);
        assert_eq!(
            claimed[0].subscription.notifier_profile.as_deref(),
            Some("default")
        );
        assert_eq!(claimed[0].event.task_id, default_task.id);
        assert_eq!(
            store
                .list_notify_subs(Some(&other_task.id))
                .unwrap()
                .first()
                .unwrap()
                .last_event_id,
            0
        );
    }

    #[test]
    fn slash_response_help_and_create() {
        let (_dir, store) = store();
        assert!(kanban_response_with_store(None, &store).contains("/kanban"));

        let created = kanban_response_with_store(Some("create Review release"), &store);
        let value: JsonValue = serde_json::from_str(&created).unwrap();
        assert_eq!(value["title"], "Review release");
    }

    #[test]
    fn records_events_for_task_lifecycle() {
        let (_dir, store) = store();
        let task = create(&store, "Trace task");

        store
            .add_comment(&task.id, "Investigating", Some("agent"))
            .unwrap();
        store.block_task(&task.id, "Need logs").unwrap();
        store.unblock_task(&task.id, None).unwrap();
        store.complete_task(&task.id, Some("Finished")).unwrap();

        let events = store.events(&task.id, 50).unwrap();
        let kinds = events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"created"));
        assert!(kinds.contains(&"commented"));
        assert!(kinds.contains(&"blocked"));
        assert!(kinds.contains(&"unblocked"));
        assert!(kinds.contains(&"completed"));
    }

    #[test]
    fn diagnostics_surface_block_cycles_and_missing_heartbeat() {
        let (_dir, store) = store();
        let task = create(&store, "Cycle task");

        store.block_task(&task.id, "First blocker").unwrap();
        store.unblock_task(&task.id, None).unwrap();
        store.block_task(&task.id, "Second blocker").unwrap();

        let diagnostics = store.diagnostics(Some(&task.id)).unwrap();
        assert!(diagnostics.iter().any(|diag| diag.kind == "block_cycle"));

        let running = store
            .create_task(CreateTask {
                title: "Worker task".to_string(),
                body: None,
                assignee: Some("worker".to_string()),
                status: "running".to_string(),
                priority: 0,
            })
            .unwrap();
        let diagnostics = store.diagnostics(Some(&running.id)).unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.kind == "missing_heartbeat")
        );
    }

    #[test]
    fn slash_events_and_diagnostics_return_json() {
        let (_dir, store) = store();
        let task = create(&store, "Inspect trail");

        let events = kanban_response_with_store(Some(&format!("events {}", task.id)), &store);
        let events: JsonValue = serde_json::from_str(&events).unwrap();
        assert_eq!(events.as_array().unwrap()[0]["kind"], "created");

        let diagnostics =
            kanban_response_with_store(Some(&format!("diagnostics {}", task.id)), &store);
        let diagnostics: JsonValue = serde_json::from_str(&diagnostics).unwrap();
        assert!(diagnostics.as_array().unwrap().is_empty());
    }

    #[test]
    fn board_slug_validation_blocks_path_traversal() {
        assert_eq!(normalize_board_slug("Project_A").unwrap(), "project_a");
        assert!(normalize_board_slug("../secret").is_err());
        assert!(normalize_board_slug("-hidden").is_err());
    }

    #[test]
    fn explicit_board_tool_args_use_isolated_databases() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let home = dir.path().to_string_lossy().to_string();
        let _env = EnvGroup::new(&[("HAKIMI_KANBAN_HOME", Some(&home))]);

        create_board("alpha", None, None).unwrap();
        create_board("beta", None, None).unwrap();
        let default_store = KanbanStore::for_board(None).unwrap();

        execute_kanban_tool(
            KanbanToolKind::Create,
            &json!({"title": "Alpha task", "board": "alpha"}),
            &default_store,
        )
        .unwrap();
        execute_kanban_tool(
            KanbanToolKind::Create,
            &json!({"title": "Beta task", "board": "beta"}),
            &default_store,
        )
        .unwrap();

        let alpha = execute_kanban_tool(
            KanbanToolKind::List,
            &json!({"board": "alpha"}),
            &default_store,
        )
        .unwrap();
        let beta = execute_kanban_tool(
            KanbanToolKind::List,
            &json!({"board": "beta"}),
            &default_store,
        )
        .unwrap();

        assert!(alpha.contains("Alpha task"));
        assert!(!alpha.contains("Beta task"));
        assert!(beta.contains("Beta task"));
        assert!(!beta.contains("Alpha task"));
    }

    #[test]
    fn slash_boards_create_switch_and_route_commands() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let home = dir.path().to_string_lossy().to_string();
        let _env = EnvGroup::new(&[("HAKIMI_KANBAN_HOME", Some(&home))]);
        let store = KanbanStore::for_board(Some(DEFAULT_BOARD)).unwrap();

        let created = kanban_response_with_store(Some("boards create project-x Project X"), &store);
        let created: JsonValue = serde_json::from_str(&created).unwrap();
        assert_eq!(created["slug"], "project-x");
        assert_eq!(created["name"], "Project X");

        let duplicate =
            kanban_response_with_store(Some("boards create project-x Ignored Name"), &store);
        let duplicate: JsonValue = serde_json::from_str(&duplicate).unwrap();
        assert_eq!(duplicate["slug"], "project-x");
        assert_eq!(duplicate["name"], "Project X");

        let switched = kanban_response_with_store(Some("boards switch project-x"), &store);
        let switched: JsonValue = serde_json::from_str(&switched).unwrap();
        assert_eq!(switched["slug"], "project-x");
        assert_eq!(current_board_slug().unwrap(), "project-x");

        let routed = kanban_response(Some("create Routed task"));
        let routed: JsonValue = serde_json::from_str(&routed).unwrap();
        assert_eq!(routed["title"], "Routed task");

        let default_list = kanban_response_with_store(Some("--board default list"), &store);
        assert!(!default_list.contains("Routed task"));
        let project_list = kanban_response_with_store(Some("--board project-x list"), &store);
        assert!(project_list.contains("Routed task"));
    }

    struct EnvGroup {
        _guards: Vec<EnvGuard>,
    }

    impl EnvGroup {
        fn new(overrides: &[(&'static str, Option<&str>)]) -> Self {
            let mut guards = Vec::new();
            let keys = [
                "HAKIMI_KANBAN_HOME",
                "HERMES_KANBAN_HOME",
                "HAKIMI_KANBAN_DB",
                "HERMES_KANBAN_DB",
                "HAKIMI_KANBAN_BOARD",
                "HERMES_KANBAN_BOARD",
            ];
            for key in keys {
                let value = overrides
                    .iter()
                    .find_map(|(override_key, value)| (*override_key == key).then_some(*value))
                    .flatten();
                guards.push(EnvGuard::set(key, value));
            }
            Self { _guards: guards }
        }
    }

    struct EnvGuard {
        key: &'static str,
        old: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let old = std::env::var(key).ok();
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(old) = &self.old {
                    std::env::set_var(self.key, old);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }
}
