//! 错误追踪模块
//!
//! 提供错误分类、记录、统计和恢复策略。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// 错误严重程度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorSeverity {
    /// 低级别 - 可以忽略的错误
    Low,
    /// 中等 - 需要注意但不影响核心功能
    Medium,
    /// 高级别 - 影响核心功能
    High,
    /// 严重 - 系统无法正常运行
    Critical,
}

/// 错误类别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCategory {
    /// 网络错误
    Network,
    /// 数据库错误
    Database,
    /// 文件系统错误
    FileSystem,
    /// 配置错误
    Configuration,
    /// 认证/授权错误
    Authentication,
    /// 业务逻辑错误
    Business,
    /// 外部服务错误
    ExternalService,
    /// 内部错误
    Internal,
    /// 未知错误
    Unknown,
}

/// 错误记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorRecord {
    /// 错误 ID
    pub id: String,
    /// 错误消息
    pub message: String,
    /// 错误类别
    pub category: ErrorCategory,
    /// 错误严重程度
    pub severity: ErrorSeverity,
    /// 发生时间戳
    pub timestamp: u64,
    /// 上下文信息
    pub context: HashMap<String, String>,
    /// 堆栈追踪（如果有）
    pub stack_trace: Option<String>,
    /// 是否已恢复
    pub recovered: bool,
    /// 恢复尝试次数
    pub recovery_attempts: u32,
}

impl ErrorRecord {
    /// 创建新的错误记录
    pub fn new(
        message: impl Into<String>,
        category: ErrorCategory,
        severity: ErrorSeverity,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id: format!("err_{}", timestamp),
            message: message.into(),
            category,
            severity,
            timestamp,
            context: HashMap::new(),
            stack_trace: None,
            recovered: false,
            recovery_attempts: 0,
        }
    }

    /// 添加上下文信息
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// 添加堆栈追踪
    pub fn with_stack_trace(mut self, trace: impl Into<String>) -> Self {
        self.stack_trace = Some(trace.into());
        self
    }

    /// 标记为已恢复
    pub fn mark_recovered(&mut self) {
        self.recovered = true;
    }

    /// 增加恢复尝试次数
    pub fn increment_recovery_attempts(&mut self) {
        self.recovery_attempts += 1;
    }
}

/// 错误统计
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorStats {
    /// 总错误数
    pub total_errors: u64,
    /// 按类别统计
    pub by_category: HashMap<ErrorCategory, u64>,
    /// 按严重程度统计
    pub by_severity: HashMap<ErrorSeverity, u64>,
    /// 已恢复错误数
    pub recovered_errors: u64,
    /// 未恢复错误数
    pub unrecovered_errors: u64,
}

/// 错误恢复策略
pub trait RecoveryStrategy: Send + Sync {
    /// 尝试恢复错误
    fn attempt_recovery(&self, error: &mut ErrorRecord) -> Result<(), String>;

    /// 是否可以重试
    fn can_retry(&self, error: &ErrorRecord) -> bool;

    /// 获取最大重试次数
    fn max_retries(&self) -> u32;
}

/// 默认恢复策略
pub struct DefaultRecoveryStrategy {
    max_retries: u32,
}

impl Default for DefaultRecoveryStrategy {
    fn default() -> Self {
        Self { max_retries: 3 }
    }
}

impl RecoveryStrategy for DefaultRecoveryStrategy {
    fn attempt_recovery(&self, error: &mut ErrorRecord) -> Result<(), String> {
        error.increment_recovery_attempts();

        // 简单的恢复逻辑：如果错误严重程度不是 Critical，可以尝试恢复
        match error.severity {
            ErrorSeverity::Critical => Err("Critical errors cannot be auto-recovered".to_string()),
            _ => {
                if error.recovery_attempts <= self.max_retries {
                    error.mark_recovered();
                    Ok(())
                } else {
                    Err("Max recovery attempts reached".to_string())
                }
            }
        }
    }

    fn can_retry(&self, error: &ErrorRecord) -> bool {
        error.recovery_attempts < self.max_retries && error.severity != ErrorSeverity::Critical
    }

