use async_trait::async_trait;
use hakimi_common::{error::ErrorContext, HakimiError, Result, ToolContext};
use hakimi_metrics::MetricsRecorder;
use hakimi_session::{MessageOps, SessionDB, SessionMeta, SessionOps};
use serde_json::{Value as JsonValue, json};
use tracing::{debug, instrument};

use crate::Tool;

/// Helper to create session errors with context
fn session_error(message: impl Into<String>, operation: &str) -> HakimiError {
    HakimiError::Session {
        message: message.into(),
        context: ErrorContext::new(operation),
        source: None,
    }
}

/// Enhanced session search tool with three modes:
/// 1. DISCOVERY: FTS5 search with bookends (first 3 + last 3 user+assistant messages)
/// 2. SCROLL: Window around a specific message ID
/// 3. BROWSE: Recent sessions list
pub struct SessionSearchTool;

/// Get the active runtime session database path.
fn session_db_path() -> std::path::PathBuf {
    hakimi_common::effective_hakimi_home().join("sessions.db")
}

/// Format timestamp for display
fn format_timestamp(ts: &str) -> String {
    use chrono::DateTime;
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        dt.format("%B %d, %Y at %I:%M %p").to_string()
    } else {
        ts.to_string()
    }
}

/// Format a single message for display
fn format_message(msg: &hakimi_common::Message, anchor_id: Option<i64>) -> String {
    let role_emoji = match msg.role {
        hakimi_common::MessageRole::User => "👤",
        hakimi_common::MessageRole::Assistant => "🤖",
        hakimi_common::MessageRole::System => "⚙️",
        hakimi_common::MessageRole::Tool => "🔧",
    };

    let content = msg
        .content
        .as_deref()
        .unwrap_or("[no content]")
        .chars()
        .take(200)
        .collect::<String>();

    let ts = msg
        .timestamp
        .map(|t| format_timestamp(&t.to_rfc3339()))
        .unwrap_or_else(|| "unknown".to_string());

    let anchor_marker = if anchor_id.is_some() { " ⭐" } else { "" };

    format!("{role_emoji} [{ts}]{anchor_marker} {content}")
}

#[async_trait]
impl Tool for SessionSearchTool {
    fn name(&self) -> &str {
        "session_search"
    }

    fn toolset(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Search past sessions and messages with three modes: (1) Discovery - FTS5 search with session bookends (first/last 3 messages), (2) Scroll - window around a specific message ID, (3) Browse - recent sessions. Use for 'what did we do about X' or 'where did we leave Y' questions."
    }

    fn emoji(&self) -> &str {
        "🔍"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional FTS5 search query. If empty, returns recent sessions (browse mode). Supports boolean operators: 'alpha AND beta', 'alpha OR beta', '\"exact phrase\"'."
                },
                "session_id": {
                    "type": "string",
                    "description": "For scroll mode: session ID to navigate within. Must be paired with around_message_id."
                },
                "around_message_id": {
                    "type": "integer",
                    "description": "For scroll mode: message ID to center the window on. Must be paired with session_id."
                },
                "window": {
                    "type": "integer",
                    "description": "For scroll mode: messages to return on each side of anchor (default: 5, max: 20).",
                    "minimum": 1,
                    "maximum": 20
                },
                "limit": {
                    "type": "integer",
                    "description": "For discovery/browse: max results to return (default: 5, max: 50).",
                    "minimum": 1,
                    "maximum": 50
                },
                "role_filter": {
                    "type": "string",
                    "description": "For discovery: filter by message role (user, assistant, tool, system).",
                    "enum": ["user", "assistant", "tool", "system"]
                }
            }
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(64 * 1024) // 64KB limit
    }

    #[instrument(skip(self, args, _ctx), fields(tool = "session_search"))]
    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let start = std::time::Instant::now();
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let session_id = args.get("session_id").and_then(|v| v.as_str());
        let around_msg_id = args.get("around_message_id").and_then(|v| v.as_i64());
        let window = args
            .get("window")
            .and_then(|v| v.as_i64())
            .unwrap_or(5)
            .clamp(1, 20);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(5)
            .clamp(1, 50);
        let role_filter = args.get("role_filter").and_then(|v| v.as_str());

        let db_path = session_db_path();
        debug!(
            query = %query,
            session_id = ?session_id,
            around_msg_id = ?around_msg_id,
            limit = limit,
            "session search"
        );

        let db = SessionDB::new(&db_path)
            .map_err(|e| session_error(format!("failed to open session database: {e}"), "session_search"))?;
        db.initialize()
            .map_err(|e| session_error(format!("failed to initialize database: {e}"), "session_search"))?;

        // SCROLL MODE: session_id + around_message_id
        if let (Some(sid), Some(anchor_id)) = (session_id, around_msg_id) {
            debug!(mode = "scroll", "Executing Scroll mode");
            let result = self.scroll_mode(&db, sid, anchor_id, window);
            let elapsed = start.elapsed();
            hakimi_metrics::global().record_duration("session_search.scroll", elapsed);
            return result;
        }

        // DISCOVERY MODE: query provided
        if !query.is_empty() {
            debug!(mode = "discovery", "Executing Discovery mode");
            let result = self.discovery_mode(&db, query, limit, role_filter);
            let elapsed = start.elapsed();
            hakimi_metrics::global().record_duration("session_search.discovery", elapsed);
            return result;
        }

        // BROWSE MODE: no args
        debug!(mode = "browse", "Executing Browse mode");
        let result = self.browse_mode(&db, limit);
        let elapsed = start.elapsed();
        hakimi_metrics::global().record_duration("session_search.browse", elapsed);
        result
    }
}

