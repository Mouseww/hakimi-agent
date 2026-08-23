# Hakimi SmartContextEngine 产品化设计

> Task P3.2：把现有压缩型 `SmartContextEngine` 产品化为可观测、可扩展、按需加载的 ContextProvider 架构设计。本文是设计记录，不在本任务中做大规模实现迁移。

## 背景

当前 `hakimi-context` 已有几类上下文能力：

- `ContextEngine` trait：负责 token 统计、压缩触发、会话生命周期。
- `SmartContextEngine`：三层压缩策略（丢弃旧工具结果、总结旧轮次、滑动窗口）。
- `MemoryProvider` / `FileMemoryProvider`：文件记忆、prefetch、工具定义与工具调用处理。
- `prompt_builder`：组装 context files、skills、environment hints、system prompt。
- `ToolSanitizer` / `ContextPlanner`：已承担一部分预算和工具噪声控制。

问题是这些能力仍偏“拼装式”：memory/session/skills/project context 的加载时机、预算归属、可观测性和降级策略没有统一抽象。P3.2 的目标是先定义产品化边界，后续按小任务落地。

## 目标

1. 引入统一 `ContextProvider` 概念，让 memory/session/skill/project context 都以同一接口参与上下文构建。
2. 保持 durable history 与 request-local planning 分离：历史仍在 session/memory 层持久化，请求内预算和选择由 planner 决定。
3. 提供按需加载：只有当前请求需要的 provider 才读取磁盘、检索 DB 或渲染大块文本。
4. 建立预算和可观测性：每个 provider 报告 token 估算、耗时、命中/跳过原因、降级情况。
5. 不破坏现有 Gateway/TUI/CLI 行为；先做适配层，逐步替换直接 prompt 拼接。

## 非目标

- 本设计不重写 provider SSE / transport 层。
- 本设计不恢复旧 WebUI runtime。
- 本设计不把 Hakimi 功能改用 Python 实现。
- 本设计不一次性替换 `SmartContextEngine` 的三层压缩逻辑。
- 本设计不要求本机全量 release build；验证以 targeted test/CI 为主。

## 分层关系

```text
Session / Memory / Skills / Project / Runtime hints
        │
        ▼
ContextProvider implementations
        │  collect lightweight candidates + metadata
        ▼
SmartContextPlanner
        │  rank, budget, sanitize, degrade
        ▼
ContextAssembly
        │  deterministic prompt blocks + durable history tail
        ▼
hakimi-core request messages
        │
        ▼
ContextEngine compression / provider telemetry
```

关键原则：

- `ContextProvider` 只提供候选上下文，不直接决定最终 prompt 全貌。
- `SmartContextPlanner` 是 request-local；它可以丢弃/压缩候选，但不能修改 durable history。
- `ContextEngine` 继续负责会话级压缩和统计；provider 架构先作为其前置 assembly 层。

## 核心类型草案

```rust
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait ContextProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn kind(&self) -> ContextProviderKind;
    fn priority(&self) -> ContextPriority;

    /// Cheap availability check. Must not load large files or query large DB ranges.
    fn is_available(&self, request: &ContextRequest) -> bool;

    /// Optional cheap estimate for planning before expensive load.
    fn estimate(&self, request: &ContextRequest) -> ContextEstimate;

    /// Load provider candidates for this single request.
    async fn load(&self, request: &ContextRequest) -> hakimi_common::Result<Vec<ContextCandidate>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextProviderKind {
    Memory,
    Session,
    Skill,
    Project,
    Environment,
    Persona,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextPriority {
    Critical,
    High,
    Normal,
    Low,
}

#[derive(Debug, Clone)]
pub struct ContextRequest {
    pub session_id: Option<String>,
    pub user_prompt: String,
    pub cwd: std::path::PathBuf,
    pub active_skills: Vec<String>,
    pub model_context_length: usize,
    pub remaining_budget_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct ContextCandidate {
    pub provider_id: &'static str,
    pub title: String,
    pub content: String,
    pub estimated_tokens: usize,
    pub priority: ContextPriority,
    pub freshness: ContextFreshness,
    pub source: ContextSourceRef,
}
```

## Provider 初始清单

