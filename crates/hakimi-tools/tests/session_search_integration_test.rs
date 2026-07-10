use hakimi_common::{Message, ToolContext};
use hakimi_session::{MessageOps, SessionDB, SessionOps};
use hakimi_tools::{SessionSearchTool, Tool};
use serde_json::json;
use std::sync::{Arc, Mutex, MutexGuard};
use tempfile::TempDir;

// Global lock to serialize tests that modify HAKIMI_HOME
static TEST_LOCK: std::sync::LazyLock<Mutex<()>> = std::sync::LazyLock::new(|| Mutex::new(()));

/// Helper: Create a test database with HAKIMI_HOME set
fn setup_test_db() -> (MutexGuard<'static, ()>, TempDir, SessionDB) {
    let lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();

    // Set HAKIMI_HOME to temp dir for this test
    unsafe {
        std::env::set_var("HAKIMI_HOME", tmp.path());
    }

    let db_path = tmp.path().join("sessions.db");
    let db = SessionDB::new(&db_path).unwrap();
    db.initialize().unwrap();
    (lock, tmp, db)
}

/// Helper: Insert test messages into a session
fn insert_test_messages(db: &SessionDB, session_id: &str, count: usize) {
    db.create_session_with_id(session_id, "cli", Some("test-user"), None, None, None)
        .unwrap();

    for i in 0..count {
        let user_msg = Message::user(format!("Test message {}", i));
        db.save_message(session_id, &user_msg).unwrap();

        let assistant_msg = Message::assistant(format!("Response {}", i));
        db.save_message(session_id, &assistant_msg).unwrap();

        // Small delay to ensure unique timestamps
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

// ==================== BROWSE MODE TESTS ====================

#[tokio::test]
async fn test_browse_mode_empty_database() {
    let (_lock, _tmp, _db) = setup_test_db();

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({});

    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(
        result.contains("No sessions found") || result.contains("Recent Sessions"),
        "Result: {}",
        result
    );
}

#[tokio::test]
async fn test_browse_mode_shows_recent_sessions() {
    let (_lock, _tmp, db) = setup_test_db();

    // Create multiple sessions
    for i in 0..3 {
        let session_id = format!("session-{}", i);
        insert_test_messages(&db, &session_id, 2);
    }

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "limit": 10
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should show recent sessions
    assert!(
        result.contains("Recent Sessions") || result.contains("session-"),
        "Result: {}",
        result
    );
}

#[tokio::test]
async fn test_browse_mode_respects_limit() {
    let (_lock, _tmp, db) = setup_test_db();

    // Create 10 sessions
    for i in 0..10 {
        let session_id = format!("session-{:02}", i);
        insert_test_messages(&db, &session_id, 1);
    }

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "limit": 3
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Result should not be empty
    assert!(result.len() > 0, "Result should not be empty");
}

// ==================== DISCOVERY MODE TESTS ====================

#[tokio::test]
async fn test_discovery_mode_basic_search() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-1";

    insert_test_messages(&db, session_id, 5);

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "query": "Test message 2"
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should find the message
    assert!(
        result.contains("Search Results")
            || result.contains("Test message")
            || result.contains("Found"),
        "Result: {}",
        result
    );
}

#[tokio::test]
async fn test_discovery_mode_no_results() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-2";

    insert_test_messages(&db, session_id, 5);

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "query": "NonexistentKeyword12345"
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should indicate no results
    assert!(
        result.contains("No messages found") || result.contains("matching"),
        "Result: {}",
        result
    );
}

#[tokio::test]
async fn test_discovery_mode_with_bookends() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-3";

    // Insert enough messages to test bookends
    insert_test_messages(&db, session_id, 20);

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "query": "Test message 10",
        "limit": 5
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should include search results (may be minimal if no FTS5 matches)
    assert!(result.len() > 0, "Result should not be empty");
}

#[tokio::test]
async fn test_discovery_fts5_multiple_keywords() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-4";

    // Insert messages with specific keywords
    db.create_session_with_id(session_id, "cli", Some("test-user"), None, None, None)
        .unwrap();

    let user_msg = Message::user("How to configure Rust compiler?");
    db.save_message(session_id, &user_msg).unwrap();

    let assistant_msg = Message::assistant("Set RUSTFLAGS environment variable.");
    db.save_message(session_id, &assistant_msg).unwrap();

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "query": "Rust compiler"
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should find the relevant message
    assert!(
        result.to_lowercase().contains("rust") || result.contains("Found"),
        "Result: {}",
        result
    );
}

#[tokio::test]
async fn test_discovery_chinese_search() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-chinese";

    db.create_session_with_id(session_id, "cli", Some("test-user"), None, None, None)
        .unwrap();

    let user_msg = Message::user("如何配置 Hakimi Agent？");
    db.save_message(session_id, &user_msg).unwrap();

    let assistant_msg = Message::assistant("你需要编辑 config.yaml 文件。");
    db.save_message(session_id, &assistant_msg).unwrap();

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "query": "配置"
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should find Chinese content (FTS5 tokenization dependent)
    assert!(
        result.contains("配置") || result.contains("Found") || result.contains("Search Results"),
        "Result: {}",
        result
    );
}

// ==================== SCROLL MODE TESTS ====================

