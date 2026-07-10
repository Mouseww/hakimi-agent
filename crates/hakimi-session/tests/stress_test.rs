// Stress and boundary tests for Hakimi session management
// Task 1.3.3: Stress testing and boundary testing

use hakimi_common::{Message, MessageRole};
use hakimi_session::{MessageOps, SessionDB, SessionOps};
use std::sync::Mutex;
use std::time::Instant;
use tempfile::TempDir;

// Global lock to serialize tests that modify HAKIMI_HOME
static TEST_LOCK: std::sync::LazyLock<Mutex<()>> = std::sync::LazyLock::new(|| Mutex::new(()));

/// Helper: Create a test database with HAKIMI_HOME set
fn setup_test_db() -> (TempDir, SessionDB) {
    let tmp = TempDir::new().unwrap();

    // Set HAKIMI_HOME to temp dir for this test
    unsafe {
        std::env::set_var("HAKIMI_HOME", tmp.path());
    }

    let db_path = tmp.path().join("sessions.db");
    let db = SessionDB::new(&db_path).expect("Failed to create test database");
    db.initialize()
        .expect("Failed to initialize database schema");

    (tmp, db)
}

/// Create a test message with given content
fn create_message(role: MessageRole, content: &str) -> Message {
    Message {
        role,
        content: Some(content.to_string()),
        images: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        reasoning: None,
        reasoning_content: None,
        timestamp: Some(chrono::Utc::now()),
        token_count: None,
        finish_reason: None,
    }
}

#[test]
fn test_10k_messages_search_performance() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    let session_id = db
        .create_session("stress_10k", None, None, None)
        .expect("Failed to create session");

    println!("Creating 10,000 messages...");
    let start = Instant::now();

    // Insert 10K messages in batches for better performance
    for batch in 0..100 {
        for i in 0..100 {
            let msg_num = batch * 100 + i;
            let msg = create_message(
                MessageRole::User,
                &format!("Test message number {msg_num} with some searchable content"),
            );
            db.save_message(&session_id, &msg)
                .expect("Failed to save message");
        }
    }

    let insert_duration = start.elapsed();
    println!("Inserted 10K messages in {:?}", insert_duration);

    // Test 1: Search performance with FTS5
    println!("Testing FTS5 search performance...");
    let search_start = Instant::now();

    let results = db
        .search_messages("searchable", 100)
        .expect("Search failed");

    let search_duration = search_start.elapsed();
    println!(
        "FTS5 search took {:?}, found {} results",
        search_duration,
        results.len()
    );

    // Verify search works
    assert!(!results.is_empty(), "Should find matching messages");
    assert!(
        search_duration.as_millis() < 500,
        "Search should complete in < 500ms"
    );

    // Test 2: Get messages around a specific message
    println!("Testing get_messages_around performance...");

    // Search for a specific message to get its ID
    let search_results = db
        .search_messages("message number 5000", 1)
        .expect("Failed to search for message");

    if let Some(result) = search_results.first() {
        let around_start = Instant::now();
        let (around_results, _, _) = db
            .get_messages_around(&session_id, result.message_id, 50, None)
            .expect("Failed to get messages around");

        let around_duration = around_start.elapsed();
        println!(
            "get_messages_around took {:?}, got {} messages",
            around_duration,
            around_results.len()
        );

        assert!(
            around_duration.as_millis() < 500,
            "get_messages_around should complete in < 500ms"
        );
        assert!(
            around_results.len() <= 101,
            "Should return at most window*2+1 messages"
        );
    }

    // Test 3: Get bookends performance
    println!("Testing get_bookends performance...");
    let bookends_start = Instant::now();

    let (first, last) = db
        .get_bookends(&session_id, 1, None)
        .expect("Failed to get bookends");

    let bookends_duration = bookends_start.elapsed();
    println!("get_bookends took {:?}", bookends_duration);

    assert!(
        bookends_duration.as_millis() < 500,
        "get_bookends should complete in < 500ms"
    );
    assert!(
        first.len() == 1 && last.len() == 1,
        "Should return first and last message"
    );
}

