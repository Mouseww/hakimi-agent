# Hakimi 记忆能力增强 v0.5.56

**发布日期：** 2026-07-09  
**版本：** 0.5.56  
**目标：** 缩小与 Hermes Agent 记忆能力的差距，实现生产级长期记忆管理

---

## 🎯 背景

Hermes Agent 的记忆系统经过多年打磨，具备：
- 插件化记忆后端（Honcho, Mem0, SuperMemory）
- 三模式会话搜索（Discovery/Scroll/Browse）
- Bookends（会话首尾上下文）
- Lineage（父子会话关系）

Hakimi v0.5.55 之前只有基础的文件记忆 + 简单 FTS5 搜索，本次更新对标 Hermes 核心功能，实现生产级记忆管理。

---

## ✨ 新功能

### 1. 分级记忆系统

**设计理念：**
- **长期记忆 (memory.md)**：Agent 的持久化知识库，跨会话保留
- **用户档案 (user.md)**：用户偏好、个人信息，相对稳定
- **工作记忆 (working_memory.md)**：当前会话临时上下文，会话结束可清空

**实现细节：**

```rust
// crates/hakimi-context/src/memory.rs
let title = match name.to_lowercase().as_str() {
    "user" => "USER PROFILE (who the user is)",
    "memory" => "MEMORY (your personal notes)",
    "working_memory" | "working" => "WORKING MEMORY (current session)",
    _ => name,
};
```

**工具接口：**

```json
{
  "name": "memory",
  "parameters": {
    "action": "add|replace|remove",
    "target": "memory|user|working_memory",
    "content": "...",
    "old_text": "..." 
  }
}
```

**使用场景：**

| 记忆类型 | 保留时长 | 典型内容 |
|---------|---------|---------|
| `user` | 永久 | 姓名、职业、偏好、沟通风格 |
| `memory` | 永久 | 工具用法、项目约定、已修复的 bug |
| `working_memory` | 会话级 | "用户正在调试 Rust 错误"、"当前任务：优化性能" |

---

### 2. 增强版会话搜索

**三种模式：**

#### **A. Discovery 模式** — FTS5 搜索 + Bookends

当用户问"我们之前讨论过 X 吗？"时触发。

**参数：**
```json
{
  "query": "Rust memory safety",
  "limit": 5,
  "role_filter": "user|assistant|tool|system"
}
```

**返回内容：**
```
## Search Results for "Rust memory safety"
Found 12 message(s) across 3 session(s)

### Rust 系统编程入门 (2026-07-01 10:30 AM)
**Session ID:** `session-abc123`
**Messages:** 45 | **Tool calls:** 12

**Session Start (first 3 messages):**
  👤 [2026-07-01 10:30 AM] 我想学习 Rust 的内存安全机制
  🤖 [2026-07-01 10:32 AM] Rust 的所有权系统是核心...
  👤 [2026-07-01 10:35 AM] 能详细讲讲生命周期吗？

**Match (2 total):**
  Rust 的内存安全是通过编译期检查实现的，主要包括所有权、借用和生命周期三大机制。不同于 C++ 的手动内存管理，Rust 在编译时就能捕获悬垂指针、数据竞争等问题...

**Session End (last 3 messages):**
  🤖 [2026-07-01 11:45 AM] 这就是 Rust 内存安全的核心原理
  👤 [2026-07-01 11:46 AM] 明白了，非常感谢！
  🤖 [2026-07-01 11:47 AM] 不客气，有问题随时问我

---
```

**Bookends 的价值：**
- **会话开头**：了解用户最初的目标和问题背景
- **匹配内容**：精准定位相关讨论片段
- **会话结尾**：查看最终结论和决策

---

#### **B. Scroll 模式** — 围绕消息的滑动窗口

当用户想查看某个搜索结果的更多上下文时使用。

**参数：**
```json
{
  "session_id": "session-abc123",
  "around_message_id": 142,
  "window": 5
}
```

**返回内容：**
```
## Scroll: Rust 系统编程入门 (Session: `session-abc123`)
Anchor: message #142 | 8 messages before | 12 after

👤 [2026-07-01 10:45 AM] 那生命周期注解是怎么写的？
🤖 [2026-07-01 10:46 AM] 生命周期注解用单引号表示，比如 `'a`...
🤖 [2026-07-01 10:48 AM] ⭐ 举个例子：`fn longest<'a>(x: &'a str, y: &'a str) -> &'a str`
👤 [2026-07-01 10:50 AM] 为什么需要显式标注？
🤖 [2026-07-01 10:51 AM] 因为编译器无法自动推断返回值的生命周期...
👤 [2026-07-01 10:53 AM] 明白了，再问一个问题...

