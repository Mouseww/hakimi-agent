# 任务 1.3.1: 补充 session_search 工具集成测试

**状态**: 🚧 进行中 (0%)  
**开始时间**: 2026-07-10 03:00 UTC  

**优先级**: 🔴 高  
**预估时间**: 4 小时  
**依赖**: 任务 1.1.x, 1.2.x (已完成)  
**阻塞**: 任务 1.3.2, 1.3.3 (需要此任务完成后才能继续)

---

## 📋 目标

为 `builtin_session_search` 工具添加全面的集成测试，确保所有核心功能都有测试覆盖，包括：
1. Discovery 模式 + bookends 完整性
2. Scroll 边界检测（首尾）
3. Browse 排序正确性
4. FTS5 中文分词（如适用）

---

## 🎯 验收标准

- [x] Discovery 模式测试（bookends 完整性）
- [x] Scroll 模式边界测试（到达首尾的行为）
- [x] Browse 模式排序测试（时间倒序）
- [x] FTS5 搜索测试（关键词匹配）
- [x] 错误路径测试（无效参数、空会话等）
- [x] 所有测试通过：`cargo test --package hakimi-tools session_search`

---

## 📁 涉及文件

### 核心实现
- `crates/hakimi-tools/src/builtin_session_search.rs` — session_search 工具主要实现
- `crates/hakimi-session/src/message_ops.rs` — 底层查询实现

### 测试
- `crates/hakimi-tools/tests/session_search_integration_test.rs` — 新建集成测试文件

---

## 🔧 实施步骤

### 步骤 1: 创建测试文件 (15 分钟)

在 `crates/hakimi-tools/tests/` 目录下创建 `session_search_integration_test.rs`：

```rust
use hakimi_session::SessionDB;
use hakimi_tools::builtin_session_search::*;
use serde_json::json;
use std::path::PathBuf;
use tempfile::TempDir;

// Helper 函数：创建测试数据库
fn setup_test_db() -> (TempDir, SessionDB) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.db");
    let db = SessionDB::new(&db_path).unwrap();
    (tmp, db)
}

// Helper 函数：插入测试消息
fn insert_test_messages(db: &SessionDB, session_id: &str, count: usize) {
    db.create_session(session_id, "test-user", "test-platform").unwrap();
    for i in 0..count {
        db.add_message(
            session_id,
            "user",
            &format!("Test message {}", i),
            None,
        ).unwrap();
        db.add_message(
            session_id,
            "assistant",
            &format!("Response {}", i),
            None,
        ).unwrap();
    }
}
```

---

### 步骤 2: Discovery 模式测试 (45 分钟)

```rust
#[tokio::test]
async fn test_discovery_mode_basic() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-1";
    
    // 插入 10 条消息对
    insert_test_messages(&db, session_id, 10);
    
    // 执行 Discovery 查询
    let params = json!({
        "query": "Test message 5",
        "mode": "discovery",
        "session_id": session_id
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    
    // 验证结果
    assert!(result.get("matches").is_some());
    let matches = result["matches"].as_array().unwrap();
    assert!(matches.len() > 0);
    
    // 验证 bookends 存在
    assert!(matches[0].get("before_context").is_some());
    assert!(matches[0].get("after_context").is_some());
}

#[tokio::test]
async fn test_discovery_bookends_completeness() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-2";
    
    insert_test_messages(&db, session_id, 20);
    
    let params = json!({
        "query": "Test message 10",
        "mode": "discovery",
        "session_id": session_id,
        "context_messages": 3
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let matches = result["matches"].as_array().unwrap();
    
    // 验证 bookends 数量
    for match_item in matches {
        let before = match_item["before_context"].as_array().unwrap();
        let after = match_item["after_context"].as_array().unwrap();
        
        assert!(before.len() <= 3, "Before context should not exceed 3");
        assert!(after.len() <= 3, "After context should not exceed 3");
        
        // 验证角色交替
        for msg in before.iter().chain(after.iter()) {
            assert!(msg.get("role").is_some());
            assert!(msg.get("content").is_some());
        }
    }
}

#[tokio::test]
async fn test_discovery_boundary_bookends() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-3";
    
    insert_test_messages(&db, session_id, 5);
    
    // 查询第一条消息
    let params = json!({
        "query": "Test message 0",
        "mode": "discovery",
        "session_id": session_id,
        "context_messages": 5
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let matches = result["matches"].as_array().unwrap();
    
    // 第一条消息应该没有 before_context
    let first_match = &matches[0];
    let before = first_match["before_context"].as_array().unwrap();
    assert_eq!(before.len(), 0, "First message should have no before context");
}
```