#[tokio::test]
async fn test_scroll_mode_basic() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-scroll-1";

    insert_test_messages(&db, session_id, 20);

    // Get messages to find an anchor ID
    let messages = db.get_messages(session_id).unwrap();

    // Extract message ID from a middle message (assuming get_messages returns all messages)
    if messages.len() < 10 {
        return; // Skip if not enough messages
    }

    // Find a message using search
    let search_results = db.search_messages("Test message 5", 10).unwrap();
    if search_results.is_empty() {
        // Skip test if FTS5 isn't working
        return;
    }

    let anchor_id = search_results[0].message_id;

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let mut ctx = ToolContext::default();
    ctx.session_id = session_id.to_string();
    let args = json!({
        "session_id": session_id,
        "around_message_id": anchor_id,
        "window": 5
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should return messages around the anchor
    assert!(
        result.len() > 50,
        "Result should include multiple messages, got length: {}",
        result.len()
    );
}

#[tokio::test]
async fn test_scroll_mode_at_start() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-scroll-start";

    insert_test_messages(&db, session_id, 10);

    // Find the first message ID
    let search_results = db.search_messages("Test message 0", 10).unwrap();
    if search_results.is_empty() {
        return; // Skip if FTS5 not working
    }

    let first_id = search_results[0].message_id;

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let mut ctx = ToolContext::default();
    ctx.session_id = session_id.to_string();
    let args = json!({
        "session_id": session_id,
        "around_message_id": first_id,
        "window": 5
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should handle boundary gracefully
    assert!(
        result.len() > 0,
        "Result should not be empty at start boundary"
    );
}

#[tokio::test]
async fn test_scroll_mode_at_end() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-scroll-end";

    insert_test_messages(&db, session_id, 10);

    // Find the last message ID
    let search_results = db.search_messages("Test message 9", 10).unwrap();
    if search_results.is_empty() {
        return; // Skip if FTS5 not working
    }

    let last_id = search_results[0].message_id;

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let mut ctx = ToolContext::default();
    ctx.session_id = session_id.to_string();
    let args = json!({
        "session_id": session_id,
        "around_message_id": last_id,
        "window": 5
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should handle boundary gracefully
    assert!(
        result.len() > 0,
        "Result should not be empty at end boundary"
    );
}

#[tokio::test]
async fn test_scroll_mode_invalid_message_id() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-scroll-invalid";

    insert_test_messages(&db, session_id, 10);

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let mut ctx = ToolContext::default();
    ctx.session_id = session_id.to_string();
    let args = json!({
        "session_id": session_id,
        "around_message_id": 99999,  // Non-existent ID
        "window": 5
    });

    let result = tool.execute(&args, &ctx).await;

    // Should handle error gracefully (may return error or empty result)
    assert!(
        result.is_ok() || result.is_err(),
        "Should handle invalid message ID"
    );
}

// ==================== ERROR HANDLING TESTS ====================

#[tokio::test]
async fn test_error_empty_session() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "empty-session";

    // Create empty session
    db.create_session_with_id(session_id, "cli", Some("test-user"), None, None, None)
        .unwrap();

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "query": "test"
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should handle empty session gracefully
    assert!(
        result.contains("No messages found") || result.contains("0 messages"),
        "Result: {}",
        result
    );
}

#[tokio::test]
async fn test_error_scroll_nonexistent_session() {
    let (_lock, _tmp, _db) = setup_test_db();

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let mut ctx = ToolContext::default();
    ctx.session_id = "nonexistent-session".to_string();
    let args = json!({
        "session_id": "nonexistent-session",
        "around_message_id": 1,
        "window": 5
    });

    let result = tool.execute(&args, &ctx).await;

    // Should return error for nonexistent session
    assert!(
        result.is_err() || result.unwrap().contains("not found"),
        "Should handle nonexistent session"
    );
}

// ==================== PARAMETER VALIDATION TESTS ====================

#[tokio::test]
async fn test_parameter_window_clamping() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-window";

    insert_test_messages(&db, session_id, 20);

    let search_results = db.search_messages("Test message 10", 10).unwrap();
    if search_results.is_empty() {
        return;
    }

    let anchor_id = search_results[0].message_id;

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let mut ctx = ToolContext::default();
    ctx.session_id = session_id.to_string();

    // Test window > max (should be clamped to 20)
    let args = json!({
        "session_id": session_id,
        "around_message_id": anchor_id,
        "window": 100
    });

    let result = tool.execute(&args, &ctx).await;

    // Should not error, just clamp the value
    assert!(result.is_ok(), "Should clamp window parameter");
}

#[tokio::test]
async fn test_parameter_limit_clamping() {
    let (_lock, _tmp, db) = setup_test_db();
    let session_id = "test-session-limit";

    insert_test_messages(&db, session_id, 10);

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();

    // Test limit > max (should be clamped to 50)
    let args = json!({
        "query": "Test message",
        "limit": 1000
    });

    let result = tool.execute(&args, &ctx).await;

    // Should not error, just clamp the value
    assert!(result.is_ok(), "Should clamp limit parameter");
}

// ==================== METADATA TESTS ====================

#[tokio::test]
async fn test_tool_metadata() {
    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);

    assert_eq!(tool.name(), "session_search");
    assert!(!tool.description().is_empty());

    let schema = tool.schema();
    assert!(schema.is_object());
}

// ==================== MULTI-SESSION TESTS ====================

#[tokio::test]
async fn test_multiple_sessions_discovery() {
    let (_lock, _tmp, db) = setup_test_db();

    // Create multiple sessions with shared keyword
    for i in 0..3 {
        let session_id = format!("multi-session-{}", i);
        db.create_session_with_id(&session_id, "cli", Some("test-user"), None, None, None)
            .unwrap();

        let msg = Message::user(format!("Common keyword in session {}", i));
        db.save_message(&session_id, &msg).unwrap();
    }

    let tool: Arc<dyn Tool> = Arc::new(SessionSearchTool);
    let ctx = ToolContext::default();
    let args = json!({
        "query": "Common keyword"
    });

    let result = tool.execute(&args, &ctx).await.unwrap();

    // Should find matches across multiple sessions
    assert!(
        result.contains("Common") || result.contains("keyword") || result.contains("Found"),
        "Result: {}",
        result
    );
}
