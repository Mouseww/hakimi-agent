# 任务 1.1.3: 错误追踪与报警

**状态**: 🔄 进行中 (0%)  
**开始时间**: 2026-07-10 08:00 UTC  
**预计完成**: 2026-07-10 12:00 UTC  

**优先级**: 🔴 高  
**预估时间**: 4 小时  
**依赖**: 任务 1.1.1, 1.1.2 (tracing + metrics)  
**阻塞**: 任务 1.2.x (记忆管理需要完善的错误处理)

---

## 📋 目标

为 Hakimi Agent 构建结构化错误追踪系统，使得：
1. 所有错误携带完整上下文（session_id, user_id, timestamp）
2. 错误类型化，便于日志查询和告警规则
3. 错误日志包含完整调试信息和 backtrace
4. 为未来集成 Sentry/Datadog 等工具奠定基础

---

## 🎯 验收标准

- [ ] 定义自定义错误类型：`SessionError`, `MemoryError`, `ContextError`, `ToolError`
- [ ] 所有核心 crate 迁移到结构化错误
- [ ] 错误日志包含 session_id/user_id/timestamp 字段
- [ ] 添加错误处理集成测试（>10 个测试用例）
- [ ] 更新文档说明错误处理最佳实践

### 实际完成内容

待执行...

---

## 📁 涉及文件

### 核心实现
- `crates/hakimi-common/src/error.rs` — 错误类型定义
- `crates/hakimi-session/src/error.rs` — Session 专属错误
- `crates/hakimi-context/src/error.rs` — Context/Memory 错误
- `crates/hakimi-tools/src/error.rs` — Tool 执行错误

### 集成点
- `crates/hakimi-session/src/message_ops.rs` — 迁移错误处理
- `crates/hakimi-context/src/memory.rs` — 迁移错误处理
- `crates/hakimi-tools/src/builtin_*.rs` — 各工具错误处理

### 测试
- `crates/hakimi-common/tests/error_test.rs` — 错误转换测试
- `crates/hakimi-session/tests/error_handling_test.rs` — 错误场景测试

---

## 🛠️ 实施步骤

### 步骤 1: 定义通用错误基础设施 (60 分钟)

**crates/hakimi-common/src/error.rs**:
```rust
use std::fmt;
use thiserror::Error;

/// 错误上下文 — 所有自定义错误都应包含此结构
#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorContext {
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    pub timestamp: String,  // ISO 8601
    pub operation: String,  // 操作名称，如 "get_messages_around"
    pub details: serde_json::Value,  // 额外详细信息
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
        if let Some(obj) = self.details.as_object_mut() {
            obj.insert(key.to_string(), value);
        }
        self
    }
}

/// 通用结果类型
pub type HakimiResult<T> = Result<T, HakimiError>;

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

    #[error("Memory error: {message}")]
    Memory {
        message: String,
        context: ErrorContext,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Context error: {message}")]
    Context {
        message: String,
        context: ErrorContext,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Tool error: {message}")]
    Tool {
        message: String,
        context: ErrorContext,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl HakimiError {
    /// 记录错误到日志（带完整上下文）
    pub fn log(&self) {
        match self {
            Self::Session { message, context, .. } => {
                tracing::error!(
                    error_type = "session",
                    message = %message,
                    session_id = ?context.session_id,
                    user_id = ?context.user_id,
                    timestamp = %context.timestamp,
                    operation = %context.operation,
                    details = ?context.details,
                    "Session error occurred"
                );
            }
            Self::Memory { message, context, .. } => {
                tracing::error!(
                    error_type = "memory",
                    message = %message,
                    session_id = ?context.session_id,
                    user_id = ?context.user_id,
                    timestamp = %context.timestamp,
                    operation = %context.operation,
                    details = ?context.details,
                    "Memory error occurred"
                );
            }
            // ... 其他错误类型类似
            _ => {
                tracing::error!(error = %self, "Error occurred");
            }
        }

        // 如果有 source，记录 backtrace
        if let Some(source) = self.source() {
            tracing::debug!(source = %source, "Error source");
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
}
```

**Cargo.toml** 依赖:
```toml
[dependencies]
thiserror = "1.0"
chrono = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
```

**验收**: `cargo build --package hakimi-common` 通过

---

### 步骤 2: Session 错误类型 (45 分钟)

**crates/hakimi-session/src/error.rs**:
```rust
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
```