| Provider | 来源 | 加载策略 | 预算策略 |
|---|---|---|---|
| `MemoryContextProvider` | `MemoryProvider` / `FileMemoryProvider` | 先使用 cache/prefetch；按 query 检索相关条目 | High；超预算时保留摘要或最相关片段 |
| `SessionContextProvider` | `hakimi-session` SQLite | 只加载当前 session tail / 搜索命中 | Critical/High；durable history tail 独立于 request-local候选 |
| `SkillContextProvider` | `hakimi-skills::SkillLoader` / active skills | 仅渲染本轮激活技能 | High；技能说明可按段截断 |
| `ProjectContextProvider` | cwd、context files、workspace metadata | 只读取显式 context files / lightweight metadata | Normal；大文件必须走 planner 和 sanitizer |
| `EnvironmentContextProvider` | `prompt_builder::build_environment_hints` | Cheap eager load | Low/Normal；优先保留安全/运行环境提示 |
| `PersonaContextProvider` | persona/session config | Cheap eager load | High；影响回复风格和权限边界 |

## Planner 行为

`SmartContextPlanner` 应在 `build_send_messages` 之前运行，输出确定性的 `ContextAssembly`：

1. 收集 provider availability 与 estimates。
2. 按 `ContextPriority`、相关性、freshness 排序。
3. 调用 provider `load()` 获取候选。
4. 对候选应用 `ToolSanitizer` / secret redaction / path deny policy。
5. 在 request-local budget 内选择候选；超预算时按 provider 提供的降级策略压缩或跳过。
6. 记录 telemetry：provider id、耗时、token 估计、included/skipped/degraded 原因。

```rust
pub struct ContextAssembly {
    pub system_blocks: Vec<ContextCandidate>,
    pub history_tail: Vec<hakimi_common::Message>,
    pub telemetry: ContextTelemetry,
}
```

## 与现有 SmartContextEngine 的关系

短期：

- 保留 `SmartContextEngine` 的三层压缩实现。
- 在进入 `ContextEngine::compress()` 之前增加 provider assembly 层。
- `MemoryProvider` 先通过 adapter 形式实现 `ContextProvider`，避免一次性改动所有调用点。

中期：

- `SmartContextEngine` 增加 provider telemetry 汇总。
- `ContextPlanner` 与 provider assembly 合流，形成单一预算入口。
- Gateway/TUI 可显示“本轮加载了哪些上下文”和“哪些因预算跳过”。

长期：

- `SmartContextEngine` 从“压缩器”演进为“上下文编排器”：provider selection + request assembly + durable history compression。
- 如果后续出现 `hakimi-runtime` crate，可把 provider orchestration 放到 runtime 层，`hakimi-context` 保留类型和算法。

## 测试要求

后续实现应按小步补测试：

1. provider estimate 不触发大文件读取。
2. memory/session/skill/project candidates 按优先级和预算稳定排序。
3. durable history tail 不被 request-local provider planner 修改。
4. 超预算时先降级 Low/Normal provider，Critical provider 保留或摘要化。
5. Unicode token/字符估算不得按 byte 截断用户可见内容。
6. telemetry 覆盖 included/skipped/degraded 三类结果。

## 渐进迁移计划

1. 新增 `context_provider` 类型模块与纯单元测试，不接入主路径。
2. 为 `FileMemoryProvider` 添加 adapter，实现 `ContextProvider`。
3. 把 `build_skills_prompt` / `build_context_files_prompt` 包成 provider adapter。
4. 在 CLI/Gateway request 构建路径中引入 feature-gated 或内部开关的 assembly 层。
5. 与现有 `ContextPlanner` 合并预算逻辑，移除重复 prompt 拼接。
6. 将 telemetry 暴露给 `hakimi doctor` 或 debug logs，方便用户理解“为什么上下文没有被加载”。

## 风险与约束

- 不要在 provider `is_available()` / `estimate()` 中做昂贵 IO。
- 不要让 provider 直接修改 session DB 或 memory files；写入仍由原持久化路径负责。
- 不要把工具输出噪声重新塞回 assistant prose；provider 输出进入 prompt 前必须可审计、可截断。
- 不要一次性迁移 Gateway/TUI/Core；每轮 heartbeat 只做一个可验证子项。

## 下一步建议

P3.2 后可以进入 P4.1 runtime/renderer 边界设计；如果继续实现 Context，则优先新增 `ContextProvider` 类型草案和纯单元测试，不改主请求路径。
