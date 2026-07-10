//! Hakimi Metrics - 内存内性能指标收集
//!
//! 提供轻量级、无外部依赖的性能监控。

use std::time::Duration;

pub mod error_tracker;
pub mod memory_store;
pub mod tracer;
pub mod tracing;

pub use error_tracker::{
    ErrorCategory, ErrorRecord, ErrorSeverity, ErrorStats, ErrorTracker, RecoveryStrategy,
};
pub use memory_store::MemoryMetricsStore;
pub use tracer::{Tracer, TracerStats};
pub use tracing::{Span, SpanContext, SpanEvent, SpanId, SpanStatus, TraceId};

/// 全局 metrics 注册表
pub trait MetricsRecorder: Send + Sync {
    /// 记录操作耗时
    fn record_duration(&self, name: &str, duration: Duration);

    /// 记录计数
    fn increment_counter(&self, name: &str, value: u64);

    /// 获取所有 metrics 快照
    fn snapshot(&self) -> MetricsSnapshot;

    /// 重置所有 metrics（用于测试）
    fn reset(&self);
}

/// Metrics 快照
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSnapshot {
    pub durations: Vec<DurationMetric>,
    pub counters: Vec<CounterMetric>,
}

/// 耗时指标
#[derive(Debug, Clone, serde::Serialize)]
pub struct DurationMetric {
    pub name: String,
    pub count: u64,
    pub sum_ms: u64,
    pub min_ms: u64,
    pub max_ms: u64,
    pub avg_ms: f64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
}

/// 计数器指标
#[derive(Debug, Clone, serde::Serialize)]
pub struct CounterMetric {
    pub name: String,
    pub value: u64,
}

/// 全局单例 metrics 存储
use once_cell::sync::Lazy;

static GLOBAL_METRICS: Lazy<MemoryMetricsStore> = Lazy::new(MemoryMetricsStore::new);

/// 获取全局 metrics 实例
pub fn global() -> &'static MemoryMetricsStore {
    &GLOBAL_METRICS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_singleton() {
        let m1 = global();
        let m2 = global();
        assert!(std::ptr::eq(m1, m2));
    }
}
