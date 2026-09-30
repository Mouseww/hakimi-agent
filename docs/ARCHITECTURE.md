# Hakimi Agent 架构设计

> 本文面向新贡献者与维护者，目标是在 30 分钟内帮助读者理解 Hakimi Agent 的整体架构、核心数据流、关键抽象和各 crate 职责。

## 1. 系统概览

Hakimi Agent 是一个 Rust 实现的多入口 AI Agent 系统。它围绕同一个核心 Agent 运行时，提供 CLI、TUI、HTTP Server、WebUI、多平台 Gateway、定时任务、批处理、工具调用、记忆管理、技能系统和插件生态。

核心设计目标：

1. **窄核心，宽边缘**：核心负责会话、上下文、工具调度和模型交互；平台接入、UI、插件和技能尽量放在边缘模块。
2. **可观测与可恢复**：关键路径使用 tracing 与 metrics，错误带上下文，定时任务和批处理支持进度与重试。
3. **持久会话与可搜索记忆**：会话消息持久化，支持 discovery、scroll、browse、lineage 等查询模式；记忆分层存储并受容量约束。
4. **可扩展生态**：技能提供文本/提示层扩展，插件提供运行时钩子和动态加载，MCP/工具系统提供能力集成。

## 2. 工作区模块架构

Hakimi 使用 Cargo workspace 管理多个 crate。入口层 crate 调用核心能力层，核心能力层再依赖基础设施层。

```mermaid
graph TD
    subgraph Entry[入口层]
        CLI[hakimi-cli]
        BIN[hakimi]
        TUI[hakimi-tui]
        SERVER[hakimi-server]
        WEBUI[hakimi-webui]
        GATEWAY[hakimi-gateway]
    end

    subgraph Core[核心能力层]
        CORE[hakimi-core]
        TOOLS[hakimi-tools]
        SESSION[hakimi-session]
        CONTEXT[hakimi-context]
        KNOWLEDGE[hakimi-knowledge]
        CRON[hakimi-cron]
        BATCH[hakimi-batch]
        SKILLS[hakimi-skills]
        MCP[hakimi-mcp]
        PLUGIN[hakimi-plugin]
    end

    subgraph Infra[基础设施层]
        COMMON[hakimi-common]
        CONFIG[hakimi-config]
        TRANSPORTS[hakimi-transports]
        METRICS[hakimi-metrics]
        I18N[hakimi-i18n]
    end

    CLI --> CORE
    BIN --> CLI
    TUI --> CORE
    SERVER --> CORE
    SERVER --> GATEWAY
    WEBUI --> SERVER
    GATEWAY --> CORE

    CORE --> SESSION
    CORE --> CONTEXT
    CORE --> TOOLS
    CORE --> SKILLS
    CORE --> TRANSPORTS
    CORE --> CONFIG
    CORE --> COMMON

    TOOLS --> SESSION
    TOOLS --> CRON
    TOOLS --> METRICS
    MCP --> TOOLS
    KNOWLEDGE --> COMMON
    SESSION --> COMMON
    SESSION --> METRICS
    CONTEXT --> CONFIG
    TRANSPORTS --> CONFIG
    PLUGIN --> COMMON
    CRON --> COMMON
    BATCH --> COMMON

    CONFIG --> COMMON
    METRICS --> COMMON
```

### 2.1 分层说明

- **入口层**：面向用户或外部系统，负责解析输入、呈现输出、启动服务，不应承载复杂业务逻辑。
- **核心能力层**：实现 Agent 的会话、上下文、工具、知识、插件、定时任务等核心行为。
- **基础设施层**：配置、错误、传输、指标、国际化等可复用能力。

## 3. 请求处理数据流

一次典型用户请求从入口进入，经过配置加载、会话恢复、上下文构建、工具调用和模型交互，最终写回会话并返回响应。

```mermaid
sequenceDiagram
    participant U as User/Platform
    participant E as CLI/Gateway/Server
    participant C as hakimi-core
    participant S as hakimi-session
    participant X as hakimi-context
    participant T as hakimi-tools
    participant M as LLM Transport

    U->>E: 输入消息
    E->>C: run turn(request)
    C->>S: 加载或创建 Session
    S-->>C: 历史消息与元数据
    C->>X: 构建上下文
    X-->>C: system prompt + memory + recent messages
    C->>M: 发送模型请求
    M-->>C: assistant/tool call/stream chunk
    alt 需要工具调用
        C->>T: 执行工具
        T-->>C: 工具结果
        C->>M: 继续模型请求
        M-->>C: 最终回答
    end
    C->>S: 追加用户消息与助手消息
    C-->>E: 响应
    E-->>U: 展示结果
```