    fn max_retries(&self) -> u32 {
        self.max_retries
    }
}

/// 错误追踪器
pub struct ErrorTracker {
    /// 错误记录存储
    errors: Arc<Mutex<Vec<ErrorRecord>>>,
    /// 恢复策略
    recovery_strategy: Arc<dyn RecoveryStrategy>,
    /// 最大存储错误数（防止内存溢出）
    max_stored_errors: usize,
}

impl Default for ErrorTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorTracker {
    /// 创建新的错误追踪器
    pub fn new() -> Self {
        Self {
            errors: Arc::new(Mutex::new(Vec::new())),
            recovery_strategy: Arc::new(DefaultRecoveryStrategy::default()),
            max_stored_errors: 1000,
        }
    }

    /// 设置恢复策略
    pub fn with_recovery_strategy(mut self, strategy: Arc<dyn RecoveryStrategy>) -> Self {
        self.recovery_strategy = strategy;
        self
    }

    /// 设置最大存储错误数
    pub fn with_max_stored_errors(mut self, max: usize) -> Self {
        self.max_stored_errors = max;
        self
    }

    /// 记录错误
    pub fn track_error(&self, error: ErrorRecord) {
        let mut errors = self.errors.lock().unwrap();

        // 如果超过最大存储数，移除最旧的错误
        if errors.len() >= self.max_stored_errors {
            errors.remove(0);
        }

        errors.push(error);
    }

    /// 尝试恢复错误
    pub fn attempt_recovery(&self, error_id: &str) -> Result<(), String> {
        let mut errors = self.errors.lock().unwrap();

        if let Some(error) = errors.iter_mut().find(|e| e.id == error_id) {
            self.recovery_strategy.attempt_recovery(error)
        } else {
            Err(format!("Error with id {} not found", error_id))
        }
    }

    /// 获取错误统计
    pub fn get_stats(&self) -> ErrorStats {
        let errors = self.errors.lock().unwrap();

        let mut stats = ErrorStats::default();
        stats.total_errors = errors.len() as u64;

        for error in errors.iter() {
            *stats.by_category.entry(error.category).or_insert(0) += 1;
            *stats.by_severity.entry(error.severity).or_insert(0) += 1;

            if error.recovered {
                stats.recovered_errors += 1;
            } else {
                stats.unrecovered_errors += 1;
            }
        }

        stats
    }

    /// 获取所有错误记录
    pub fn get_errors(&self) -> Vec<ErrorRecord> {
        self.errors.lock().unwrap().clone()
    }

    /// 按类别筛选错误
    pub fn get_errors_by_category(&self, category: ErrorCategory) -> Vec<ErrorRecord> {
        let errors = self.errors.lock().unwrap();
        errors
            .iter()
            .filter(|e| e.category == category)
            .cloned()
            .collect()
    }

    /// 按严重程度筛选错误
    pub fn get_errors_by_severity(&self, severity: ErrorSeverity) -> Vec<ErrorRecord> {
        let errors = self.errors.lock().unwrap();
        errors
            .iter()
            .filter(|e| e.severity == severity)
            .cloned()
            .collect()
    }

    /// 获取未恢复的错误
    pub fn get_unrecovered_errors(&self) -> Vec<ErrorRecord> {
        let errors = self.errors.lock().unwrap();
        errors.iter().filter(|e| !e.recovered).cloned().collect()
    }

    /// 清除所有错误记录
    pub fn clear(&self) {
        self.errors.lock().unwrap().clear();
    }

    /// 清除已恢复的错误
    pub fn clear_recovered(&self) {
        let mut errors = self.errors.lock().unwrap();
        errors.retain(|e| !e.recovered);
    }
}

/// 全局错误追踪器实例
use once_cell::sync::Lazy;

static GLOBAL_ERROR_TRACKER: Lazy<ErrorTracker> = Lazy::new(ErrorTracker::new);

/// 获取全局错误追踪器
pub fn global() -> &'static ErrorTracker {
    &GLOBAL_ERROR_TRACKER
}