**使用示例**:
```rust
// 在 message_ops.rs 中
use crate::error::{SessionError, session_error};

pub fn get_messages_around(
    &self,
    session_id: &str,
    anchor_id: i64,
    window: i64,
) -> Result<(Vec<Message>, i64, i64), HakimiError> {
    let start = Instant::now();
    debug!(session_id = %session_id, anchor_id, window, "Starting get_messages_around");

    // 验证 session 存在
    if !self.session_exists(session_id)? {
        let err = SessionError::NotFound(session_id.to_string());
        return Err(session_error!(err, "get_messages_around", session_id));
    }

    // ... 原有查询逻辑 ...

    match self.execute_query(query, params) {
        Ok(messages) => {
            metrics().record_duration("message_ops.get_messages_around", start.elapsed());
            debug!(count = messages.len(), "Query successful");
            Ok((messages, before_count, after_count))
        }
        Err(e) => {
            let err = SessionError::DatabaseError(e);
            let hakimi_err = session_error!(err, "get_messages_around", session_id);
            hakimi_err.log();  // 自动记录带上下文的错误
            Err(hakimi_err)
        }
    }
}
```

**验收**: `cargo test --package hakimi-session` 通过

---

### 步骤 3: Memory 错误类型 (45 分钟)

**crates/hakimi-context/src/error.rs**:
```rust
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
```

**使用示例**:
```rust
// 在 memory.rs 中
use crate::error::{MemoryError, memory_error};

#[instrument(skip(self), fields(target = %target))]
pub fn load_memory(&self, target: &str, session_id: &str) -> Result<String, HakimiError> {
    debug!(target = %target, "Loading memory file");

    let path = self.get_memory_path(target)?;

    // 检查文件大小
    if let Ok(metadata) = std::fs::metadata(&path) {
        const LIMIT: usize = 64 * 1024;  // 64KB
        let size = metadata.len() as usize;

        if size > LIMIT {
            let err = MemoryError::FileTooLarge { size, limit: LIMIT };
            let hakimi_err = memory_error!(err, "load_memory", session_id, target);
            hakimi_err.log();
            return Err(hakimi_err);
        }
    }

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
            let err = MemoryError::IoError(e);
            let hakimi_err = memory_error!(err, "load_memory", session_id, target);
            hakimi_err.log();
            Err(hakimi_err)
        }
    }
}
```

**验收**: `cargo test --package hakimi-context memory` 通过

---

### 步骤 4: Tool 错误类型 (30 分钟)

**crates/hakimi-tools/src/error.rs**:
```rust
use hakimi_common::error::{ErrorContext, HakimiError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Tool execution failed: {tool_name}: {reason}")]
    ExecutionFailed { tool_name: String, reason: String },

    #[error("Invalid tool arguments: {0}")]
    InvalidArguments(String),

    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Timeout: tool {0} exceeded {1}s")]
    Timeout(String, u64),
}

impl ToolError {
    pub fn into_hakimi_error(
        self,
        tool_name: impl Into<String>,
        session_id: Option<impl Into<String>>,
    ) -> HakimiError {
        let mut context = ErrorContext::new("tool_execution");
        if let Some(id) = session_id {
            context = context.with_session_id(id);
        }
        context = context.with_detail("tool_name", serde_json::json!(tool_name.into()));

        HakimiError::Tool {
            message: self.to_string(),
            context,
            source: Some(Box::new(self)),
        }
    }
}
```

**使用示例**:
```rust
// 在 builtin_session_search.rs 中
use crate::error::ToolError;

#[instrument(skip(self, args), fields(tool = "session_search"))]
async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, HakimiError> {
    let params: SessionSearchParams = serde_json::from_value(args)
        .map_err(|e| {
            let err = ToolError::InvalidArguments(e.to_string());
            err.into_hakimi_error("session_search", ctx.session_id.as_ref())
        })?;

    // ... 执行逻辑 ...

    match self.execute_discovery(params).await {
        Ok(result) => Ok(ToolOutput::success(result)),
        Err(e) => {
            let err = ToolError::ExecutionFailed {
                tool_name: "session_search".to_string(),
                reason: e.to_string(),
            };
            let hakimi_err = err.into_hakimi_error("session_search", ctx.session_id.as_ref());
            hakimi_err.log();
            Err(hakimi_err)
        }
    }
}
```

**验收**: `cargo test --package hakimi-tools` 通过

---

### 步骤 5: 添加错误处理测试 (45 分钟)

