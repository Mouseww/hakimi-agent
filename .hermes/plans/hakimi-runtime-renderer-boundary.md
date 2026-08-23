# Hakimi Runtime / Renderer 边界设计

> Task P4.1 — 明确 Core Runtime 是唯一真相源，Gateway/TUI/Future WebUI/Studio 都只订阅事件并渲染。

## 目标

Hakimi 当前已经有多个入口（CLI、TUI、Gateway、Studio backend），但 streaming UI、工具进度、会话运行、取消与平台渲染仍存在入口层重复逻辑。P4 的目标不是一次性大重构，而是先固定边界：

1. **Runtime 拥有执行事实**：会话状态、run lifecycle、取消/排队、工具调用、LLM stream、持久化。
2. **Renderer 只做呈现**：把 runtime 事件转换为平台消息、终端 UI、Studio WS 事件或未来 WebUI DOM 更新。
3. **事件是唯一跨层契约**：入口层不直接推断 agent 内部状态，不把工具噪声写进 assistant prose。
4. **保持现有 Gateway/TUI 可用**：先文档化和加测试，再按 crate 分层迁移。

## 当前事实源审计

### 已有事实源

- `hakimi-core::AIAgent`：agent loop、LLM transport、tool dispatch、stream callback/event callback。
- `hakimi-session`：持久会话消息与搜索，是 durable history 的事实源。
- `hakimi-studio-api::StudioRuntime` + `EventBus`：Studio 已有 seq-numbered event bus、session state、queue/cancel 结构。
- `hakimi-cli::GatewayStreamUiState`：Gateway streaming 目前在 CLI entry 内维护 render-local preview state。
- `hakimi-tui::AgentEvent`：TUI 有本地事件 enum，但还不是全局 runtime 协议。

### 当前风险

- Gateway renderer 与 agent stream 绑定过深，容易在工具边界、媒体边界、delegate 边界产生重复/污染。
- TUI、Gateway、Studio 各自定义事件形状，未来功能需要多处重复实现。
- 取消、busy input、queue、session recovery 容易在入口层分叉。
- `docs/ARCHITECTURE.md` 仍保留历史 WebUI 运行态图示；运行态 WebUI 已移除，未来只允许重新以 renderer/client 形式接入。

## 目标分层

```text
hakimi-core
  - Agent loop / LLM transport / tool dispatch / memory/context hooks
  - 不知道 Telegram/TUI/Studio 渲染细节

hakimi-runtime (目标 crate，可渐进从 studio-api/runtime 抽出)
  - Session execution / run queue / cancellation / active-run ownership
  - Durable history write-through
  - Emits AgentUiEvent / RuntimeEvent stream
  - 是入口层运行状态的唯一事实源

hakimi-gateway
  - Telegram / Teams / Clawbot / other adapters
  - Subscribes runtime events, renders platform messages
  - Owns platform flood-control/edit limitations only

hakimi-cli
  - CLI one-shot, setup, doctor, gateway launcher, TUI launcher
  - 不持有长期 run state

hakimi-tui
  - Terminal renderer and input controller
  - Subscribes runtime events, renders panels/messages/tools

hakimi-server / hakimi-studio-api
  - Studio protocol WS/API and local/remote runtime attachment
  - Studio EventBus can wrap or bridge runtime events
```

## 事件边界

P2.1 已定义 `AgentUiEvent` 草案。P4 将其扩展为 runtime/renderer 双层：

```rust
pub enum RuntimeEvent {
    RunStarted { run_id: String, session_id: String },
    Ui(AgentUiEvent),
    Usage(UsageSnapshot),
    Persisted { message_id: String },
    RunFinished { run_id: String, outcome: RunOutcome },
    RunCancelled { run_id: String, reason: String },
    Error { run_id: Option<String>, message: String },
}
```