---

### 步骤 3: Scroll 模式边界测试 (45 分钟)

```rust
#[tokio::test]
async fn test_scroll_mode_forward() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-4";
    
    insert_test_messages(&db, session_id, 20);
    
    // 从中间位置向后滚动
    let params = json!({
        "mode": "scroll",
        "session_id": session_id,
        "anchor_id": 10,
        "direction": "forward",
        "limit": 5
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let messages = result["messages"].as_array().unwrap();
    
    assert_eq!(messages.len(), 5);
    
    // 验证消息 ID 递增
    for i in 0..messages.len()-1 {
        let id1 = messages[i]["id"].as_i64().unwrap();
        let id2 = messages[i+1]["id"].as_i64().unwrap();
        assert!(id2 > id1, "Messages should be in ascending order");
    }
}

#[tokio::test]
async fn test_scroll_mode_backward() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-5";
    
    insert_test_messages(&db, session_id, 20);
    
    // 从中间位置向前滚动
    let params = json!({
        "mode": "scroll",
        "session_id": session_id,
        "anchor_id": 20,
        "direction": "backward",
        "limit": 5
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let messages = result["messages"].as_array().unwrap();
    
    assert_eq!(messages.len(), 5);
    
    // 验证消息 ID 递减
    for i in 0..messages.len()-1 {
        let id1 = messages[i]["id"].as_i64().unwrap();
        let id2 = messages[i+1]["id"].as_i64().unwrap();
        assert!(id2 < id1, "Messages should be in descending order");
    }
}

#[tokio::test]
async fn test_scroll_at_boundary_start() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-6";
    
    insert_test_messages(&db, session_id, 10);
    
    // 从第一条消息向前滚动（应该返回空或边界标记）
    let params = json!({
        "mode": "scroll",
        "session_id": session_id,
        "anchor_id": 1,
        "direction": "backward",
        "limit": 5
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let messages = result["messages"].as_array().unwrap();
    
    // 应该返回空或包含边界标记
    assert!(
        messages.is_empty() || result.get("at_boundary").is_some(),
        "Should indicate boundary reached"
    );
}

#[tokio::test]
async fn test_scroll_at_boundary_end() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-7";
    
    insert_test_messages(&db, session_id, 10);
    
    // 获取最后一条消息 ID
    let last_id = db.get_last_message_id(session_id).unwrap();
    
    // 从最后一条消息向后滚动
    let params = json!({
        "mode": "scroll",
        "session_id": session_id,
        "anchor_id": last_id,
        "direction": "forward",
        "limit": 5
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let messages = result["messages"].as_array().unwrap();
    
    // 应该返回空或包含边界标记
    assert!(
        messages.is_empty() || result.get("at_boundary").is_some(),
        "Should indicate boundary reached"
    );
}
```

---

### 步骤 4: Browse 模式排序测试 (30 分钟)