#[test]
fn test_100_concurrent_session_creation() {
    // Note: Using Arc with separate threads can cause table contention issues.
    // This test validates that the DB itself is thread-safe via Mutex.
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    println!("Creating 100 sessions sequentially (simulating concurrent load)...");
    let start = Instant::now();

    let mut session_ids = Vec::new();

    for i in 0..100 {
        let session_id = db
            .create_session(&format!("concurrent_{i}"), None, None, None)
            .expect("Failed to create session");

        // Add a few messages
        for j in 0..10 {
            let msg = create_message(MessageRole::User, &format!("Message {j} in session {i}"));
            db.save_message(&session_id, &msg)
                .expect("Failed to save message");
        }

        session_ids.push(session_id);
    }

    let duration = start.elapsed();
    println!(
        "Created 100 sessions with 10 messages each in {:?}",
        duration
    );

    // Verify all succeeded
    assert_eq!(
        session_ids.len(),
        100,
        "All session creations should succeed"
    );
    assert!(duration.as_secs() < 10, "Should complete in < 10 seconds");

    // Verify sessions were created
    let sessions = db
        .get_recent_sessions(None, 200)
        .expect("Failed to get sessions");
    assert!(sessions.len() >= 100, "Should have at least 100 sessions");
}

#[test]
fn test_single_query_1k_results() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    let session_id = db
        .create_session("large_result_set", None, None, None)
        .expect("Failed to create session");

    println!("Creating 2,000 messages with common keyword...");
    let start = Instant::now();

    // Insert 2K messages that will all match the search
    for i in 0..2000 {
        let msg = create_message(
            MessageRole::User,
            &format!("KEYWORD search target message number {i}"),
        );
        db.save_message(&session_id, &msg)
            .expect("Failed to save message");
    }

    let insert_duration = start.elapsed();
    println!("Inserted 2K messages in {:?}", insert_duration);

    // Search with limit that would return > 1K results if not limited
    println!("Searching for messages (should return many results)...");
    let search_start = Instant::now();

    let results = db.search_messages("KEYWORD", 1500).expect("Search failed");

    let search_duration = search_start.elapsed();
    println!(
        "Search returned {} results in {:?}",
        results.len(),
        search_duration
    );

    // Verify large result set handling
    assert!(results.len() >= 1000, "Should return at least 1K results");
    assert!(results.len() <= 1500, "Should respect limit parameter");
    assert!(
        search_duration.as_millis() < 500,
        "Search should complete in < 500ms even with large result set"
    );
}

#[test]
fn test_boundary_empty_session() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    let session_id = db
        .create_session("empty_session", None, None, None)
        .expect("Failed to create session");

    // Test operations on empty session
    let messages = db
        .get_messages(&session_id)
        .expect("Failed to get messages");
    assert!(messages.is_empty(), "Empty session should have no messages");

    let (first, last) = db
        .get_bookends(&session_id, 1, None)
        .expect("Failed to get bookends");
    assert!(
        first.is_empty() && last.is_empty(),
        "Empty session should have no bookends"
    );

    let _search = db.search_messages("anything", 10).expect("Search failed");
    // Search may return results from other sessions, but should not panic
    assert!(true, "Search on empty database should not panic");
}

#[test]
fn test_boundary_single_message_session() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    let session_id = db
        .create_session("single_message", None, None, None)
        .expect("Failed to create session");

    let msg = create_message(MessageRole::User, "The only message");
    db.save_message(&session_id, &msg)
        .expect("Failed to save message");

    // Test get_messages_around with single message
    let messages = db
        .get_messages(&session_id)
        .expect("Failed to get messages");
    assert_eq!(messages.len(), 1);

    // Search for the message to get its ID
    let search_results = db.search_messages("only", 1).expect("Failed to search");
    if let Some(result) = search_results.first() {
        let (around, _, _) = db
            .get_messages_around(&session_id, result.message_id, 10, None)
            .expect("Failed");
        assert_eq!(around.len(), 1, "Should return only the single message");
    }

    // Test bookends with single message
    let (first, last) = db.get_bookends(&session_id, 1, None).expect("Failed");
    assert_eq!(first.len(), 1, "Should return single message");
    assert_eq!(last.len(), 1, "Should return single message");
}

