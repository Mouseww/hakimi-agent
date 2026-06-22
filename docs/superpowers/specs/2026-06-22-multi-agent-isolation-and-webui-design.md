# Hakimi 多 Agent 空间隔离 + WebUI 重新设计 · 设计文档

- 日期: 2026-06-22
- 状态: 设计已确认,待编写实现计划
- 作者: brainstorm 协作产出

## 1. 背景与目标

当前 Hakimi 是**单例共享 Agent** 架构:整个进程只有一个 `AIAgent`(`Arc<Mutex<AIAgent>>`),所有 channel / chat / WebUI 请求共享它。不同对话之间靠 per-chat 的 `histories` HashMap 和 WebUI 的 per-request clone + SessionDB 做消息隔离,但 `skill_store`、`context_engine`、`system_prompt`、`model`、`memory` 在实例层面是共享的。`crates/hakimi-cli/src/entry.rs:6861` 的注释已自述:"In a production multi-user scenario, you'd want per-chat agents"。

**目标**:在**一个实例内并发托管 N 个 Agent(人格 / Persona)**,每个人格的 `memory / skills / system prompt / model / channel 绑定` 相互独立隔离;并**重新设计 WebUI**,使其能动态管理和配置这些人格。

**关键洞察**:现有 Profile 系统(`RuntimeHome`)已经实现了 `config/memory/sessions/skills/cron/trajectories` 的**数据级隔离**,但它是**进程级、一次激活一个**。本设计不是再造隔离,而是:
1. 把"重资源"抽成全实例共享的 `SharedRuntime`;
2. 把"轻人格状态"做成可并发的 `PersonaRegistry`;
3. 加一层 `platform:bot_id → 人格` 的绑定路由。

Profile 与 Persona 是**嵌套关系**:Profile 仍是进程级"实例"(一次激活一个),Persona 是实例内并发的 N 个 Agent。

## 2. 核心设计决策(确认清单)

| 维度 | 决策 |
|---|---|
| 隔离模型 | 共享地基 + 轻量人格 |
| 共享(全实例一份) | provider 连接 + 凭证池 + 工具注册表(63+) + 知识图谱 + embedding |
| 隔离(每人格一份) | model + memory + skills + system prompt + context engine + sessions + cron + channel 绑定 |
| 模型粒度 | 共享 provider/凭证,`model` 字符串按人格可选 |
| 路由 | `platform:bot_id` → 人格;未匹配落**默认人格**;WebUI 显式选人格(不走绑定) |
| 生命周期 | WebUI 全动态 CRUD,改动热生效,持久化到磁盘 |
| WebUI 导航 | Layout A:左侧人格栏(工作区式)+ 底部"实例设置"入口 |
| 落地方式 | 方式 1:显式抽取 `SharedRuntime`,类型强制隔离边界 |

## 3. Part A:多 Agent 隔离运行时

### 3.1 组件模型

```
        ┌──────────────── SharedRuntime (Arc, 全实例一份) ────────────────┐
        │  transport(provider+凭证池)   tool_registry(63+)                 │
        │  knowledge_searcher           embedding_provider                 │
        └───────────────────────────────┬─────────────────────────────────┘
                                         │ Arc 引用(每人格持有)
   ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
   │  Persona A  │  │  Persona B  │  │  Persona …  │   ← PersonaRegistry
   │ model/prompt│  │ model/prompt│  │             │     HashMap<id, PersonaRuntime>
   │ skills(独立) │  │ skills(独立) │  │             │   + default_id
   │ context(独立)│  │ context(独立)│  │             │   + binding_index: "p:bot"→id
   │ memory(独立) │  │ memory(独立) │  │             │
   │ bindings[]  │  │ bindings[]  │  │             │
   └─────────────┘  └─────────────┘  └─────────────┘
```

### 3.2 数据结构草图

> 仅为方向性草图,字段以实现时为准。位置:`crates/hakimi-core/src/`。

