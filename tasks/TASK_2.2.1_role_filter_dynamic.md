# 任务 2.2.1: SQL 查询角色过滤动态化

**状态**: 🔄 进行中 (0%)  
**开始时间**: 2026-07-10 09:20 UTC  
**预估时间**: 3 小时  

**优先级**: 🟡 中  
**依赖**: 无  
**解锁**: 任务 2.2.2（session_search 工具暴露参数）

---

## 📋 目标

重构 message_ops 中的 SQL 查询，使角色过滤动态化，支持灵活的角色组合查询。

当前问题：
- `get_bookends()` 和相关方法硬编码角色过滤 `role IN ('user', 'assistant')`
- 无法查询工具输出（role='tool'）
- 无法自定义角色组合

目标：
- 支持任意角色组合查询
- 保持向后兼容（默认行为不变）
- 为 session_search 工具提供灵活性

---

## 🎯 验收标准

- [ ] `get_bookends()` 接受 `roles: Option<&[&str]>` 参数
- [ ] 动态构建 `WHERE role IN (?, ?, ...)` 子句
- [ ] 默认值为 `['user', 'assistant']`（向后兼容）
- [ ] `get_messages_around()` 同样支持角色过滤
- [ ] 单元测试覆盖所有角色组合
- [ ] 性能无退化（动态 SQL 不影响查询速度）

---

## 📁 涉及文件

### 主要修改
- `crates/hakimi-session/src/message_ops.rs` (630 行)
  - `get_bookends()`
  - `get_messages_around()`
  - `get_messages()`（内部方法）

### 测试
- `crates/hakimi-session/tests/message_ops_test.rs`

---

## 🛠️ 实施步骤

### 步骤 1: 修改 get_bookends() 签名 (30 分钟)

**当前签名**:
```rust
pub fn get_bookends(
    &self,
    session_id: &str,
    count: i64,
) -> Result<(Vec<Message>, Vec<Message>)>
```

**新签名**:
```rust
pub fn get_bookends(
    &self,
    session_id: &str,
    count: i64,
    roles: Option<&[&str]>,  // 新增参数
) -> Result<(Vec<Message>, Vec<Message>)>
```

**默认值处理**:
```rust
let role_filter = roles.unwrap_or(&["user", "assistant"]);
```

---

### 步骤 2: 动态构建 SQL 查询 (60 分钟)

**原始查询**:
```sql
SELECT * FROM messages 
WHERE session_id = ? AND role IN ('user', 'assistant')
ORDER BY id ASC 
LIMIT ?
```

**动态构建**:
```rust
fn build_role_filter_sql(roles: &[&str]) -> (String, Vec<String>) {
    if roles.is_empty() {
        // 无角色限制
        return (String::new(), vec![]);
    }
    
    let placeholders = roles.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let clause = format!("AND role IN ({})", placeholders);
    let params = roles.iter().map(|r| r.to_string()).collect();
    
    (clause, params)
}

pub fn get_bookends(
    &self,
    session_id: &str,
    count: i64,
    roles: Option<&[&str]>,
) -> Result<(Vec<Message>, Vec<Message>)> {
    let conn = self.pool.get()?;
    let role_filter = roles.unwrap_or(&["user", "assistant"]);
    let (role_clause, role_params) = build_role_filter_sql(role_filter);
    
    // 获取首部消息
    let query = format!(
        "SELECT * FROM messages WHERE session_id = ? {} ORDER BY id ASC LIMIT ?",
        role_clause
    );
    
    let mut stmt = conn.prepare(&query)?;
    let mut param_idx = 1;
    stmt.raw_bind_parameter(param_idx, session_id)?;
    param_idx += 1;
    
    for role_param in &role_params {
        stmt.raw_bind_parameter(param_idx, role_param)?;
        param_idx += 1;
    }
    
    stmt.raw_bind_parameter(param_idx, count)?;
    
    let start_messages = stmt
        .raw_query()
        .mapped(|row| Message::from_row(row))
        .collect::<Result<Vec<_>, _>>()?;
    
    // 类似处理尾部消息...
    
    Ok((start_messages, end_messages))
}
```

**注意事项**:
- 使用 `raw_bind_parameter()` 进行参数化查询（防止 SQL 注入）
- 动态参数索引管理
- 保持查询性能（索引仍然有效）

---

### 步骤 3: 更新 get_messages_around() (30 分钟)

```rust
pub fn get_messages_around(
    &self,
    session_id: &str,
    anchor_id: i64,
    window: i64,
    roles: Option<&[&str]>,  // 新增参数
) -> Result<(Vec<Message>, i64, i64)> {
    let conn = self.pool.get()?;
    let role_filter = roles.unwrap_or(&["user", "assistant"]);
    let (role_clause, role_params) = build_role_filter_sql(role_filter);
    
    // 前向查询
    let query_before = format!(
        "SELECT * FROM messages 
         WHERE session_id = ? AND id < ? {}
         ORDER BY id DESC LIMIT ?",
        role_clause
    );
    
    // ... 类似 get_bookends 的参数绑定逻辑
    
    Ok((messages, messages_before, messages_after))
}
```

