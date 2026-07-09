//! Message CRUD operations and FTS search.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, warn};

use crate::db::SessionDB;
use crate::session_ops::generate_session_title;
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
    fn restore_session(
        &self,
        session_id: &str,
        max_messages: Option<usize>,
    ) -> Result<Vec<Message>>;
    fn search_messages(&self, query: &str, limit: i64) -> Result<Vec<SearchResult>>;
    fn delete_message(&self, session_id: &str, message_id: &str) -> Result<bool>;
    /// Get a window of messages around an anchor message ID.
    /// Returns (window, messages_before, messages_after) where window includes the anchor.
    fn get_messages_around(
        &self,
        session_id: &str,
        anchor_id: i64,
        window: i64,
    ) -> Result<(Vec<Message>, i64, i64)>;
    /// Get bookend messages: first N and last N user+assistant messages of a session.
    /// Returns (start_messages, end_messages).
    fn get_bookends(&self, session_id: &str, count: i64) -> Result<(Vec<Message>, Vec<Message>)>;
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

        if msg.role == MessageRole::User {
            maybe_set_generated_title(&conn, session_id, msg)?;
        }

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
            .query_map(params![session_id], row_to_message)
            .context("Failed to query messages")?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    fn restore_session(
        &self,
        session_id: &str,
        max_messages: Option<usize>,
    ) -> Result<Vec<Message>> {
        match max_messages {
            Some(limit) => {
                let conn = self.conn().lock().unwrap();
                // Get the last N messages, then re-order by id ASC
                let mut stmt = conn
                    .prepare(
                        "SELECT role, content, tool_call_id, tool_calls, tool_name,
                                timestamp, token_count, finish_reason, reasoning
                         FROM (
                             SELECT role, content, tool_call_id, tool_calls, tool_name,
                                    timestamp, token_count, finish_reason, reasoning, id
                             FROM messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2
                         ) sub ORDER BY id ASC",
                    )
                    .context("Failed to prepare restore_session statement")?;

                let rows = stmt
                    .query_map(params![session_id, limit as i64], row_to_message)
                    .context("Failed to query restore_session")?;

                let mut msgs = Vec::new();
                for row in rows {
                    msgs.push(row?);
                }
                Ok(msgs)
            }
            None => self.get_messages(session_id),
        }
    }

    /// Full-text search across message content, tool names, and tool calls.
    #[instrument(
        skip(self),
        fields(
            query = %query,
            limit = limit,
        )
    )]
    fn search_messages(&self, query: &str, limit: i64) -> Result<Vec<SearchResult>> {
        debug!("Starting FTS5 search");
        let start = std::time::Instant::now();

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

        let elapsed = start.elapsed();
        debug!(
            results_count = results.len(),
            duration_ms = elapsed.as_millis(),
            "FTS5 search completed"
        );

        if elapsed.as_millis() > 500 {
            warn!(
                query = %query,
                duration_ms = elapsed.as_millis(),
                "Slow FTS5 query detected"
            );
        }

        Ok(results)
    }

    /// Delete a single message by ID within a session.
    /// Returns true if the message was deleted, false if not found.
    fn delete_message(&self, session_id: &str, message_id: &str) -> Result<bool> {
        let conn = self.conn().lock().unwrap();

        // Ensure the message exists and belongs to the specified session
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND id = ?2",
                params![session_id, message_id],
                |row| row.get(0),
            )
            .context("Failed to check message existence")?;

        if count == 0 {
            return Ok(false);
        }

        // Delete the message
        conn.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND id = ?2",
            params![session_id, message_id],
        )
        .context("Failed to delete message")?;

        // Decrement message_count on the parent session
        conn.execute(
            "UPDATE sessions SET message_count = message_count - 1 WHERE id = ?1",
            params![session_id],
        )
        .context("Failed to decrement message_count")?;

        debug!("Deleted message {message_id} from session {session_id}");
        Ok(true)
    }

    #[instrument(
        skip(self),
        fields(
            session_id = %session_id,
            anchor_id = anchor_id,
            window = window,
        )
    )]
    fn get_messages_around(
        &self,
        session_id: &str,
        anchor_id: i64,
        window: i64,
    ) -> Result<(Vec<Message>, i64, i64)> {
        debug!("Starting get_messages_around");
        let conn = self.conn().lock().unwrap();

        // First, verify the anchor exists and belongs to this session
        let anchor_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE session_id = ?1 AND id = ?2)",
                params![session_id, anchor_id],
                |row| row.get(0),
            )
            .context("Failed to check anchor message")?;

        if !anchor_exists {
            anyhow::bail!(
                "Anchor message {} not found in session {}",
                anchor_id,
                session_id
            );
        }

        // Count messages before and after the anchor
        let messages_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND id < ?2",
                params![session_id, anchor_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let messages_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND id > ?2",
                params![session_id, anchor_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Fetch window: anchor ± window messages
        let lower_bound = anchor_id - window;
        let upper_bound = anchor_id + window;

        let mut stmt = conn.prepare(
            "SELECT role, content, tool_call_id, tool_calls, tool_name, timestamp, token_count, finish_reason, reasoning
             FROM messages
             WHERE session_id = ?1 AND id >= ?2 AND id <= ?3
             ORDER BY id ASC",
        )?;

        let rows = stmt.query_map(params![session_id, lower_bound, upper_bound], |row| {
            row_to_message(row)
        })?;

        let messages: Vec<Message> = rows.collect::<rusqlite::Result<Vec<_>>>()?;

        debug!(
            messages_count = messages.len(),
            messages_before = messages_before,
            messages_after = messages_after,
            "Completed get_messages_around"
        );

        Ok((messages, messages_before, messages_after))
    }

    #[instrument(
        skip(self),
        fields(
            session_id = %session_id,
            count = count,
        )
    )]
    fn get_bookends(&self, session_id: &str, count: i64) -> Result<(Vec<Message>, Vec<Message>)> {
        debug!("Fetching session bookends");
        let conn = self.conn().lock().unwrap();

        // First N user+assistant messages
        let mut start_stmt = conn.prepare(
            "SELECT role, content, tool_call_id, tool_calls, tool_name, timestamp, token_count, finish_reason, reasoning
             FROM messages
             WHERE session_id = ?1 AND role IN ('user', 'assistant')
             ORDER BY id ASC
             LIMIT ?2",
        )?;

        let start_rows =
            start_stmt.query_map(params![session_id, count], |row| row_to_message(row))?;

        let start_messages: Vec<Message> = start_rows.collect::<rusqlite::Result<Vec<_>>>()?;

        // Last N user+assistant messages
        let mut end_stmt = conn.prepare(
            "SELECT role, content, tool_call_id, tool_calls, tool_name, timestamp, token_count, finish_reason, reasoning
             FROM messages
             WHERE session_id = ?1 AND role IN ('user', 'assistant')
             ORDER BY id DESC
             LIMIT ?2",
        )?;

        let end_rows = end_stmt.query_map(params![session_id, count], |row| row_to_message(row))?;

        let mut end_messages: Vec<Message> = end_rows.collect::<rusqlite::Result<Vec<_>>>()?;
        // Reverse to maintain chronological order
        end_messages.reverse();

        debug!(
            start_count = start_messages.len(),
            end_count = end_messages.len(),
            "Bookends retrieved"
        );

        Ok((start_messages, end_messages))
    }
}