**Navigation:** To scroll forward, call with `around_message_id=147`. To scroll back, use `around_message_id=137`.
```

---

#### **C. Browse 模式** — 最近会话列表

无参数调用时自动触发，快速浏览最近的对话。

**参数：**
```json
{}  // 无参数
```

**返回内容：**
```
## Recent Sessions (5)

**Rust 系统编程入门** (July 01, 2026 at 10:30 AM)
- Session ID: `session-abc123`
- Source: telegram
- Messages: 45 | Tool calls: 12

**Hakimi 性能优化** (July 02, 2026 at 02:15 PM)
- Session ID: `session-def456`
- Source: cli
- Messages: 23 | Tool calls: 8

...

Showing 5 most recent sessions. Pass a `query` to search, or `session_id` + `around_message_id` to scroll.
```

---

## 🏗️ 技术实现

### 数据库层（`hakimi-session`）

新增两个 SQL 查询方法：

```rust
/// 获取围绕 anchor 的消息窗口
fn get_messages_around(
    &self,
    session_id: &str,
    anchor_id: i64,
    window: i64,
) -> Result<(Vec<Message>, i64, i64)> {
    // 1. 验证 anchor 存在
    // 2. 统计前后消息数
    // 3. 查询 [anchor - window, anchor + window] 范围内的消息
    // 返回 (messages, messages_before, messages_after)
}

/// 获取会话首尾的 user+assistant 消息
fn get_bookends(
    &self,
    session_id: &str,
    count: i64,
) -> Result<(Vec<Message>, Vec<Message>)> {
    // 1. 查询前 N 条 user/assistant 消息
    // 2. 查询后 N 条（DESC 后 reverse）
    // 返回 (start_messages, end_messages)
}
```

**SQL 查询示例：**

```sql
-- Bookends - 前 3 条
SELECT * FROM messages
WHERE session_id = ? AND role IN ('user', 'assistant')
ORDER BY id ASC LIMIT 3;

-- Bookends - 后 3 条（需 reverse）
SELECT * FROM messages
WHERE session_id = ? AND role IN ('user', 'assistant')
ORDER BY id DESC LIMIT 3;

-- Around - 窗口查询
SELECT * FROM messages
WHERE session_id = ? AND id >= ? AND id <= ?
ORDER BY id ASC;
```

---

### 工具层（`hakimi-tools`）

**SessionSearchTool 架构：**

```rust
pub struct SessionSearchTool;

impl Tool for SessionSearchTool {
    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        // 模式检测
        if let (Some(sid), Some(anchor)) = (session_id, around_msg_id) {
            return self.scroll_mode(db, sid, anchor, window);
        }
        if !query.is_empty() {
            return self.discovery_mode(db, query, limit, role_filter);
        }
        self.browse_mode(db, limit)
    }
}

impl SessionSearchTool {
    fn browse_mode(&self, db: &SessionDB, limit: i64) -> Result<String> { ... }
    fn discovery_mode(&self, db: &SessionDB, query: &str, ...) -> Result<String> { ... }
    fn scroll_mode(&self, db: &SessionDB, sid: &str, anchor: i64, ...) -> Result<String> { ... }
    fn format_session_with_bookends(...) -> Result<String> { ... }
}
```

**格式化输出：**
- 使用 Markdown headers (`##`, `###`)
- Emoji 角色标识（👤 user, 🤖 assistant, 🔧 tool）
- ⭐ anchor 标记
- 时间戳人类可读格式
- 内容截断（200-300 字符）

---

## 📈 性能优化

### 索引策略

```sql
-- 已有索引
CREATE INDEX idx_messages_session ON messages(session_id, timestamp);

-- FTS5 虚拟表
CREATE VIRTUAL TABLE messages_fts USING fts5(
    content, tool_name, tool_calls,
    content=messages, content_rowid=id
);
```

### 查询优化

1. **Bookends 查询**：两次独立查询（前 N + 后 N），避免复杂子查询
2. **Around 查询**：直接 `id >= ? AND id <= ?` 范围查询，利用主键索引
3. **FTS5 搜索**：`rank` 排序 + `LIMIT` 提前截断
4. **结果大小限制**：64KB 上限，防止 OOM

---

## 🔄 与 Hermes 对比

| 功能点 | Hermes (Python) | Hakimi (Rust) v0.5.56 | 差距 |
|-------|----------------|----------------------|-----|
| **记忆分级** | ❌ 仅 memory + user | ✅ memory + user + working | **持平** |
| **Discovery 模式** | ✅ Bookends + window | ✅ Bookends + snippet | **持平** |
| **Scroll 模式** | ✅ Around message | ✅ Around message | **持平** |
| **Browse 模式** | ✅ Recent sessions | ✅ Recent sessions | **持平** |
| **Lineage 支持** | ✅ parent_session_id | ❌ 未实现 | **待补** |
| **角色过滤** | ✅ SQL 级别过滤 | ⚠️ Schema 定义但未实现 | **待补** |
| **插件化后端** | ✅ Honcho/Mem0/etc | ❌ 仅文件后端 | **待补** |
| **异步 prefetch** | ✅ 后台任务 | ❌ 同步读取 | **待补** |
| **向量检索** | ✅ Mem0 插件 | ❌ 未实现 | **待补** |
| **性能** | Python + asyncio | ✅ Rust + tokio | **Hakimi 优势** |
| **类型安全** | 运行时检查 | ✅ 编译期保证 | **Hakimi 优势** |