```rust
// 全实例共享,启动时构造一次
pub struct SharedRuntime {
    pub transport: Arc<dyn ProviderTransport>,        // 含凭证池
    pub tool_registry: ToolRegistry,
    pub knowledge_searcher: Arc<dyn KnowledgeSearcher>,
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
}

// 持久化到 persona.yaml(serde)
pub struct PersonaConfig {
    pub id: String,                  // 稳定 slug,如 "coding-assistant"
    pub name: String,
    pub avatar: String,              // emoji
    pub description: String,
    pub model: String,               // 共享 provider,独立 model 字符串
    pub reasoning_effort: Option<String>,
    pub system_prompt: String,
    pub enabled_skills: Vec<String>, // 该人格启用的技能名
    pub bindings: Vec<String>,       // "platform:bot_id" 列表,可空
    pub is_default: bool,
}

// 运行时活实例
pub struct PersonaRuntime {
    pub config: PersonaConfig,
    pub agent: AIAgent,                              // 见下,持有 shared + 隔离态
    pub histories: HashMap<String, Vec<Message>>,   // 人格内 per-chat 隔离(沿用现机制)
    pub home: PersonaHome,                           // 该人格数据目录解析器
}

// AIAgent 改造(方式 1):共享态走 Arc<SharedRuntime>,其余每人格独立
pub struct AIAgent {
    pub shared: Arc<SharedRuntime>,                  // 新增:替代直接持有 transport/tools/...
    pub model: String,                               // 隔离
    pub system_prompt: Option<String>,               // 隔离
    pub skill_store: Option<SkillStore>,             // 隔离(不再 clone 共享)
    pub context_engine: Arc<RwLock<dyn ContextEngine>>, // 隔离(每人格一个)
    pub messages: Vec<Message>,                      // 隔离
    pub session_id: String,
    pub streaming_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    // tts / transcription / tool-search 配置照旧
}

pub struct PersonaRegistry {
    shared: Arc<SharedRuntime>,
    personas: HashMap<String, Arc<Mutex<PersonaRuntime>>>,
    default_id: String,
    binding_index: HashMap<String, String>,          // "platform:bot_id" -> persona_id
    home_root: PathBuf,                              // <runtime_home>/agents
}

impl PersonaRegistry {
    fn resolve_for_channel(&self, platform: &str, bot_id: &str) -> Arc<Mutex<PersonaRuntime>>; // 命中绑定否则 default
    fn get(&self, id: &str) -> Option<Arc<Mutex<PersonaRuntime>>>;
    fn create(&mut self, cfg: PersonaConfig) -> Result<()>;
    fn update(&mut self, id: &str, cfg: PersonaConfig) -> Result<()>; // 热生效
    fn delete(&mut self, id: &str) -> Result<()>;
    fn rebuild_binding_index(&mut self);
    fn persist(&self) -> Result<()>;                 // 写 registry.yaml + 各 persona.yaml
}
```

### 3.3 存储布局

Persona 数据存在**当前实例的 home** 下(默认 `~/.hakimi/`,或 `profiles/<name>/`):

```
<runtime_home>/
├── config.yaml              # [共享] provider / 凭证 / 工具 运行时配置
├── knowledge/               # [共享] 知识图谱
├── checkpoints/  *_cache/   # [共享]
└── agents/                  # 人格注册表根(新增)
    ├── registry.yaml        # 人格清单 + 默认人格 + (派生)绑定表
    ├── <persona-id>/
    │   ├── persona.yaml      #   name / model / system_prompt / skills[] / bindings[] / is_default
    │   ├── memory/           #   [隔离] memory.md, user.md
    │   ├── skills/           #   [隔离] 该人格自己的技能
    │   ├── sessions.db       #   [隔离] 对话历史
    │   └── cron.db           #   [隔离] 定时任务
    └── …
```

路径解析复用 `RuntimeHome` 风格,key 从 profile 换成 persona(可引入 `PersonaHome`)。

### 3.4 路由与消息流

- **入站(Gateway)**:消息带 `platform:bot_id:chat_id` → `registry.resolve_for_channel(platform, bot_id)` 查 `binding_index` → 未命中走默认人格 → 派发给该人格 agent(用其 prompt/skills/memory/model)→ 人格内部仍按完整 `task_key` 做 per-chat 历史隔离。
- **入站(WebUI)**:请求显式带 `persona_id` → `registry.get(persona_id)` 直接派发,**不查绑定**。
- **出站(send_message)**:目标解析逻辑不变(`crates/hakimi-tools/src/builtin_send_message.rs`),但记录发起人格,保证回复路由回正确 channel。

### 3.5 动态生命周期与热生效

WebUI CRUD → 改 `Arc<RwLock<PersonaRegistry>>` + 落盘 `persona.yaml`/`registry.yaml` → `rebuild_binding_index()`。

- **热生效(免重启)**:新建/编辑/删除人格、改 prompt/skills/model、把人格重新绑定到**已连接的** bot。
- **需重连(UI 提示)**:新增平台凭证 / bot token(Gateway 要为它连一个新 adapter)。

