//! Session CRUD operations.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

use crate::db::SessionDB;
use hakimi_common::Usage;

/// Full metadata for a session row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub source: Option<String>,
    pub user_id: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub parent_session_id: Option<String>,
    pub root_session_id: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub end_reason: Option<String>,
    pub message_count: i32,
    pub tool_call_count: i32,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub title: Option<String>,
    pub api_call_count: i32,
    pub workdir: Option<String>,
}

/// Trait providing session CRUD operations on a `SessionDB`.
pub trait SessionOps {
    fn create_session(
        &self,
        source: &str,
        user_id: Option<&str>,
        model: Option<&str>,
        system_prompt: Option<&str>,
    ) -> Result<String>;

    fn create_session_with_id(
        &self,
        id: &str,
        source: &str,
        user_id: Option<&str>,
        model: Option<&str>,
        system_prompt: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<String>;

    fn get_session(&self, id: &str) -> Result<Option<SessionMeta>>;

    fn update_session_totals(&self, id: &str, usage: &Usage, api_calls: i32) -> Result<()>;

    fn end_session(&self, id: &str, reason: &str) -> Result<()>;

    fn set_title(&self, session_id: &str, title: &str) -> Result<()>;

    fn set_unique_title(&self, session_id: &str, title: &str) -> Result<String>;

    fn clear_title(&self, session_id: &str) -> Result<()>;

    fn delete_session(&self, id: &str) -> Result<bool>;

    fn clear_session_messages(&self, id: &str) -> Result<bool>;

    fn get_session_with_messages(
        &self,
        session_id: &str,
        max_messages: Option<usize>,
    ) -> Result<Option<(SessionMeta, Vec<hakimi_common::Message>)>>;

    fn get_recent_sessions(&self, source: Option<&str>, limit: i64) -> Result<Vec<SessionMeta>>;

    // Lineage methods
    fn get_session_root(&self, session_id: &str) -> Result<Option<String>>;
    fn has_parent(&self, session_id: &str) -> Result<bool>;
    fn get_session_depth(&self, session_id: &str) -> Result<usize>;
    fn get_child_sessions(&self, session_id: &str) -> Result<Vec<SessionMeta>>;
    
    /// Get the complete lineage chain from current session to root.
    /// Returns a vector of SessionMeta ordered from the given session_id to its root ancestor.
    fn get_session_lineage(&self, session_id: &str) -> Result<Vec<SessionMeta>>;
    
    /// Get the root session metadata for a given session.
    /// Returns the full SessionMeta of the root session.
    fn get_root_session_meta(&self, session_id: &str) -> Result<SessionMeta>;
}

/// Generate a concise session title from the conversation messages.
///
/// Strategy: take the first user message, clean it up, truncate to ~60 chars.
pub fn generate_session_title(messages: &[hakimi_common::Message]) -> String {
    const MAX_CHARS: usize = 60;

    let first_user = messages
        .iter()
        .find(|m| m.role == hakimi_common::MessageRole::User);

    let content = match first_user.and_then(|m| m.content.as_deref()) {
        Some(c) => c,
        None => return "Untitled session".to_string(),
    };

    // Collapse whitespace and newlines
    let cleaned: String = content.split_whitespace().collect::<Vec<&str>>().join(" ");

    if cleaned.chars().count() <= MAX_CHARS {
        cleaned
    } else {
        let truncated: String = cleaned.chars().take(MAX_CHARS).collect();
        if let Some(last_space) = truncated.rfind(' ') {
            format!("{}...", truncated[..last_space].trim_end())
        } else {
            format!("{}...", truncated.trim_end())
        }
    }
}

impl SessionOps for SessionDB {
    /// Create a new session and return its UUID.
    fn create_session(
        &self,
        source: &str,
        user_id: Option<&str>,
        model: Option<&str>,
        system_prompt: Option<&str>,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.create_session_with_id(&id, source, user_id, model, system_prompt, None)
    }