Renderer 只消费事件，不反向修改 runtime 内部状态。Renderer 可以维护 request-local preview state（例如 Telegram edit message id、TUI scroll offset），但不得成为 durable truth。

## Ownership 规则

| 状态/行为 | Owner | Renderer 是否可缓存 | 说明 |
|---|---|---:|---|
| session messages / lineage | `hakimi-session` via runtime | 只读缓存 | durable history 不能由 renderer 拼接 |
| active run id / cancellation | runtime | 否 | 入口发送 cancel command，runtime 判定 ownership |
| tool progress semantic state | runtime/core event stream | 可缓存显示 | 工具进度独立通道，不污染 assistant text |
| platform edit message id | renderer | 是 | 仅平台 transport state |
| flood-control backoff | renderer | 是 | 平台限制，不影响 runtime truth |
| queued user inputs | runtime | 可显示 | busy/queue/preempt 规则集中处理 |
| Studio seq/replay | Studio/EventBus bridge | 是 | seq 是 protocol 层事实，不替代 runtime run state |

## 渐进迁移计划

### Step 1 — 冻结 Gateway 边界测试（已完成大部分）

- 首句重复、tool/media/delegate boundary、final duplicate、UTF-8 chunk 等测试继续作为迁移保护。
- `GatewayStreamUiState` 保留为 renderer-local preview state。

### Step 2 — 引入内部 runtime event 类型

- 优先放在 `hakimi-common` 或新 `hakimi-runtime` 的轻量模块中，避免循环依赖。
- 不一次性替换所有 `StreamEvent`；先提供从 `hakimi_transports::StreamEvent` / tool progress marker 到 `AgentUiEvent` 的 adapter。

### Step 3 — Gateway 改为消费 `AgentUiEvent`

- Gateway adapter 只处理：Content/TextDelta、ToolProgress、MediaGenerated、DelegateProgress、MessageBoundary、FinalAnswer、Error。
- 删除入口层对工具 marker 字符串的散落判断，集中到 adapter。

### Step 4 — TUI/Studio 共享事件桥

- `hakimi-tui::AgentEvent` 从 `AgentUiEvent` 派生或直接替换。
- Studio `StudioEvent` 保持协议兼容，但由 runtime event bridge 生成。

### Step 5 — 抽出 `hakimi-runtime`

- 从 `hakimi-studio-api::runtime` 与 Gateway active task 管理中抽出通用 run controller。
- 入口只负责构建 config/agent/runtime，所有 run lifecycle 通过 runtime API。

## 非目标

- 本阶段不恢复旧 WebUI 运行态。
- 本阶段不要求全量重写 Gateway streaming。
- 本阶段不改变现有 systemd：`hakimi.service` 仍只跑 `/root/.hakimi/bin/hakimi --gateway start`。
- 本阶段不在本机做 release build；发布继续依赖 GitHub CI。

## 验证要求

每次迁移子项至少满足：

1. `cargo fmt --all`
2. 相关 targeted tests：
   - Gateway: `cargo test -p hakimi-cli gateway_ -- --nocapture`
   - TUI: `cargo test -p hakimi-tui <focused_test> -- --nocapture`
   - Studio runtime/event bus: `cargo test -p hakimi-studio-api event_bus -- --nocapture`
3. 涉及用户可见入口时同步 README。
4. CI 红时下一轮 heartbeat 优先修 CI。

## Blocker / 待决策

- `AgentUiEvent` 最终落点：`hakimi-common` 简单共享 vs 新建 `hakimi-runtime` crate。
- `hakimi-studio-api::StudioRuntime` 是否拆分为 protocol crate + runtime crate，避免 Studio 协议污染通用 runtime。
- Gateway busy input 的 queue/preempt 规则是否完全上移 runtime，还是保留平台策略输入后再交 runtime。

建议下一轮 P4 子项：先创建最小 `AgentUiEvent` Rust 类型与 adapter 单元测试，避免直接大规模移动 Gateway 代码。