**crates/hakimi-session/tests/error_handling_test.rs**:
```rust
use hakimi_session::*;
use hakimi_common::error::HakimiError;

#[test]
fn test_session_not_found_error() {
    let db = test_db();
    let result = db.get_messages_around("nonexistent_session", 1, 10);

    assert!(result.is_err());
    let err = result.unwrap_err();

    match &err {
        HakimiError::Session { message, context, .. } => {
            assert!(message.contains("Session not found"));
            assert_eq!(context.session_id.as_deref(), Some("nonexistent_session"));
            assert_eq!(context.operation, "get_messages_around");
        }
        _ => panic!("Expected SessionError"),
    }
}

#[test]
fn test_message_not_found_error() {
    let db = test_db();
    let sid = create_test_session(&db);

    let result = db.get_messages_around(&sid, 999999, 10);

    assert!(result.is_err());
    let err = result.unwrap_err();
    // 验证错误包含正确的 message_id 和 session_id
}

#[test]
fn test_fts5_search_error_context() {
    let db = test_db();

    // 故意使用非法 FTS5 语法触发错误
    let result = db.search_messages("NEAR(invalid syntax", 10);

    assert!(result.is_err());
    let err = result.unwrap_err();

    match &err {
        HakimiError::Session { message, context, .. } => {
            assert!(message.contains("search failed"));
            assert_eq!(context.operation, "search_messages");
        }
        _ => panic!("Expected SessionError"),
    }
}
```

**crates/hakimi-context/tests/memory_error_test.rs**:
```rust
use hakimi_context::memory::*;
use hakimi_common::error::HakimiError;

#[test]
fn test_memory_file_too_large() {
    let mem = test_memory_provider();

    // 创建一个超过 64KB 的记忆文件
    let large_content = "x".repeat(70 * 1024);
    std::fs::write(mem.get_memory_path("test").unwrap(), large_content).unwrap();

    let result = mem.load_memory("test", "test_session");

    assert!(result.is_err());
    let err = result.unwrap_err();

    match &err {
        HakimiError::Memory { message, context, .. } => {
            assert!(message.contains("too large"));
            assert!(message.contains("64"));  // limit
            assert_eq!(context.session_id.as_deref(), Some("test_session"));
            
            // 验证详情包含 target
            let target = context.details.get("target").and_then(|v| v.as_str());
            assert_eq!(target, Some("test"));
        }
        _ => panic!("Expected MemoryError"),
    }
}

#[test]
fn test_memory_permission_denied() {
    let mem = test_memory_provider();
    let path = mem.get_memory_path("readonly").unwrap();

    // 创建只读文件
    std::fs::write(&path, "content").unwrap();
    let metadata = std::fs::metadata(&path).unwrap();
    let mut permissions = metadata.permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();

    // 尝试写入
    let result = mem.save_memory("readonly", "new content", "test_session");

    assert!(result.is_err());
    // ... 验证错误详情
}
```

**验收**: 
- `cargo test --package hakimi-session error_handling` — 3+ 测试通过
- `cargo test --package hakimi-context memory_error` — 2+ 测试通过
- `cargo test --package hakimi-tools error` — 2+ 测试通过

---

### 步骤 6: 更新文档 (30 分钟)

**crates/hakimi-common/README.md** (新建):
```markdown
# Hakimi Common - 错误处理最佳实践

## 错误类型层次

```
HakimiError (顶层，所有对外接口返回此类型)
  ├── SessionError (会话操作)
  ├── MemoryError (记忆读写)
  ├── ContextError (上下文管理)
  ├── ToolError (工具执行)
  └── ... (标准库错误: Io, Database, Serialization)
```

## 使用指南

### 1. 在 crate 内部定义具体错误

```rust
// your_crate/src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum YourError {
    #[error("Something went wrong: {0}")]
    SomethingWrong(String),
}

impl YourError {
    pub fn into_hakimi_error(
        self,
        operation: &str,
        session_id: Option<&str>,
    ) -> HakimiError {
        let mut context = ErrorContext::new(operation);
        if let Some(id) = session_id {
            context = context.with_session_id(id);
        }

        HakimiError::YourCategory {
            message: self.to_string(),
            context,
            source: Some(Box::new(self)),
        }
    }
}
```

### 2. 在方法中使用

```rust
pub fn your_method(&self, session_id: &str) -> Result<Data, HakimiError> {
    match self.internal_operation() {
        Ok(data) => Ok(data),
        Err(e) => {
            let hakimi_err = e.into_hakimi_error("your_method", Some(session_id));
            hakimi_err.log();  // 自动记录带上下文的结构化日志
            Err(hakimi_err)
        }
    }
}
```

### 3. 调用方处理错误

```rust
match hakimi_method(session_id) {
    Ok(result) => { /* ... */ },
    Err(e) => {
        // 错误已记录日志，这里只需处理业务逻辑
        if let Some(ctx) = e.context() {
            eprintln!("Operation {} failed at {}", ctx.operation, ctx.timestamp);
        }
        // 或向用户返回友好提示
        return Err("抱歉，操作失败，请稍后重试".to_string());
    }
}
```

## 日志输出示例

```
2026-07-10T08:30:45.123Z ERROR hakimi_session: Session error occurred
  error_type: "session"
  message: "Session not found: abc123"
  session_id: "abc123"
  user_id: "user_456"
  timestamp: "2026-07-10T08:30:45.123Z"
  operation: "get_messages_around"
  details: {"anchor_id": 100, "window": 20}