    fn create_session_with_id(
        &self,
        id: &str,
        source: &str,
        user_id: Option<&str>,
        model: Option<&str>,
        system_prompt: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<String> {
        let now = Utc::now().to_rfc3339();

        // Calculate root_session_id based on parent
        let root_session_id = if let Some(parent_id) = parent_session_id {
            // Get the root of the parent session
            self.get_session_root(parent_id)?
                .or(Some(parent_id.to_string()))
        } else {
            None
        };

        let conn = self.conn().lock().unwrap();
        conn.execute(
            "INSERT INTO sessions
                (id, source, user_id, model, system_prompt, parent_session_id, root_session_id, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                source,
                user_id,
                model,
                system_prompt,
                parent_session_id,
                root_session_id,
                now
            ],
        )
        .context("Failed to create session")?;

        debug!(
            "Created session {id} from source={source}, parent={:?}, root={:?}",
            parent_session_id, root_session_id
        );
        Ok(id.to_string())
    }

    /// Fetch a single session by ID.
    fn get_session(&self, id: &str) -> Result<Option<SessionMeta>> {
        let conn = self.conn().lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        root_session_id, started_at, ended_at, end_reason, message_count, 
                        tool_call_count, input_tokens, output_tokens, cache_read_tokens, 
                        cache_write_tokens, reasoning_tokens, title, api_call_count, workdir
                 FROM sessions WHERE id = ?1",
            )
            .context("Failed to prepare get_session statement")?;

        let row = stmt.query_row(params![id], |row| {
            Ok(SessionMeta {
                id: row.get(0)?,
                source: row.get(1)?,
                user_id: row.get(2)?,
                model: row.get(3)?,
                system_prompt: row.get(4)?,
                parent_session_id: row.get(5)?,
                root_session_id: row.get(6)?,
                started_at: row.get(7)?,
                ended_at: row.get(8)?,
                end_reason: row.get(9)?,
                message_count: row.get(10)?,
                tool_call_count: row.get(11)?,
                input_tokens: row.get(12)?,
                output_tokens: row.get(13)?,
                cache_read_tokens: row.get(14)?,
                cache_write_tokens: row.get(15)?,
                reasoning_tokens: row.get(16)?,
                title: row.get(17)?,
                api_call_count: row.get(18)?,
                workdir: row.get(19)?,
            })
        });

        match row {
            Ok(meta) => Ok(Some(meta)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("Failed to get session"),
        }
    }

    /// Increment token usage and API call counters for a session.
    fn update_session_totals(&self, id: &str, usage: &Usage, api_calls: i32) -> Result<()> {
        let conn = self.conn().lock().unwrap();
        let updated = conn
            .execute(
                "UPDATE sessions SET
                    input_tokens = input_tokens + ?2,
                    output_tokens = output_tokens + ?3,
                    cache_read_tokens = cache_read_tokens + ?4,
                    reasoning_tokens = reasoning_tokens + ?5,
                    api_call_count = api_call_count + ?6
                 WHERE id = ?1",
                params![
                    id,
                    usage.prompt_tokens as i64,
                    usage.completion_tokens as i64,
                    usage.cached_tokens as i64,
                    usage.reasoning_tokens as i64,
                    api_calls,
                ],
            )
            .context("Failed to update session totals")?;

        if updated == 0 {
            anyhow::bail!("Session {id} not found for totals update");
        }
        Ok(())
    }

    /// Mark a session as ended with a reason.
    fn end_session(&self, id: &str, reason: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn().lock().unwrap();
        let updated = conn
            .execute(
                "UPDATE sessions SET ended_at = ?2, end_reason = ?3 WHERE id = ?1",
                params![id, now, reason],
            )
            .context("Failed to end session")?;

        if updated == 0 {
            anyhow::bail!("Session {id} not found for end_session");
        }
        debug!("Ended session {id}: {reason}");
        Ok(())
    }

    fn set_title(&self, session_id: &str, title: &str) -> Result<()> {
        let conn = self.conn().lock().unwrap();
        let updated = conn
            .execute(
                "UPDATE sessions SET title = ?2 WHERE id = ?1",
                params![session_id, title],
            )
            .context("Failed to set session title")?;

        if updated == 0 {
            anyhow::bail!("Session {session_id} not found for set_title");
        }
        Ok(())
    }

    fn set_unique_title(&self, session_id: &str, title: &str) -> Result<String> {
        let conn = self.conn().lock().unwrap();
        let mut candidate = title.trim().to_string();
        if candidate.is_empty() {
            candidate = "Untitled session".to_string();
        }

        let base = candidate.clone();
        let mut attempt = 1usize;
        loop {
            let conflict: Option<String> = conn
                .query_row(
                    "SELECT id FROM sessions WHERE title = ?1 AND id != ?2 LIMIT 1",
                    params![candidate, session_id],
                    |row| row.get(0),
                )
                .optional()
                .context("Failed to check session title uniqueness")?;

            if conflict.is_none() {
                let updated = conn
                    .execute(
                        "UPDATE sessions SET title = ?2 WHERE id = ?1",
                        params![session_id, candidate],
                    )
                    .context("Failed to set unique session title")?;

                if updated == 0 {
                    anyhow::bail!("Session {session_id} not found for set_unique_title");
                }
                return Ok(candidate);
            }

            attempt += 1;
            candidate = format!("{base} ({attempt})");
        }
    }

    fn clear_title(&self, session_id: &str) -> Result<()> {
        let conn = self.conn().lock().unwrap();
        let updated = conn
            .execute(
                "UPDATE sessions SET title = NULL WHERE id = ?1",
                params![session_id],
            )
            .context("Failed to clear session title")?;

        if updated == 0 {
            anyhow::bail!("Session {session_id} not found for clear_title");
        }
        Ok(())
    }

    fn delete_session(&self, id: &str) -> Result<bool> {
        let mut conn = self.conn().lock().unwrap();
        let tx = conn
            .transaction()
            .context("Failed to start session delete transaction")?;
        tx.execute("DELETE FROM messages WHERE session_id = ?1", params![id])
            .context("Failed to delete session messages")?;
        let deleted = tx
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])
            .context("Failed to delete session")?
            > 0;
        tx.commit()
            .context("Failed to commit session delete transaction")?;
        Ok(deleted)
    }

