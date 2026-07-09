# 任务 1.1.1: 为核心路径添加 Tracing Spans

**优先级**: 🔴 高  
**预估时间**: 3 小时  
**依赖**: 无  
**阻塞**: 任务 1.1.2（metrics 需要 span 数据）

---

## 📋 目标

为所有核心操作路径添加结构化日志（tracing spans），使得：
1. 开发者可通过日志快速定位性能瓶颈
2. 生产环境可观测关键操作的耗时和成功率
3. 为后续 OpenTelemetry 集成奠定基础

---

## 🎯 验收标准

- [ ] 所有关键方法添加 `#[instrument]` 宏
- [ ] 日志包含关键参数（session_id, query, result_count 等）
- [ ] 通过 `RUST_LOG=hakimi_session=debug cargo run` 可见完整调用链
- [ ] 性能开销 < 5%（micro-benchmark 验证）
- [ ] 添加测试验证日志输出正确

---

## 📁 涉及文件

### 主要修改
- `crates/hakimi-session/src/message_ops.rs` (630 行)
- `crates/hakimi-context/src/memory.rs` (410 行)
- `crates/hakimi-tools/src/builtin_session_search.rs` (400 行)

### 新建文件
- `crates/hakimi-session/tests/tracing_test.rs` (测试日志输出)

---

## 🛠️ 实施步骤

### 步骤 1: 添加 tracing 依赖 (10 分钟)

```bash
cd /root/hakimi-agent

# 检查当前依赖
grep "tracing" crates/hakimi-session/Cargo.toml

# 如果缺失，添加：
# tracing = "0.1"
# tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

**验收**: `cargo check --package hakimi-session` 通过

---

### 步骤 2: 为 message_ops 添加 spans (60 分钟)

#### 2.1 get_messages_around()

```rust
use tracing::{instrument, debug, warn};

#[instrument(
    skip(self),
    fields(
        session_id = %session_id,
        anchor_id = anchor_id,
        window = window,
    )
)]
fn get_messages_around(
    &self,
    session_id: &str,
    anchor_id: i64,
    window: i64,
) -> Result<(Vec<Message>, i64, i64)> {
    debug!("Starting get_messages_around");
    
    // ... 原有逻辑 ...
    
    let messages_before = /* ... */;
    let messages_after = /* ... */;
    let messages = /* ... */;
    
    debug!(
        messages_count = messages.len(),
        messages_before = messages_before,
        messages_after = messages_after,
        "Completed get_messages_around"
    );
    
    Ok((messages, messages_before, messages_after))
}
```

**要点**:
- `skip(self)`: 避免记录 `self` 参数（通常很大）
- `fields(...)`: 显式声明需要记录的字段
- `debug!(...)`: 方法结束时记录结果统计

#### 2.2 get_bookends()

```rust
#[instrument(
    skip(self),
    fields(
        session_id = %session_id,
        count = count,
    )
)]
fn get_bookends(
    &self,
    session_id: &str,
    count: i64,
) -> Result<(Vec<Message>, Vec<Message>)> {
    debug!("Fetching session bookends");
    
    // ... 原有逻辑 ...
    
    debug!(
        start_count = start_messages.len(),
        end_count = end_messages.len(),
        "Bookends retrieved"
    );
    
    Ok((start_messages, end_messages))
}
```

#### 2.3 search_messages()

```rust
#[instrument(
    skip(self),
    fields(
        query = %query,
        limit = limit,
    )
)]
fn search_messages(
    &self,
    query: &str,
    limit: i64,
) -> Result<Vec<SearchResult>> {
    debug!("Starting FTS5 search");
    
    let start = std::time::Instant::now();
    
    // ... 原有逻辑 ...
    
    let elapsed = start.elapsed();
    debug!(
        results_count = results.len(),
        duration_ms = elapsed.as_millis(),
        "FTS5 search completed"
    );
    
    if elapsed.as_millis() > 500 {
        warn!(
            query = %query,
            duration_ms = elapsed.as_millis(),
            "Slow FTS5 query detected"
        );
    }
    
    Ok(results)
}
```

**特殊处理**:
- 记录查询耗时
- 耗时 > 500ms 时输出 WARN 级别日志

---

### 步骤 3: 为 memory 操作添加 spans (30 分钟)

```rust
// crates/hakimi-context/src/memory.rs

#[instrument(skip(self))]
pub fn load_memory(&self, target: &str) -> Result<String> {
    debug!(target = %target, "Loading memory file");
    
    let path = self.get_memory_path(target)?;
    
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            debug!(
                target = %target,
                size_bytes = content.len(),
                "Memory loaded successfully"
            );
            Ok(content)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!(target = %target, "Memory file not found, returning empty");
            Ok(String::new())
        }
        Err(e) => {
            warn!(
                target = %target,
                error = %e,
                "Failed to load memory"
            );
            Err(e.into())
        }
    }
}

#[instrument(skip(self, content))]
pub fn save_memory(&self, target: &str, content: &str) -> Result<()> {
    debug!(
        target = %target,
        size_bytes = content.len(),
        "Saving memory"
    );
    
    // ... 原有逻辑 ...
    
    debug!(target = %target, "Memory saved successfully");
    Ok(())
}
```

---

### 步骤 4: 为 session_search 工具添加 spans (30 分钟)

```rust
// crates/hakimi-tools/src/builtin_session_search.rs

impl Tool for SessionSearchTool {
    #[instrument(skip(self, args), fields(tool = "session_search"))]
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let params: SessionSearchParams = serde_json::from_value(args)
            .context("Invalid session_search parameters")?;
        