---

### 步骤 4: 向后兼容适配 (30 分钟)

**现有调用点**:
- `crates/hakimi-tools/src/builtin_session_search.rs`
- `crates/hakimi-context/src/memory.rs`

**适配方式 1 - 默认参数**:
```rust
// 旧代码不需要修改，直接传 None
let (start, end) = db.get_bookends(session_id, 5, None)?;
```

**适配方式 2 - 显式传值**:
```rust
// 需要特定角色的地方显式传
let (start, end) = db.get_bookends(
    session_id, 
    5, 
    Some(&["user", "assistant", "tool"])
)?;
```

---

### 步骤 5: 单元测试 (60 分钟)

```rust
#[test]
fn test_get_bookends_default_roles() {
    let db = test_db();
    let sid = create_session(&db);
    
    // 插入多种角色消息
    db.save_message(&sid, &Message::user("user msg")).unwrap();
    db.save_message(&sid, &Message::assistant("assistant msg")).unwrap();
    db.save_message(&sid, &Message::tool("tool output")).unwrap();
    
    // 默认只返回 user + assistant
    let (start, end) = db.get_bookends(&sid, 10, None).unwrap();
    
    assert_eq!(start.len(), 2);
    assert!(start.iter().all(|m| m.role == "user" || m.role == "assistant"));
}

#[test]
fn test_get_bookends_custom_roles() {
    let db = test_db();
    let sid = create_session(&db);
    
    db.save_message(&sid, &Message::user("user msg")).unwrap();
    db.save_message(&sid, &Message::tool("tool output")).unwrap();
    
    // 自定义角色过滤
    let (start, end) = db.get_bookends(&sid, 10, Some(&["tool"])).unwrap();
    
    assert_eq!(start.len(), 1);
    assert_eq!(start[0].role, "tool");
}

#[test]
fn test_get_bookends_all_roles() {
    let db = test_db();
    let sid = create_session(&db);
    
    db.save_message(&sid, &Message::user("user msg")).unwrap();
    db.save_message(&sid, &Message::assistant("assistant msg")).unwrap();
    db.save_message(&sid, &Message::tool("tool output")).unwrap();
    db.save_message(&sid, &Message::system("system prompt")).unwrap();
    
    // 传空数组 = 不过滤角色
    let (start, end) = db.get_bookends(&sid, 10, Some(&[])).unwrap();
    
    assert_eq!(start.len(), 4);
}

#[test]
fn test_get_bookends_empty_result() {
    let db = test_db();
    let sid = create_session(&db);
    
    db.save_message(&sid, &Message::user("user msg")).unwrap();
    
    // 过滤不存在的角色
    let (start, end) = db.get_bookends(&sid, 10, Some(&["nonexistent"])).unwrap();
    
    assert_eq!(start.len(), 0);
    assert_eq!(end.len(), 0);
}
```

---

### 步骤 6: 性能基准测试 (30 分钟)

```rust
// crates/hakimi-session/benches/role_filter_bench.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_get_bookends_static(c: &mut Criterion) {
    // 旧版硬编码查询（基准）
    c.bench_function("get_bookends_static", |b| {
        b.iter(|| {
            let db = setup_db_with_1000_messages();
            db.get_bookends_old(black_box("session_1"), black_box(10))
        });
    });
}

fn bench_get_bookends_dynamic(c: &mut Criterion) {
    // 新版动态查询
    c.bench_function("get_bookends_dynamic", |b| {
        b.iter(|| {
            let db = setup_db_with_1000_messages();
            db.get_bookends(black_box("session_1"), black_box(10), None)
        });
    });
}

criterion_group!(benches, bench_get_bookends_static, bench_get_bookends_dynamic);
criterion_main!(benches);
```

**验收**: 性能差异 < 5%

---

## 📊 完成检查清单

- [ ] `get_bookends()` 签名更新完成
- [ ] `get_messages_around()` 签名更新完成
- [ ] `build_role_filter_sql()` 辅助函数实现
- [ ] 所有现有调用点适配完成
- [ ] 单元测试通过（4+ 测试用例）
- [ ] 性能基准测试无退化（< 5%）
- [ ] 编译无错误：`cargo build --release`
- [ ] 集成测试通过：`cargo test --package hakimi-session`
- [ ] 文档更新（函数注释）

---

## 🔗 参考资料

- [rusqlite 动态参数绑定](https://docs.rs/rusqlite/latest/rusqlite/struct.Statement.html#method.raw_bind_parameter)
- [SQL IN 子句最佳实践](https://use-the-index-luke.com/sql/where-clause/in-list-parameter)

---

**创建时间**: 2026-07-10  
**预计完成**: 2026-07-10（3 小时内）  
**实际完成**: _待填写_
