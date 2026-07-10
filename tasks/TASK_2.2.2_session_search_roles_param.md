# 任务 2.2.2: session_search 工具暴露 roles 参数

**状态**: 🔄 待开始  
**优先级**: 🟡 中  
**预估时间**: 2 小时  
**依赖**: TASK 2.2.1（SQL 查询角色过滤动态化）✅  
**解锁**: 无

---

## 📋 目标

在 session_search 工具的 JSON 参数中暴露 `roles` 参数，允许用户自定义查询的消息角色。

**当前问题：**
- session_search 工具调用 `get_bookends()` 和 `get_messages_around()` 时固定传 `None`
- 用户无法通过工具参数控制查询哪些角色的消息
- 缺乏查询工具输出（role='tool'）的能力

**目标：**
- 在 `SessionSearchArgs` 结构体中添加 `roles` 字段
- 将用户传入的 roles 参数传递给底层 SQL 查询
- 提供清晰的文档说明参数用法
- 保持向后兼容（默认行为不变）

---

## 🎯 验收标准

- [ ] `SessionSearchArgs` 添加 `roles: Option<Vec<String>>` 字段
- [ ] JSON Schema 文档更新，说明 roles 参数用法
- [ ] `discovery` 模式支持 roles 过滤
- [ ] `browse` 模式支持 roles 过滤
- [ ] 默认值为 `None`（使用底层默认的 user + assistant）
- [ ] 单元测试覆盖 roles 参数场景
- [ ] 工具描述文档更新

---

## 📁 涉及文件

### 主要修改
- `crates/hakimi-tools/src/builtin_session_search.rs` (~600 行)
  - `SessionSearchArgs` 结构体
  - `discovery` 方法
  - `browse` 方法
  - 工具 JSON Schema 描述

### 测试
- `crates/hakimi-tools/src/builtin_session_search.rs` (测试模块)

---

## 🛠️ 实施步骤

### 步骤 1: 更新 SessionSearchArgs 结构体 (15 分钟)

**当前结构**:
```rust
#[derive(Debug, Deserialize)]
struct SessionSearchArgs {
    mode: String,
    session_id: Option<String>,
    anchor_id: Option<i64>,
    window: Option<i64>,
}
```

**新结构**:
```rust
#[derive(Debug, Deserialize)]
struct SessionSearchArgs {
    mode: String,
    session_id: Option<String>,
    anchor_id: Option<i64>,
    window: Option<i64>,
    /// Filter messages by role. Default: ["user", "assistant"]
    /// Valid values: "user", "assistant", "tool", "system"
    /// Pass [] to include all roles.
    #[serde(default)]
    roles: Option<Vec<String>>,
}
```

---

### 步骤 2: 传递 roles 参数到底层查询 (30 分钟)

**discovery 模式更新**:
```rust
fn discovery(&self, db: &SessionDB, args: &SessionSearchArgs) -> Result<String> {
    // ...
    
    // 将 Vec<String> 转换为 &[&str]
    let roles_slice: Option<Vec<&str>> = args.roles.as_ref().map(|v| {
        v.iter().map(|s| s.as_str()).collect()
    });
    let roles_ref = roles_slice.as_ref().map(|v| v.as_slice());
    
    let (start_msgs, end_msgs) = db
        .get_bookends(&session.id, 3, roles_ref)
        .unwrap_or_default();
    
    // ...
}
```

**browse 模式更新**:
```rust
fn browse(&self, db: &SessionDB, args: &SessionSearchArgs) -> Result<String> {
    // ...
    
    let roles_slice: Option<Vec<&str>> = args.roles.as_ref().map(|v| {
        v.iter().map(|s| s.as_str()).collect()
    });
    let roles_ref = roles_slice.as_ref().map(|v| v.as_slice());
    
    let (messages, before, after) = db
        .get_messages_around(session_id, anchor_id, window, roles_ref)
        .map_err(|e| {
            session_error(
                format!("failed to get messages around anchor: {e}"),
                session_id,
            )
        })?;
    
    // ...
}
```

---

### 步骤 3: 更新 JSON Schema 描述 (30 分钟)

**工具描述更新**:
```rust
fn description(&self) -> String {
    r#"Search and browse Hakimi agent sessions.

Modes:
- discovery: List all sessions with metadata and sample messages
- browse: View messages around a specific anchor point in a session

Parameters:
- mode (required): "discovery" or "browse"
- session_id (browse only): Target session ID
- anchor_id (browse only): Reference message ID for context window
- window (browse only, default: 5): Number of messages before/after anchor
- roles (optional): Filter messages by role
  - Default: ["user", "assistant"]
  - Valid values: "user", "assistant", "tool", "system"
  - Pass [] to include all roles
  - Example: ["user", "tool"] to see user inputs and tool outputs

Examples:
- List all sessions: {"mode": "discovery"}
- Browse around message 42: {"mode": "browse", "session_id": "abc123", "anchor_id": 42}
- Browse only tool outputs: {"mode": "browse", "session_id": "abc123", "anchor_id": 42, "roles": ["tool"]}
- Browse all messages: {"mode": "browse", "session_id": "abc123", "anchor_id": 42, "roles": []}
"#.to_string()
}
```