/// 便捷宏：记录错误
#[macro_export]
macro_rules! track_error {
    ($message:expr, $category:expr, $severity:expr) => {
        $crate::error_tracker::global().track_error(
            $crate::error_tracker::ErrorRecord::new($message, $category, $severity)
        )
    };
    ($message:expr, $category:expr, $severity:expr, $($key:expr => $value:expr),+) => {
        {
            let mut error = $crate::error_tracker::ErrorRecord::new($message, $category, $severity);
            $(
                error = error.with_context($key, $value);
            )+
            $crate::error_tracker::global().track_error(error)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_record_creation() {
        let error = ErrorRecord::new("Test error", ErrorCategory::Internal, ErrorSeverity::Medium);

        assert_eq!(error.message, "Test error");
        assert_eq!(error.category, ErrorCategory::Internal);
        assert_eq!(error.severity, ErrorSeverity::Medium);
        assert!(!error.recovered);
        assert_eq!(error.recovery_attempts, 0);
    }

    #[test]
    fn test_error_with_context() {
        let error = ErrorRecord::new("Test error", ErrorCategory::Database, ErrorSeverity::High)
            .with_context("session_id", "12345")
            .with_context("operation", "query");

        assert_eq!(error.context.get("session_id").unwrap(), "12345");
        assert_eq!(error.context.get("operation").unwrap(), "query");
    }

    #[test]
    fn test_error_tracker() {
        let tracker = ErrorTracker::new();

        let error1 = ErrorRecord::new("Error 1", ErrorCategory::Network, ErrorSeverity::Low);
        let error2 = ErrorRecord::new("Error 2", ErrorCategory::Database, ErrorSeverity::High);

        tracker.track_error(error1);
        tracker.track_error(error2);

        let stats = tracker.get_stats();
        assert_eq!(stats.total_errors, 2);
        assert_eq!(stats.by_category.get(&ErrorCategory::Network).unwrap(), &1);
        assert_eq!(stats.by_category.get(&ErrorCategory::Database).unwrap(), &1);
    }

    #[test]
    fn test_error_recovery() {
        let tracker = ErrorTracker::new();

        let error = ErrorRecord::new(
            "Recoverable error",
            ErrorCategory::Network,
            ErrorSeverity::Medium,
        );
        let error_id = error.id.clone();

        tracker.track_error(error);

        // 尝试恢复
        let result = tracker.attempt_recovery(&error_id);
        assert!(result.is_ok());

        // 检查统计
        let stats = tracker.get_stats();
        assert_eq!(stats.recovered_errors, 1);
        assert_eq!(stats.unrecovered_errors, 0);
    }

    #[test]
    fn test_critical_error_no_recovery() {
        let tracker = ErrorTracker::new();

        let error = ErrorRecord::new(
            "Critical error",
            ErrorCategory::Internal,
            ErrorSeverity::Critical,
        );
        let error_id = error.id.clone();

        tracker.track_error(error);

        // 尝试恢复应该失败
        let result = tracker.attempt_recovery(&error_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_filter_by_category() {
        let tracker = ErrorTracker::new();

        tracker.track_error(ErrorRecord::new(
            "Network error",
            ErrorCategory::Network,
            ErrorSeverity::Low,
        ));
        tracker.track_error(ErrorRecord::new(
            "Database error",
            ErrorCategory::Database,
            ErrorSeverity::High,
        ));
        tracker.track_error(ErrorRecord::new(
            "Another network error",
            ErrorCategory::Network,
            ErrorSeverity::Medium,
        ));

        let network_errors = tracker.get_errors_by_category(ErrorCategory::Network);
        assert_eq!(network_errors.len(), 2);
    }

    #[test]
    fn test_max_stored_errors() {
        let tracker = ErrorTracker::new().with_max_stored_errors(5);

        // 添加 10 个错误
        for i in 0..10 {
            tracker.track_error(ErrorRecord::new(
                format!("Error {}", i),
                ErrorCategory::Internal,
                ErrorSeverity::Low,
            ));
        }

        // 应该只保留最后 5 个
        let errors = tracker.get_errors();
        assert_eq!(errors.len(), 5);
        assert_eq!(errors[0].message, "Error 5");
    }
}
