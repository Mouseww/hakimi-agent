# Hakimi Agent UI Event Protocol 设计草案

> Task P2.1：先以设计文档锁定内部事件协议边界，后续再按小步迁移 Gateway/TUI 渲染。

## 背景

当前 Gateway streaming 仍以字符串回调为主：assistant 正文、工具进度、媒体/子 Agent 边界通过约定前缀或局部状态机混合传递。P0/P1 已通过回归测试和 `GatewayStreamUiState` ownership 注释稳定了首句重复问题，但根因层面的方向应该是：Core Runtime 输出结构化事件，Gateway/TUI/未来 WebUI 只订阅事件并渲染。

## 目标

1. 把 assistant 正文、工具、媒体、delegate、边界、最终答案、错误拆成显式事件。
2. 让 renderer 不再从正文字符串里解析工具噪声。
3. 为 Gateway streaming、TUI timeline、未来 runtime/renderer 解耦提供稳定协议。
4. 先兼容现有 `hakimi_transports::StreamEvent` 和 Gateway 的 `GatewayStreamUiEvent`，不在本任务中做大迁移。

## 非目标

- 本任务不替换现有 Gateway streaming 状态机。
- 本任务不恢复旧 WebUI。
- 本任务不改变 LLM provider wire format；provider SSE 仍先归一到现有 transport 层事件。
- 本任务不定义跨进程公开 API；这是内部 runtime → renderer 协议草案。

## 分层关系

```text
Provider SSE / HTTP
        │
        ▼
hakimi-transports::StreamEvent
        │  provider/tool-call delta accumulation
        ▼
hakimi-core / future hakimi-runtime
        │  normalize assistant/tool/media/delegate semantics
        ▼
AgentUiEvent
        │
        ├── Gateway renderer: Telegram / Clawbot / Teams / ...
        ├── TUI renderer
        └── Future HTTP/Web renderer
```

## 事件类型草案

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentUiEvent {
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCallStarted { meta: ToolCallMeta },
    ToolCallProgress { progress: ToolProgress },
    ToolCallFinished { meta: ToolResultMeta },
    MediaGenerated { media: MediaRef },
    DelegateProgress { event: DelegateEvent },
    MessageBoundary { reason: BoundaryReason },
    FinalAnswer { text: String },
    Error { message: String, recoverable: bool },
}
```

### Supporting structs

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolCallMeta {
    pub id: String,
    pub name: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolProgress {
    pub tool_call_id: String,
    pub name: String,
    pub line: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolResultMeta {
    pub tool_call_id: String,
    pub name: String,
    pub is_error: bool,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MediaRef {
    pub kind: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DelegateEvent {
    pub task_id: String,
    pub title: String,
    pub line: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryReason {
    Tool,
    Media,
    Delegate,
    OverflowChunk,
    Final,
    Error,
}
```

## 语义规则

### TextDelta

- 只表示 assistant 可见正文增量。
- 不得包含工具进度、内部 marker、媒体 placeholder。
- Renderer 可选择 edit 当前消息或缓冲后发送。

### ReasoningDelta

- 表示 provider reasoning/thinking 增量。
- 默认不进入最终 assistant prose。
- Gateway 可忽略，TUI 可折叠显示。

### ToolCallStarted / ToolCallProgress / ToolCallFinished

- 工具生命周期独立于 assistant prose。
- `ToolCallProgress` 用于类似 `⚙️ HH:MM terminal ...` 的独立进度通道。
- `ToolCallFinished` 只给 renderer 一个摘要，不强制暴露完整工具输出。
- 工具事件前后如果需要 assistant 新气泡，应显式发 `MessageBoundary { reason: Tool }`。

### MediaGenerated

- 表示图片、音频、文件等媒体已经产生。
- 媒体不通过 TextDelta 拼接。
- 媒体事件后如果继续正文，应显式发 `MessageBoundary { reason: Media }`。

### DelegateProgress

- 对应当前 Gateway 中的 `DelegateProgressEvent`，但从前缀字符串解析迁移为结构化事件。
- 子 Agent 进度应使用独立 bubble/timeline，不污染最终正文。

### MessageBoundary

- 表示 renderer 应结束当前 assistant prose segment。
- Gateway 中相当于调用 `finish_tool_boundary()` / segment reset 的结构化来源。
- Boundary 本身不携带正文，不得更新“已渲染正文 ownership marker”；ownership 仍由实际 NewMessage 渲染建立。

### FinalAnswer

- 表示本轮完整最终答案。
- Renderer 用它决定是否 edit 已有 preview、发送 fresh final，或因内容相同而跳过。
- 不应重复发送已经由 streaming preview 完整渲染且内容一致的消息。

### Error

- 表示用户可见错误。
- `recoverable=true` 可用于提示稍后重试或继续排队。

## 与现有代码的映射

| 现有来源 | AgentUiEvent 映射 |
|---|---|
| `hakimi_transports::StreamEvent::ContentDelta` | `TextDelta` |
| `StreamEvent::ReasoningDelta` | `ReasoningDelta` |
| `StreamEvent::ToolCallDelta` accumulator 首次出现 name/id | `ToolCallStarted` |
| `\u{001e}hakimi_tool:...` 字符串回调 | `ToolCallProgress` 或 `ToolCallFinished` |
| `GatewayStreamUiEvent::Media` | `MediaGenerated` + `MessageBoundary(Media)` |
| `GatewayStreamUiEvent::Delegate` | `DelegateProgress` + 必要时 `MessageBoundary(Delegate)` |
| `ConversationResult.final_text` | `FinalAnswer` |
| agent/gateway error response | `Error` |

## 渐进迁移计划

1. **类型落点审计**：优先考虑新建在 `hakimi-common`，因为 Gateway/TUI/Core 都可依赖；如果后续 runtime crate 出现，再评估是否迁移。
2. **适配器层**：新增从现有 string callback / `StreamEvent` 到 `AgentUiEvent` 的薄适配，不直接改 renderer 行为。
3. **Gateway 双轨测试**：复用 P0/P0.2 的 streaming 边界测试，确保事件化后首句不重复、final 不重发、UTF-8 char-safe。
4. **工具进度独立化**：P2.2 优先把 `hakimi_tool` 前缀迁移为 `ToolCallProgress`，最终正文只消费 `TextDelta`/`FinalAnswer`。
5. **TUI timeline**：TUI 使用同一事件协议渲染 timeline，不再重写工具/媒体解析规则。

## 测试要求

后续实现类型和适配器时至少覆盖：

- `TextDelta` → Gateway content segment 不重复。
- `MessageBoundary(Tool)` 后的下一段 `TextDelta` 新开消息。
- `ToolCallProgress` 不进入 final assistant prose。
- `MediaGenerated` / `DelegateProgress` 后续正文新开 segment。
- `FinalAnswer` 与 preview 相同则不重复发送。
- 中文/emoji chunk 仍按字符处理，不按 byte 截断。

## 风险与约束

- 不要一次性把 Gateway 大状态机迁完；先适配、再替换。
- 不要把 provider 的 `StreamEvent` 直接暴露给 UI；它仍是 transport 层事件，不包含 Hakimi runtime 语义。
- 不要用字符串 prefix 作为长期协议；prefix 只能作为兼容输入。
- 不要恢复旧 WebUI runtime。

## 下一步

P2.2 可以基于本设计先做最小闭环：为工具进度建立结构化事件/适配层，让 Gateway renderer 从独立通道显示工具进度，同时保持最终 assistant prose 纯净。
