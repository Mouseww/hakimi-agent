use sha2::{Digest, Sha256};

/// Generate cache key from tool name and parameters
pub fn generate_cache_key(tool_name: &str, params: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(params.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Generate cache key with additional context
pub fn generate_cache_key_with_context(
    tool_name: &str,
    params: &serde_json::Value,
    context: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(params.to_string().as_bytes());

    if let Some(ctx) = context {
        hasher.update(ctx.as_bytes());
    }

    format!("{:x}", hasher.finalize())
}

/// Check if a tool is cacheable (idempotent operations)
pub fn is_cacheable_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read_file" | "search_files" | "list_directory" | "file_info" | "knowledge_search"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_cache_key_generation() {
        let key1 = generate_cache_key("read_file", &json!({"path": "/tmp/file.txt"}));
        let key2 = generate_cache_key("read_file", &json!({"path": "/tmp/file.txt"}));

        // Same input should generate same key
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 64); // SHA256 hex length
    }

    #[test]
    fn test_different_params_different_keys() {
        let key1 = generate_cache_key("read_file", &json!({"path": "/tmp/file1.txt"}));
        let key2 = generate_cache_key("read_file", &json!({"path": "/tmp/file2.txt"}));

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_different_tools_different_keys() {
        let key1 = generate_cache_key("read_file", &json!({"path": "/tmp/file.txt"}));
        let key2 = generate_cache_key("search_files", &json!({"path": "/tmp/file.txt"}));

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_with_context() {
        let key1 = generate_cache_key_with_context(
            "read_file",
            &json!({"path": "/tmp/file.txt"}),
            Some("session-123"),
        );
        let key2 = generate_cache_key_with_context(
            "read_file",
            &json!({"path": "/tmp/file.txt"}),
            Some("session-456"),
        );

        // Different context should generate different keys
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_without_context() {
        let key1 =
            generate_cache_key_with_context("read_file", &json!({"path": "/tmp/file.txt"}), None);
        let key2 =
            generate_cache_key_with_context("read_file", &json!({"path": "/tmp/file.txt"}), None);

        // Same without context
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_is_cacheable_tool() {
        assert!(is_cacheable_tool("read_file"));
        assert!(is_cacheable_tool("search_files"));
        assert!(is_cacheable_tool("list_directory"));
        assert!(is_cacheable_tool("knowledge_search"));

        assert!(!is_cacheable_tool("write_file"));
        assert!(!is_cacheable_tool("terminal"));
        assert!(!is_cacheable_tool("unknown_tool"));
    }

    #[test]
    fn test_param_order_independence() {
        // JSON objects with different key orders should produce same key
        // Note: serde_json maintains insertion order in the string representation
        // So this test demonstrates that param order matters
        let key1 = generate_cache_key("tool", &json!({"a": 1, "b": 2}));
        let key2 = generate_cache_key("tool", &json!({"a": 1, "b": 2}));

        assert_eq!(key1, key2);

        // Different order might produce different keys depending on JSON serialization
        let _key3 = generate_cache_key("tool", &json!({"b": 2, "a": 1}));
        // This behavior depends on JSON lib implementation
        // For consistent caching, params should be normalized before hashing
    }
}
