//! Session CRUD operations.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::params;
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

    fn get_session(&self, id: &str) -> Result<Option<SessionMeta>>;

    fn update_session_totals(
        &self,
        id: &str,
        usage: &Usage,
        api_calls: i32,
    ) -> Result<()>;

    fn end_session(&self, id: &str, reason: &str) -> Result<()>;

    fn get_recent_sessions(
        &self,
        source: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SessionMeta>>;
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
        let now = Utc::now().to_rfc3339();

        let conn = self.conn().lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, source, user_id, model, system_prompt, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, source, user_id, model, system_prompt, now],
        )
        .context("Failed to create session")?;

        debug!("Created session {id} from source={source}");
        Ok(id)
    }

    /// Fetch a single session by ID.
    fn get_session(&self, id: &str) -> Result<Option<SessionMeta>> {
        let conn = self.conn().lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        started_at, ended_at, end_reason, message_count, tool_call_count,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                        reasoning_tokens, title, api_call_count
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
                started_at: row.get(6)?,
                ended_at: row.get(7)?,
                end_reason: row.get(8)?,
                message_count: row.get(9)?,
                tool_call_count: row.get(10)?,
                input_tokens: row.get(11)?,
                output_tokens: row.get(12)?,
                cache_read_tokens: row.get(13)?,
                cache_write_tokens: row.get(14)?,
                reasoning_tokens: row.get(15)?,
                title: row.get(16)?,
                api_call_count: row.get(17)?,
            })
        });

        match row {
            Ok(meta) => Ok(Some(meta)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("Failed to get session"),
        }
    }

    /// Increment token usage and API call counters for a session.
    fn update_session_totals(
        &self,
        id: &str,
        usage: &Usage,
        api_calls: i32,
    ) -> Result<()> {
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

    /// List recent sessions, optionally filtered by source.
    fn get_recent_sessions(
        &self,
        source: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SessionMeta>> {
        let conn = self.conn().lock().unwrap();

        let sql = match source {
            Some(_) => {
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        started_at, ended_at, end_reason, message_count, tool_call_count,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                        reasoning_tokens, title, api_call_count
                 FROM sessions WHERE source = ?1 ORDER BY started_at DESC LIMIT ?2"
            }
            None => {
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        started_at, ended_at, end_reason, message_count, tool_call_count,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                        reasoning_tokens, title, api_call_count
                 FROM sessions ORDER BY started_at DESC LIMIT ?1"
            }
        };

        let mut stmt = conn.prepare(sql).context("Failed to prepare get_recent_sessions")?;

        let rows = if source.is_some() {
            stmt.query_map(params![source.unwrap(), limit], row_to_session_meta)
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
        started_at: row.get(6)?,
        ended_at: row.get(7)?,
        end_reason: row.get(8)?,
        message_count: row.get(9)?,
        tool_call_count: row.get(10)?,
        input_tokens: row.get(11)?,
        output_tokens: row.get(12)?,
        cache_read_tokens: row.get(13)?,
        cache_write_tokens: row.get(14)?,
        reasoning_tokens: row.get(15)?,
        title: row.get(16)?,
        api_call_count: row.get(17)?,
    })
}
