//! Message CRUD operations and FTS search.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::db::SessionDB;
use hakimi_common::{Message, MessageRole};

/// A full-text search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub session_id: String,
    pub message_id: i64,
    pub content: Option<String>,
    pub rank: f64,
}

/// Trait providing message CRUD operations on a `SessionDB`.
pub trait MessageOps {
    fn save_message(&self, session_id: &str, msg: &Message) -> Result<()>;
    fn get_messages(&self, session_id: &str) -> Result<Vec<Message>>;
    fn search_messages(&self, query: &str, limit: i64) -> Result<Vec<SearchResult>>;
}

impl MessageOps for SessionDB {
    /// Persist a message and bump the session's message_count.
    fn save_message(&self, session_id: &str, msg: &Message) -> Result<()> {
        let role_str = msg.role.to_string();
        let timestamp = msg
            .timestamp
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| Utc::now().to_rfc3339());

        let tool_calls_json = msg
            .tool_calls
            .as_ref()
            .map(|tc| serde_json::to_string(tc).unwrap_or_default());

        let tool_name = msg.name.as_deref();

        let conn = self.conn().lock().unwrap();
        conn.execute(
            "INSERT INTO messages
                (session_id, role, content, tool_call_id, tool_calls, tool_name,
                 timestamp, token_count, finish_reason, reasoning)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                session_id,
                role_str,
                msg.content,
                msg.tool_call_id,
                tool_calls_json,
                tool_name,
                timestamp,
                msg.token_count.map(|v| v as i64),
                msg.finish_reason,
                msg.reasoning,
            ],
        )
        .context("Failed to save message")?;

        // Increment message_count on the parent session.
        conn.execute(
            "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
            params![session_id],
        )
        .context("Failed to increment message_count")?;

        debug!("Saved message for session {session_id}");
        Ok(())
    }

    /// Retrieve all messages for a session, ordered by timestamp.
    fn get_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let conn = self.conn().lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT role, content, tool_call_id, tool_calls, tool_name,
                        timestamp, token_count, finish_reason, reasoning
                 FROM messages WHERE session_id = ?1 ORDER BY id ASC",
            )
            .context("Failed to prepare get_messages statement")?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let role_str: String = row.get(0)?;
                let tool_calls_json: Option<String> = row.get(3)?;
                let timestamp_str: Option<String> = row.get(5)?;
                let token_count: Option<i64> = row.get(6)?;

                let role = match role_str.as_str() {
                    "system" => MessageRole::System,
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    "tool" => MessageRole::Tool,
                    _ => MessageRole::User, // fallback
                };

                let tool_calls = tool_calls_json.and_then(|json| {
                    serde_json::from_str(&json).ok()
                });

                let timestamp = timestamp_str.and_then(|s| {
                    chrono::DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                });

                Ok(Message {
                    role,
                    content: row.get(1)?,
                    tool_calls,
                    tool_call_id: row.get(2)?,
                    name: row.get(4)?,
                    reasoning: row.get(8)?,
                    reasoning_content: None,
                    timestamp,
                    token_count: token_count.map(|v| v as u32),
                    finish_reason: row.get(7)?,
                })
            })
            .context("Failed to query messages")?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    /// Full-text search across message content, tool names, and tool calls.
    fn search_messages(&self, query: &str, limit: i64) -> Result<Vec<SearchResult>> {
        let conn = self.conn().lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT m.session_id, m.id, m.content, rank
                 FROM messages_fts fts
                 JOIN messages m ON m.id = fts.rowid
                 WHERE messages_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .context("Failed to prepare FTS search statement")?;

        let rows = stmt
            .query_map(params![query, limit], |row| {
                Ok(SearchResult {
                    session_id: row.get(0)?,
                    message_id: row.get(1)?,
                    content: row.get(2)?,
                    rank: row.get(3)?,
                })
            })
            .context("Failed to execute FTS search")?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}