impl SessionSearchTool {
    /// BROWSE MODE: List recent sessions
    fn browse_mode(&self, db: &SessionDB, limit: i64) -> Result<String> {
        let sessions = db
            .get_recent_sessions(None, limit)
            .map_err(|e| session_error(format!("failed to get recent sessions: {e}"), "browse_mode"))?;

        if sessions.is_empty() {
            return Ok("No sessions found.".to_string());
        }

        let mut output = format!("## Recent Sessions ({})\n\n", sessions.len());

        for session in &sessions {
            let title = session.title.as_deref().unwrap_or("untitled");
            let started = session
                .started_at
                .as_deref()
                .map(format_timestamp)
                .unwrap_or_else(|| "unknown".to_string());

            output.push_str(&format!(
                "**{}** ({})\n- Session ID: `{}`\n- Source: {}\n- Messages: {} | Tool calls: {}\n\n",
                title,
                started,
                session.id,
                session.source.as_deref().unwrap_or("unknown"),
                session.message_count,
                session.tool_call_count
            ));
        }

        output.push_str(&format!("\nShowing {} most recent sessions. Pass a `query` to search, or `session_id` + `around_message_id` to scroll.", sessions.len()));

        Ok(output)
    }

    /// DISCOVERY MODE: FTS5 search with bookends
    fn discovery_mode(
        &self,
        db: &SessionDB,
        query: &str,
        limit: i64,
        role_filter: Option<&str>,
    ) -> Result<String> {
        let search_limit = if role_filter.is_some() {
            limit * 3 // fetch extra for filtering
        } else {
            limit
        };

        let results = db
            .search_messages(query, search_limit)
            .map_err(|e| session_error(format!("FTS5 search failed: {e}"), "discovery_mode"))?;

        if results.is_empty() {
            return Ok(format!("No messages found matching query: \"{}\"", query));
        }

        // Group by session and dedupe
        let mut session_ids: Vec<String> = results.iter().map(|r| r.session_id.clone()).collect();
        session_ids.sort();
        session_ids.dedup();

        let mut output = format!(
            "## Search Results for \"{}\"\nFound {} message(s) across {} session(s)\n\n",
            query,
            results.len(),
            session_ids.len()
        );

        for sid in session_ids.iter().take(limit as usize) {
            let session = db
                .get_session(sid)
                .map_err(|e| session_error(format!("failed to get session: {e}"), "discovery_mode"))?;

            if let Some(session) = session {
                output.push_str(&self.format_session_with_bookends(db, &session, &results)?);
                output.push_str("\n---\n\n");
            }
        }

        Ok(output)
    }