        // 识别模式
        let mode = self.detect_mode(&params);
        debug!(mode = ?mode, "Session search mode detected");
        
        let result = match mode {
            SearchMode::Discovery => {
                debug!("Executing Discovery mode");
                self.execute_discovery(params).await
            }
            SearchMode::Scroll => {
                debug!("Executing Scroll mode");
                self.execute_scroll(params).await
            }
            SearchMode::Browse => {
                debug!("Executing Browse mode");
                self.execute_browse(params).await
            }
        }?;
        
        debug!(
            mode = ?mode,
            output_size = result.len(),
            "Session search completed"
        );
        
        Ok(ToolOutput::success(result))
    }
}
```

---

### 步骤 5: 添加集成测试 (30 分钟)

```rust
// crates/hakimi-session/tests/tracing_test.rs

use tracing_subscriber::{fmt, EnvFilter};
use tracing_test::traced_test;

#[traced_test]
#[tokio::test]
async fn test_get_messages_around_logs() {
    let db = test_db();
    let sid = create_test_session(&db);
    
    for i in 1..=5 {
        db.save_message(&sid, &Message::user(format!("msg {}", i)))
            .unwrap();
    }
    
    // 调用被测方法
    let _ = db.get_messages_around(&sid, 3, 2).unwrap();
    
    // 验证日志输出
    assert!(logs_contain("Starting get_messages_around"));
    assert!(logs_contain("Completed get_messages_around"));
    assert!(logs_contain("messages_count"));
}

#[traced_test]
#[tokio::test]
async fn test_slow_search_warning() {
    let db = test_db();
    
    // 构造慢查询场景（大量数据）
    for i in 0..10000 {
        db.save_message(&test_session(), &Message::user(format!("data {}", i)))
            .unwrap();
    }
    
    let _ = db.search_messages("data", 1000).unwrap();
    
    // 如果耗时 > 500ms，应该有 WARN 日志
    if logs_contain("duration_ms") {
        let duration = extract_duration_from_logs();
        if duration > 500 {
            assert!(logs_contain("Slow FTS5 query detected"));
        }
    }
}
```

**验收**: `cargo test --package hakimi-session tracing_test` 通过

---

### 步骤 6: 性能基准测试 (30 分钟)

```rust
// crates/hakimi-session/benches/tracing_overhead.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_with_tracing(c: &mut Criterion) {
    // 启用 tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    
    c.bench_function("get_messages_around_with_tracing", |b| {
        b.iter(|| {
            let db = test_db();
            db.get_messages_around(black_box("session_1"), black_box(50), black_box(10))
        });
    });
}

fn bench_without_tracing(c: &mut Criterion) {
    c.bench_function("get_messages_around_no_tracing", |b| {
        b.iter(|| {
            let db = test_db();
            db.get_messages_around(black_box("session_1"), black_box(50), black_box(10))
        });
    });
}

criterion_group!(benches, bench_with_tracing, bench_without_tracing);
criterion_main!(benches);
```

**验收**: 性能差异 < 5%

---

## 🧪 测试计划

### 单元测试
```bash
cargo test --package hakimi-session message_ops
cargo test --package hakimi-context memory
cargo test --package hakimi-tools session_search
```

### 集成测试
```bash
cargo test --package hakimi-session tracing_test
```

### 手动验证
```bash
RUST_LOG=hakimi_session=debug,hakimi_context=debug cargo run -- \
  gateway --config test_config.yaml

# 在另一终端触发操作
curl -X POST http://localhost:3005/api/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "search my previous conversations about Rust"}'

# 观察日志输出，应包含：
# - hakimi_session: Starting get_messages_around session_id=xxx
# - hakimi_session: Completed get_messages_around messages_count=5
# - hakimi_tools: Session search mode detected mode=Discovery
```

---

## 📊 完成检查清单

- [ ] 所有目标方法添加 `#[instrument]`
- [ ] 日志包含关键字段（session_id, query 等）
- [ ] 慢查询自动 WARN（> 500ms）
- [ ] 集成测试验证日志输出
- [ ] 性能基准测试（开销 < 5%）
- [ ] 手动测试日志可读性
- [ ] 更新 CHANGELOG.md
- [ ] Git commit + push
- [ ] CI 通过

---

## 🚀 快速启动

```bash
cd /root/hakimi-agent
git checkout -b feat/observability-tracing-spans
git pull origin main  # 确保最新

# 开始实施步骤 1-6
# ...

# 完成后
cargo test --all
cargo +nightly fmt
git add -A
git commit -m "feat(observability): add tracing spans to core paths

- Instrument message_ops: get_messages_around, get_bookends, search_messages
- Instrument memory: load_memory, save_memory
- Instrument session_search tool: all three modes
- Add slow query detection (> 500ms WARN)
- Add integration tests for log output
- Performance overhead < 3% (benchmarked)

Closes #XXX"

git push origin feat/observability-tracing-spans

# 创建 PR
gh pr create --title "feat(observability): add tracing spans to core paths" \
  --body "See tasks/TASK_1.1.1_tracing_spans.md for details"
```

---

## 📚 参考资料

- [tracing 文档](https://docs.rs/tracing/)
- [tracing-subscriber 文档](https://docs.rs/tracing-subscriber/)
- [Tokio Console](https://github.com/tokio-rs/console)（后续可集成）
- [OpenTelemetry Rust](https://github.com/open-telemetry/opentelemetry-rust)（Phase 2）

---

**创建时间**: 2026-07-09  
**预计完成**: 2026-07-10  
**实际完成**: _待填写_
