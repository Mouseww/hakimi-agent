//! Context 和 Memory 操作相关错误类型

use hakimi_common::error::{ErrorContext, HakimiError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Memory file not found: {0}")]
    FileNotFound(String),

    #[error("Memory file too large: {size} bytes (limit: {limit})")]
    FileTooLarge { size: usize, limit: usize },

    #[error("Invalid memory target: {0}")]
    InvalidTarget(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl MemoryError {
    pub fn into_hakimi_error(
        self,
        operation: impl Into<String>,
        session_id: Option<impl Into<String>>,
        target: Option<&str>,
    ) -> HakimiError {
        let mut context = ErrorContext::new(operation);
        if let Some(id) = session_id {
            context = context.with_session_id(id);
        }
        if let Some(t) = target {
            context = context.with_detail("target", serde_json::json!(t));
        }

        HakimiError::Memory {
            message: self.to_string(),
            context,
            source: Some(Box::new(self)),
        }
    }
}

/// 便捷宏
#[macro_export]
macro_rules! memory_error {
    ($err:expr, $op:expr, $session_id:expr, $target:expr) => {
        $err.into_hakimi_error($op, Some($session_id), Some($target))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_error_file_too_large() {
        let err = MemoryError::FileTooLarge {
            size: 70000,
            limit: 64000,
        };
        let hakimi_err = err.into_hakimi_error("load_memory", Some("test_session"), Some("memory"));

        match hakimi_err {
            HakimiError::Memory {
                message, context, ..
            } => {
                assert!(message.contains("too large"));
                assert!(message.contains("70000"));
                assert_eq!(context.session_id, Some("test_session".to_string()));
                assert_eq!(context.operation, "load_memory");
                assert_eq!(
                    context.details.get("target").and_then(|v| v.as_str()),
                    Some("memory")
                );
            }
            _ => panic!("Expected Memory error"),
        }
    }

    #[test]
    fn test_memory_error_invalid_target() {
        let err = MemoryError::InvalidTarget("invalid_target".to_string());
        assert!(err.to_string().contains("invalid_target"));
    }
}
