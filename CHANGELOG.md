# Changelog

## [0.5.70] - 2026-07-10

### Added
- WebUI session tree visualization component (TASK_2.1.4) ✅
  - New `SessionTree` React component for hierarchical session display
  - Tree structure rendering with recursive child sessions
  - Expand/collapse interaction for tree navigation
  - Click-to-navigate session switching
  - Session metadata display (creation date, message count)
  - Breadcrumb lineage path from root to current session
  - Responsive design with Tailwind CSS
  - Integrated into main App with toggle button (FolderTree icon)
  
### Added (API)
- Added `SessionMetadata`, `SessionTreeNode`, `SessionTreeResponse` types to api.ts
- Added `fetchSessionTree()` API function with authentication support

### Technical Details
- **Component**: `hakimi-webui/src/SessionTree.tsx`
- **Integration**: Toggle button in session list header
- **Icons**: lucide-react (ChevronRight, ChevronDown, MessageSquare, FolderTree)
- **State management**: React hooks for expand/collapse state
- **Auto-expansion**: Current session and ancestors auto-expand on load
- **Build**: TypeScript compilation successful, Vite build optimized

## [0.5.69] - 2026-07-10

### Added
- Session tree visualization API endpoint (TASK_2.1.4)
  - New GET /api/sessions/:id/tree endpoint for complete session tree structure
  - Returns current session, root session, full lineage, and recursive children
  - Data structures: `SessionTreeResponse` and `SessionTreeNode`
  
### Fixed
- Proper error handling for session tree API operations
  - All database operations now use `.map_err()` for consistent error types
  - Improved error messages for debugging
  - Compilation errors resolved

### Technical Details
- **API endpoint**: GET /api/sessions/:id/tree
- **Response structure**: 
  - `current`: Current session information
  - `root`: Root session metadata
  - `lineage`: Full path from root to current
  - `children`: Recursive child session tree
- Reuses existing hakimi-session crate APIs
- Foundation for WebUI session tree visualization


All notable changes to Hakimi Agent will be documented in this file.

## [0.5.68] - 2026-07-10

### Added
- Performance metrics collection in core agent operations (TASK_1.1.2)
  - Created comprehensive `ConversationMetrics` structure
  - Added metrics for latency, token counts, and API calls
  - Integrated `MetricsTimer` for duration tracking
  - Added metrics field to `ConversationResult`
  - Automatic metrics collection in conversation loops
  - Support for iteration and token limit detection

### Technical Details
- **metrics.rs**: New module with `ConversationMetrics`, `ToolMetric`, and `MetricsTimer`
- **conversation.rs**: Added `metrics` field to `ConversationResult`
- **loop_impl.rs**: Integrated metrics tracking throughout execution
- Metrics include: total_duration_ms, api_call_count, token usage, tool execution times
- Foundation for observability dashboards and performance analysis

## [0.5.67] - 2026-07-10

### Added
- Tracing instrumentation in core agent operations (TASK_1.1.1)
  - Added `#[instrument]` macro to key public APIs in `AIAgent`
  - Added tracing spans to conversation loop functions
  - Added tracing spans to tool dispatch operations
  - Configured session_id, message metadata, and tool names as span fields
  - Integrated tracing infrastructure for observability

### Technical Details
- Enhanced `agent.rs` with instrumentation on `chat()`, `run_conversation()` methods
- Enhanced `loop_impl.rs` with spans on `run_loop()`, `run_loop_streaming()`, and `dispatch_tool()`
- All spans include contextual information (session_id, tool names, etc.)
- Foundation for future observability features

## [0.5.66] - 2026-07-10

### 📊 记忆容量监控 (任务 1.2.2)

#### Added
- **记忆文件大小限制**：
  - 软限制（60KB）：警告日志，提示用户清理
  - 硬限制（64KB）：拒绝加载，返回友好错误信息
  - 常量定义：`MEMORY_WARN_SIZE_BYTES`, `MEMORY_MAX_SIZE_BYTES`
  
- **新增方法**：
  - `FileMemoryProvider::check_file_size(filename)` — 检查单个文件大小
  - 返回 `Result<(), String>`，超限时包含大小和限制说明
  
- **自动容量检查**：
  - `system_prompt_block()` 加载时检查文件大小
  - 超限文件自动跳过并记录错误日志
  - 警告区间（60-64KB）使用 `warn!` 日志

#### Improved
- **友好错误提示**：
  - 超限错误包含文件名、当前大小、限制大小
  - 引导用户使用清理或归档命令
  - 格式：`Memory file 'memory.md' exceeds maximum size (70 KB > 64 KB). Please clean up or archive old content.`

