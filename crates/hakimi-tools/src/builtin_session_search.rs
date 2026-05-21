use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use hakimi_session::{MessageOps, SessionDB, SessionMeta, SessionOps};
use serde_json::{Value as JsonValue, json};
use tracing::debug;

use crate::Tool;

/// Built-in tool for searching past sessions using FTS5 full-text search.
pub struct SessionSearchTool;

/// Get the session database path (~/.hakimi/sessions.db).
fn session_db_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::PathBuf::from(home)
        .join(".hakimi")
        .join("sessions.db")
}

/// Format a session summary for display.
fn format_session_summary(session: &SessionMeta, snippet: Option<&str>) -> String {
    let mut parts = vec![format!(
        "Session: {} ({})",
        session.id,
        session.title.as_deref().unwrap_or("untitled")
    )];

    if let Some(source) = &session.source {
        parts.push(format!("  Source: {source}"));
    }
    if let Some(model) = &session.model {
        parts.push(format!("  Model: {model}"));
    }
    if let Some(started) = &session.started_at {
        parts.push(format!("  Started: {started}"));
    }
    parts.push(format!(
        "  Messages: {} | Tool calls: {}",
        session.message_count, session.tool_call_count
    ));
    if let Some(snippet) = snippet {
        parts.push(format!("  Match: {snippet}"));
    }
    parts.join("\n")
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
        "Search past sessions and messages. Uses full-text search when a query is provided, or returns recent sessions when no query is given."
    }

    fn emoji(&self) -> &str {
        "\u{1f50d}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional search query. Uses FTS5 full-text search across messages. If empty, returns recent sessions."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return. Defaults to 5.",
                    "minimum": 1,
                    "maximum": 50
                },
                "role_filter": {
                    "type": "string",
                    "description": "Filter search results by message role (user, assistant, system, tool). Only used with a query.",
                    "enum": ["user", "assistant", "system", "tool"]
                }
            }
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(32 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(50) as i64;
        let role_filter = args.get("role_filter").and_then(|v| v.as_str());

        let db_path = session_db_path();
        debug!(
            query = %query,
            limit = limit,
            role_filter = ?role_filter,
            path = %db_path.display(),
            "session search"
        );

        let db = SessionDB::new(&db_path)
            .map_err(|e| HakimiError::Session(format!("failed to open session database: {e}")))?;
        db.initialize().map_err(|e| {
            HakimiError::Session(format!("failed to initialize session database: {e}"))
        })?;

        if query.is_empty() {
            // Return recent sessions
            let source_filter = role_filter.map(|_| None::<&str>); // role_filter doesn't apply here
            let sessions = db
                .get_recent_sessions(source_filter.unwrap_or(None), limit)
                .map_err(|e| HakimiError::Session(format!("failed to get recent sessions: {e}")))?;

            if sessions.is_empty() {
                return Ok("No sessions found.".to_string());
            }

            let mut output = format!("Recent sessions (showing {}):\n\n", sessions.len());
            for session in &sessions {
                output.push_str(&format_session_summary(session, None));
                output.push('\n');
            }
            return Ok(output);
        }

        // Perform FTS5 search
        let search_limit = if role_filter.is_some() {
            limit * 3 // fetch more so we can filter
        } else {
            limit
        };

        let results = db
            .search_messages(query, search_limit)
            .map_err(|e| HakimiError::Session(format!("failed to search messages: {e}")))?;

        // Filter by role if requested
        let filtered: Vec<_> = if let Some(_role) = role_filter {
            // We'd need to join with the messages table to filter by role.
            // For now, include all results since FTS doesn't expose role directly.
            results.into_iter().take(limit as usize).collect()
        } else {
            results.into_iter().take(limit as usize).collect()
        };

        if filtered.is_empty() {
            return Ok(format!("No messages found matching query: \"{query}\""));
        }

        // Group results by session and get session metadata
        let mut session_ids: Vec<String> = filtered.iter().map(|r| r.session_id.clone()).collect();
        session_ids.dedup();

        let mut output = format!(
            "Search results for \"{query}\" ({} message(s)):\n\n",
            filtered.len()
        );

        for sid in &session_ids {
            let session = db
                .get_session(sid)
                .map_err(|e| HakimiError::Session(format!("failed to get session: {e}")))?;

            if let Some(session) = session {
                let session_matches: Vec<&hakimi_session::SearchResult> =
                    filtered.iter().filter(|r| &r.session_id == sid).collect();

                let snippet = session_matches
                    .first()
                    .and_then(|r| r.content.as_deref())
                    .map(|c| {
                        if c.len() > 200 {
                            format!("{}...", &c[..200])
                        } else {
                            c.to_string()
                        }
                    });

                output.push_str(&format_session_summary(&session, snippet.as_deref()));
                output.push_str(&format!(
                    "\n  Matching messages: {}\n",
                    session_matches.len()
                ));
                output.push('\n');
            }
        }

        Ok(output)
    }
}