```rust
#[tokio::test]
async fn test_browse_mode_time_order() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-8";
    
    insert_test_messages(&db, session_id, 15);
    
    let params = json!({
        "mode": "browse",
        "session_id": session_id,
        "limit": 10
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let messages = result["messages"].as_array().unwrap();
    
    assert_eq!(messages.len(), 10);
    
    // 验证时间倒序（最新消息在前）
    for i in 0..messages.len()-1 {
        let ts1 = messages[i]["timestamp"].as_str().unwrap();
        let ts2 = messages[i+1]["timestamp"].as_str().unwrap();
        assert!(ts1 >= ts2, "Messages should be in descending time order");
    }
}

#[tokio::test]
async fn test_browse_mode_pagination() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-9";
    
    insert_test_messages(&db, session_id, 30);
    
    // 第一页
    let params1 = json!({
        "mode": "browse",
        "session_id": session_id,
        "limit": 10,
        "offset": 0
    });
    
    let result1 = execute_session_search(params1, &db).await.unwrap();
    let page1 = result1["messages"].as_array().unwrap();
    
    // 第二页
    let params2 = json!({
        "mode": "browse",
        "session_id": session_id,
        "limit": 10,
        "offset": 10
    });
    
    let result2 = execute_session_search(params2, &db).await.unwrap();
    let page2 = result2["messages"].as_array().unwrap();
    
    assert_eq!(page1.len(), 10);
    assert_eq!(page2.len(), 10);
    
    // 验证两页不重叠
    let id1_last = page1.last().unwrap()["id"].as_i64().unwrap();
    let id2_first = page2.first().unwrap()["id"].as_i64().unwrap();
    assert_ne!(id1_last, id2_first, "Pages should not overlap");
}
```

---

### 步骤 5: FTS5 搜索测试 (30 分钟)

```rust
#[tokio::test]
async fn test_fts5_keyword_search() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-10";
    
    // 插入包含特定关键词的消息
    db.create_session(session_id, "test-user", "cli").unwrap();
    db.add_message(session_id, "user", "How to configure Rust compiler?", None).unwrap();
    db.add_message(session_id, "assistant", "You need to set RUSTFLAGS environment variable.", None).unwrap();
    db.add_message(session_id, "user", "What about Python setup?", None).unwrap();
    db.add_message(session_id, "assistant", "Use pip install for Python packages.", None).unwrap();
    
    // 搜索 "Rust"
    let params = json!({
        "query": "Rust",
        "mode": "discovery",
        "session_id": session_id
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let matches = result["matches"].as_array().unwrap();
    
    assert!(matches.len() >= 1);
    
    // 验证匹配内容包含 "Rust"
    let found = matches.iter().any(|m| {
        let content = m["content"].as_str().unwrap();
        content.to_lowercase().contains("rust")
    });
    
    assert!(found, "Should find messages containing 'Rust'");
}

#[tokio::test]
async fn test_fts5_chinese_search() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session-11";
    
    // 插入中文消息
    db.create_session(session_id, "test-user", "cli").unwrap();
    db.add_message(session_id, "user", "如何配置 Hakimi Agent？", None).unwrap();
    db.add_message(session_id, "assistant", "你需要编辑 config.yaml 文件。", None).unwrap();
    db.add_message(session_id, "user", "Docker 部署怎么做？", None).unwrap();
    
    // 搜索 "配置"
    let params = json!({
        "query": "配置",
        "mode": "discovery",
        "session_id": session_id
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let matches = result["matches"].as_array().unwrap();
    
    assert!(matches.len() >= 1);
    
    // 验证匹配内容包含 "配置"
    let found = matches.iter().any(|m| {
        let content = m["content"].as_str().unwrap();
        content.contains("配置")
    });
    
    assert!(found, "Should find messages containing '配置'");
}
```

---

### 步骤 6: 错误路径测试 (30 分钟)

```rust
#[tokio::test]
async fn test_error_invalid_mode() {
    let (_tmp, db) = setup_test_db();
    
    let params = json!({
        "mode": "invalid_mode",
        "session_id": "test-session"
    });
    
    let result = execute_session_search(params, &db).await;
    assert!(result.is_err(), "Should return error for invalid mode");
}

#[tokio::test]
async fn test_error_empty_session() {
    let (_tmp, db) = setup_test_db();
    let session_id = "empty-session";
    
    db.create_session(session_id, "test-user", "cli").unwrap();
    
    let params = json!({
        "query": "test",
        "mode": "discovery",
        "session_id": session_id
    });
    
    let result = execute_session_search(params, &db).await.unwrap();
    let matches = result["matches"].as_array().unwrap();
    
    assert_eq!(matches.len(), 0, "Empty session should return no matches");
}

#[tokio::test]
async fn test_error_nonexistent_session() {
    let (_tmp, db) = setup_test_db();
    
    let params = json!({
        "mode": "browse",
        "session_id": "nonexistent-session"
    });
    
    let result = execute_session_search(params, &db).await;
    assert!(result.is_err(), "Should return error for nonexistent session");
}

#[tokio::test]
async fn test_error_negative_limit() {
    let (_tmp, db) = setup_test_db();
    let session_id = "test-session";
    
    insert_test_messages(&db, session_id, 10);
    
    let params = json!({
        "mode": "browse",
        "session_id": session_id,
        "limit": -5
    });
    
    let result = execute_session_search(params, &db).await;
    assert!(result.is_err(), "Should return error for negative limit");
}
```

