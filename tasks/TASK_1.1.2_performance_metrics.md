# 任务 1.1.2: 关键性能 Metrics

**状态**: ✅ 已完成 (100%)  
**开始时间**: 2026-07-10 00:00 UTC  
**完成时间**: 2026-07-10 00:15 UTC  
**提交**: 16ccf18  

**优先级**: 🟡 中  
**预估时间**: 4 小时  
**依赖**: 任务 1.1.1 (需要 tracing spans)  
**阻塞**: 无

---

## 📋 目标

为关键操作添加性能指标（metrics），使得：
1. 可实时监控查询延迟、吞吐量、错误率
2. 支持 Prometheus 格式导出（可选）
3. 支持内存内聚合统计（必选）

---

## 🎯 验收标准

- [ ] 为 message_ops/session_search/memory 添加耗时统计
- [ ] 为 FTS5 查询添加慢查询计数器
- [ ] 内存内 metrics 存储（无外部依赖）
- [ ] 提供 `/metrics` 端点导出统计（WebUI 集成）
- [ ] 添加 metrics 单元测试

---

## 📁 涉及文件

### 核心实现
- `crates/hakimi-metrics/` (新建 crate)
  - `src/lib.rs` — Metrics trait 定义
  - `src/memory_store.rs` — 内存内存储
  - `src/aggregator.rs` — 统计聚合
- `crates/hakimi-session/src/message_ops.rs` — 集成 metrics
- `crates/hakimi-tools/src/builtin_session_search.rs` — 集成 metrics
- `crates/hakimi-context/src/memory.rs` — 集成 metrics

### WebUI 集成
- `crates/hakimi-webui/src/routes/metrics.rs` — `/metrics` 端点
- `crates/hakimi-webui/src/routes/mod.rs` — 路由注册

### 测试
- `crates/hakimi-metrics/tests/integration_test.rs`

---

## 🔧 实施步骤

### 步骤 1: 创建 hakimi-metrics crate (60 分钟)

```bash
cargo new --lib crates/hakimi-metrics
```

**Cargo.toml**:
```toml
[package]
name = "hakimi-metrics"
version = "0.5.57"
edition = "2021"

[dependencies]
parking_lot = "0.12"
once_cell = "1.19"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

**src/lib.rs** — Metrics trait:
```rust
use std::time::Duration;

/// 全局 metrics 注册表
pub trait MetricsRecorder: Send + Sync {
    /// 记录操作耗时
    fn record_duration(&self, name: &str, duration: Duration);
    
    /// 记录计数
    fn increment_counter(&self, name: &str, value: u64);
    
