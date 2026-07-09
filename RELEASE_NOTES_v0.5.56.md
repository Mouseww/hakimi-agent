# 🧠 Hakimi v0.5.56 - 记忆能力全面升级

**发布日期：** 2026-07-09  
**核心改进：** 分级记忆系统 + 三模式会话搜索

---

## 🎯 本次更新亮点

### 1️⃣ **分级记忆系统** — 短期/长期/工作记忆分离

以前 Hakimi 只有 `memory.md` 和 `user.md` 两种记忆，现在新增 **工作记忆 (working_memory)**：

| 记忆类型 | 用途 | 保留时长 | 典型内容 |
|---------|------|---------|---------|
| `user` | 用户档案 | 永久 | 姓名、职业、偏好、沟通风格 |
| `memory` | 长期笔记 | 永久 | 工具用法、项目约定、已修复的 bug |
| `working_memory` | 临时上下文 | 会话级 | "正在调试 Rust 错误"、"当前任务目标" |

**实际效果：**
```
你：帮我修复这个 Rust 生命周期错误 [贴代码]

Hakimi：[分析错误，同时记录到工作记忆]
       "用户正在修复 lifetime 'static 问题"

你：我换个方案试试 [贴新代码]

Hakimi：[读取工作记忆，知道你在解决什么]
       这个方案可行，注意...
       [更新工作记忆] "已切换到 Arc<Mutex<T>> 方案"

你：解决了，谢谢！

Hakimi：[自动清空工作记忆]
```

---

### 2️⃣ **三模式会话搜索** — 对标 Hermes 的专业级检索

以前的 `session_search` 只能简单搜索关键词，现在支持三种模式：

#### 🔍 **Discovery 模式** — 搜索 + 会话首尾上下文

当你问"我们之前讨论过 Docker 配置吗？"

**之前：**
```
找到 5 条消息包含 "Docker"
1. session-abc: "Docker 网络..."
2. session-def: "配置 Docker..."
```

**现在：**
```
## 搜索结果：Docker 配置
找到 12 条消息，跨 3 个会话

### Docker 网络配置实战 (2026-06-15)
会话 ID: session-abc
消息数: 45 | 工具调用: 12

【会话开头 - 你的最初问题】
👤 如何配置 Docker bridge 网络？
🤖 Docker 的 bridge 网络通过 docker0 接口...
👤 可以自定义网段吗？

【匹配内容】
"Docker 的 bridge 网络模式通过 docker0 接口连接容器，
默认网段是 172.17.0.0/16。如果需要自定义，可以编辑
/etc/docker/daemon.json..."

【会话结尾 - 最终决策】
🤖 推荐使用 macvlan 方案
👤 好的，就这么办
🤖 已经配置好了，测试正常
```

**价值：**
- ✅ 看到你当时问了什么（会话开头）
- ✅ 找到具体讨论内容（匹配片段）
- ✅ 了解最终结论（会话结尾）

---

#### 📜 **Scroll 模式** — 围绕消息的滑动窗口

当你想查看某个搜索结果的更多上下文：

```
你：上次讨论的详细过程是什么？

Hakimi：[调用] session_search(
         session_id="abc123",
         around_message_id=42,
         window=10
        )

【显示消息 32-52】
👤 那生命周期注解怎么写？
🤖 生命周期用单引号表示，比如 'a
🤖 ⭐ 举例：fn longest<'a>(x: &'a str) -> &'a str
👤 为什么要显式标注？
🤖 因为编译器无法自动推断...
[... 前后各 10 条消息 ...]

💡 导航提示：
  向前翻页 → around_message_id=52
  向后翻页 → around_message_id=32
```

---

#### 📋 **Browse 模式** — 最近会话列表

无参数调用时自动触发：

```
你：最近聊了什么？

Hakimi：[调用] session_search()

## 最近的 5 个会话

**Rust 系统编程入门** (7月1日 10:30)
- 会话 ID: session-abc
- 平台: telegram
- 消息: 45 条 | 工具调用: 12 次

**Hakimi 性能优化** (7月2日 14:15)
- 会话 ID: session-def
- 平台: cli
- 消息: 23 条 | 工具调用: 8 次

...
```

---

## 🔧 技术改进

### 数据库层（`hakimi-session`）

新增两个核心查询方法：