    fn clear_session_messages(&self, id: &str) -> Result<bool> {
        let mut conn = self.conn().lock().unwrap();
        let tx = conn
            .transaction()
            .context("Failed to start session message clear transaction")?;
        let exists: Option<String> = tx
            .query_row(
                "SELECT id FROM sessions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .context("Failed to check session before clearing messages")?;
        if exists.is_none() {
            tx.commit()
                .context("Failed to commit no-op session message clear transaction")?;
            return Ok(false);
        }

        tx.execute("DELETE FROM messages WHERE session_id = ?1", params![id])
            .context("Failed to clear session messages")?;
        tx.execute(
            "UPDATE sessions SET
                message_count = 0,
                tool_call_count = 0,
                input_tokens = 0,
                output_tokens = 0,
                cache_read_tokens = 0,
                cache_write_tokens = 0,
                reasoning_tokens = 0,
                api_call_count = 0
             WHERE id = ?1",
            params![id],
        )
        .context("Failed to reset session counters after clearing messages")?;
        tx.commit()
            .context("Failed to commit session message clear transaction")?;
        Ok(true)
    }

    fn get_session_with_messages(
        &self,
        session_id: &str,
        max_messages: Option<usize>,
    ) -> Result<Option<(SessionMeta, Vec<hakimi_common::Message>)>> {
        use crate::message_ops::MessageOps;

        let meta = self.get_session(session_id)?;
        let meta = match meta {
            Some(m) => m,
            None => return Ok(None),
        };

        let messages = self.restore_session(session_id, max_messages)?;
        Ok(Some((meta, messages)))
    }

    /// List recent sessions, optionally filtered by source.
    fn get_recent_sessions(&self, source: Option<&str>, limit: i64) -> Result<Vec<SessionMeta>> {
        let conn = self.conn().lock().unwrap();

        let sql = match source {
            Some(_) => {
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        root_session_id, started_at, ended_at, end_reason, message_count, 
                        tool_call_count, input_tokens, output_tokens, cache_read_tokens, 
                        cache_write_tokens, reasoning_tokens, title, api_call_count, workdir
                 FROM sessions WHERE source = ?1 ORDER BY started_at DESC LIMIT ?2"
            }
            None => {
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        root_session_id, started_at, ended_at, end_reason, message_count, 
                        tool_call_count, input_tokens, output_tokens, cache_read_tokens, 
                        cache_write_tokens, reasoning_tokens, title, api_call_count, workdir
                 FROM sessions ORDER BY started_at DESC LIMIT ?1"
            }
        };

        let mut stmt = conn
            .prepare(sql)
            .context("Failed to prepare get_recent_sessions")?;

        let rows = if let Some(source_val) = source {
            stmt.query_map(params![source_val, limit], row_to_session_meta)
                .context("Failed to query recent sessions")?
        } else {
            stmt.query_map(params![limit], row_to_session_meta)
                .context("Failed to query recent sessions")?
        };

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    /// Get the root session ID for a given session (following parent chain).
    fn get_session_root(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn().lock().unwrap();

        // Query the session, handling both "no row" and "NULL value" cases
        let result = conn.query_row(
            "SELECT root_session_id FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get::<_, Option<String>>(0),
        );

        match result {
            Ok(root) => Ok(root),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                anyhow::bail!("Session {} not found", session_id)
            }
            Err(e) => Err(e).context("Failed to get session root"),
        }
    }

    /// Check if a session has a parent.
    fn has_parent(&self, session_id: &str) -> Result<bool> {
        let conn = self.conn().lock().unwrap();
        let parent = conn.query_row(
            "SELECT parent_session_id FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get::<_, Option<String>>(0),
        );

        match parent {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                anyhow::bail!("Session {} not found", session_id)
            }
            Err(e) => Err(e).context("Failed to check parent"),
        }
    }