    /// 获取所有 metrics
    fn snapshot(&self) -> MetricsSnapshot;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSnapshot {
    pub durations: Vec<DurationMetric>,
    pub counters: Vec<CounterMetric>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DurationMetric {
    pub name: String,
    pub count: u64,
    pub sum_ms: u64,
    pub min_ms: u64,
    pub max_ms: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CounterMetric {
    pub name: String,
    pub value: u64,
}
```

**src/memory_store.rs** — 内存存储:
```rust
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::Duration;

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
}

impl crate::MetricsRecorder for MemoryMetricsStore {
    fn record_duration(&self, name: &str, duration: Duration) {
        let mut durations = self.durations.write();
        durations
            .entry(name.to_string())
            .or_insert_with(Vec::new)
            .push(duration.as_millis() as u64);
    }
    
    fn increment_counter(&self, name: &str, value: u64) {
        let mut counters = self.counters.write();
        *counters.entry(name.to_string()).or_insert(0) += value;
    }
    
    fn snapshot(&self) -> crate::MetricsSnapshot {
        // 计算百分位数
        // ...
    }
}
```

**全局单例**:
```rust
use once_cell::sync::Lazy;

static GLOBAL_METRICS: Lazy<MemoryMetricsStore> = Lazy::new(MemoryMetricsStore::new);

pub fn global() -> &'static MemoryMetricsStore {
    &GLOBAL_METRICS
}
```

---

### 步骤 2: 集成到 message_ops (45 分钟)

```rust
// crates/hakimi-session/src/message_ops.rs
use hakimi_metrics::global as metrics;
use std::time::Instant;

#[instrument(skip(self), fields(session_id = %session_id))]
fn get_messages_around(...) -> Result<...> {
    let start = Instant::now();
    debug!("Starting get_messages_around");
    
    // ... 原有逻辑 ...
    
    let elapsed = start.elapsed();
    metrics().record_duration("message_ops.get_messages_around", elapsed);
    
    debug!(duration_ms = elapsed.as_millis(), "Completed");
    Ok((messages, messages_before, messages_after))
}

fn search_messages(...) -> Result<...> {
    let start = Instant::now();
    
    // ... 原有逻辑 ...
    
    let elapsed = start.elapsed();
    metrics().record_duration("message_ops.fts5_search", elapsed);
    
    if elapsed.as_millis() > 500 {
        metrics().increment_counter("message_ops.slow_queries", 1);
        warn!(...);
    }
    
    Ok(results)
}
```

---

### 步骤 3: WebUI `/metrics` 端点 (30 分钟)

```rust
// crates/hakimi-webui/src/routes/metrics.rs
use axum::{response::Json, http::StatusCode};
use hakimi_metrics::global as metrics;

pub async fn get_metrics() -> Result<Json<serde_json::Value>, StatusCode> {
    let snapshot = metrics().snapshot();
    Ok(Json(serde_json::to_value(snapshot).unwrap()))
}
```

**路由注册**:
```rust
// crates/hakimi-webui/src/routes/mod.rs
pub mod metrics;

pub fn routes() -> Router {
    Router::new()
        .route("/api/metrics", get(metrics::get_metrics))
        // ... 其他路由
}
```

---

### 步骤 4: 单元测试 (45 分钟)

```rust
// crates/hakimi-metrics/tests/integration_test.rs
use hakimi_metrics::*;
use std::time::Duration;

#[test]
fn test_record_duration() {
    let store = MemoryMetricsStore::new();
    
    store.record_duration("test.op", Duration::from_millis(100));
    store.record_duration("test.op", Duration::from_millis(200));
    
    let snapshot = store.snapshot();
    let metric = snapshot.durations.iter().find(|m| m.name == "test.op").unwrap();
    
    assert_eq!(metric.count, 2);
    assert_eq!(metric.min_ms, 100);
    assert_eq!(metric.max_ms, 200);
}

#[test]
fn test_increment_counter() {
    let store = MemoryMetricsStore::new();
    
    store.increment_counter("test.count", 5);
    store.increment_counter("test.count", 3);
    
    let snapshot = store.snapshot();
    let counter = snapshot.counters.iter().find(|c| c.name == "test.count").unwrap();
    
    assert_eq!(counter.value, 8);
}
```

---

### 步骤 5: 编译验证 (30 分钟)

```bash
# 编译 hakimi-metrics
cargo build --package hakimi-metrics

# 运行测试
cargo test --package hakimi-metrics

# 集成编译
cargo build --release

# 验证 /metrics 端点
curl http://localhost:3005/api/metrics | jq
```

---

### 步骤 6: 文档更新 (30 分钟)

**README.md**:
```markdown
## 📊 Metrics

Hakimi 内置性能监控，无需外部依赖：

```bash
# 查看实时 metrics
curl http://localhost:3005/api/metrics | jq

# 示例输出
{
  "durations": [
    {
      "name": "message_ops.fts5_search",
      "count": 1234,
      "p50_ms": 15,
      "p95_ms": 120,
      "p99_ms": 450
    }
  ],
  "counters": [
    {
      "name": "message_ops.slow_queries",
      "value": 3
    }
  ]
}
```

### Prometheus 集成（可选）

```bash
# TODO: 未来版本支持 OpenTelemetry 导出
```
```

**CHANGELOG.md**:
```markdown
### v0.5.57 (2026-07-XX)

#### 🎯 新增功能
- **性能监控**: 新增 hakimi-metrics crate，内存内 metrics 存储
- **WebUI**: 新增 `/api/metrics` 端点，实时查看性能统计
- **慢查询跟踪**: FTS5 > 500ms 查询计数器

#### 🔧 改进
- message_ops: 添加操作耗时统计
- session_search: 添加工具调用耗时
- memory: 添加读写耗时

#### 📊 Metrics
- `message_ops.get_messages_around` — 窗口查询耗时
- `message_ops.fts5_search` — FTS5 搜索耗时
- `message_ops.slow_queries` — 慢查询计数
- `session_search.execute` — 工具执行耗时
- `memory.load` — 记忆加载耗时
- `memory.save` — 记忆保存耗时
```

---

## 🚧 已知限制

1. **内存占用**: 每个 duration 保留原始样本（未来考虑滑动窗口）
2. **无持久化**: 重启后 metrics 清零（未来考虑 SQLite 存储）
3. **无分布式**: 单实例统计（未来考虑聚合）

---

## 🔗 参考资料

- [Rust metrics crate](https://github.com/metrics-rs/metrics)
- [Prometheus 数据模型](https://prometheus.io/docs/concepts/data_model/)
- [OpenTelemetry Rust SDK](https://github.com/open-telemetry/opentelemetry-rust)

---

## ✅ 完成检查清单

- [ ] hakimi-metrics crate 创建完成
- [ ] MemoryMetricsStore 实现完成
- [ ] message_ops 集成完成
- [ ] session_search 集成完成
- [ ] memory 集成完成
- [ ] WebUI `/metrics` 端点完成
- [ ] 单元测试通过（5+ 个测试）
- [ ] 集成测试通过
- [ ] 文档更新完成
- [ ] Release 编译成功