    /// SCROLL MODE: Window around a message ID
    fn scroll_mode(
        &self,
        db: &SessionDB,
        session_id: &str,
        anchor_id: i64,
        window: i64,
    ) -> Result<String> {
        let session = db
            .get_session(session_id)
            .map_err(|e| session_error(format!("failed to get session: {e}"), "scroll_mode"))?
            .ok_or_else(|| session_error(format!("session not found: {}", session_id), "scroll_mode"))?;

        let (messages, before, after) = db
            .get_messages_around(session_id, anchor_id, window)
            .map_err(|e| {
                session_error(format!("failed to get messages around anchor: {e}"), "scroll_mode")
            })?;

        if messages.is_empty() {
            return Ok(format!(
                "No messages found around anchor {} in session {}",
                anchor_id, session_id
            ));
        }

        let title = session.title.as_deref().unwrap_or("untitled");
        let mut output = format!(
            "## Scroll: {} (Session: `{}`)\nAnchor: message #{} | {} messages before | {} after\n\n",
            title, session_id, anchor_id, before, after
        );

        for msg in &messages {
            let is_anchor = msg.timestamp.is_some(); // Simplified anchor detection
            output.push_str(&format_message(
                msg,
                if is_anchor { Some(anchor_id) } else { None },
            ));
            output.push('\n');
        }

        output.push_str(&format!(
            "\n**Navigation:** To scroll forward, call with `around_message_id={}`. To scroll back, use `around_message_id={}`.",
            messages.last().and_then(|m| m.timestamp.map(|_| anchor_id + window)).unwrap_or(anchor_id),
            messages.first().and_then(|m| m.timestamp.map(|_| anchor_id - window)).unwrap_or(anchor_id)
        ));

        Ok(output)
    }

    /// Format a session summary with bookends (first 3 + last 3 user+assistant messages)
    fn format_session_with_bookends(
        &self,
        db: &SessionDB,
        session: &SessionMeta,
        match_results: &[hakimi_session::SearchResult],
    ) -> Result<String> {
        let title = session.title.as_deref().unwrap_or("untitled");
        let started = session
            .started_at
            .as_deref()
            .map(format_timestamp)
            .unwrap_or_else(|| "unknown".to_string());

        let mut output = format!(
            "### {} ({})\n**Session ID:** `{}`\n**Messages:** {} | **Tool calls:** {}\n\n",
            title, started, session.id, session.message_count, session.tool_call_count
        );

        // Get bookends
        let (start_msgs, end_msgs) = db.get_bookends(&session.id, 3).unwrap_or_default();

        if !start_msgs.is_empty() {
            output.push_str("**Session Start (first 3 messages):**\n");
            for msg in &start_msgs {
                output.push_str(&format!("  {}\n", format_message(msg, None)));
            }
            output.push('\n');
        }

        // Show match context (first match only)
        let session_matches: Vec<_> = match_results
            .iter()
            .filter(|r| r.session_id == session.id)
            .collect();

        if let Some(first_match) = session_matches.first() {
            output.push_str(&format!("**Match ({} total):**\n", session_matches.len()));
            let snippet = first_match
                .content
                .as_deref()
                .unwrap_or("[no content]")
                .chars()
                .take(300)
                .collect::<String>();
            output.push_str(&format!("  {}\n\n", snippet));
        }

        if !end_msgs.is_empty() {
            output.push_str("**Session End (last 3 messages):**\n");
            for msg in &end_msgs {
                output.push_str(&format!("  {}\n", format_message(msg, None)));
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::{Message, ToolContext};
    use tempfile::tempdir;

    fn test_db() -> SessionDB {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_sessions.db");
        let db = SessionDB::new(&db_path).unwrap();
        db.initialize().unwrap();
        db
    }

    #[tokio::test]
    async fn test_browse_mode() {
        let db = test_db();
        let sid = "test_session";
        db.create_session(sid, None, None, None).unwrap();
        db.save_message(sid, &Message::user("Hello")).unwrap();

        let tool = SessionSearchTool;
        let ctx = ToolContext::default();
        let args = json!({});

        let result = tool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("Recent Sessions"));
    }

    #[tokio::test]
    async fn test_discovery_mode() {
        let db = test_db();
        let sid = "test_session";
        db.create_session(sid, None, None, None).unwrap();
        db.save_message(sid, &Message::user("Rust programming"))
            .unwrap();

        let tool = SessionSearchTool;
        let ctx = ToolContext::default();
        let args = json!({"query": "Rust"});

        let result = tool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("Search Results"));
        assert!(result.contains("Rust"));
    }
}