```

## 测试错误场景

所有错误路径都应该有测试覆盖：

```rust
#[test]
fn test_error_contains_context() {
    let result = db.operation_that_fails("session_id");
    let err = result.unwrap_err();

    assert!(matches!(err, HakimiError::Session { .. }));
    assert_eq!(err.context().unwrap().session_id, Some("session_id".to_string()));
}
```
```

**CHANGELOG.md** 更新:
```markdown
### v0.5.57 (2026-07-10)

#### 🛡️ 错误处理增强 (任务 1.1.3)
- **结构化错误类型**: 新增 `SessionError`, `MemoryError`, `ToolError`，所有错误携带完整上下文
- **自动日志记录**: `HakimiError::log()` 自动输出结构化日志（包含 session_id/user_id/timestamp）
- **错误追踪**: 所有核心操作错误可通过日志查询（支持按 session_id/operation/error_type 过滤）
- **测试覆盖**: 新增 10+ 错误场景测试，验证边界情况

#### 🔧 改进
- `message_ops`: 所有数据库错误携带 session_id 上下文
- `memory`: 文件大小限制错误包含 target/size/limit 详细信息
- `session_search`: FTS5 错误包含查询字符串和参数

#### 📚 文档
- 新增 `crates/hakimi-common/README.md` — 错误处理最佳实践
- 更新开发文档，说明如何定义和使用自定义错误类型
```

---

## 🧪 测试计划

### 单元测试
```bash
# 测试错误类型转换
cargo test --package hakimi-common error

# 测试 Session 错误场景
cargo test --package hakimi-session error_handling

# 测试 Memory 错误场景
cargo test --package hakimi-context memory_error

# 测试 Tool 错误场景
cargo test --package hakimi-tools error
```

### 集成测试（手动）
```bash
# 启动服务并观察日志
RUST_LOG=hakimi=debug cargo run -- gateway --config test_config.yaml

# 触发错误场景
curl -X POST http://localhost:3005/api/chat \
  -H "Content-Type: application/json" \
  -d '{"session_id": "nonexistent", "message": "search something"}'

# 观察日志应包含：
# - error_type: "session"
# - session_id: "nonexistent"
# - operation: "search_messages"
# - timestamp: "2026-07-10T..."
```

### 日志验证
```bash
# 查询特定 session 的所有错误
grep "session_id.*test_session_123" ~/.hakimi/logs/agent.log | grep ERROR

# 按错误类型统计
grep "error_type" ~/.hakimi/logs/agent.log | cut -d'"' -f4 | sort | uniq -c
```

---

## 📊 完成检查清单

- [ ] `hakimi-common/src/error.rs` 定义完成
- [ ] `hakimi-session/src/error.rs` 定义完成
- [ ] `hakimi-context/src/error.rs` 定义完成
- [ ] `hakimi-tools/src/error.rs` 定义完成
- [ ] `message_ops.rs` 迁移到新错误类型
- [ ] `memory.rs` 迁移到新错误类型
- [ ] `builtin_session_search.rs` 迁移到新错误类型
- [ ] 错误处理测试 (10+ 个用例)
- [ ] 文档更新完成 (README + CHANGELOG)
- [ ] 所有测试通过 (`cargo test --all`)
- [ ] Release 编译成功 (`cargo build --release`)

---

## 🔗 参考资料

- [thiserror 文档](https://docs.rs/thiserror/)
- [Rust Error Handling Book](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [tracing 结构化日志](https://docs.rs/tracing/)
- [Sentry Rust SDK](https://docs.sentry.io/platforms/rust/)（未来集成）

---

**创建时间**: 2026-07-10  
**预计完成**: 2026-07-10 (当日完成)
