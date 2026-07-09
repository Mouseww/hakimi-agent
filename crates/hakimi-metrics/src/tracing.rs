use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

/// Span ID - 唯一标识一个 Span
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId(Uuid);

impl SpanId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SpanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Trace ID - 标识整个调用链
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(Uuid);

impl TraceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Span 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpanStatus {
    /// Span 正在运行
    Running,
    /// Span 成功完成
    Success,
    /// Span 失败
    Error,
    /// Span 被取消
    Cancelled,
}

/// Span 数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// Span ID
    pub span_id: SpanId,
    /// Trace ID
    pub trace_id: TraceId,
    /// 父 Span ID（可选）
    pub parent_span_id: Option<SpanId>,
    /// Span 名称
    pub name: String,
    /// 开始时间
    pub start_time: DateTime<Utc>,
    /// 结束时间（可选）
    pub end_time: Option<DateTime<Utc>>,
    /// 持续时间（纳秒）
    pub duration_ns: Option<u64>,
    /// Span 状态
    pub status: SpanStatus,
    /// 标签/属性
    pub tags: HashMap<String, String>,
    /// 事件列表
    pub events: Vec<SpanEvent>,
}

impl Span {
    /// 创建新的 Span
    pub fn new(name: impl Into<String>, trace_id: TraceId) -> Self {
        Self {
            span_id: SpanId::new(),
            trace_id,
            parent_span_id: None,
            name: name.into(),
            start_time: Utc::now(),
            end_time: None,
            duration_ns: None,
            status: SpanStatus::Running,
            tags: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// 创建子 Span
    pub fn child(&self, name: impl Into<String>) -> Self {
        Self {
            span_id: SpanId::new(),
            trace_id: self.trace_id,
            parent_span_id: Some(self.span_id),
            name: name.into(),
            start_time: Utc::now(),
            end_time: None,
            duration_ns: None,
            status: SpanStatus::Running,
            tags: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// 添加标签
    pub fn add_tag(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.tags.insert(key.into(), value.into());
    }

    /// 添加事件
    pub fn add_event(&mut self, event: SpanEvent) {
        self.events.push(event);
    }

    /// 记录错误
    pub fn record_error(&mut self, error: impl Into<String>) {
        self.status = SpanStatus::Error;
        self.add_tag("error", error.into());
    }

    /// 结束 Span
    pub fn finish(&mut self) {
        if self.end_time.is_none() {
            self.end_time = Some(Utc::now());
            if let Some(end) = self.end_time {
                let duration = end.signed_duration_since(self.start_time);
                self.duration_ns = Some(duration.num_nanoseconds().unwrap_or(0) as u64);
            }
            if self.status == SpanStatus::Running {
                self.status = SpanStatus::Success;
            }
        }
    }

    /// 获取持续时间
    pub fn duration(&self) -> Option<Duration> {
        self.duration_ns.map(Duration::from_nanos)
    }
}

/// Span 事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    /// 事件时间
    pub timestamp: DateTime<Utc>,
    /// 事件名称
    pub name: String,
    /// 事件属性
    pub attributes: HashMap<String, String>,
}

impl SpanEvent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            name: name.into(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// Span 上下文 - 管理当前线程的 Span
pub struct SpanContext {
    current_span: Option<Span>,
}

impl SpanContext {
    pub fn new(span: Span) -> Self {
        Self {
            current_span: Some(span),
        }
    }

    pub fn span(&self) -> Option<&Span> {
        self.current_span.as_ref()
    }

    pub fn span_mut(&mut self) -> Option<&mut Span> {
        self.current_span.as_mut()
    }

    pub fn finish(mut self) -> Option<Span> {
        if let Some(span) = self.current_span.as_mut() {
            span.finish();
        }
        self.current_span.take()
    }
}

impl Drop for SpanContext {
    fn drop(&mut self) {
        if let Some(span) = self.current_span.as_mut() {
            if span.end_time.is_none() {
                span.finish();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_lifecycle() {
        let trace_id = TraceId::new();
        let mut span = Span::new("test_span", trace_id);

        assert_eq!(span.status, SpanStatus::Running);
        assert!(span.end_time.is_none());

        span.add_tag("key", "value");
        span.finish();

        assert_eq!(span.status, SpanStatus::Success);
        assert!(span.end_time.is_some());
        assert!(span.duration_ns.is_some());
    }

    #[test]
    fn test_child_span() {
        let trace_id = TraceId::new();
        let parent = Span::new("parent", trace_id);
        let child = parent.child("child");

        assert_eq!(child.trace_id, parent.trace_id);
        assert_eq!(child.parent_span_id, Some(parent.span_id));
    }

    #[test]
    fn test_span_context() {
        let trace_id = TraceId::new();
        let span = Span::new("context_test", trace_id);
        let span_id = span.span_id;

        let mut ctx = SpanContext::new(span);
        ctx.span_mut().unwrap().add_tag("test", "value");

        let finished = ctx.finish().unwrap();
        assert_eq!(finished.span_id, span_id);
        assert!(finished.end_time.is_some());
    }

    #[test]
    fn test_span_event() {
        let mut span = Span::new("event_test", TraceId::new());

        let event = SpanEvent::new("test_event").with_attribute("key", "value");

        span.add_event(event);
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "test_event");
    }
}