#### Testing
- ✅ 4 个单元测试全部通过：
  - `test_check_file_size_within_limits` — 正常大小文件（30KB）
  - `test_check_file_size_warning_zone` — 警告区间文件（62KB）
  - `test_check_file_size_exceeds_limit` — 超限文件（70KB）
  - `test_check_file_size_nonexistent_file` — 不存在的文件
- 编译无错误，兼容现有代码

#### Technical
- 使用 `std::fs::metadata()` 获取文件大小（低开销）
- 日志级别：`warn!`（警告区间）、`error!`（硬限制）
- 为未来 MemoryTool 集成预留接口

#### Dependencies
- 依赖：任务 1.2.1（工作记忆生命周期管理）
- 解锁：任务 1.2.3（记忆归档机制，引用容量限制错误提示）

## [0.5.65] - 2026-07-10

### session_search 工具集成 Lineage (任务 2.1.3)

#### Features
- **session_search lineage 支持**
  - 新增 `include_lineage` 参数（默认 true）
  - Discovery 模式去重时优先保留 root 会话
  - 搜索结果显示会话父子关系
  - Browse 模式和 Discovery 模式都显示 lineage 信息

#### Implementation
- **辅助函数**:
  - `format_lineage(&SessionMeta, &SessionDB)` - 格式化 lineage 信息
  - `get_session_depth(&SessionMeta, &SessionDB)` - 计算会话深度（root = 0）
- **去重优先级**: Discovery 模式按会话深度排序（root 会话优先）
- **输出格式**: 
  - 显示父会话 ID 和标题
  - 显示根会话 ID 和标题（如果与当前会话不同）
  - 缩进格式：`  - Parent: \`id\` (title)`

#### Improvements
- **会话搜索排序**: 根会话优先于子会话显示
- **会话元信息**: 自动显示父会话和根会话标题
- **循环检测**: 防止无限循环（100 层深度限制）

#### Testing
- 所有现有集成测试通过 (18/18)
- 编译成功，无新增错误

#### Files Changed
- `crates/hakimi-tools/src/builtin_session_search.rs`: 主要实现 (+93 行)
- `tasks/TASK_2.1.3_session_search_lineage.md`: 任务文档

## [0.5.64] - 2026-07-10

### Lineage 查询 API (任务 2.1.2)

#### Features
- **新增会话谱系查询 API**
  - `get_session_lineage(&self, session_id: &str) -> Result<Vec<SessionMeta>>`
    - 获取从当前会话到根会话的完整谱系链
    - 返回顺序：[当前会话, 父会话, ..., 根会话]
    - 支持单会话、多代会话树、多分支树场景
  - `get_root_session_meta(&self, session_id: &str) -> Result<SessionMeta>`
    - 快速获取根会话的完整元数据
    - 优先使用 `root_session_id` 字段，回退到父节点遍历

#### Safety & Reliability
- **循环引用检测**：使用 HashSet 追踪已访问节点，防止无限循环
- **深度限制保护**：限制最大深度 100 层，防止栈溢出
- **孤儿会话检测**：检测 parent_id 指向不存在的会话
- **完整错误处理**：所有边界情况都有清晰的错误消息

#### Testing
- 新增 5 个单元测试覆盖所有场景：
  - `test_get_session_lineage_single_session` - 单个会话场景
  - `test_get_session_lineage_three_generations` - 三代会话树
  - `test_get_session_lineage_nonexistent_session` - 错误处理
  - `test_get_root_session_meta_single_session` - 根会话查询
  - `test_get_root_session_meta_multi_branch` - 多分支树
- 所有测试通过 (13/13 passed in test_lineage.rs)

#### Code Quality
- 修复 clippy 警告：redundant closure, enclosing Ok 和 ? operator
- 代码通过 `cargo clippy` 检查
- 完整构建成功

#### Files Changed
- `crates/hakimi-session/src/session_ops.rs`: 添加 trait 方法和实现 (+134 行)
- `crates/hakimi-session/tests/test_lineage.rs`: 添加测试用例 (+184 行)
- `tasks/TASK_2.1.2_lineage_query_api.md`: 任务文档

## [0.5.63] - 2026-07-10

### 会话管理压力测试和边界测试 (任务 1.3.3)