---

## 🚀 下一步计划（Phase 2）

### 短期（v0.5.57-v0.5.60）

1. **Lineage 支持**
   - 实现 `resolve_to_parent()` 递归遍历
   - 防止在当前会话 lineage 内搜索（避免重复）
   - 子 agent 会话自动关联 `parent_session_id`

2. **角色过滤完善**
   - 在 FTS5 后二次过滤
   - 添加 SQL 索引 `idx_messages_role`

3. **工作记忆自动清理**
   - 会话结束时清空 `working_memory.md`
   - Gateway `/memory clear working` 命令

### 中期（v0.5.61-v0.5.70）

4. **异步 prefetch**
   - `FileMemoryProvider::prefetch()` 改为后台任务
   - 预取结果缓存（TTL 5分钟）

5. **记忆压缩策略**
   - SmartContextEngine 优先压缩工作记忆
   - Tier 1: 丢弃 working_memory 内容
   - Tier 2: 摘要 memory 旧条目
   - Tier 3: 滑动窗口（保留 user）

### 长期（v0.6.0+）

6. **向量检索集成**
   - 集成 `qdrant-client` 或 `milvus-sdk`
   - 语义搜索 + FTS5 混合排序
   - Embedding 模型：`text-embedding-3-small`

7. **插件化记忆后端**
   - 定义 `MemoryBackend` trait
   - 实现 Honcho 适配器
   - 实现 Mem0 适配器

8. **知识图谱增强**
   - 实体关系提取
   - 图谱可视化（WebUI 集成）
   - Cypher 查询接口

---

## 📊 测试覆盖

### 单元测试

```rust
// crates/hakimi-session/src/message_ops.rs

#[test]
fn test_get_messages_around() {
    let db = test_db();
    let sid = create_test_session(&db);
    
    // 保存 10 条消息
    for i in 1..=10 {
        db.save_message(&sid, &Message::user(format!("msg {i}"))).unwrap();
    }
    
    // 获取消息 5 前后 2 条
    let (window, before, after) = db.get_messages_around(&sid, 5, 2).unwrap();
    
    assert_eq!(before, 4);  // 消息 1-4
    assert_eq!(after, 5);   // 消息 6-10
    assert_eq!(window.len(), 5);  // 消息 3-7
}

#[test]
fn test_get_bookends() {
    let db = test_db();
    let sid = create_test_session(&db);
    
    db.save_message(&sid, &Message::user("Q1")).unwrap();
    db.save_message(&sid, &Message::assistant("A1")).unwrap();
    db.save_message(&sid, &Message::tool("...")).unwrap();  // 应该被跳过
    db.save_message(&sid, &Message::user("Q2")).unwrap();
    db.save_message(&sid, &Message::assistant("A2")).unwrap();
    
    let (start, end) = db.get_bookends(&sid, 2).unwrap();
    
    assert_eq!(start.len(), 2);  // Q1, A1
    assert_eq!(end.len(), 2);    // Q2, A2
}
```

### 集成测试

```rust
// crates/hakimi-tools/src/builtin_session_search.rs

#[tokio::test]
async fn test_discovery_mode() {
    let db = test_db();
    let sid = "test_session";
    db.create_session(sid, None).unwrap();
    db.save_message(sid, &Message::user("Rust programming")).unwrap();
    
    let tool = SessionSearchTool;
    let args = json!({"query": "Rust"});
    let result = tool.execute(&args, &Default::default()).await.unwrap();
    
    assert!(result.contains("Search Results"));
    assert!(result.contains("Rust"));
    assert!(result.contains("Session Start"));
}

#[tokio::test]
async fn test_scroll_mode() {
    let db = test_db();
    let sid = "test_session";
    db.create_session(sid, None).unwrap();
    
    // 保存 10 条消息
    for i in 1..=10 {
        db.save_message(sid, &Message::user(format!("msg {i}"))).unwrap();
    }
    
    let tool = SessionSearchTool;
    let args = json!({
        "session_id": sid,
        "around_message_id": 5,
        "window": 2
    });
    
    let result = tool.execute(&args, &Default::default()).await.unwrap();
    
    assert!(result.contains("Scroll"));
    assert!(result.contains("Anchor: message #5"));
    assert!(result.contains("Navigation"));
}
```

---

## 🎓 用户指南

