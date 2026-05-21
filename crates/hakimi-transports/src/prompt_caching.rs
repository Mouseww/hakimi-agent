//! Anthropic prompt caching support.
//!
//! Implements cache control directives and breakpoint placement strategies
//! for Anthropic's prompt caching feature.
//!
//! See: <https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching>

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

/// Cache control directive for Anthropic prompt caching.
///
/// When attached to a content block or tool definition, tells the Anthropic
/// API to create a cache breakpoint at that point in the prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheControl {
    /// The cache type. Currently always `"ephemeral"`.
    #[serde(rename = "type")]
    pub cache_type: String,

    /// Optional time-to-live for the cache entry.
    /// Supported values: `"5m"` (5 minutes, the default) or `"1h"` (1 hour).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

impl CacheControl {
    /// Create a default ephemeral cache control (5-minute TTL).
    pub fn ephemeral() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: None,
        }
    }

    /// Create an ephemeral cache control with an explicit TTL.
    ///
    /// # Arguments
    /// * `ttl` - Time-to-live string, e.g. `"5m"` or `"1h"`.
    pub fn with_ttl(ttl: impl Into<String>) -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: Some(ttl.into()),
        }
    }
}

/// Layout strategy for placing prompt caching breakpoints.
///
/// Anthropic's prompt caching allows marking specific points in a prompt
/// for caching. Different layouts place breakpoints differently based on
/// the expected conversation patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLayout {
    /// 4 breakpoints with 5-minute TTL:
    /// 1. Last system content block
    /// 2. Last tool definition
    /// 3. First user message
    /// 4. Middle message in the conversation
    SystemAnd3,

    /// Split-TTL layout: prefix gets 1-hour TTL, later messages get 5-minute TTL.
    /// 1. Last system content block (1h)
    /// 2. Last tool definition (1h)
    /// 3. First user message (1h)
    /// 4. Later message (~67% mark) (5m)
    PrefixAnd2,
}

impl std::str::FromStr for CacheLayout {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "system_and_3" | "systemand3" => Ok(Self::SystemAnd3),
            "prefix_and_2" | "prefixand2" => Ok(Self::PrefixAnd2),
            _ => Err(format!(
                "unknown cache layout: '{s}'. Expected 'system_and_3' or 'prefix_and_2'"
            )),
        }
    }
}

/// The `anthropic-beta` header value required for prompt caching.
pub const CACHE_BETA_HEADER_VALUE: &str = "prompt-caching-2024-07-31";

/// Apply prompt caching breakpoints to an Anthropic request body.
///
/// Modifies the request body in-place by adding `cache_control` directives
/// to the appropriate content blocks based on the chosen layout.
///
/// # Arguments
/// * `body` - Mutable reference to the JSON request body.
/// * `layout` - The caching layout strategy to apply.
pub fn apply_caching(body: &mut JsonValue, layout: CacheLayout) {
    match layout {
        CacheLayout::SystemAnd3 => apply_system_and_3(body),
        CacheLayout::PrefixAnd2 => apply_prefix_and_2(body),
    }
}

/// Apply the `SystemAnd3` layout: 4 breakpoints, all with 5-minute TTL.
fn apply_system_and_3(body: &mut JsonValue) {
    let cc = CacheControl::ephemeral();

    // 1. Last system content block
    apply_system_cache(body, &cc);

    // 2. Last tool definition
    apply_tools_cache(body, &cc);

    // 3. First message (user message)
    apply_message_cache_at(body, 0, &cc);

    // 4. Middle message (skip if only 1 message or fewer)
    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        let len = messages.len();
        let mid = len / 2;
        if mid > 0 && mid < len {
            apply_message_cache_at(body, mid, &cc);
        }
    }
}

/// Apply the `PrefixAnd2` layout: prefix (1h) + later messages (5m).
fn apply_prefix_and_2(body: &mut JsonValue) {
    let cc_long = CacheControl::with_ttl("1h");
    let cc_short = CacheControl::ephemeral();

    // 1. Last system content block (1h TTL)
    apply_system_cache(body, &cc_long);

    // 2. Last tool definition (1h TTL)
    apply_tools_cache(body, &cc_long);

    // 3. First message (1h TTL) — part of the "prefix"
    apply_message_cache_at(body, 0, &cc_long);

    // 4. Later message at ~67% mark (5m TTL)
    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        let len = messages.len();
        let split = len * 2 / 3;
        if split > 0 && split < len {
            apply_message_cache_at(body, split, &cc_short);
        }
    }
}

/// Add `cache_control` to the last system content block.
///
/// Converts the system field from a string to an array of content blocks
/// if necessary, then adds `cache_control` to the last block.
fn apply_system_cache(body: &mut JsonValue, cc: &CacheControl) {
    if let Some(system) = body.get_mut("system") {
        if system.is_string() {
            let text = system.as_str().unwrap_or("").to_string();
            *system = json!([{
                "type": "text",
                "text": text,
                "cache_control": cc
            }]);
        } else if let Some(arr) = system.as_array_mut() {
            if let Some(last) = arr.last_mut() {
                last["cache_control"] = json!(cc);
            }
        }
    }
}