关键约束：

- 会话历史是事实来源，工具结果和模型输出都应可追踪。
- 上下文构建必须尊重上下文窗口与记忆容量限制。
- 工具调用应通过统一 registry 调度，避免入口层直接实现业务能力。

## 4. 会话与搜索架构

`hakimi-session` 负责消息存储、查询和 lineage 关系。它为工具和核心提供统一接口。

```mermaid
graph LR
    A[Session ID] --> B[Message Store]
    B --> C[Messages]
    B --> D[Metadata]
    B --> E[Lineage]

    F[Discovery Search] --> B
    G[Scroll Mode] --> B
    H[Browse Mode] --> B
    I[Lineage Query] --> E

    C --> J[Context Builder]
    C --> K[session_search Tool]
```

### 4.1 Session

Session 表示一次持续对话，包含：

- `session_id`：唯一标识。
- messages：用户、助手、工具等消息序列。
- metadata：创建时间、更新时间、标签等。
- lineage：会话之间或消息之间的派生关系。

常见操作：

- 创建或加载会话。
- 追加消息。
- 搜索消息。
- 查询上下文片段。
- 追踪 lineage。

## 5. 记忆与上下文架构

Hakimi 的记忆系统以文件和索引为核心，分为用户设定、长期记忆和工作记忆。

```mermaid
graph TD
    UP[user_prompt.md] --> CB[Context Builder]
    LM[memory.md 长期记忆] --> CB
    WM[working_memory.md 工作记忆] --> CB
    ARCH[archive/ 归档] --> SEARCH[Knowledge/Search]
    SEARCH --> CB
    CB --> PROMPT[最终模型上下文]

    WM -->|会话结束/归档| LM
    LM -->|超过阈值| ARCH
```

### 5.1 Memory

记忆分层：