### 使用工作记忆

**场景 1：调试过程中的临时上下文**

```
User: 帮我修复这个 Rust 编译错误 [贴代码]

Agent: [分析后] 发现是生命周期问题...
       [调用] memory(action="add", target="working_memory", 
                     content="用户正在修复 lifetime 'static 错误")
       
User: 我换个方案试试 [贴新代码]

Agent: [读取 working_memory，知道用户在解决什么问题]
       这个方案可行，但要注意...
       [调用] memory(action="replace", target="working_memory",
                     content="已切换到 Arc<Mutex<T>> 方案")
       
User: 解决了，谢谢！

Agent: [调用] memory(action="remove", target="working_memory",
                     old_text="已切换到 Arc<Mutex<T>> 方案")
```

---

### 使用会话搜索

**场景 2：回忆之前的讨论**

```
User: 我们之前讨论过 Docker 网络配置吗？

Agent: [调用] session_search(query="Docker network config")
       
       找到了！在 2026-06-15 的会话中，我们讨论了：
       
       会话开头：你问我如何配置 bridge 网络
       匹配内容：Docker 的 bridge 网络模式通过 docker0 接口连接容器...
       会话结尾：最终决定使用 macvlan 方案
       
User: 能再看看那次讨论的详细内容吗？

Agent: [调用] session_search(session_id="session-xyz", around_message_id=42, window=10)
       
       [显示前后 10 条消息的完整对话]
```

---

## 📝 API 文档

### MemoryTool

**Schema:**
```json
{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": ["add", "replace", "remove"],
      "description": "操作类型"
    },
    "target": {
      "type": "string",
      "enum": ["memory", "user", "working_memory"],
      "description": "目标记忆文件"
    },
    "content": {
      "type": "string",
      "description": "新内容（add/replace 必需）"
    },
    "old_text": {
      "type": "string",
      "description": "要删除的文本（remove 必需）"
    }
  },
  "required": ["action", "target"]
}
```

**示例：**
```bash
# 添加长期记忆
{"action": "add", "target": "memory", "content": "用户喜欢简洁的代码"}

# 更新用户档案
{"action": "replace", "target": "user", "content": "Name: Alice\nRole: Backend Engineer"}

# 删除工作记忆
{"action": "remove", "target": "working_memory", "old_text": "临时上下文"}
```

---

### SessionSearchTool

**Schema:**
```json
{
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "description": "FTS5 搜索查询（Discovery 模式）"
    },
    "session_id": {
      "type": "string",
      "description": "会话 ID（Scroll 模式，需配合 around_message_id）"
    },
    "around_message_id": {
      "type": "integer",
      "description": "锚点消息 ID（Scroll 模式）"
    },
    "window": {
      "type": "integer",
      "minimum": 1,
      "maximum": 20,
      "default": 5,
      "description": "窗口大小（Scroll 模式）"
    },
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 50,
      "default": 5,
      "description": "结果数量（Discovery/Browse 模式）"
    },
    "role_filter": {
      "type": "string",
      "enum": ["user", "assistant", "tool", "system"],
      "description": "角色过滤（Discovery 模式）"
    }
  }
}
```

**示例：**
```bash
# Browse 模式
{}

# Discovery 模式
{"query": "Rust memory safety", "limit": 10}

# Scroll 模式
{"session_id": "abc123", "around_message_id": 42, "window": 5}
```

---

## 🔐 安全考虑

### 1. 工作记忆隐私
- 不应包含敏感信息（密码、密钥）
- 会话结束时自动清空（TODO）
- 不会同步到外部服务

### 2. 会话搜索权限
- 仅搜索当前用户的会话
- 不跨用户检索（Gateway 层隔离）
- 敏感内容应标记 `source="private"`

### 3. 数据大小限制
- 单次记忆操作 < 10KB
- 搜索结果输出 < 64KB
- 工作记忆总大小 < 50KB（建议）

---

## 🎉 总结

本次更新让 Hakimi 的记忆能力从"基础可用"跃升至"生产级"，核心改进：

1. **分级记忆** → 清晰的短期/长期/工作记忆分离
2. **三模式搜索** → Discovery/Scroll/Browse 对标 Hermes
3. **Bookends** → 会话首尾上下文增强理解
4. **类型安全** → Rust 编译期保证 vs Python 运行时检查

下一步将补齐 Lineage、角色过滤和异步 prefetch，在 v0.6.0 实现向量检索和插件化后端，届时 Hakimi 的记忆能力将全面超越 Hermes。

---

**参考资料：**
- Hermes `tools/session_search_tool.py` (602 行)
- Hermes `tools/memory_tool.py` (586 行)
- Hakimi `crates/hakimi-session/src/message_ops.rs` (826 行)
- Hakimi `crates/hakimi-tools/src/builtin_session_search.rs` (400+ 行)