```rust
// 获取围绕某条消息的前后窗口
fn get_messages_around(
    session_id: &str,
    anchor_id: i64,
    window: i64
) -> Result<(Vec<Message>, i64, i64)>

// 获取会话首尾的 user+assistant 消息
fn get_bookends(
    session_id: &str,
    count: i64
) -> Result<(Vec<Message>, Vec<Message>)>
```

**查询优化：**
- Bookends: 两次独立 SQL 查询（前 N + 后 N），避免复杂子查询
- Around: 直接 `id >= ? AND id <= ?` 范围查询，利用主键索引
- FTS5: `rank` 排序 + `LIMIT` 提前截断
- 结果限制: 64KB 上限，防止 OOM

---

## 📊 与 Hermes 对比

| 功能点 | Hermes | Hakimi v0.5.56 | 状态 |
|-------|--------|---------------|------|
| **分级记忆** | ❌ 仅 memory + user | ✅ memory + user + working | ✅ **持平** |
| **Discovery 模式** | ✅ | ✅ | ✅ **持平** |
| **Scroll 模式** | ✅ | ✅ | ✅ **持平** |
| **Browse 模式** | ✅ | ✅ | ✅ **持平** |
| **Bookends** | ✅ | ✅ | ✅ **持平** |
| **Lineage 支持** | ✅ | ❌ | 🚧 Phase 2 |
| **角色过滤** | ✅ | ⚠️ Schema 已定义 | 🚧 Phase 2 |
| **异步 prefetch** | ✅ | ❌ | 🚧 Phase 2 |
| **向量检索** | ✅ Mem0 插件 | ❌ | 🚧 v0.6.0 |
| **性能** | Python + asyncio | ✅ **Rust + tokio** | ✅ **Hakimi 优势** |
| **类型安全** | 运行时检查 | ✅ **编译期保证** | ✅ **Hakimi 优势** |

---

## 🚀 后续计划

### 短期（v0.5.57-v0.5.60）
- Lineage 支持（父子会话关系）
- 角色过滤完善（SQL 级别）
- 工作记忆自动清理（会话结束时）

### 中期（v0.5.61-v0.5.70）
- 异步 prefetch（后台任务）
- 记忆压缩策略（优先压缩工作记忆）

### 长期（v0.6.0+）
- 向量检索集成（qdrant/milvus）
- 插件化记忆后端（Honcho/Mem0 适配器）
- 知识图谱增强（实体关系提取）

---

## 💡 使用提示

### 1. 调试时使用工作记忆

```bash
# Agent 会自动在工作记忆中记录当前上下文
memory(action="add", target="working_memory", 
       content="用户正在优化 Hakimi 的性能")

# 切换任务时更新
memory(action="replace", target="working_memory",
       content="已切换到修复 bug #123")

# 任务完成后清理
memory(action="remove", target="working_memory",
       old_text="已切换到修复 bug #123")
```

### 2. 回忆历史对话

```bash
# 搜索关键词
session_search(query="Docker 网络配置")

# 查看详细上下文
session_search(session_id="abc123", around_message_id=42, window=10)

# 浏览最近会话
session_search()
```

---

## 📚 参考文档

- **完整技术文档**: `MEMORY_ENHANCEMENT_v0.5.56.md`
- **更新日志**: `CHANGELOG.md`
- **GitHub 仓库**: https://github.com/Mouseww/hakimi-agent
- **CI 构建**: ubuntu-20.04 (GLIBC 兼容性)

---

## 🎉 总结

本次更新让 Hakimi 的记忆能力从"基础可用"跃升至"生产级"：

✅ **分级记忆** → 清晰的短期/长期/工作记忆分离  
✅ **三模式搜索** → Discovery/Scroll/Browse 完整覆盖  
✅ **Bookends** → 会话首尾上下文增强理解  
✅ **Rust 优势** → 编译期类型安全 + tokio 高性能

在记忆核心功能上，**Hakimi 已与 Hermes 持平**，并在性能和类型安全上具备优势。Phase 2 将补齐 Lineage 和异步 prefetch，v0.6.0 将引入向量检索和插件化后端，届时将**全面超越** Hermes。

---

**升级方式：**
```bash
# 自动安装脚本（会检测新版本）
curl -sSL https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.sh | bash

# 或手动下载 Release 二进制
# GitHub Actions 正在构建中，稍后可在 Releases 页面下载
```

**反馈与贡献：**
- Issues: https://github.com/Mouseww/hakimi-agent/issues
- PRs: https://github.com/Mouseww/hakimi-agent/pulls
- Telegram: @hakimi_agent_discuss