fn maybe_set_generated_title(
    conn: &rusqlite::Connection,
    session_id: &str,
    msg: &Message,
) -> Result<()> {
    if msg.content.as_deref().unwrap_or_default().trim().is_empty() {
        return Ok(());
    }

    let existing_title: Option<String> = conn
        .query_row(
            "SELECT title FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()
        .context("Failed to read session title")?
        .flatten();

    if existing_title
        .as_deref()
        .map(str::trim)
        .is_some_and(|title| !title.is_empty())
    {
        return Ok(());
    }

    let title = generate_session_title(std::slice::from_ref(msg));
    if title == "Untitled session" {
        return Ok(());
    }

    let title = unique_generated_title(conn, session_id, &title)?;
    conn.execute(
        "UPDATE sessions SET title = ?2 WHERE id = ?1 AND (title IS NULL OR trim(title) = '')",
        params![session_id, title],
    )
    .context("Failed to set generated session title")?;

    Ok(())
}

fn unique_generated_title(
    conn: &rusqlite::Connection,
    session_id: &str,
    title: &str,
) -> Result<String> {
    let short_id: String = session_id.chars().take(8).collect();
    let mut candidate = title.to_string();
    let mut attempt = 0usize;

    loop {
        let conflict: Option<String> = conn
            .query_row(
                "SELECT id FROM sessions WHERE title = ?1 AND id != ?2 LIMIT 1",
                params![candidate, session_id],
                |row| row.get(0),
            )
            .optional()
            .context("Failed to check generated session title uniqueness")?;

        if conflict.is_none() {
            return Ok(candidate);
        }

        attempt += 1;
        candidate = if attempt == 1 {
            format!("{title} #{short_id}")
        } else {
            format!("{title} #{short_id}-{attempt}")
        };
    }
}

/// Helper to map a rusqlite Row to Message.
fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<Message> {
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

    let tool_calls = tool_calls_json.and_then(|json| serde_json::from_str(&json).ok());

    let timestamp = timestamp_str.and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    });

    Ok(Message {
        role,
        content: row.get(1)?,
        images: None,
        tool_calls,
        tool_call_id: row.get(2)?,
        name: row.get(4)?,
        reasoning: row.get(8)?,
        reasoning_content: None,
        timestamp,
        token_count: token_count.map(|v| v as u32),
        finish_reason: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_db;
    use crate::session_ops::SessionOps;
    use chrono::{TimeZone, Utc};

    /// Helper: create a session and return its ID.
    fn create_test_session(db: &crate::db::SessionDB) -> String {
        db.create_session("test", None, None, None).unwrap()
    }

    #[test]
    fn test_save_and_get_message() {
        let db = test_db();
        let sid = create_test_session(&db);

        let msg = Message::user("Hello, world!");
        db.save_message(&sid, &msg).unwrap();

        let messages = db.get_messages(&sid).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[0].content.as_deref(), Some("Hello, world!"));
    }

    #[test]
    fn test_save_multiple_messages() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::system("You are helpful."))
            .unwrap();
        db.save_message(&sid, &Message::user("Hi")).unwrap();
        db.save_message(&sid, &Message::assistant("Hello!"))
            .unwrap();

        let messages = db.get_messages(&sid).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(messages[1].role, MessageRole::User);
        assert_eq!(messages[2].role, MessageRole::Assistant);
    }

    #[test]
    fn test_save_message_increments_count() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("msg1")).unwrap();
        db.save_message(&sid, &Message::user("msg2")).unwrap();

        let meta = db.get_session(&sid).unwrap().unwrap();
        assert_eq!(meta.message_count, 2);
    }

    #[test]
    fn test_save_first_user_message_generates_title() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("Plan the release checklist"))
            .unwrap();

        let meta = db.get_session(&sid).unwrap().unwrap();
        assert_eq!(meta.title.as_deref(), Some("Plan the release checklist"));
    }

    #[test]
    fn test_save_message_preserves_existing_title() {
        let db = test_db();
        let sid = create_test_session(&db);
        db.set_title(&sid, "Manual title").unwrap();

        db.save_message(&sid, &Message::user("Different generated title"))
            .unwrap();

        let meta = db.get_session(&sid).unwrap().unwrap();
        assert_eq!(meta.title.as_deref(), Some("Manual title"));
    }

    #[test]
    fn test_generated_title_avoids_unique_conflict() {
        let db = test_db();
        let first = create_test_session(&db);
        let second = create_test_session(&db);

        db.save_message(&first, &Message::user("Same opening prompt"))
            .unwrap();
        db.save_message(&second, &Message::user("Same opening prompt"))
            .unwrap();

        let first_title = db.get_session(&first).unwrap().unwrap().title.unwrap();
        let second_title = db.get_session(&second).unwrap().unwrap().title.unwrap();
        assert_eq!(first_title, "Same opening prompt");
        assert!(second_title.starts_with("Same opening prompt #"));
        assert_ne!(first_title, second_title);
    }

    #[test]
    fn test_get_messages_empty_session() {
        let db = test_db();
        let sid = create_test_session(&db);

        let messages = db.get_messages(&sid).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_message_ordering() {
        let db = test_db();
        let sid = create_test_session(&db);

        // Insert messages with explicit timestamps in order
        let mut msg1 = Message::user("first");
        msg1.timestamp = Some(Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap());
        let mut msg2 = Message::assistant("second");
        msg2.timestamp = Some(Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 1).unwrap());
        let mut msg3 = Message::user("third");
        msg3.timestamp = Some(Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 2).unwrap());

        db.save_message(&sid, &msg3).unwrap(); // Insert in reverse order
        db.save_message(&sid, &msg1).unwrap();
        db.save_message(&sid, &msg2).unwrap();

        let messages = db.get_messages(&sid).unwrap();
        assert_eq!(messages.len(), 3);
        // Messages are ordered by id (insertion order), not timestamp
        assert_eq!(messages[0].content.as_deref(), Some("third"));
        assert_eq!(messages[1].content.as_deref(), Some("first"));
        assert_eq!(messages[2].content.as_deref(), Some("second"));
    }

    #[test]
    fn test_save_tool_result_message() {
        let db = test_db();
        let sid = create_test_session(&db);

        let msg = Message::tool_result("call_123", "get_weather", "{\"temp\":72}");
        db.save_message(&sid, &msg).unwrap();

        let messages = db.get_messages(&sid).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::Tool);
        assert_eq!(messages[0].tool_call_id.as_deref(), Some("call_123"));
        assert_eq!(messages[0].name.as_deref(), Some("get_weather"));
        assert_eq!(messages[0].content.as_deref(), Some("{\"temp\":72}"));
    }

    #[test]
    fn test_message_timestamp_preserved() {
        let db = test_db();
        let sid = create_test_session(&db);

        let ts = Utc.with_ymd_and_hms(2025, 6, 15, 12, 30, 0).unwrap();
        let mut msg = Message::user("test");
        msg.timestamp = Some(ts);

        db.save_message(&sid, &msg).unwrap();

        let messages = db.get_messages(&sid).unwrap();
        let recovered = messages[0].timestamp.unwrap();
        // Compare to second precision (RFC3339 stores seconds)
        assert_eq!(
            recovered.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2025-06-15T12:30:00"
        );
    }

    #[test]
    fn test_message_with_reasoning() {
        let db = test_db();
        let sid = create_test_session(&db);

        let mut msg = Message::assistant("The answer is 42.");
        msg.reasoning = Some("Let me think step by step...".to_string());

        db.save_message(&sid, &msg).unwrap();

        let messages = db.get_messages(&sid).unwrap();
        assert_eq!(
            messages[0].reasoning.as_deref(),
            Some("Let me think step by step...")
        );
    }

    // ── FTS5 Search Tests ──────────────────────────────────────────────────

    #[test]
    fn test_fts_search_basic() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("The quick brown fox"))
            .unwrap();
        db.save_message(&sid, &Message::assistant("The lazy dog"))
            .unwrap();
        db.save_message(&sid, &Message::user("Foxes are clever animals"))
            .unwrap();

        let results = db.search_messages("fox", 10).unwrap();
        assert!(!results.is_empty());
        // "fox" appears in "The quick brown fox"
        assert!(
            results
                .iter()
                .any(|r| r.content.as_deref() == Some("The quick brown fox"))
        );
    }

    #[test]
    fn test_fts_search_multiple_keywords() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(
            &sid,
            &Message::user("Rust is a systems programming language"),
        )
        .unwrap();
        db.save_message(&sid, &Message::assistant("Python is great for scripting"))
            .unwrap();
        db.save_message(&sid, &Message::user("I love Rust programming"))
            .unwrap();

        let results = db.search_messages("Rust", 10).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.session_id == sid));
    }

    #[test]
    fn test_fts_search_no_results() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("Hello world"))
            .unwrap();

        let results = db.search_messages("nonexistent", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fts_search_limit() {
        let db = test_db();
        let sid = create_test_session(&db);

        for i in 0..5 {
            db.save_message(&sid, &Message::user(format!("test message number {i}")))
                .unwrap();
        }

        let results = db.search_messages("test", 2).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_fts_search_across_sessions() {
        let db = test_db();
        let sid1 = create_test_session(&db);
        let sid2 = create_test_session(&db);

        db.save_message(&sid1, &Message::user("Rust is amazing"))
            .unwrap();
        db.save_message(&sid2, &Message::user("Rust memory safety"))
            .unwrap();

        let results = db.search_messages("Rust", 10).unwrap();
        assert_eq!(results.len(), 2);
        let session_ids: Vec<&str> = results.iter().map(|r| r.session_id.as_str()).collect();
        assert!(session_ids.contains(&sid1.as_str()));
        assert!(session_ids.contains(&sid2.as_str()));
    }

    #[test]
    fn test_fts_search_phrase() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("machine learning algorithms"))
            .unwrap();
        db.save_message(&sid, &Message::user("deep learning models"))
            .unwrap();
        db.save_message(&sid, &Message::user("learning to code"))
            .unwrap();

        // FTS5 phrase search with double quotes
        let results = db.search_messages("\"deep learning\"", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content.as_deref(), Some("deep learning models"));
    }

    #[test]
    fn test_fts_search_has_rank() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("rust programming language"))
            .unwrap();

        let results = db.search_messages("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        // Rank is a negative number (closer to 0 = better)
        assert!(results[0].rank < 0.0);
    }

    #[test]
    fn test_fts_search_message_id_is_correct() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("alpha")).unwrap();
        db.save_message(&sid, &Message::user("beta")).unwrap();
        db.save_message(&sid, &Message::user("alpha gamma"))
            .unwrap();

        let results = db.search_messages("alpha", 10).unwrap();
        assert_eq!(results.len(), 2);
        // Each result should have a valid message_id (positive)
        for r in &results {
            assert!(r.message_id > 0);
        }
    }

    #[test]
    fn test_fts_search_deleted_message() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("findable keyword"))
            .unwrap();

        let results = db.search_messages("findable", 10).unwrap();
        assert_eq!(results.len(), 1);

        // Delete the message
        {
            let conn = db.conn().lock().unwrap();
            conn.execute(
                "DELETE FROM messages WHERE content = ?1",
                rusqlite::params!["findable keyword"],
            )
            .unwrap();
        }

        // After deletion, FTS should also reflect the removal (via trigger)
        let results = db.search_messages("findable", 10).unwrap();
        assert!(results.is_empty());
    }

    // ── Restore Session Tests ────────────────────────────────────────────

    #[test]
    fn test_restore_session_with_limit() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("msg1")).unwrap();
        db.save_message(&sid, &Message::assistant("reply1"))
            .unwrap();
        db.save_message(&sid, &Message::user("msg2")).unwrap();
        db.save_message(&sid, &Message::assistant("reply2"))
            .unwrap();
        db.save_message(&sid, &Message::user("msg3")).unwrap();

        let messages = db.restore_session(&sid, Some(3)).unwrap();
        assert_eq!(messages.len(), 3);
        // Should be the last 3 messages, in original order
        assert_eq!(messages[0].content.as_deref(), Some("msg2"));
        assert_eq!(messages[1].content.as_deref(), Some("reply2"));
        assert_eq!(messages[2].content.as_deref(), Some("msg3"));
    }

    #[test]
    fn test_restore_session_no_limit() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("msg1")).unwrap();
        db.save_message(&sid, &Message::assistant("reply1"))
            .unwrap();
        db.save_message(&sid, &Message::user("msg2")).unwrap();

        let messages = db.restore_session(&sid, None).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content.as_deref(), Some("msg1"));
        assert_eq!(messages[1].content.as_deref(), Some("reply1"));
        assert_eq!(messages[2].content.as_deref(), Some("msg2"));
    }

    // ── Around & Bookends Tests ──────────────────────────────────────────

    #[test]
    fn test_get_messages_around() {
        let db = test_db();
        let sid = create_test_session(&db);

        // 保存 10 条消息
        for i in 1..=10 {
            db.save_message(&sid, &Message::user(format!("msg {i}")))
                .unwrap();
        }

        // 获取消息 5 前后 2 条（应返回 3-7）
        let (window, before, after) = db.get_messages_around(&sid, 5, 2).unwrap();

        assert_eq!(before, 4); // 消息 1-4 在前面
        assert_eq!(after, 5); // 消息 6-10 在后面
        assert_eq!(window.len(), 5); // 窗口包含 3-7
    }

    #[test]
    fn test_get_messages_around_at_boundaries() {
        let db = test_db();
        let sid = create_test_session(&db);

        for i in 1..=5 {
            db.save_message(&sid, &Message::user(format!("msg {i}")))
                .unwrap();
        }

        // anchor 在开头
        let (_, before, _) = db.get_messages_around(&sid, 1, 2).unwrap();
        assert_eq!(before, 0);

        // anchor 在末尾
        let (_, _, after) = db.get_messages_around(&sid, 5, 2).unwrap();
        assert_eq!(after, 0);
    }

    #[test]
    fn test_get_messages_around_invalid_anchor() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("msg 1")).unwrap();

        // 不存在的 anchor
        let result = db.get_messages_around(&sid, 999, 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_get_bookends() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("Q1")).unwrap();
        db.save_message(&sid, &Message::assistant("A1")).unwrap();
        db.save_message(
            &sid,
            &Message::tool_result("call_1", "test_tool", "tool output"),
        )
        .unwrap(); // 应被跳过
        db.save_message(&sid, &Message::user("Q2")).unwrap();
        db.save_message(&sid, &Message::assistant("A2")).unwrap();
        db.save_message(&sid, &Message::user("Q3")).unwrap();
        db.save_message(&sid, &Message::assistant("A3")).unwrap();

        let (start, end) = db.get_bookends(&sid, 2).unwrap();

        // 前 2 条 user+assistant
        assert_eq!(start.len(), 2);
        assert_eq!(start[0].content.as_deref(), Some("Q1"));
        assert_eq!(start[1].content.as_deref(), Some("A1"));

        // 后 2 条 user+assistant（倒序后）
        assert_eq!(end.len(), 2);
        assert_eq!(end[0].content.as_deref(), Some("Q3"));
        assert_eq!(end[1].content.as_deref(), Some("A3"));
    }

    #[test]
    fn test_get_bookends_fewer_than_requested() {
        let db = test_db();
        let sid = create_test_session(&db);

        db.save_message(&sid, &Message::user("Q1")).unwrap();
        db.save_message(&sid, &Message::assistant("A1")).unwrap();

        // 请求 5 条，但只有 2 条
        let (start, end) = db.get_bookends(&sid, 5).unwrap();

        assert_eq!(start.len(), 2);
        assert_eq!(end.len(), 2);
    }

    #[test]
    fn test_get_bookends_empty_session() {
        let db = test_db();
        let sid = create_test_session(&db);

        let (start, end) = db.get_bookends(&sid, 3).unwrap();

        assert!(start.is_empty());
        assert!(end.is_empty());
    }
}