#### Testing
- 新增 9 个全面的压力测试和边界测试用例
  - **压力测试 (5 个)**：
    - 10K 消息搜索性能测试：验证 FTS5 搜索、get_messages_around、get_bookends 性能 (< 500ms)
    - 100 会话并发创建测试：创建 100 个会话 × 10 条消息，验证完成时间 < 10 秒
    - 大结果集查询测试：2,000 条消息，返回 1,500 个结果，验证性能 < 500ms
    - P95 延迟基准测试：100 次搜索操作的 P95 延迟 < 500ms
    - 数据库完整性测试：10 个会话 × 100 条消息，验证数据完整性
  - **边界条件测试 (4 个)**：
    - 空会话测试：验证空会话操作不崩溃
    - 单消息会话测试：验证只有一条消息的会话处理正确
    - 超长消息测试：100KB 消息存储和检索测试
    - 特殊字符测试：会话 ID 中的破折号、下划线、点等特殊字符
- 使用 tempfile crate 创建隔离测试环境
- 使用全局锁序列化测试避免 HAKIMI_HOME 冲突
- 所有测试通过 ✅

#### Technical
- 新增文件：crates/hakimi-session/tests/stress_test.rs (470+ 行)
- 新增依赖：tempfile 3.8, futures 0.3 (dev-dependencies)
- 修复 Message 结构初始化：移除不存在的 cached 字段
- 修复测试数据库初始化：添加 initialize() 调用

#### Documentation
- tasks/TASK_1.3.3_stress_and_boundary_tests.md (任务文档待创建)
- PR #18: Comprehensive Stress and Boundary Tests

## [0.5.62] - 2026-07-10

### Memory 工具错误路径测试 (任务 1.3.2)

#### Testing
- 新增 11 个 memory 工具错误路径测试用例
  - 错误处理 (2 个)：文件不存在、权限拒绝（Unix）
  - 大内容处理 (2 个)：65KB、1MB 内容测试
  - 并发测试 (1 个)：10 个并发写入验证（无 panic + 部分数据保留）
  - 边界情况 (6 个)：空内容、Unicode、特殊字符、别名、部分移除、多次移除
- 使用 tempfile crate 创建隔离测试环境
- 并发测试使用 tokio::spawn + futures::join_all
- Unix 平台权限测试使用 PermissionsExt
- 所有测试通过 ✅

#### Technical
- 新增文件：crates/hakimi-tools/tests/memory_error_paths_test.rs (300+ 行)
- 测试覆盖从快乐路径到错误边界的完整场景
- 已知限制：并发写入的文件系统竞态条件，权限测试仅限 Unix

#### Documentation
- tasks/TASK_1.3.2_memory_tool_error_paths.md (任务文档)

## [0.5.61] - 2026-07-10

### Session Search 集成测试 (任务 1.3.1)

#### Testing
- 新增 18 个 session_search 工具集成测试用例
  - Browse 模式：列出最近会话 (3 个测试)
  - Discovery 模式：FTS5 搜索 + bookends 上下文 (5 个测试)
  - Scroll 模式：围绕消息滚动，边界检测 (6 个测试)
  - 错误处理：空会话、不存在的会话 (2 个测试)
  - 参数验证：limit 和 window 参数限制 (2 个测试)
- FTS5 搜索测试：支持英文和中文关键词
- 测试环境隔离：使用 HAKIMI_HOME 环境变量和临时目录
- 测试执行：需使用 `--test-threads=1` 避免环境变量竞争

#### Technical
- 新增文件：crates/hakimi-tools/tests/session_search_integration_test.rs (519 行)
- 使用 tempfile crate 创建临时测试数据库
- 全局 Mutex 锁确保环境变量设置的原子性
- 所有测试通过 ✅

## [0.5.60] - 2026-07-10

### Tracing Spans 追踪系统 (任务 1.1.1)

#### Added
- 新增 Span 数据结构：追踪操作生命周期
  - SpanId 和 TraceId：基于 UUID v4 的唯一标识
  - 父子关系：parent_span_id 支持嵌套追踪
  - 状态管理：Running/Success/Error/Cancelled
  - 时间追踪：自动记录开始/结束时间，计算持续时间（纳秒精度）
  - 元数据存储：tags (HashMap) + events (SpanEvent)
- 新增 Tracer 管理器：收集和存储 Span
  - start_trace(name)：创建新的 Trace
  - record_span(span)：记录完成的 Span
  - get_trace_spans(trace_id)：查询完整调用链
  - clear_trace(trace_id)：清理过期数据
  - stats()：获取统计信息（总数、平均值）
- 新增 SpanContext：RAII 模式自动管理 Span 生命周期
  - Drop 时自动调用 finish()
  - 简化作用域内的追踪管理
- SpanEvent：记录 Span 内的重要事件
  - 时间戳 + 名称 + 属性

#### Technical
- 依赖：uuid v1 (features: v4, serde), chrono v0.4 (features: serde)
- 线程安全：Arc<RwLock> 实现并发访问
- 新增模块：
  - crates/hakimi-metrics/src/tracing.rs (276 行)
  - crates/hakimi-metrics/src/tracer.rs (209 行)