/// Add `cache_control` to the last tool definition.
fn apply_tools_cache(body: &mut JsonValue, cc: &CacheControl) {
    if let Some(tools) = body.get_mut("tools").and_then(|v| v.as_array_mut()) {
        if let Some(last) = tools.last_mut() {
            last["cache_control"] = json!(cc);
        }
    }
}

/// Add `cache_control` to a specific message by index.
///
/// If the message content is a string, converts it to an array format
/// with a single text content block that has `cache_control`.
/// If it's already an array, adds `cache_control` to the last content block.
fn apply_message_cache_at(body: &mut JsonValue, index: usize, cc: &CacheControl) {
    if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
        if let Some(msg) = messages.get_mut(index) {
            if let Some(content) = msg.get_mut("content") {
                if content.is_string() {
                    let text = content.as_str().unwrap_or("").to_string();
                    *content = json!([{
                        "type": "text",
                        "text": text,
                        "cache_control": cc
                    }]);
                } else if let Some(arr) = content.as_array_mut() {
                    if let Some(last) = arr.last_mut() {
                        last["cache_control"] = json!(cc);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CacheControl serialization ──────────────────────────────────────

    #[test]
    fn test_cache_control_serialization() {
        let cc = CacheControl::ephemeral();
        let json = serde_json::to_string(&cc).unwrap();
        assert_eq!(json, r#"{"type":"ephemeral"}"#);
        // ttl should be omitted (skip_serializing_if)
        assert!(!json.contains("ttl"));
    }

    #[test]
    fn test_cache_control_with_ttl() {
        let cc = CacheControl::with_ttl("5m");
        let json = serde_json::to_value(&cc).unwrap();
        assert_eq!(json["type"], "ephemeral");
        assert_eq!(json["ttl"], "5m");

        let cc_1h = CacheControl::with_ttl("1h");
        let json_1h = serde_json::to_value(&cc_1h).unwrap();
        assert_eq!(json_1h["type"], "ephemeral");
        assert_eq!(json_1h["ttl"], "1h");
    }

    #[test]
    fn test_cache_control_deserialization() {
        let cc: CacheControl = serde_json::from_str(r#"{"type":"ephemeral"}"#).unwrap();
        assert_eq!(cc.cache_type, "ephemeral");
        assert!(cc.ttl.is_none());

        let cc_ttl: CacheControl =
            serde_json::from_str(r#"{"type":"ephemeral","ttl":"1h"}"#).unwrap();
        assert_eq!(cc_ttl.cache_type, "ephemeral");
        assert_eq!(cc_ttl.ttl.as_deref(), Some("1h"));
    }

    // ── CacheLayout parsing ─────────────────────────────────────────────

    #[test]
    fn test_cache_layout_from_str() {
        assert_eq!(
            "system_and_3".parse::<CacheLayout>(),
            Ok(CacheLayout::SystemAnd3)
        );
        assert_eq!(
            "SystemAnd3".parse::<CacheLayout>(),
            Ok(CacheLayout::SystemAnd3)
        );
        assert_eq!(
            "prefix_and_2".parse::<CacheLayout>(),
            Ok(CacheLayout::PrefixAnd2)
        );
        assert_eq!(
            "PrefixAnd2".parse::<CacheLayout>(),
            Ok(CacheLayout::PrefixAnd2)
        );
        // Hyphens normalized to underscores
        assert_eq!(
            "prefix-and-2".parse::<CacheLayout>(),
            Ok(CacheLayout::PrefixAnd2)
        );
        assert!("invalid".parse::<CacheLayout>().is_err());
    }

    // ── apply_caching tests ─────────────────────────────────────────────

    #[test]
    fn test_apply_caching_system_and_3() {
        let mut body = json!({
            "model": "claude-3-opus",
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": [{"type": "text", "text": "Hi!"}]},
                {"role": "user", "content": "How are you?"}
            ],
            "tools": [
                {"name": "tool1", "description": "A tool", "input_schema": {}},
                {"name": "tool2", "description": "Another tool", "input_schema": {}}
            ]
        });

        apply_caching(&mut body, CacheLayout::SystemAnd3);

        // System should be array with cache_control on last (only) block
        let sys = body["system"].as_array().unwrap();
        assert_eq!(sys.len(), 1);
        assert_eq!(sys[0]["cache_control"]["type"], "ephemeral");
        assert!(sys[0]["cache_control"]["ttl"].is_null());

        // Last tool should have cache_control, first should not
        let tools = body["tools"].as_array().unwrap();
        assert!(tools[0].get("cache_control").is_none());
        assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");

        // First message (user) should have cache_control
        let msgs = body["messages"].as_array().unwrap();
        let first_content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(first_content[0]["cache_control"]["type"], "ephemeral");

        // Middle message (index 1, assistant) should have cache_control
        let mid_content = msgs[1]["content"].as_array().unwrap();
        assert_eq!(
            mid_content.last().unwrap()["cache_control"]["type"],
            "ephemeral"
        );

        // Last message (index 2) should NOT have cache_control
        let last_content = msgs[2]["content"].as_str().unwrap();
        assert_eq!(last_content, "How are you?");
    }

    #[test]
    fn test_apply_caching_prefix_and_2() {
        let mut body = json!({
            "model": "claude-3-opus",
            "system": "System prompt",
            "messages": [
                {"role": "user", "content": "msg1"},
                {"role": "assistant", "content": [{"type": "text", "text": "reply1"}]},
                {"role": "user", "content": "msg2"},
                {"role": "assistant", "content": [{"type": "text", "text": "reply2"}]},
                {"role": "user", "content": "msg3"}
            ],
            "tools": [
                {"name": "t1", "description": "T1", "input_schema": {}}
            ]
        });

        apply_caching(&mut body, CacheLayout::PrefixAnd2);

        // System should have 1h TTL
        let sys = body["system"].as_array().unwrap();
        assert_eq!(sys[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(sys[0]["cache_control"]["ttl"], "1h");

        // Last tool should have 1h TTL
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(tools[0]["cache_control"]["ttl"], "1h");

        // First message should have 1h TTL
        let msgs = body["messages"].as_array().unwrap();
        let first_content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(first_content[0]["cache_control"]["ttl"], "1h");

        // Later message at ~67% (5*2/3=3, index 3) should have 5m TTL
        let split_content = msgs[3]["content"].as_array().unwrap();
        assert_eq!(
            split_content.last().unwrap()["cache_control"]["type"],
            "ephemeral"
        );
        assert!(split_content.last().unwrap()["cache_control"]["ttl"].is_null());

        // Message at index 2 should NOT have cache_control (not in the prefix or the 5m breakpoint)
        let mid_content = msgs[2]["content"].as_str().unwrap();
        assert_eq!(mid_content, "msg2");
    }

    // ── Individual breakpoint tests ─────────────────────────────────────

    #[test]
    fn test_cache_control_on_last_system_message() {
        let mut body = json!({
            "system": "System text here",
            "messages": []
        });

        apply_system_cache(&mut body, &CacheControl::ephemeral());

        let sys = body["system"].as_array().unwrap();
        assert_eq!(sys.len(), 1);
        assert_eq!(sys[0]["type"], "text");
        assert_eq!(sys[0]["text"], "System text here");
        assert_eq!(sys[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_cache_control_on_last_tool() {
        let mut body = json!({
            "tools": [
                {"name": "a", "description": "A", "input_schema": {}},
                {"name": "b", "description": "B", "input_schema": {}},
                {"name": "c", "description": "C", "input_schema": {}}
            ],
            "messages": []
        });

        apply_tools_cache(&mut body, &CacheControl::ephemeral());

        let tools = body["tools"].as_array().unwrap();
        assert!(tools[0].get("cache_control").is_none());
        assert!(tools[1].get("cache_control").is_none());
        assert_eq!(tools[2]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_cache_control_on_first_user_message() {
        let mut body = json!({
            "messages": [
                {"role": "user", "content": "Hello world"},
                {"role": "assistant", "content": [{"type": "text", "text": "Hi!"}]}
            ]
        });

        apply_message_cache_at(&mut body, 0, &CacheControl::ephemeral());

        let msgs = body["messages"].as_array().unwrap();
        let first_content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(first_content.len(), 1);
        assert_eq!(first_content[0]["type"], "text");
        assert_eq!(first_content[0]["text"], "Hello world");
        assert_eq!(first_content[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_caching_preserves_existing_content() {
        let mut body = json!({
            "model": "claude-3-opus",
            "max_tokens": 4096,
            "system": "Be helpful",
            "messages": [
                {"role": "user", "content": "Hi"}
            ],
            "tools": [
                {"name": "search", "description": "Search", "input_schema": {"type": "object"}}
            ],
            "temperature": 0.7
        });

        apply_caching(&mut body, CacheLayout::SystemAnd3);

        // Original fields preserved
        assert_eq!(body["model"], "claude-3-opus");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["temperature"], 0.7);

        // System content preserved (converted to array but text is the same)
        let sys = body["system"].as_array().unwrap();
        assert_eq!(sys[0]["text"], "Be helpful");

        // Message content preserved
        let msgs = body["messages"].as_array().unwrap();
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["text"], "Hi");

        // Tool content preserved
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "search");

        // Cache control was added
        assert_eq!(sys[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(content[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(tools[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_apply_caching_no_system_no_tools() {
        let mut body = json!({
            "model": "claude-3-opus",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });

        apply_caching(&mut body, CacheLayout::SystemAnd3);

        // No system or tools fields, should not panic
        assert!(body.get("system").is_none());
        assert!(body.get("tools").is_none());

        // First message should still get cache_control
        let msgs = body["messages"].as_array().unwrap();
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["cache_control"]["type"], "ephemeral");
    }
}