### 3.6 对外 API 契约(供 WebUI 消费)

新增 agent 维度 REST(增量式;保留现有端点指向默认人格做向后兼容)。位置:`crates/hakimi-server/src/api.rs`。

```
GET    /api/agents                列出人格(含状态/绑定/model)
POST   /api/agents                新建人格
GET    /api/agents/{id}           人格详情/配置
PATCH  /api/agents/{id}           更新(prompt/model/skills/bindings/is_default)
DELETE /api/agents/{id}           删除人格
POST   /api/agents/{id}/chat      与指定人格对话(替代单一 /api/chat)
POST   /api/agents/{id}/chat/stream  流式
GET    /api/agents/{id}/sessions  人格作用域会话
GET    /api/agents/{id}/memory    人格作用域记忆
GET    /api/agents/{id}/skills    人格作用域技能(可用 + 已启用)
POST   /api/agents/{id}/bindings  绑定/解绑 channel
GET    /api/bindings              全局绑定总览(platform:bot_id → 人格 + 默认人格)
```

### 3.7 落地方式与改造点(方式 1)

抽取 `SharedRuntime`,隔离边界由类型强制,从根上杜绝交叉污染类 bug(如 `/clear` 影响所有聊天)。主要改造点:

- `crates/hakimi-core/src/agent.rs`:`AIAgent` 结构改造,`agent.transport` 等访问改为 `agent.shared.transport`(编译器全程兜底,机械改动)。修正 `Clone`:`skill_store`/`context_engine` 不再共享 Arc,需给每人格独立实例。
- `crates/hakimi-core/src/`:新增 `shared_runtime.rs`、`persona.rs`、`persona_registry.rs`。
- `crates/hakimi-server/src/server.rs:23`:`AppState.agent: Arc<Mutex<AIAgent>>` → `AppState.registry: Arc<RwLock<PersonaRegistry>>`。
- `crates/hakimi-server/src/api.rs`:新增 `/api/agents/*`、`/api/bindings`;现有 handler 改为通过 registry 取默认人格。
- `crates/hakimi-cli/src/entry.rs:6861`:Gateway 消息循环用 `registry.resolve_for_channel()` 取代全局 `agent_arc`;per-chat `histories` 下沉到 `PersonaRuntime`。
- `crates/hakimi-config/src/config.rs`:`PersonaConfig` 序列化;`agents/` 目录解析。

### 3.8 向后兼容与迁移

- 首次启动若无 `agents/` 目录:自动创建 `default` 人格,其 `system_prompt`/`model` 取自现有 `config.yaml`,数据路径**回退到现有 root 级位置**(`<home>/memory`、`<home>/sessions.db`、`<home>/skills`、`<home>/cron.db`),**不做数据搬迁**,现有数据零损失。
- 命名人格(后续新建)落在 `agents/<id>/` 下。
- 现有 `/api/chat`、`/api/sessions`、`/v1/*` 端点在缺省 `persona_id` 时一律指向默认人格。

## 4. Part B:WebUI 重新设计

### 4.1 技术栈与现状

- 现状:`hakimi-webui/`,React 19 + Vite 8 + TS + Tailwind 4 + lucide-react;无路由(手动 tab),无状态库;5 个 tab(Runtime/Tools/Skills/Control/Gateway);左 session、中 chat、右面板;构建产物经 `include_str!` 嵌入二进制(`api.rs`)。
- 复用度:`api.ts` 约 80% 可复用(加 `persona_id`/路径段);UI 组件约 70%(加人格上下文)。

### 4.2 导航模型(Layout A:左侧人格栏 · 工作区式)

```
┌──┬──────┬─────────────────┬────────┐
│编│ 会话  │                 │ 面板    │
│写│ list │   对话区         │ tools  │
│客│      │                 │ skills │
│＋│      │                 │ binding│
│⚙ │      │                 │        │   ← 底部齿轮 = 实例设置
└──┴──────┴─────────────────┴────────┘
 ↑人格rail(选中=进入该人格上下文)
```

- 最左窄栏竖排人格,点谁进谁;进入后是熟悉的"会话 / 对话 / 右侧面板"。
- `＋` 新建人格;选中人格的齿轮进入其配置表单。
- 人格栏底部独立 `⚙` = 实例设置(见 4.4),与人格区分开。
- 无绑定的人格照常显示在 rail,WebUI 内完全可用。

### 4.3 人格配置表单

点 `＋` 新建或选中人格点齿轮编辑,打开同一张表单。分组:

1. **身份**:emoji / 名称 / 简介
2. **模型**:`model` 下拉 + 推理强度(provider/凭证池全局共享,此处不重复)
3. **系统提示词**:大 textarea(人格身份)
4. **技能**:可勾选 chips(每人格独立启用集)
5. **Channel 绑定**:多个 `platform:bot_id`,**可留空(=仅 WebUI 可用)**;"设为默认人格"开关
6. **协作(即将支持,占位)**:"允许被其他 Agent 寻址调用(`agent:<id>`)"灰态开关,呼应前向兼容

底部:保存(热生效)/ 取消 / 删除。

### 4.4 实例设置 / 绑定总览

人格栏底部齿轮进入,管两类:

- **共享运行时配置** sub-nav:概览 / Provider & 凭证池 / 工具 & MCP / 知识库 / Gateway 平台凭证 / WebUI & 主题。
- **路由绑定总览**:`platform:bot_id → 人格` 映射表,标注未绑定项(落默认人格)与默认人格(兜底)。
- **共享运行时状态**:Provider 连接、凭证池健康、工具数、知识条目数、人格数/在线数。

### 4.5 前端组件与 API 调整

- `api.ts`:新增 `agents` 资源(list/get/create/update/delete)、`bindings`、人格作用域的 chat/sessions/memory/skills;聊天与会话请求带 `persona_id`。
- 新增组件:`PersonaRail`(左侧栏)、`PersonaConfigForm`、`InstanceSettings`(含 `BindingsOverview`)。
- 复用并改造:现有 chat / sessions / 右面板组件接收 `personaId` 上下文。
- 现有 `SettingsPanel` / `GatewayPanel` 内容并入"实例设置"。
- 建议引入轻量客户端路由(`/agents/:id`、`/instance`),或保留手动切换 + URL hash。

## 5. 前向兼容钩子(本期只预留,不实现完整功能)

### 5.1 无 channel 的 WebUI 使用(本期即生效)

WebUI 走显式 `persona_id`,不经绑定。`bindings[]` 为空的人格在 WebUI 内是一等公民。规则:**WebUI 用人格 = 无需绑定;Gateway 用人格 = 需绑定或落默认人格。**

### 5.2 多 Agent 互相对话(`agent:<id>` 寻址)

让人格成为路由网络里的**可寻址端点**:新增 target scheme `agent:<persona_id>`,与 `telegram:...` 平级。将来 A 人格 `send_message(target="agent:writer")` 时,共享路由层把它当成一条入站消息投递给 writer 人格(用 writer 自己的 memory/context 处理),writer 再回给 A。可复用现有 `send_message` 工具、swarm graph、sub-agent delegation 基础设施。

本期只预留寻址能力(target 解析支持 `agent:` 前缀 + 人格配置里的占位开关),**完整的多 Agent 编排留作后续单独 spec**。

## 6. 范围之外(本期不做)

- 完整的多 Agent 编排 / 自动协作(只留 `agent:<id>` 寻址钩子)。
- 知识图谱按人格隔离(本期保持共享,见第 7 节)。
- 人格级权限/RBAC、多用户登录(WebUI 仍用单一 Bearer token)。
- WASM 插件运行时、PTY 终端等既有 roadmap 未决项。

## 7. 已确认假设

- `memory/` 按人格隔离(明确要求);**知识图谱保持共享**(明确选择)。二者在 Hakimi 是不同子系统,此为有意为之。
- `sessions.db` 与 `cron.db` 按人格隔离(每人格独立对话与定时任务)。
- 共享:provider 连接 + 凭证池 + 工具注册表 + embedding;隔离:model + memory + skills + prompt + context engine + sessions + cron + channel 绑定。

## 8. 实现分期建议

1. **P1 抽取 SharedRuntime**:`AIAgent` 改造持有 `shared: Arc<SharedRuntime>`,保持单人格行为,测试转绿。
2. **P2 PersonaRegistry + 存储**:`persona.yaml`/`registry.yaml` 读写 + 默认人格加载;`AppState` 改持 registry,现有端点指向默认人格。
3. **P3 Gateway 路由**:`binding_index` + 默认兜底;per-chat `histories` 下沉到 `PersonaRuntime`。
4. **P4 Agent 维度 API**:`/api/agents/*`、`/api/bindings`。
5. **P5 WebUI 重设计**:Layout A 人格栏 + 人格配置表单 + 实例设置/绑定总览。
6. **P6(后续单独 spec)**:`agent:<id>` 多 Agent 互聊编排。
```

