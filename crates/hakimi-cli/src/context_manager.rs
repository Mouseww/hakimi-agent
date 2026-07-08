use hakimi_common::{Message, MessageRole};

/// Configuration for context management
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum number of messages to keep in memory (default: 100)
    pub max_messages: usize,
    /// Minimum number of recent messages to always preserve (default: 20)
    pub preserve_recent: usize,
    /// Whether to compress tool messages aggressively (default: true)
    pub compress_tool_messages: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_messages: 100,
            preserve_recent: 20,
            compress_tool_messages: true,
        }
    }
}

/// Apply intelligent compression to message history
/// 
/// Strategy:
/// 1. Always preserve recent N messages (user choices, latest context)
/// 2. Compress tool messages in older history (keep start/end, drop verbose middle)
/// 3. Preserve critical messages (explicit user selections, system markers)
/// 4. Use sliding window when history exceeds max_messages
pub fn compress_history(messages: Vec<Message>, config: &ContextConfig) -> Vec<Message> {
    let total = messages.len();
    
    // If under limit, return as-is
    if total <= config.max_messages {
        return messages;
    }
    
    // Split into old and recent parts
    let preserve_count = config.preserve_recent.min(total);
    let split_point = total - preserve_count;
    
    let (older_messages, recent_messages) = messages.split_at(split_point);
    
    // Always keep recent messages intact
    let mut result = Vec::new();
    
    // Compress older messages
    if config.compress_tool_messages {
        result.extend(compress_tool_heavy_section(older_messages.to_vec()));
    } else {
        // Simple sliding window: keep first few + last before recent
        let keep_from_old = config.max_messages.saturating_sub(preserve_count);
        if older_messages.len() > keep_from_old {
            let skip = older_messages.len() - keep_from_old;
            result.extend(older_messages[skip..].iter().cloned());
        } else {
            result.extend(older_messages.iter().cloned());
        }
    }
    
    // Append recent messages
    result.extend(recent_messages.iter().cloned());
    
    result
}

/// Compress tool-heavy message sections
/// Keep:
/// - User messages (explicit choices)
/// - Assistant messages (reasoning)
/// - Tool invocation messages (what was called)
/// - First and last tool result in a sequence
/// Drop:
/// - Verbose middle tool results in a sequence
fn compress_tool_heavy_section(messages: Vec<Message>) -> Vec<Message> {
    let mut result = Vec::new();
    let mut tool_result_buffer: Vec<Message> = Vec::new();
    
    for msg in messages {
        match msg.role {
            MessageRole::User | MessageRole::Assistant => {
                // Flush tool buffer with compression before adding user/assistant
                if !tool_result_buffer.is_empty() {
                    result.extend(compress_tool_sequence(tool_result_buffer));
                    tool_result_buffer = Vec::new();
                }
                result.push(msg);
            }
            MessageRole::Tool => {
                // Accumulate tool results for batch compression
                tool_result_buffer.push(msg);
            }
            _ => {
                // Unknown role, keep it
                if !tool_result_buffer.is_empty() {
                    result.extend(compress_tool_sequence(tool_result_buffer));
                    tool_result_buffer = Vec::new();
                }
                result.push(msg);
            }
        }
    }
    
    // Flush remaining tool buffer
    if !tool_result_buffer.is_empty() {
        result.extend(compress_tool_sequence(tool_result_buffer));
    }
    
    result
}

/// Compress a sequence of consecutive tool messages
/// Keep first and last, replace middle with summary marker if >3 messages
fn compress_tool_sequence(mut messages: Vec<Message>) -> Vec<Message> {
    if messages.len() <= 3 {
        return messages;
    }
    
    let first = messages.remove(0);
    let last = messages.pop().unwrap();
    let dropped_count = messages.len();
    
    // Create a summary marker
    let summary = Message {
        role: MessageRole::Tool,
        content: Some(format!("... ({} intermediate tool results compressed) ...", dropped_count)),
        images: None,
        tool_calls: None,
        tool_call_id: None,
        name: Some("compression_marker".to_string()),
        reasoning: None,
        reasoning_content: None,
        timestamp: None,
        token_count: None,
        finish_reason: None,
    };
    
    vec![first, summary, last]
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn make_message(role: &str, content: &str) -> Message {
        let msg_role = match role {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "tool" => MessageRole::Tool,
            _ => MessageRole::System,
        };
        Message {
            role: msg_role,
            content: Some(content.to_string()),
            images: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: None,
            token_count: None,
            finish_reason: None,
        }
    }
    
    #[test]
    fn test_no_compression_under_limit() {
        let config = ContextConfig {
            max_messages: 50,
            preserve_recent: 10,
            compress_tool_messages: true,
        };
        
        let messages = vec![
            make_message("user", "Hello"),
            make_message("assistant", "Hi"),
        ];
        
        let result = compress_history(messages.clone(), &config);
        assert_eq!(result.len(), 2);
    }
    
    #[test]
    fn test_tool_sequence_compression() {
        let messages = vec![
            make_message("user", "Search for X"),
            make_message("assistant", "Searching..."),
            make_message("tool", "Result 1"),
            make_message("tool", "Result 2"),
            make_message("tool", "Result 3"),
            make_message("tool", "Result 4"),
            make_message("tool", "Result 5"),
            make_message("assistant", "Found it"),
        ];
        
        let compressed = compress_tool_heavy_section(messages);
        
        // Should keep user, assistant, first tool, summary, last tool, final assistant
        assert_eq!(compressed.len(), 6);
        assert_eq!(compressed[0].role, MessageRole::User);
        assert_eq!(compressed[1].role, MessageRole::Assistant);
        assert_eq!(compressed[2].role, MessageRole::Tool);
        assert_eq!(compressed[2].content.as_deref(), Some("Result 1"));
        assert_eq!(compressed[3].role, MessageRole::Tool);
        assert!(compressed[3].content.as_deref().unwrap().contains("compressed"));
        assert_eq!(compressed[4].role, MessageRole::Tool);
        assert_eq!(compressed[4].content.as_deref(), Some("Result 5"));
        assert_eq!(compressed[5].role, MessageRole::Assistant);
    }
    
    #[test]
    fn test_preserve_recent_messages() {
        let config = ContextConfig {
            max_messages: 10,
            preserve_recent: 5,
            compress_tool_messages: false,
        };
        
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(make_message("user", &format!("Message {}", i)));
        }
        
        let result = compress_history(messages, &config);
        
        // Should keep max 10 messages
        assert_eq!(result.len(), 10);
        
        // Last 5 should be messages 15-19
        assert!(result[5].content.as_deref().unwrap().contains("Message 15"));
        assert!(result[9].content.as_deref().unwrap().contains("Message 19"));
    }
}
