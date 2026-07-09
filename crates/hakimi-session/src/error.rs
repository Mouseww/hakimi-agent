//! Session 操作相关错误类型

use hakimi_common::error::{ErrorContext, HakimiError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    NotFound(String),

    #[error("Invalid session ID: {0}")]
    InvalidId(String),

    #[error("Message not found: id={0}")]
    MessageNotFound(i64),

    #[error("FTS5 search failed: {0}")]
    SearchFailed(String),

    #[error("Database operation failed: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("Serialization failed: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl SessionError {
    /// 转换为 HakimiError（添加上下文）
    pub fn into_hakimi_error(
        self,
        operation: impl Into<String>,
        session_id: Option<impl Into<String>>,
    ) -> HakimiError {
        let mut context = ErrorContext::new(operation);
        if let Some(id) = session_id {
            context = context.with_session_id(id);
        }

        HakimiError::Session {
            message: self.to_string(),
            context,
            source: Some(Box::new(self)),
        }
    }
}

/// 便捷宏：自动添加上下文
#[macro_export]
macro_rules! session_error {
    ($err:expr, $op:expr, $session_id:expr) => {
        $err.into_hakimi_error($op, Some($session_id))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_error_conversion() {
        let err = SessionError::NotFound("test_session".to_string());
        let hakimi_err = err.into_hakimi_error("test_operation", Some("test_session"));

        match hakimi_err {
            HakimiError::Session {
                message, context, ..
            } => {
                assert!(message.contains("Session not found"));
                assert_eq!(context.session_id, Some("test_session".to_string()));
                assert_eq!(context.operation, "test_operation");
            }
            _ => panic!("Expected Session error"),
        }
    }

    #[test]
    fn test_message_not_found_error() {
        let err = SessionError::MessageNotFound(123);
        assert!(err.to_string().contains("123"));
    }
}