**JSON Schema 更新**:
```json
{
  "type": "object",
  "properties": {
    "mode": {
      "type": "string",
      "enum": ["discovery", "browse"],
      "description": "Search mode"
    },
    "session_id": {
      "type": "string",
      "description": "Session ID (browse mode)"
    },
    "anchor_id": {
      "type": "integer",
      "description": "Reference message ID (browse mode)"
    },
    "window": {
      "type": "integer",
      "default": 5,
      "description": "Context window size (browse mode)"
    },
    "roles": {
      "type": "array",
      "items": {
        "type": "string",
        "enum": ["user", "assistant", "tool", "system"]
      },
      "description": "Filter messages by role (default: [\"user\", \"assistant\"])"
    }
  },
  "required": ["mode"]
}
```

---

### 步骤 4: 单元测试 (45 分钟)

```rust
#[test]
fn test_discovery_with_custom_roles() {
    let db = setup_test_db();
    let tool = SessionSearchTool;
    
    // 创建会话并添加多种角色消息
    let sid = db.create_session("test", None).unwrap();
    db.save_message(&sid, &Message::user("user msg")).unwrap();
    db.save_message(&sid, &Message::assistant("assistant msg")).unwrap();
    db.save_message(&sid, &Message::tool_result("call_1", "test", "tool output")).unwrap();
    
    // 只查询 tool 角色
    let args = SessionSearchArgs {
        mode: "discovery".to_string(),
        session_id: None,
        anchor_id: None,
        window: None,
        roles: Some(vec!["tool".to_string()]),
    };
    
    let result = tool.execute(serde_json::to_value(args).unwrap()).unwrap();
    
    // 验证结果只包含 tool 消息
    assert!(result.contains("tool output"));
    assert!(!result.contains("user msg"));
    assert!(!result.contains("assistant msg"));
}

#[test]
fn test_browse_with_all_roles() {
    let db = setup_test_db();
    let tool = SessionSearchTool;
    
    let sid = db.create_session("test", None).unwrap();
    db.save_message(&sid, &Message::user("Q1")).unwrap();
    db.save_message(&sid, &Message::assistant("A1")).unwrap();
    db.save_message(&sid, &Message::tool_result("call_1", "test", "T1")).unwrap();
    db.save_message(&sid, &Message::system("S1")).unwrap();
    
    // 查询所有角色（传空数组）
    let args = SessionSearchArgs {
        mode: "browse".to_string(),
        session_id: Some(sid.clone()),
        anchor_id: Some(2),
        window: Some(2),
        roles: Some(vec![]),
    };
    
    let result = tool.execute(serde_json::to_value(args).unwrap()).unwrap();
    
    // 验证结果包含所有角色消息
    assert!(result.contains("Q1"));
    assert!(result.contains("A1"));
    assert!(result.contains("T1"));
    assert!(result.contains("S1"));
}

#[test]
fn test_browse_with_default_roles() {
    let db = setup_test_db();
    let tool = SessionSearchTool;
    
    let sid = db.create_session("test", None).unwrap();
    db.save_message(&sid, &Message::user("Q1")).unwrap();
    db.save_message(&sid, &Message::assistant("A1")).unwrap();
    db.save_message(&sid, &Message::tool_result("call_1", "test", "T1")).unwrap();
    
    // 不传 roles（使用默认）
    let args = SessionSearchArgs {
        mode: "browse".to_string(),
        session_id: Some(sid.clone()),
        anchor_id: Some(2),
        window: Some(2),
        roles: None,
    };
    
    let result = tool.execute(serde_json::to_value(args).unwrap()).unwrap();
    
    // 验证结果只包含 user + assistant
    assert!(result.contains("Q1"));
    assert!(result.contains("A1"));
    assert!(!result.contains("T1"));
}

#[test]
fn test_browse_multiple_custom_roles() {
    let db = setup_test_db();
    let tool = SessionSearchTool;
    
    let sid = db.create_session("test", None).unwrap();
    db.save_message(&sid, &Message::user("Q1")).unwrap();
    db.save_message(&sid, &Message::assistant("A1")).unwrap();
    db.save_message(&sid, &Message::tool_result("call_1", "test", "T1")).unwrap();
    db.save_message(&sid, &Message::system("S1")).unwrap();
    
    // 查询 user + tool
    let args = SessionSearchArgs {
        mode: "browse".to_string(),
        session_id: Some(sid.clone()),
        anchor_id: Some(2),
        window: Some(3),
        roles: Some(vec!["user".to_string(), "tool".to_string()]),
    };
    
    let result = tool.execute(serde_json::to_value(args).unwrap()).unwrap();
    
    // 验证结果只包含 user + tool
    assert!(result.contains("Q1"));
    assert!(!result.contains("A1"));
    assert!(result.contains("T1"));
    assert!(!result.contains("S1"));
}
```

---

## 📊 完成检查清单

- [ ] `SessionSearchArgs` 添加 `roles` 字段
- [ ] `discovery` 方法支持 roles 参数传递
- [ ] `browse` 方法支持 roles 参数传递
- [ ] 工具 description 更新
- [ ] JSON Schema 更新
- [ ] 单元测试通过（4+ 测试用例）
- [ ] 编译无错误：`cargo build --package hakimi-tools`
- [ ] 集成测试通过：`cargo test --package hakimi-tools`
- [ ] PR 创建和合并
- [ ] CHANGELOG 更新
- [ ] README 更新
- [ ] 版本号递增

---

## 🔗 参考资料

- TASK 2.2.1: SQL 查询角色过滤动态化
- [serde default 属性](https://serde.rs/field-attrs.html#default)

---

**创建时间**: 2026-07-10  
**预计完成**: 2026-07-10（2 小时内）  
**实际完成**: _待填写_
