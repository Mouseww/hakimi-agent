# Changelog

All notable changes to Hakimi Agent will be documented in this file.

## [0.5.56] - 2026-07-09

### Added
- **分级记忆系统**：支持短期/长期/工作记忆分离
  - `working_memory` target：当前会话临时记忆（会话结束后可清空）
  - `memory` target：长期个人笔记（agent 持久化知识）
  - `user` target：用户档案（稳定用户信息）
  - 在 `MemoryTool` 和 `FileMemoryProvider` 中统一支持
  
- **增强版会话搜索 (session_search)**：对标 Hermes 的三模式搜索
  - **Discovery 模式**：FTS5 全文搜索 + bookends（会话开头和结尾各 3 条 user+assistant 消息）
    - 展示会话标题、时间、消息数、工具调用数
    - 显示搜索匹配内容的上下文片段
    - 自动按会话分组并去重
  - **Scroll 模式**：围绕特定消息 ID 的滑动窗口浏览
    - 支持前后滚动导航（`around_message_id` + `window` 参数）
    - 显示 anchor 标记和剩余消息数
    - 提供导航提示（forward/backward）
  - **Browse 模式**：最近会话列表（无参数时自动触发）
    - 按活跃时间倒序展示
    - 显示会话元数据和预览
  - 新增 `MessageOps` trait 方法：
    - `get_messages_around()`: 获取围绕 anchor 的消息窗口
    - `get_bookends()`: 获取会话首尾的 user+assistant 消息

### Technical
- 在 `hakimi-session` 中实现了 `get_messages_around` 和 `get_bookends` SQL 查询
- 支持角色过滤（user/assistant/tool/system）
- 时间戳格式化（RFC3339 → 人类可读）
- 结果大小限制（64KB）确保不会 OOM

## [0.5.21] - 2026-07-04

### Fixed
- **Team 协作流式输出**：修复委托子 agent 时无法看到执行过程的问题
  - 现在可以实时看到子 agent 的思考过程、工具调用和文本输出
  - 不再只显示"开始"和"结束"，中间所有流式内容都会转发
  - 改进 `PersonaTeamExecutor` 的 streaming callback，转发所有非控制字符的文本块

## [0.5.20] - 2026-07-04

### Added
- **Team 工具执行模式增强**：支持串行、并行和分阶段执行，解决任务依赖问题
  - **Sequential Mode**：任务串行执行，每个任务接收前序结果作为上下文（`mode: "sequential"`）
  - **Stages Mode**：分阶段执行，每个 stage 内并行，stage 之间串行（`stages` 参数）
  - **Parallel Mode**：保持原有并发行为（默认，`mode: "parallel"`）
  
**使用示例**：
```json
// 串行模式（有依赖）
{"mode": "sequential", "tasks": [
  {"teammate": "researcher", "task": "搜索方案"},
  {"teammate": "coder", "task": "基于研究实现"}
]}

// 分阶段模式（混合）
{"stages": [
  {"tasks": [{"teammate": "researcher", "task": "研究"}]},
  {"tasks": [  // 并行
    {"teammate": "backend", "task": "后端"},
    {"teammate": "frontend", "task": "前端"}
  ]},
  {"tasks": [{"teammate": "reviewer", "task": "审查"}]}
]}
```

### Fixed
- 解决多 agent 协作时无法处理任务依赖关系的问题

## [0.5.19] - 2026-07-04

### Added
- **Team 工具任务分工增强**：新增 `tasks` 参数，支持为每个 teammate 分配不同的子任务
  - 旧模式（`teammates` 数组）：所有 agent 接收相同任务（已标记为 DEPRECATED）
  - 新模式（`tasks` 数组）：每个 agent 接收专属的 `task` 和 `context`，实现真正的任务分工
  - 示例：`{"tasks": [{"teammate": "researcher", "task": "搜索解决方案"}, {"teammate": "coder", "task": "实现修复"}]}`

### Fixed
- 修复多 agent 并行调度时所有 agent 接收相同提示词的问题

## [0.5.6] - 2025-07-01

### Fixed
- Fixed OpCode Bug in qq-bot-sdk WebSocket implementation
- Corrected OpCode enum representation to match QQ Bot API specification

## [0.5.5] - Previous Release

### Previous Changes
- See git history for details
