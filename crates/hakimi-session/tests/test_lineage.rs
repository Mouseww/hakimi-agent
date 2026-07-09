//! Tests for session lineage functionality (parent/root relationships).

use anyhow::Result;
use hakimi_session::{SessionDB, SessionOps};
use tempfile::TempDir;

/// Helper to create a test database.
fn setup_test_db() -> Result<(TempDir, SessionDB)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test_sessions.db");
    let db = SessionDB::new(&db_path)?;
    db.initialize()?; // Initialize schema
    Ok((temp_dir, db))
}

#[test]
fn test_create_root_session() -> Result<()> {
    let (_dir, db) = setup_test_db()?;

    // Create a root session (no parent)
    let session_id = db.create_session("test", Some("user1"), Some("gpt-4"), None)?;

    // Verify session metadata
    let meta = db.get_session(&session_id)?.expect("Session should exist");
    assert_eq!(meta.id, session_id);
    assert_eq!(meta.source, Some("test".to_string()));
    assert_eq!(meta.parent_session_id, None);
    assert_eq!(meta.root_session_id, None);

    // Verify lineage methods
    assert_eq!(db.get_session_root(&session_id)?, None);
    assert!(!db.has_parent(&session_id)?);
    assert_eq!(db.get_session_depth(&session_id)?, 0);

    Ok(())
}

#[test]
fn test_create_child_session() -> Result<()> {
    let (_dir, db) = setup_test_db()?;

    // Create root session A
    let session_a = db.create_session("test", Some("user1"), Some("gpt-4"), None)?;

    // Create child session B from A
    let session_b = db.create_session_with_id(
        "session-b",
        "test",
        Some("user1"),
        Some("gpt-4"),
        None,
        Some(&session_a),
    )?;

    // Verify B's metadata
    let meta_b = db.get_session(&session_b)?.expect("Session B should exist");
    assert_eq!(meta_b.parent_session_id, Some(session_a.clone()));
    assert_eq!(meta_b.root_session_id, Some(session_a.clone()));

    // Verify lineage methods for B
    assert_eq!(db.get_session_root(&session_b)?, Some(session_a.clone()));
    assert!(db.has_parent(&session_b)?);
    assert_eq!(db.get_session_depth(&session_b)?, 1);

    // Verify A can find B as child
    let children = db.get_child_sessions(&session_a)?;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, session_b);

    Ok(())
}

#[test]
fn test_create_grandchild_session() -> Result<()> {
    let (_dir, db) = setup_test_db()?;

    // Create A → B → C lineage
    let session_a = db.create_session("test", Some("user1"), Some("gpt-4"), None)?;
    let session_b = db.create_session_with_id(
        "session-b",
        "test",
        Some("user1"),
        Some("gpt-4"),
        None,
        Some(&session_a),
    )?;
    let session_c = db.create_session_with_id(
        "session-c",
        "test",
        Some("user1"),
        Some("gpt-4"),
        None,
        Some(&session_b),
    )?;

    // Verify C's metadata
    let meta_c = db.get_session(&session_c)?.expect("Session C should exist");
    assert_eq!(meta_c.parent_session_id, Some(session_b.clone()));
    assert_eq!(meta_c.root_session_id, Some(session_a.clone())); // Should inherit root from B

    // Verify lineage methods for C
    assert_eq!(db.get_session_root(&session_c)?, Some(session_a.clone()));
    assert!(db.has_parent(&session_c)?);
    assert_eq!(db.get_session_depth(&session_c)?, 2);

    // Verify depths
    assert_eq!(db.get_session_depth(&session_a)?, 0);
    assert_eq!(db.get_session_depth(&session_b)?, 1);
    assert_eq!(db.get_session_depth(&session_c)?, 2);

    // Verify child relationships
    let children_a = db.get_child_sessions(&session_a)?;
    assert_eq!(children_a.len(), 1);
    assert_eq!(children_a[0].id, session_b);

    let children_b = db.get_child_sessions(&session_b)?;
    assert_eq!(children_b.len(), 1);
    assert_eq!(children_b[0].id, session_c);

    let children_c = db.get_child_sessions(&session_c)?;
    assert_eq!(children_c.len(), 0); // C has no children

    Ok(())
}

#[test]
fn test_multiple_children() -> Result<()> {
    let (_dir, db) = setup_test_db()?;

    // Create one parent with multiple children
    let parent = db.create_session("test", Some("user1"), Some("gpt-4"), None)?;

    let child1 = db.create_session_with_id(
        "child-1",
        "test",
        Some("user1"),
        Some("gpt-4"),
        None,
        Some(&parent),
    )?;

    let child2 = db.create_session_with_id(
        "child-2",
        "test",
        Some("user1"),
        Some("gpt-4"),
        None,
        Some(&parent),
    )?;

    let child3 = db.create_session_with_id(
        "child-3",
        "test",
        Some("user1"),
        Some("gpt-4"),
        None,
        Some(&parent),
    )?;

    // All children should have same parent and root
    for child_id in [&child1, &child2, &child3] {
        let meta = db.get_session(child_id)?.expect("Child should exist");
        assert_eq!(meta.parent_session_id, Some(parent.clone()));
        assert_eq!(meta.root_session_id, Some(parent.clone()));
        assert_eq!(db.get_session_depth(child_id)?, 1);
    }

    // Parent should find all children
    let children = db.get_child_sessions(&parent)?;
    assert_eq!(children.len(), 3);

    let child_ids: Vec<String> = children.iter().map(|m| m.id.clone()).collect();
    assert!(child_ids.contains(&child1));
    assert!(child_ids.contains(&child2));
    assert!(child_ids.contains(&child3));

    Ok(())
}

#[test]
fn test_session_depth_calculation() -> Result<()> {
    let (_dir, db) = setup_test_db()?;

    // Create a 5-level deep lineage
    let mut current = db.create_session("test", Some("user1"), Some("gpt-4"), None)?;
    let root = current.clone();

    for i in 1..=5 {
        let next_id = format!("session-level-{}", i);
        current = db.create_session_with_id(
            &next_id,
            "test",
            Some("user1"),
            Some("gpt-4"),
            None,
            Some(&current),
        )?;
        assert_eq!(db.get_session_depth(&current)?, i);
        assert_eq!(db.get_session_root(&current)?, Some(root.clone()));
    }

    Ok(())
}

#[test]
fn test_lineage_index_performance() -> Result<()> {
    let (_dir, db) = setup_test_db()?;

    // Create a tree with 100 sessions
    let root = db.create_session("test", Some("user1"), Some("gpt-4"), None)?;

    for i in 0..100 {
        db.create_session_with_id(
            &format!("child-{}", i),
            "test",
            Some("user1"),
            Some("gpt-4"),
            None,
            Some(&root),
        )?;
    }

    // Query should be fast due to idx_sessions_parent index
    let start = std::time::Instant::now();
    let children = db.get_child_sessions(&root)?;
    let elapsed = start.elapsed();

    assert_eq!(children.len(), 100);
    assert!(
        elapsed.as_millis() < 100,
        "Query took too long: {:?}",
        elapsed
    );

    Ok(())
}

#[test]
fn test_backwards_compatibility() -> Result<()> {
    let (_dir, db) = setup_test_db()?;

    // Existing code that doesn't specify parent should still work
    let session_id = db.create_session("test", Some("user1"), Some("gpt-4"), None)?;

    let meta = db.get_session(&session_id)?.expect("Session should exist");
    assert_eq!(meta.parent_session_id, None);
    assert_eq!(meta.root_session_id, None);

    Ok(())
}