    /// Get the depth of a session in its lineage tree (root = 0).
    fn get_session_depth(&self, session_id: &str) -> Result<usize> {
        let mut depth = 0;
        let mut current_id = session_id.to_string();
        let conn = self.conn().lock().unwrap();

        loop {
            let parent = conn.query_row(
                "SELECT parent_session_id FROM sessions WHERE id = ?1",
                params![current_id],
                |row| row.get::<_, Option<String>>(0),
            );

            match parent {
                Ok(Some(pid)) => {
                    depth += 1;
                    current_id = pid;
                    if depth > 100 {
                        anyhow::bail!("Session lineage depth exceeds 100, possible cycle");
                    }
                }
                Ok(None) => break,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    anyhow::bail!("Session {} not found while calculating depth", current_id)
                }
                Err(e) => return Err(e).context("Failed to get parent for depth calculation"),
            }
        }
        Ok(depth)
    }

    /// Get direct child sessions of a given session.
    fn get_child_sessions(&self, session_id: &str) -> Result<Vec<SessionMeta>> {
        let conn = self.conn().lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        root_session_id, started_at, ended_at, end_reason, message_count,
                        tool_call_count, input_tokens, output_tokens, cache_read_tokens,
                        cache_write_tokens, reasoning_tokens, title, api_call_count, workdir
                 FROM sessions WHERE parent_session_id = ?1 ORDER BY started_at ASC",
            )
            .context("Failed to prepare get_child_sessions")?;

        let rows = stmt
            .query_map(params![session_id], row_to_session_meta)
            .context("Failed to query child sessions")?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    /// Get the complete lineage chain from the given session to its root ancestor.
    /// Returns a vector ordered from the given session_id to root (e.g., [grandchild, child, root]).
    fn get_session_lineage(&self, session_id: &str) -> Result<Vec<SessionMeta>> {
        let mut lineage = Vec::new();
        let mut current_id = session_id.to_string();
        let conn = self.conn().lock().unwrap();
        let mut visited = std::collections::HashSet::new();

        loop {
            // Prevent infinite loops due to cycles
            if !visited.insert(current_id.clone()) {
                anyhow::bail!(
                    "Detected cycle in session lineage at session {}",
                    current_id
                );
            }

            if visited.len() > 100 {
                anyhow::bail!("Session lineage exceeds 100 levels, possible data corruption");
            }

            // Get current session metadata
            let meta_result = conn.query_row(
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        root_session_id, started_at, ended_at, end_reason, message_count,
                        tool_call_count, input_tokens, output_tokens, cache_read_tokens,
                        cache_write_tokens, reasoning_tokens, title, api_call_count, workdir
                 FROM sessions WHERE id = ?1",
                params![current_id],
                row_to_session_meta,
            );

            let meta = match meta_result {
                Ok(m) => m,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    if lineage.is_empty() {
                        anyhow::bail!("Session {} not found", session_id);
                    } else {
                        anyhow::bail!(
                            "Orphaned session detected: parent {} does not exist",
                            current_id
                        );
                    }
                }
                Err(e) => return Err(e).context("Failed to query session in lineage"),
            };

            lineage.push(meta.clone());

            // Check for parent
            match meta.parent_session_id {
                Some(pid) => {
                    current_id = pid;
                }
                None => break,
            }
        }

        Ok(lineage)
    }

    /// Get the root session metadata for a given session.
    fn get_root_session_meta(&self, session_id: &str) -> Result<SessionMeta> {
        let conn = self.conn().lock().unwrap();

        // First, try to get root_session_id from the current session
        let root_id_opt: Option<String> = conn
            .query_row(
                "SELECT root_session_id FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .context("Failed to query root_session_id")?
            .flatten();

        // If root_id is set, fetch that session
        let root_id = if let Some(rid) = root_id_opt {
            rid
        } else {
            // Otherwise, traverse the lineage to find the root
            let mut current_id = session_id.to_string();
            let mut visited = std::collections::HashSet::new();

            loop {
                if !visited.insert(current_id.clone()) {
                    anyhow::bail!("Detected cycle while finding root session for {}", session_id);
                }

                if visited.len() > 100 {
                    anyhow::bail!("Session lineage exceeds 100 levels");
                }

                let parent_opt: Option<String> = conn
                    .query_row(
                        "SELECT parent_session_id FROM sessions WHERE id = ?1",
                        params![current_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .context("Failed to query parent in root traversal")?
                    .flatten();

                match parent_opt {
                    Some(pid) => current_id = pid,
                    None => break,
                }
            }

            current_id
        };

        // Fetch the full metadata of the root session
        let root_meta = conn
            .query_row(
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        root_session_id, started_at, ended_at, end_reason, message_count,
                        tool_call_count, input_tokens, output_tokens, cache_read_tokens,
                        cache_write_tokens, reasoning_tokens, title, api_call_count, workdir
                 FROM sessions WHERE id = ?1",
                params![root_id],
                row_to_session_meta,
            )
            .context("Failed to fetch root session metadata")?;

        Ok(root_meta)
    }
}

/// Helper to map a rusqlite Row to SessionMeta.
fn row_to_session_meta(row: &rusqlite::Row) -> rusqlite::Result<SessionMeta> {
    Ok(SessionMeta {
        id: row.get(0)?,
        source: row.get(1)?,
        user_id: row.get(2)?,
        model: row.get(3)?,
        system_prompt: row.get(4)?,
        parent_session_id: row.get(5)?,
        root_session_id: row.get(6)?,
        started_at: row.get(7)?,
        ended_at: row.get(8)?,
        end_reason: row.get(9)?,
        message_count: row.get(10)?,
        tool_call_count: row.get(11)?,
        input_tokens: row.get(12)?,
        output_tokens: row.get(13)?,
        cache_read_tokens: row.get(14)?,
        cache_write_tokens: row.get(15)?,
        reasoning_tokens: row.get(16)?,
        title: row.get(17)?,
        api_call_count: row.get(18)?,
        workdir: row.get(19)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_db;
    use hakimi_common::Usage;

    #[test]
    fn test_create_and_get_session() {
        let db = test_db();
        let id = db
            .create_session("telegram", Some("user1"), Some("gpt-4"), Some("Be helpful"))
            .unwrap();

        assert!(!id.is_empty());

        let meta = db.get_session(&id).unwrap().expect("session should exist");
        assert_eq!(meta.id, id);
        assert_eq!(meta.source.as_deref(), Some("telegram"));
        assert_eq!(meta.user_id.as_deref(), Some("user1"));
        assert_eq!(meta.model.as_deref(), Some("gpt-4"));
        assert_eq!(meta.system_prompt.as_deref(), Some("Be helpful"));
        assert!(meta.started_at.is_some());
        assert!(meta.ended_at.is_none());
        assert_eq!(meta.message_count, 0);
        assert_eq!(meta.api_call_count, 0);
    }

    #[test]
    fn test_create_session_with_none_fields() {
        let db = test_db();
        let id = db.create_session("web", None, None, None).unwrap();

        let meta = db.get_session(&id).unwrap().unwrap();
        assert_eq!(meta.source.as_deref(), Some("web"));
        assert!(meta.user_id.is_none());
        assert!(meta.model.is_none());
        assert!(meta.system_prompt.is_none());
    }

    #[test]
    fn test_get_session_not_found() {
        let db = test_db();
        let result = db.get_session("nonexistent-id").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_create_multiple_sessions() {
        let db = test_db();
        let id1 = db.create_session("telegram", None, None, None).unwrap();
        let id2 = db.create_session("web", None, None, None).unwrap();
        assert_ne!(id1, id2);

        let meta1 = db.get_session(&id1).unwrap().unwrap();
        let meta2 = db.get_session(&id2).unwrap().unwrap();
        assert_eq!(meta1.source.as_deref(), Some("telegram"));
        assert_eq!(meta2.source.as_deref(), Some("web"));
    }

    #[test]
    fn test_get_recent_sessions_unfiltered() {
        let db = test_db();
        db.create_session("telegram", None, None, None).unwrap();
        db.create_session("web", None, None, None).unwrap();
        db.create_session("telegram", None, None, None).unwrap();

        let all = db.get_recent_sessions(None, 10).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_get_recent_sessions_filtered_by_source() {
        let db = test_db();
        db.create_session("telegram", None, None, None).unwrap();
        db.create_session("web", None, None, None).unwrap();
        db.create_session("telegram", None, None, None).unwrap();

        let tg = db.get_recent_sessions(Some("telegram"), 10).unwrap();
        assert_eq!(tg.len(), 2);
        for s in &tg {
            assert_eq!(s.source.as_deref(), Some("telegram"));
        }
    }

    #[test]
    fn test_get_recent_sessions_limit() {
        let db = test_db();
        for _ in 0..5 {
            db.create_session("telegram", None, None, None).unwrap();
        }

        let limited = db.get_recent_sessions(None, 2).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_get_recent_sessions_empty() {
        let db = test_db();
        let sessions = db.get_recent_sessions(None, 10).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_end_session() {
        let db = test_db();
        let id = db.create_session("telegram", None, None, None).unwrap();

        db.end_session(&id, "user_disconnect").unwrap();

        let meta = db.get_session(&id).unwrap().unwrap();
        assert!(meta.ended_at.is_some());
        assert_eq!(meta.end_reason.as_deref(), Some("user_disconnect"));
    }

    #[test]
    fn test_end_session_not_found() {
        let db = test_db();
        let result = db.end_session("nonexistent", "reason");
        assert!(result.is_err());
    }

    #[test]
    fn test_update_session_totals() {
        let db = test_db();
        let id = db.create_session("telegram", None, None, None).unwrap();

        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            cached_tokens: 20,
            reasoning_tokens: 10,
        };
        db.update_session_totals(&id, &usage, 1).unwrap();

        let meta = db.get_session(&id).unwrap().unwrap();
        assert_eq!(meta.input_tokens, 100);
        assert_eq!(meta.output_tokens, 50);
        assert_eq!(meta.cache_read_tokens, 20);
        assert_eq!(meta.reasoning_tokens, 10);
        assert_eq!(meta.api_call_count, 1);
    }

    #[test]
    fn test_update_session_totals_accumulates() {
        let db = test_db();
        let id = db.create_session("telegram", None, None, None).unwrap();

        let usage1 = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            cached_tokens: 20,
            reasoning_tokens: 10,
        };
        let usage2 = Usage {
            prompt_tokens: 200,
            completion_tokens: 100,
            total_tokens: 300,
            cached_tokens: 40,
            reasoning_tokens: 30,
        };

        db.update_session_totals(&id, &usage1, 1).unwrap();
        db.update_session_totals(&id, &usage2, 2).unwrap();

        let meta = db.get_session(&id).unwrap().unwrap();
        assert_eq!(meta.input_tokens, 300);
        assert_eq!(meta.output_tokens, 150);
        assert_eq!(meta.cache_read_tokens, 60);
        assert_eq!(meta.reasoning_tokens, 40);
        assert_eq!(meta.api_call_count, 3);
    }

    #[test]
    fn test_update_session_totals_not_found() {
        let db = test_db();
        let usage = Usage::default();
        let result = db.update_session_totals("nonexistent", &usage, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_session_with_parent() {
        let db = test_db();
        let parent_id = db.create_session("telegram", None, None, None).unwrap();

        // Manually create a child session referencing the parent
        let child_id = db.create_session("telegram", None, None, None).unwrap();
        {
            let conn = db.conn().lock().unwrap();
            conn.execute(
                "UPDATE sessions SET parent_session_id = ?1 WHERE id = ?2",
                rusqlite::params![parent_id, child_id],
            )
            .unwrap();
        }

        let child = db.get_session(&child_id).unwrap().unwrap();
        assert_eq!(child.parent_session_id.as_deref(), Some(parent_id.as_str()));
    }

    #[test]
    fn test_clear_session_messages_keeps_session_and_resets_counters() {
        use crate::message_ops::MessageOps;
        use hakimi_common::{Message, Usage};

        let db = test_db();
        let id = db
            .create_session("web", None, Some("test-model"), None)
            .unwrap();
        db.save_message(&id, &Message::user("persistent clear marker"))
            .unwrap();
        db.save_message(&id, &Message::assistant("assistant reply"))
            .unwrap();
        db.update_session_totals(
            &id,
            &Usage {
                prompt_tokens: 7,
                completion_tokens: 11,
                total_tokens: 18,
                cached_tokens: 3,
                reasoning_tokens: 5,
            },
            1,
        )
        .unwrap();

        assert!(db.clear_session_messages(&id).unwrap());
        let meta = db.get_session(&id).unwrap().expect("session remains");
        assert_eq!(meta.message_count, 0);
        assert_eq!(meta.input_tokens, 0);
        assert_eq!(meta.output_tokens, 0);
        assert_eq!(meta.cache_read_tokens, 0);
        assert_eq!(meta.reasoning_tokens, 0);
        assert_eq!(meta.api_call_count, 0);
        assert!(db.get_messages(&id).unwrap().is_empty());
        assert!(db.search_messages("persistent", 5).unwrap().is_empty());
        assert!(!db.clear_session_messages("missing-session").unwrap());
    }

    #[test]
    fn test_set_title() {
        let db = test_db();
        let id = db.create_session("telegram", None, None, None).unwrap();

        db.set_title(&id, "My Chat Session").unwrap();

        let meta = db.get_session(&id).unwrap().unwrap();
        assert_eq!(meta.title.as_deref(), Some("My Chat Session"));
    }

    #[test]
    fn test_set_title_not_found() {
        let db = test_db();
        let result = db.set_title("nonexistent", "title");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_title_short_message() {
        let messages = vec![
            hakimi_common::Message::system("You are helpful."),
            hakimi_common::Message::user("Hello, how are you?"),
        ];
        let title = super::generate_session_title(&messages);
        assert_eq!(title, "Hello, how are you?");
    }

    #[test]
    fn test_generate_title_long_message() {
        let long = "What is the meaning of life and everything in the universe and beyond the stars and galaxies far away";
        let messages = vec![hakimi_common::Message::user(long)];
        let title = super::generate_session_title(&messages);
        assert!(title.len() <= 63); // 60 + "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_generate_title_truncates_unicode_safely() {
        let long = "请帮我分析这个特别长的中文需求并整理成可以执行的工程任务列表同时保留关键约束和风险再补充迁移步骤验收标准回滚方案以及后续负责人";
        let messages = vec![hakimi_common::Message::user(long)];
        let title = super::generate_session_title(&messages);

        assert!(title.chars().count() <= 63);
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_generate_title_multiline() {
        let messages = vec![hakimi_common::Message::user(
            "Hello\nworld\nhow\nare\nyou\ndoing\ntoday\nin\nthis\nbeautiful\nworld",
        )];
        let title = super::generate_session_title(&messages);
        assert!(!title.contains('\n'));
        assert_eq!(
            title,
            "Hello world how are you doing today in this beautiful world"
        );
    }

    #[test]
    fn test_generate_title_empty_messages() {
        let messages: Vec<hakimi_common::Message> = vec![];
        let title = super::generate_session_title(&messages);
        assert_eq!(title, "Untitled session");
    }

    #[test]
    fn test_get_session_with_messages() {
        use crate::message_ops::MessageOps;

        let db = test_db();
        let id = db.create_session("telegram", None, None, None).unwrap();

        db.save_message(&id, &hakimi_common::Message::user("Hello"))
            .unwrap();
        db.save_message(&id, &hakimi_common::Message::assistant("Hi there!"))
            .unwrap();
        db.save_message(&id, &hakimi_common::Message::user("How are you?"))
            .unwrap();

        let result = db.get_session_with_messages(&id, None).unwrap();
        assert!(result.is_some());

        let (meta, messages) = result.unwrap();
        assert_eq!(meta.id, id);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content.as_deref(), Some("Hello"));
        assert_eq!(messages[2].content.as_deref(), Some("How are you?"));
    }

    #[test]
    fn test_get_session_with_messages_not_found() {
        let db = test_db();
        let result = db.get_session_with_messages("nonexistent", None).unwrap();
        assert!(result.is_none());
    }
}
