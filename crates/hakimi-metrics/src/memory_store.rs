//! 内存内 metrics 存储实现

use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::Duration;

use crate::{CounterMetric, DurationMetric, MetricsRecorder, MetricsSnapshot};

/// 内存内 metrics 存储
pub struct MemoryMetricsStore {
    durations: RwLock<HashMap<String, Vec<u64>>>,
    counters: RwLock<HashMap<String, u64>>,
}

impl MemoryMetricsStore {
    pub fn new() -> Self {
        Self {
            durations: RwLock::new(HashMap::new()),
            counters: RwLock::new(HashMap::new()),
        }
    }

    /// 计算百分位数
    fn percentile(sorted: &[u64], p: f64) -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        let index = ((sorted.len() as f64) * p).ceil() as usize - 1;
        sorted[index.min(sorted.len() - 1)]
    }
}

impl Default for MemoryMetricsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsRecorder for MemoryMetricsStore {
    fn record_duration(&self, name: &str, duration: Duration) {
        let ms = duration.as_millis() as u64;
        let mut durations = self.durations.write();
        durations
            .entry(name.to_string())
            .or_insert_with(Vec::new)
            .push(ms);
    }

    fn increment_counter(&self, name: &str, value: u64) {
        let mut counters = self.counters.write();
        *counters.entry(name.to_string()).or_insert(0) += value;
    }

    fn snapshot(&self) -> MetricsSnapshot {
        let durations = self.durations.read();
        let counters = self.counters.read();

        let duration_metrics: Vec<DurationMetric> = durations
            .iter()
            .map(|(name, samples)| {
                let mut sorted = samples.clone();
                sorted.sort_unstable();

                let count = sorted.len() as u64;
                let sum_ms: u64 = sorted.iter().sum();
                let min_ms = *sorted.first().unwrap_or(&0);
                let max_ms = *sorted.last().unwrap_or(&0);
                let avg_ms = if count > 0 {
                    sum_ms as f64 / count as f64
                } else {
                    0.0
                };

                let p50_ms = Self::percentile(&sorted, 0.50);
                let p95_ms = Self::percentile(&sorted, 0.95);
                let p99_ms = Self::percentile(&sorted, 0.99);

                DurationMetric {
                    name: name.clone(),
                    count,
                    sum_ms,
                    min_ms,
                    max_ms,
                    avg_ms,
                    p50_ms,
                    p95_ms,
                    p99_ms,
                }
            })
            .collect();

        let counter_metrics: Vec<CounterMetric> = counters
            .iter()
            .map(|(name, value)| CounterMetric {
                name: name.clone(),
                value: *value,
            })
            .collect();

        MetricsSnapshot {
            durations: duration_metrics,
            counters: counter_metrics,
        }
    }

    fn reset(&self) {
        self.durations.write().clear();
        self.counters.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_duration() {
        let store = MemoryMetricsStore::new();

        store.record_duration("test.op", Duration::from_millis(100));
        store.record_duration("test.op", Duration::from_millis(200));
        store.record_duration("test.op", Duration::from_millis(150));

        let snapshot = store.snapshot();
        let metric = snapshot
            .durations
            .iter()
            .find(|m| m.name == "test.op")
            .unwrap();

        assert_eq!(metric.count, 3);
        assert_eq!(metric.min_ms, 100);
        assert_eq!(metric.max_ms, 200);
        assert_eq!(metric.p50_ms, 150);
        assert!((metric.avg_ms - 150.0).abs() < 0.1);
    }

    #[test]
    fn test_increment_counter() {
        let store = MemoryMetricsStore::new();

        store.increment_counter("test.count", 5);
        store.increment_counter("test.count", 3);

        let snapshot = store.snapshot();
        let counter = snapshot
            .counters
            .iter()
            .find(|c| c.name == "test.count")
            .unwrap();

        assert_eq!(counter.value, 8);
    }

    #[test]
    fn test_percentile_calculation() {
        let samples = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(MemoryMetricsStore::percentile(&samples, 0.50), 50);
        assert_eq!(MemoryMetricsStore::percentile(&samples, 0.95), 100); // 修正: 10 个样本的 95% 是最后一个
        assert_eq!(MemoryMetricsStore::percentile(&samples, 0.99), 100);
    }

    #[test]
    fn test_reset() {
        let store = MemoryMetricsStore::new();

        store.record_duration("test.op", Duration::from_millis(100));
        store.increment_counter("test.count", 5);

        let snapshot = store.snapshot();
        assert_eq!(snapshot.durations.len(), 1);
        assert_eq!(snapshot.counters.len(), 1);

        store.reset();

        let snapshot = store.snapshot();
        assert_eq!(snapshot.durations.len(), 0);
        assert_eq!(snapshot.counters.len(), 0);
    }

    #[test]
    fn test_empty_snapshot() {
        let store = MemoryMetricsStore::new();
        let snapshot = store.snapshot();

        assert!(snapshot.durations.is_empty());
        assert!(snapshot.counters.is_empty());
    }
}
