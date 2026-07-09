//! Hakimi Agent 结构化错误处理
//!
//! 提供统一的错误类型，确保所有错误携带完整上下文信息。

use thiserror::Error;

/// 错误上下文 — 所有自定义错误都应包含此结构
#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorContext {
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    pub timestamp: String,          // ISO 8601
    pub operation: String,          // 操作名称，如 "get_messages_around"
    pub details: serde_json::Value, // 额外详细信息
}

impl ErrorContext {
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            session_id: None,
            user_id: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            operation: operation.into(),
            details: serde_json::json!({}),
        }
    }

    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn with_user_id(mut self, id: impl Into<String>) -> Self {
        self.user_id = Some(id.into());
        self
    }

    pub fn with_detail(mut self, key: &str, value: serde_json::Value) -> Self {
        if let serde_json::Value::Object(ref mut map) = self.details {
            map.insert(key.to_string(), value);
        } else {
            let mut map = serde_json::Map::new();
            map.insert(key.to_string(), value);
            self.details = serde_json::Value::Object(map);
        }
        self
    }
}

/// 通用结果类型
pub type HakimiResult<T> = std::result::Result<T, HakimiError>;

/// 向后兼容的 Result 别名
pub type Result<T> = HakimiResult<T>;

/// 根错误类型
#[derive(Error, Debug)]
pub enum HakimiError {
    #[error("Session error: {message}")]
    Session {
        message: String,
        context: ErrorContext,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Session error: {0}")]
    SessionSimple(String), // 向后兼容

    #[error("Memory error: {message}")]
    Memory {
        message: String,
        context: ErrorContext,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Memory error: {0}")]
    MemorySimple(String), // 向后兼容

    #[error("Context error: {message}")]
    Context {
        message: String,
        context: ErrorContext,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Context error: {0}")]
    ContextSimple(String), // 向后兼容

    #[error("Tool error: {message}")]
    Tool {
        message: String,
        context: ErrorContext,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Tool error: {0}")]
    ToolSimple(String), // 向后兼容的简单版本

    #[error("Transport error: {0}")]
    Transport(String), // HTTP/网络传输错误

    #[error("Configuration error: {0}")]
    Config(String), // 配置错误

    #[error("JSON error: {0}")]
    Json(String), // JSON 解析错误（已有 Serialization，但保留向后兼容）

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[cfg(feature = "rusqlite")]
    #[error("Database error: {0}")]
    Database(String), // 不直接 derive From，避免 optional 依赖问题

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl HakimiError {
    /// 记录错误到日志（带完整上下文）
    pub fn log(&self) {
        match self {
            Self::Session {
                message,
                context,
                source,
            } => {
                tracing::error!(
                    error_type = "session",
                    message = %message,
                    session_id = ?context.session_id,
                    user_id = ?context.user_id,
                    timestamp = %context.timestamp,
                    operation = %context.operation,
                    details = ?context.details,
                    source = ?source,
                    "Session error occurred"
                );
            }
            Self::Memory {
                message,
                context,
                source,
            } => {
                tracing::error!(
                    error_type = "memory",
                    message = %message,
                    session_id = ?context.session_id,
                    user_id = ?context.user_id,
                    timestamp = %context.timestamp,
                    operation = %context.operation,
                    details = ?context.details,
                    source = ?source,
                    "Memory error occurred"
                );
            }
            Self::Context {
                message,
                context,
                source,
            } => {
                tracing::error!(
                    error_type = "context",
                    message = %message,
                    session_id = ?context.session_id,
                    user_id = ?context.user_id,
                    timestamp = %context.timestamp,
                    operation = %context.operation,
                    details = ?context.details,
                    source = ?source,
                    "Context error occurred"
                );
            }
            Self::Tool {
                message,
                context,
                source,
            } => {
                tracing::error!(
                    error_type = "tool",
                    message = %message,
                    session_id = ?context.session_id,
                    user_id = ?context.user_id,
                    timestamp = %context.timestamp,
                    operation = %context.operation,
                    details = ?context.details,
                    source = ?source,
                    "Tool error occurred"
                );
            }
            Self::ToolSimple(message) => {
                tracing::error!(error_type = "tool", message = %message, "Tool error occurred");
            }
            _ => {
                tracing::error!(error = %self, "Error occurred");
            }
        }
    }

    /// 获取错误上下文（如果有）
    pub fn context(&self) -> Option<&ErrorContext> {
        match self {
            Self::Session { context, .. } => Some(context),
            Self::Memory { context, .. } => Some(context),
            Self::Context { context, .. } => Some(context),
            Self::Tool { context, .. } => Some(context),
            _ => None,
        }
    }

    /// 创建一个 Session 错误
    pub fn session(
        message: impl Into<String>,
        operation: impl Into<String>,
        session_id: Option<impl Into<String>>,
    ) -> Self {
        let mut context = ErrorContext::new(operation);
        if let Some(id) = session_id {
            context = context.with_session_id(id);
        }

        Self::Session {
            message: message.into(),
            context,
            source: None,
        }
    }

    /// 创建一个 Memory 错误
    pub fn memory(
        message: impl Into<String>,
        operation: impl Into<String>,
        session_id: Option<impl Into<String>>,
    ) -> Self {
        let mut context = ErrorContext::new(operation);
        if let Some(id) = session_id {
            context = context.with_session_id(id);
        }

        Self::Memory {
            message: message.into(),
            context,
            source: None,
        }
    }

    /// 创建一个 Tool 错误
    pub fn tool(
        message: impl Into<String>,
        operation: impl Into<String>,
        session_id: Option<impl Into<String>>,
    ) -> Self {
        let mut context = ErrorContext::new(operation);
        if let Some(id) = session_id {
            context = context.with_session_id(id);
        }

        Self::Tool {
            message: message.into(),
            context,
            source: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_context_creation() {
        let ctx = ErrorContext::new("test_operation")
            .with_session_id("session_123")
            .with_user_id("user_456")
            .with_detail("key1", serde_json::json!("value1"));

        assert_eq!(ctx.session_id, Some("session_123".to_string()));
        assert_eq!(ctx.user_id, Some("user_456".to_string()));
        assert_eq!(ctx.operation, "test_operation");
        assert_eq!(ctx.details["key1"], "value1");
    }

    #[test]
    fn test_hakimi_error_session() {
        let err = HakimiError::session("Session not found", "get_messages", Some("session_123"));

        match err {
            HakimiError::Session {
                message, context, ..
            } => {
                assert_eq!(message, "Session not found");
                assert_eq!(context.session_id, Some("session_123".to_string()));
                assert_eq!(context.operation, "get_messages");
            }
            _ => panic!("Expected Session error"),
        }
    }

    #[test]
    fn test_error_context_retrieval() {
        let err = HakimiError::memory("File too large", "load_memory", Some("session_789"));

        let ctx = err.context().unwrap();
        assert_eq!(ctx.session_id, Some("session_789".to_string()));
        assert_eq!(ctx.operation, "load_memory");
    }
}