- **user_prompt.md**：用户身份、偏好、长期系统约束。
- **memory.md**：长期记忆，跨会话保留。
- **working_memory.md**：当前或近期任务的短期工作记忆。
- **archive/**：历史记忆归档，避免主记忆无限增长。

容量策略：

- 单个记忆文件接近 60KB 时记录 WARN。
- 超过 64KB 时拒绝加载或提示清理。
- 会话结束或显式命令可触发归档与清理。

### 5.2 Context

Context 是发送给模型的输入集合，通常包括：

1. 系统提示与运行规则。
2. 用户提示和长期偏好。
3. 工作记忆和相关知识。
4. 当前会话历史片段。
5. 工具定义和工具调用结果。

`hakimi-context` 负责控制上下文窗口、压缩策略和注入顺序。设计原则是：重要信息优先，历史信息可检索，避免无界增长。

## 6. 工具、技能、插件的边界

Hakimi 同时提供工具、技能和插件三种扩展方式，它们解决的问题不同。

| 扩展方式 | 运行位置 | 适合场景 | 示例 |
|---|---|---|---|
| Tool | 核心工具调用层 | 模型需要结构化调用的能力 | session_search、memory、cron |
| Skill | 提示/文档层 | 指导 Agent 如何完成某类任务 | Git 工作流、部署流程 |
| Plugin | 运行时钩子/动态库 | 修改运行时行为或监听事件 | session logger、analytics |
| MCP | 外部能力桥接 | 连接外部工具服务器 | filesystem、browser、API 服务 |

### 6.1 Tool

`hakimi-tools` 提供内置工具 registry，并封装工具参数、执行、错误处理和 metrics。工具可以依赖会话、定时任务、指标等模块。

### 6.2 Skill

`hakimi-skills` 面向提示与操作知识，不应把所有能力都变成核心工具。技能适合低频、文本驱动、可由现有终端/文件能力完成的流程。

### 6.3 Plugin

`hakimi-plugin` 定义插件 API、动态加载器、插件管理器和市场原型。

核心抽象：

```rust
pub trait HakimiPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn on_session_start(&self, ctx: &SessionContext) -> Result<()>;
    fn on_message(&self, msg: &Message) -> Result<Option<Message>>;
    fn on_session_end(&self, ctx: &SessionContext) -> Result<()>;
}
```

插件生命周期：

```mermaid
graph LR
    A[plugins.yaml/installed.yaml] --> B[PluginManager]
    B --> C[PluginLoader]
    C --> D[动态库 .so/.dylib/.dll]
    D --> E[注册 Hook]
    E --> F[Session Start]
    E --> G[Message]
    E --> H[Session End]
```

插件市场负责：

- 从 YAML registry 读取插件元数据。
- 根据平台选择二进制文件。
- 从 GitHub Releases 下载。
- SHA256 校验。
- 写入本地 installed manifest。

## 7. Crate 职责速查表

| Crate | 职责 | 典型依赖/交互 |
|---|---|---|
| `hakimi` | 顶层二进制包装 | 调用 CLI |
| `hakimi-cli` | 命令行入口、子命令解析 | core、config、tools、gateway |
| `hakimi-core` | Agent 核心编排，模型请求、工具循环、会话协调 | session、context、tools、transports、skills |
| `hakimi-server` | HTTP/API 服务端，WebUI 后端 | core、session、gateway、metrics |
| `hakimi-webui` | Web UI 前端/静态资源相关 | server |
| `hakimi-tui` | 终端 UI | core、tools、context、gateway |
| `hakimi-gateway` | 多平台消息网关 | core、transports、config |
| `hakimi-transports` | 模型/平台传输抽象 | config、common |
| `hakimi-config` | 配置加载、默认值、路径解析 | common |
| `hakimi-common` | 通用错误、类型、工具函数 | 无核心业务依赖 |
| `hakimi-session` | 会话存储、消息查询、lineage | common、metrics |
| `hakimi-context` | 上下文构建、记忆注入、压缩策略 | config |
| `hakimi-tools` | 内置工具 registry 与执行 | session、cron、metrics |
| `hakimi-cron` | 定时任务、重试、调度 | common |
| `hakimi-batch` | 批处理与进度追踪 | common |
| `hakimi-knowledge` | 知识库、搜索、版本化 | common |
| `hakimi-skills` | 技能加载与提示层扩展 | 独立为主 |
| `hakimi-mcp` | MCP 集成 | tools、transports |
| `hakimi-plugin` | 插件 API、加载、市场 | common |
| `hakimi-metrics` | Prometheus/OpenTelemetry 指标 | 独立为主 |
| `hakimi-i18n` | 国际化资源 | 独立为主 |

## 8. 配置与运行时目录

默认用户目录为 `~/.hakimi/`。典型结构：

```text
~/.hakimi/
├── config.yaml              # 全局配置
├── sessions.db              # 会话数据库或索引
├── memory/
│   ├── user_prompt.md       # 用户身份/偏好
│   ├── memory.md            # 长期记忆
│   ├── working_memory.md    # 工作记忆
│   └── archive/             # 归档记忆
├── plugins/
│   ├── installed.yaml       # 已安装插件清单
│   └── *.so|*.dylib|*.dll   # 插件动态库
├── knowledge/               # 知识库数据
├── cron/                    # 定时任务状态
└── logs/
    └── hakimi.log           # 日志
```

配置示例：

```yaml
session:
  default_model: "gpt-4"
  context_window: 8000

memory:
  max_size: 65536
  auto_archive: true

plugins:
  enabled:
    - logger

observability:
  metrics: true
  tracing: true
```

约定：

- 用户可见的行为配置放入 `config.yaml`。
- API key、token、密码等敏感信息可使用环境变量或 secret 管理。
- 模块不得各自发明不兼容的配置路径。

## 9. 可观测性

Hakimi 的稳定性建设包括三层：

1. **tracing spans**：关键路径记录 session_id、查询模式、耗时、结果数量。
2. **metrics**：Prometheus/OpenTelemetry 指标，如搜索耗时、记忆加载大小、压缩比例。
3. **结构化错误**：错误包含上下文信息，例如 session_id、user_id、timestamp。

典型指标：

```text
session_search_duration_seconds{mode="discovery"}
memory_load_bytes{target="working"}
context_compression_ratio
```

## 10. 开发时如何定位代码

常见需求与入口：

| 需求 | 首先查看 |
|---|---|
| 修改一次对话的执行流程 | `crates/hakimi-core` |
| 添加 CLI 子命令 | `crates/hakimi-cli` |
| 修改会话搜索 | `crates/hakimi-session` 与 `crates/hakimi-tools` |
| 修改记忆注入 | `crates/hakimi-context` |
| 添加内置工具 | `crates/hakimi-tools` |
| 接入新平台 | `crates/hakimi-gateway` 或 `hakimi-transports` |
| 添加插件能力 | `crates/hakimi-plugin` |
| 添加指标 | `crates/hakimi-metrics` 与调用模块 |
| 修改 WebUI/API | `crates/hakimi-server` 与 `crates/hakimi-webui` |

## 11. 贡献设计原则

- 优先扩展已有抽象，避免新增平行体系。
- 工具是高成本接口：只有模型确实需要结构化调用时才加入核心工具。
- 技能适合文档化流程，插件适合运行时扩展，MCP 适合外部服务桥接。
- 测试应验证行为契约，而不是冻结容易变化的列表或版本号。
- 涉及配置、路径、数据库、网络、权限边界的修改，应写集成测试。

## 12. 快速阅读路径

如果你只有 30 分钟：

1. 先看第 2 节模块架构图。
2. 再看第 3 节请求处理数据流。
3. 阅读第 7 节 crate 职责表。
4. 根据你的任务跳转到第 10 节对应模块。
5. 最后阅读第 11 节贡献设计原则，避免走错扩展方向。

---

本文档会随着架构演进持续更新。若新增 crate、核心数据流或扩展点，请同步更新本文档与 `EVOLUTION_ROADMAP.md`。

## 13. 模块硬边界（加固约定）

本节把上面各节隐含的依赖方向写成**硬规则**。违反即视为架构回归，review 应直接打回。

### 13.1 依赖只能向内流

```
entry (cli / tui / server / desktop)  ->  core  ->  context / tools / session / knowledge
                                               |
                                     common / config / metrics / transports / i18n
```

- **基础设施层不得反向依赖核心层**：`common`、`config`、`metrics`、`transports`、`i18n` 不得出现 `use hakimi_core::` 或 `use hakimi_tools::`。
- **核心能力层不得依赖入口层**：`core`、`context`、`tools`、`session`、`knowledge` 不得出现 `use hakimi_cli::` 或 `use hakimi_server::`。
- 新增跨层调用前先问「这个方向是否已经存在」；不存在就走事件 / trait 回调，而不是直接 `use`。

### 13.2 单点装配

- 系统提示词只有一个装配点：`hakimi-context::prompt_assembler`。任何入口自己拼 `base_prompt + xxx` 都属于回归。
- 工具注册只有一个入口：`hakimi-cli::entry::build_agent`；`hakimi mcp serve` 复用同一份工具集定义（见 `mcp_served_tools`）。
- 会话持久化只有一条路径：`hakimi-session::SessionDB`。不要为某个入口单开 JSON 落盘。

### 13.3 网关注册白名单

- `GatewaysConfig::enabled_platforms` 是**唯一的网关开关**，默认 `["telegram", "weixin"]`。
- 每个 adapter 注册点都必须先过 `config.gateways.platform_enabled("<name>")`，再判断该平台自己的 `enabled`。
- 新增平台时：先在 `enabled_platforms` 里显式加入，再写 adapter。**adapter 存在不等于平台可用。**

### 13.4 危险工具必须过审批

- `hakimi-core::approval` 是唯一的人类审批入口，与 `hakimi-core::guardrails`（自动护栏）职责分离。
- 新增会改动宿主机或执行代码的工具时，必须同时加进 `ToolApprovalPolicy::default_dangerous_tools()` 并补测试。
- 审批失败一律 **fail closed**：超时、通道断开、无可用交互面，全部判为拒绝。
- 审批请求通过 `\u{001e}hakimi_approval:<id>:<prompt>` 事件上报，由交互面（CLI / 网关 / HTTP）渲染并回填。

### 13.5 MCP 双向

- `hakimi-mcp::McpClient` 是出站（Hakimi 调别人的 server）；`hakimi-mcp::McpServer` 是入站（别人把 Hakimi 当工具）。
- 入站模式走 `hakimi mcp serve`（stdio，换行分隔 JSON-RPC）。
- **stdout 是协议通道**：任何日志必须写 stderr。`tracing_subscriber` 已固定 `.with_writer(std::io::stderr)`，不要改回去。

### 13.6 已废弃组件

- **旧 WebUI 设计已废弃**：不再迭代 `crates/hakimi-webui/static` 下的界面。该目录暂时保留，因为 `hakimi-desktop` 通过 `include_str!` 内嵌了这些文件；新界面形态确定前不重启这条线。
- `hakimi serve` 已移除，只会返回明确的错误提示。HTTP 能力统一走 `hakimi-server`。
- 真正端到端可用的网关只有 **Telegram** 与 **微信**（见 13.3）。其余 adapter 保留代码但默认不注册。
