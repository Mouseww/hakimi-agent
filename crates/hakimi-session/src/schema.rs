//! SQL schema definitions for the session store.

/// Core tables: sessions and messages.
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    source TEXT,
    user_id TEXT,
    model TEXT,
    system_prompt TEXT,
    parent_session_id TEXT,
    started_at TEXT,
    ended_at TEXT,
    end_reason TEXT,
    message_count INTEGER DEFAULT 0,
    tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    title TEXT,
    api_call_count INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT REFERENCES sessions(id),
    role TEXT NOT NULL,
    content TEXT,
    tool_call_id TEXT,
    tool_calls TEXT,
    tool_name TEXT,
    timestamp TEXT,
    token_count INTEGER,
    finish_reason TEXT,
    reasoning TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source);
CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, timestamp);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_title_unique
    ON sessions(title) WHERE title IS NOT NULL;
"#;

/// FTS5 virtual table and triggers for full-text message search.
pub const FTS_SQL: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    tool_name,
    tool_calls,
    content=messages,
    content_rowid=id
);

CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content, tool_name, tool_calls) VALUES (
        new.id,
        COALESCE(new.content, ''),
        COALESCE(new.tool_name, ''),
        COALESCE(new.tool_calls, '')
    );
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content, tool_name, tool_calls)
    VALUES ('delete', old.id,
        COALESCE(old.content, ''),
        COALESCE(old.tool_name, ''),
        COALESCE(old.tool_calls, '')
    );
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content, tool_name, tool_calls)
    VALUES ('delete', old.id,
        COALESCE(old.content, ''),
        COALESCE(old.tool_name, ''),
        COALESCE(old.tool_calls, '')
    );
    INSERT INTO messages_fts(rowid, content, tool_name, tool_calls) VALUES (
        new.id,
        COALESCE(new.content, ''),
        COALESCE(new.tool_name, ''),
        COALESCE(new.tool_calls, '')
    );
END;
"#;