---

### 步骤 7: 运行测试并修复问题 (60 分钟)

```bash
# 编译测试
cd hakimi-agent
cargo test --package hakimi-tools --test session_search_integration_test --no-fail-fast

# 如果有失败，查看详细输出
cargo test --package hakimi-tools session_search -- --nocapture

# 运行特定测试
cargo test --package hakimi-tools test_discovery_mode_basic -- --nocapture
```

可能需要修复的问题：
1. **API 签名不匹配**：根据实际实现调整测试代码
2. **数据库 schema 差异**：检查表结构是否包含所需字段
3. **错误处理不完善**：补充缺失的错误处理
4. **边界条件未处理**：修复边界检测逻辑

---

### 步骤 8: 更新文档 (20 分钟)

**README.md** 添加测试说明：

```markdown
## 🧪 测试

### 运行所有测试
```bash
cargo test --workspace
```

### 运行特定模块测试
```bash
# session_search 集成测试
cargo test --package hakimi-tools session_search

# 查看详细输出
cargo test --package hakimi-tools session_search -- --nocapture
```

### 测试覆盖率
```bash
# 安装 tarpaulin
cargo install cargo-tarpaulin

# 生成覆盖率报告
cargo tarpaulin --workspace --out Html
```
```

**CHANGELOG.md** 添加条目：

```markdown
### v0.5.57 (2026-07-10)

#### 🧪 测试
- **session_search 集成测试**: 新增 15+ 集成测试用例
  - Discovery 模式 bookends 完整性验证
  - Scroll 模式边界检测（首尾）
  - Browse 模式排序正确性
  - FTS5 中英文搜索
  - 错误路径覆盖
- **测试覆盖率**: hakimi-tools crate 覆盖率提升至 75%+
```

---

## 🧪 验证清单

运行以下命令验证所有测试通过：

```bash
# 1. 编译检查
cargo check --package hakimi-tools

# 2. 运行测试
cargo test --package hakimi-tools session_search

# 3. 验证测试覆盖率
cargo tarpaulin --package hakimi-tools --out Stdout | grep "session_search"

# 4. 运行 clippy
cargo clippy --package hakimi-tools -- -D warnings

# 5. 格式化检查
cargo fmt --package hakimi-tools -- --check
```

预期输出：
```
✓ 所有测试通过（15+ 个测试）
✓ 覆盖率 ≥ 75%
✓ 无 clippy warnings
✓ 代码格式正确
```

---

## 🚧 已知限制

1. **中文分词**: 当前 SQLite FTS5 默认使用 simple tokenizer，中文分词效果有限。未来可考虑：
   - 集成 jieba 分词（通过 UDF）
   - 使用专用中文全文搜索引擎（如 Sonic）

2. **性能基准**: 当前测试使用小数据集（<50 条消息），未覆盖大规模场景（10K+ 消息）。

3. **并发测试**: 未测试多线程并发访问场景。

---

## ✅ 完成检查清单

- [ ] 创建测试文件 `session_search_integration_test.rs`
- [ ] Discovery 模式测试（3+ 测试用例）
- [ ] Scroll 模式测试（4+ 测试用例）
- [ ] Browse 模式测试（2+ 测试用例）
- [ ] FTS5 搜索测试（2+ 测试用例）
- [ ] 错误路径测试（4+ 测试用例）
- [ ] 所有测试通过
- [ ] 文档更新（README + CHANGELOG）
- [ ] 代码审查通过
- [ ] PR 创建并合并
