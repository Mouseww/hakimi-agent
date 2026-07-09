use crate::tracing::{Span, SpanId, TraceId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Tracer - 管理 Span 的收集和存储
#[derive(Clone)]
pub struct Tracer {
    inner: Arc<TracerInner>,
}

struct TracerInner {
    /// 存储所有 Span
    spans: RwLock<HashMap<SpanId, Span>>,
    /// Trace ID 到 Span IDs 的映射
    trace_spans: RwLock<HashMap<TraceId, Vec<SpanId>>>,
}

impl Tracer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TracerInner {
                spans: RwLock::new(HashMap::new()),
                trace_spans: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// 开始一个新的 Trace
    pub fn start_trace(&self, name: impl Into<String>) -> Span {
        let trace_id = TraceId::new();
        Span::new(name, trace_id)
    }

    /// 记录 Span
    pub fn record_span(&self, span: Span) {
        let span_id = span.span_id;
        let trace_id = span.trace_id;

        // 存储 Span
        self.inner.spans.write().unwrap().insert(span_id, span);

        // 更新 Trace 映射
        self.inner
            .trace_spans
            .write()
            .unwrap()
            .entry(trace_id)
            .or_default()
            .push(span_id);
    }

    /// 获取 Span
    pub fn get_span(&self, span_id: SpanId) -> Option<Span> {
        self.inner.spans.read().unwrap().get(&span_id).cloned()
    }

    /// 获取 Trace 中的所有 Span
    pub fn get_trace_spans(&self, trace_id: TraceId) -> Vec<Span> {
        let trace_spans = self.inner.trace_spans.read().unwrap();
        let span_ids = match trace_spans.get(&trace_id) {
            Some(ids) => ids.clone(),
            None => return Vec::new(),
        };

        let spans = self.inner.spans.read().unwrap();
        span_ids
            .iter()
            .filter_map(|id| spans.get(id).cloned())
            .collect()
    }

    /// 获取所有 Trace ID
    pub fn get_trace_ids(&self) -> Vec<TraceId> {
        self.inner
            .trace_spans
            .read()
            .unwrap()
            .keys()
            .copied()
            .collect()
    }

    /// 清理指定 Trace 的所有 Span
    pub fn clear_trace(&self, trace_id: TraceId) {
        let span_ids = {
            let mut trace_spans = self.inner.trace_spans.write().unwrap();
            trace_spans.remove(&trace_id).unwrap_or_default()
        };

        let mut spans = self.inner.spans.write().unwrap();
        for span_id in span_ids {
            spans.remove(&span_id);
        }
    }

    /// 清理所有数据
    pub fn clear_all(&self) {
        self.inner.spans.write().unwrap().clear();
        self.inner.trace_spans.write().unwrap().clear();
    }

    /// 获取统计信息
    pub fn stats(&self) -> TracerStats {
        let spans = self.inner.spans.read().unwrap();
        let trace_spans = self.inner.trace_spans.read().unwrap();

        TracerStats {
            total_spans: spans.len(),
            total_traces: trace_spans.len(),
            avg_spans_per_trace: if trace_spans.is_empty() {
                0.0
            } else {
                spans.len() as f64 / trace_spans.len() as f64
            },
        }
    }
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracer 统计信息
#[derive(Debug, Clone)]
pub struct TracerStats {
    pub total_spans: usize,
    pub total_traces: usize,
    pub avg_spans_per_trace: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracer_basic() {
        let tracer = Tracer::new();
        let mut span = tracer.start_trace("test");
        span.add_tag("key", "value");
        span.finish();

        let span_id = span.span_id;
        tracer.record_span(span);

        let retrieved = tracer.get_span(span_id).unwrap();
        assert_eq!(retrieved.name, "test");
        assert_eq!(retrieved.tags.get("key").unwrap(), "value");
    }

    #[test]
    fn test_tracer_trace_spans() {
        let tracer = Tracer::new();
        let mut parent = tracer.start_trace("parent");
        let trace_id = parent.trace_id;

        let mut child1 = parent.child("child1");
        let mut child2 = parent.child("child2");

        parent.finish();
        child1.finish();
        child2.finish();

        tracer.record_span(parent);
        tracer.record_span(child1);
        tracer.record_span(child2);

        let spans = tracer.get_trace_spans(trace_id);
        assert_eq!(spans.len(), 3);
    }

    #[test]
    fn test_tracer_clear() {
        let tracer = Tracer::new();
        let mut span = tracer.start_trace("test");
        let trace_id = span.trace_id;
        span.finish();

        tracer.record_span(span);
        assert_eq!(tracer.stats().total_spans, 1);

        tracer.clear_trace(trace_id);
        assert_eq!(tracer.stats().total_spans, 0);
    }

    #[test]
    fn test_tracer_stats() {
        let tracer = Tracer::new();

        // Trace 1: 2 spans
        let mut span1 = tracer.start_trace("trace1");
        let mut child1 = span1.child("child");
        span1.finish();
        child1.finish();
        tracer.record_span(span1);
        tracer.record_span(child1);

        // Trace 2: 1 span
        let mut span2 = tracer.start_trace("trace2");
        span2.finish();
        tracer.record_span(span2);

        let stats = tracer.stats();
        assert_eq!(stats.total_spans, 3);
        assert_eq!(stats.total_traces, 2);
        assert_eq!(stats.avg_spans_per_trace, 1.5);
    }
}