- 示例代码：crates/hakimi-metrics/examples/tracing_example.rs

#### Testing
- 14 个单元测试全部通过
- 测试覆盖：生命周期、嵌套、上下文、事件、统计

## [0.5.59] - 2026-07-10

### 记忆归档机制 (任务 1.2.3)

#### Added
- 新增 MemoryArchive 结构体，管理记忆归档操作
- 支持按日期归档旧记忆：archive_before(cutoff_date)
- 归档文件按年-月组织：~/.hakimi/memory/archive/2026-01/memory_archived.md
- 归档后在 memory.md 中保留索引，指向归档位置
- 归档统计：ArchiveStats 记录条目数、大小、路径、耗时
- 归档管理 API：list_archives(), restore_archive()
- 时间戳解析支持多种格式
- MemoryEntry 结构：timestamp + content + metadata

#### Improved
- 归档前自动备份 memory.md（带时间戳）
- 归档操作原子性（失败时可从备份恢复）
- 详细错误信息和日志记录

#### Testing
- 完整测试覆盖（6/6 通过）
- 测试场景包括：解析、分组、归档、列出、恢复

#### Technical
- 新增模块：crates/hakimi-context/src/archive.rs
- 代码行数：约 580 行（含测试）


## [0.5.58] - 2026-07-10

### 🔄 工作记忆生命周期管理 (任务 1.2.1)

#### Added
- **会话结束时自动清理工作记忆**：
  - 新增 `FileMemoryProvider::finalize_session()` 方法
  - 自动归档 `working_memory.md` 内容到 `memory.md`（带时间戳）
  - 归档后清空工作记忆，防止泄漏到新会话
  - 归档格式：`---\n[Session ended: 2026-07-10 12:34 UTC]\n<content>`

#### Improved
- **记忆管理测试覆盖**：
  - 空工作记忆场景（无归档，直接清空）
  - 有内容场景（正确归档+清空）
  - 多次归档场景（累积追加到 memory.md）
  - 所有测试通过（3/3）

#### Technical
- 使用 `tracing::info` 记录归档操作（包含字符数）
- 错误处理：文件不存在视为空内容（正常场景）
- 添加 `tempfile` 到 dev-dependencies（测试需要）
- 为未来集成 Gateway `/new` 命令和 CLI 会话重置留出扩展点

### Dependencies
- 依赖：任务 1.1.x（已完成）
- 解锁：任务 1.2.2（记忆容量监控），1.2.3（记忆归档机制）

## [0.5.57] - 2026-07-10

### 🛡️ 错误处理增强 (任务 1.1.3)

#### Added
- **结构化错误类型系统**：
  - 新增 `HakimiError` 枚举，支持结构化变体（`Session`, `Memory`, `Context`, `Tool`, `Transport`）
  - `ErrorContext` 携带完整上下文（`session_id`, `user_id`, `timestamp`, `operation`, `details`）
  - 向后兼容的简单变体（`ToolSimple`, `TransportSimple`, `ContextSimple` 等）

- **领域专属错误**：
  - `SessionError`：会话相关错误（`NotFound`, `InvalidId`, `MessageNotFound`, `SearchFailed`）
  - `MemoryError`：记忆管理错误（`FileNotFound`, `FileTooLarge`, `InvalidTarget`, `PermissionDenied`）
  - `ToolError`：工具执行错误（`ExecutionFailed`, `InvalidArguments`, `Timeout`）

- **自动日志记录**：
  - `HakimiError::log()` 方法自动输出结构化日志（包含 `session_id`/`user_id`/`timestamp`/`operation`）
  - 支持按 `error_type`/`session_id`/`operation` 过滤日志
  - 包含完整的 source 链追踪（backtrace）

#### Improved
- **错误追踪能力**：
  - `message_ops`: 所有数据库错误携带 `session_id` 上下文
  - `memory`: 文件大小限制错误包含 `target`/`size`/`limit` 详细信息
  - `session_search`: FTS5 错误包含查询字符串和参数
  - `builtin_session_search`: 新增 `session_error` 辅助函数简化错误创建

#### Technical
- 所有核心 crate 迁移到新错误类型（`hakimi-common`, `hakimi-session`, `hakimi-context`, `hakimi-tools`）
- `error_classifier` 支持新错误结构
- 测试覆盖所有错误场景（61 个测试通过）
- 为未来集成 Sentry/Datadog 等工具奠定基础

### Dependencies
- 依赖：任务 1.1.1 (tracing spans), 1.1.2 (performance metrics)
- 解锁：任务 1.2.x (记忆管理可使用完善的错误处理)

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