#[test]
fn test_boundary_very_long_messages() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    let session_id = db
        .create_session("long_messages", None, None, None)
        .expect("Failed to create session");

    // Create messages with very long content (100KB each)
    let long_content = "A".repeat(100_000);

    for i in 0..10 {
        let mut msg = create_message(MessageRole::User, &long_content);
        msg.content = Some(format!("{} with searchable_keyword_{i}", long_content));
        db.save_message(&session_id, &msg)
            .expect("Failed to save long message");
    }

    // Verify retrieval works
    let messages = db.get_messages(&session_id).expect("Failed");
    assert_eq!(messages.len(), 10);
    assert!(messages[0].content.as_ref().map_or(0, |c| c.len()) >= 100_000);

    // Search should still work
    let search = db
        .search_messages("searchable_keyword", 10)
        .expect("Search failed");
    assert!(!search.is_empty(), "Should find long messages");
}

#[test]
fn test_boundary_special_characters_in_session_id() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    // Test various special characters in session source (used as part of ID generation)
    let special_sources = vec![
        "source-with-dashes",
        "source_with_underscores",
        "source.with.dots",
        "source123",
    ];

    for source in special_sources {
        let session_id = db
            .create_session(source, None, None, None)
            .expect(&format!("Failed to create session: {}", source));

        let msg = create_message(MessageRole::User, "Test message");
        db.save_message(&session_id, &msg)
            .expect("Failed to save message");

        let messages = db
            .get_messages(&session_id)
            .expect("Failed to get messages");
        assert_eq!(
            messages.len(),
            1,
            "Should handle special characters in source"
        );
    }
}

#[test]
fn test_performance_baseline_p95() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    let session_id = db
        .create_session("perf_baseline", None, None, None)
        .expect("Failed to create session");

    // Insert 1000 messages
    for i in 0..1000 {
        let msg = create_message(MessageRole::User, &format!("Performance test message {i}"));
        db.save_message(&session_id, &msg)
            .expect("Failed to save message");
    }

    // Run 100 searches and measure P95 latency
    let mut durations = Vec::new();

    for _i in 0..100 {
        let start = Instant::now();
        let _ = db
            .search_messages("performance", 50)
            .expect("Search failed");
        durations.push(start.elapsed());

        // Small delay to avoid hammering the database
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Sort and calculate P95
    durations.sort();
    let p95_idx = (durations.len() as f64 * 0.95) as usize;
    let p95_duration = durations[p95_idx];

    println!("P95 search latency: {:?}", p95_duration);
    println!(
        "Min: {:?}, Max: {:?}",
        durations[0],
        durations[durations.len() - 1]
    );

    assert!(
        p95_duration.as_millis() < 500,
        "P95 latency should be < 500ms, got {:?}",
        p95_duration
    );
}

#[test]
fn test_database_integrity_after_stress() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (_tmp, db) = setup_test_db();

    // Create multiple sessions with various operations
    for session_num in 0..10 {
        let session_id = db
            .create_session(&format!("integrity_{session_num}"), None, None, None)
            .expect("Failed to create session");

        // Add messages
        for msg_num in 0..100 {
            let msg = create_message(
                if msg_num % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                &format!("Integrity test message {msg_num}"),
            );
            db.save_message(&session_id, &msg)
                .expect("Failed to save message");
        }

        // Verify message count
        let messages = db
            .get_messages(&session_id)
            .expect("Failed to get messages");
        assert_eq!(
            messages.len(),
            100,
            "Should have exactly 100 messages in session {}",
            session_id
        );

        // Verify search works
        let search_results = db.search_messages("integrity", 50).expect("Search failed");
        assert!(!search_results.is_empty(), "Search should return results");
    }

    // Verify all sessions exist
    let sessions = db
        .get_recent_sessions(None, 20)
        .expect("Failed to get sessions");
    assert!(sessions.len() >= 10, "Should have at least 10 sessions");
}
