# 设计:人格团队协作(Persona Team Collaboration)

- 日期:2026-06-24
- 分支:`feat/persona-team-collaboration`
- 上游设计:`docs/superpowers/specs/2026-06-22-multi-agent-isolation-and-webui-design.md`(尤其 §5.2 `agent:<id>` 寻址钩子)
- 多人格交接:`docs/superpowers/handoffs/2026-06-22-multi-agent-isolation-handoff.md`

## 1. 目标与背景

多人格隔离体系(P1-P6)已落地:一个实例内 N 个具名人格,各自隔离
`model / system_prompt / skills / memory / sessions / context_engine`,共用一份
`SharedRuntime`(transport / tools / knowledge)。`PersonaRegistry` 负责
`platform:bot_id -> persona` 路由,`build_persona_agent()` 可廉价地起一个活体人格 agent。

但人格之间目前**无法协作**:它们是彼此隔离的端点。现有的两个"多 agent"机制都不满足需求:

- `delegate_task` / `CoreDelegateExecutor`:父 agent 派生**匿名、一次性**子 agent
  (干净上下文、过滤工具、并发/重试受限)。子 agent **无身份**、不能回话,且
  `delegate_task` / `send_message` / `memory` 等对子 agent 被屏蔽。属层级式一次性委派,
  **不是**具名人格之间的对等协作。
- `mixture_of_agents`:并行调用多个**外部 OpenRouter 模型**再聚合,与本地人格无关。

本设计实现**协调者委派(orchestrator/delegation)**模型:用户对话的**主导人格(lead)**
可按需把子任务交给**具名队友人格(teammate)**,每个队友用自己的模型/技能/记忆作答,
结果并回主导人格上下文,由主导人格汇总。一句话心智模型:

> "`delegate_task`,但派给一个**真实具名人格**而非匿名子 agent。"

这正是 spec §5.2 中"留作后续单独 spec"的多 Agent 协作能力的**第一层、可控基座**;
异步对等互发消息(`send_message(agent:<id>)`)与完整团队编排可在此基座上分层叠加。

## 2. 范围

### 2.1 本期(MVP)

1. `PersonaConfig.addressable: bool` 开关(谁能被当队友),**默认开启**。
2. `TeamExecutor` trait(`hakimi-common`)+ `ToolContext.team_executor` 注入位,
   镜像现有 `DelegateExecutor` 的接线方式。
3. `PersonaTeamExecutor`(`hakimi-core/src/team.rs`):构建队友人格 agent、跑一个
   受限回合、返回结构化结果;含单个与并行批量两种入口。
4. `team` 工具(`hakimi-tools/src/builtin_team.rs`):主导人格据此咨询/委派队友,
   并能 `action:"list"` 枚举队友名册。
5. 接线:gateway(`start_gateway` / `start_unified_server`)与 server(`AppState`)从共享
   `PersonaRegistry` 构建一个 `PersonaTeamExecutor` 并注入各人格 agent。
6. 可视化:复用现有 `hakimi_delegate:` 进度气泡机制(gateway 与 WebUI 均已支持)。
7. 安全护栏:深度上限、回环检测、并发信号量、单次咨询的迭代/超时预算。
8. WebUI:把 `PersonaConfigForm` 里预留的 `addressable` 占位开关做成可用(轻量)。

执行语义:**同步、无状态**。队友按子任务起一个干净回合,加载其人格 prompt/技能,
可**只读**其长期 memory 作参考,但本次咨询**不写回**它自己的会话/记忆。

### 2.2 不做(留作后续单独 spec)

- 异步对等互发消息 `send_message(agent:<id>)`(§5.2 字面实现)。
- 有状态团队会话(主导↔队友配对持久会话、写入 per-persona sessions.db)。
- 完整团队编排工作流(规划→分派→评审→汇总)与 WebUI"团队对话"视图。
- 队友把协作产出**写回**自己的长期 memory。
- 跨人格知识图谱隔离(保持共享,与上游 spec 一致)。

## 3. 架构

```
                 用户消息
                    │
                    ▼
          ┌───────────────────┐
          │  主导人格 (lead)   │  ← 用户绑定/选择的人格,自带 model/prompt/skills/memory
          │  AIAgent           │
          └─────────┬─────────┘
                    │ 调用 team 工具 (action=consult / list)
                    ▼
          ┌───────────────────┐      ToolContext.team_executor: Arc<dyn TeamExecutor>
          │  team 工具         │──────────────────────┐
          └───────────────────┘                       │
                                                       ▼
                                        ┌──────────────────────────────┐
                                        │   PersonaTeamExecutor         │
                                        │   (hakimi-core/src/team.rs)   │
                                        │  - 校验 teammate 可寻址        │
                                        │  - build_persona_agent(...)    │
                                        │  - 跑受限回合 (retry/timeout)  │
                                        │  - 深度/回环/并发护栏          │
                                        └───────────────┬──────────────┘
                                                        │ 克隆/构建
                          ┌─────────────────────────────┼─────────────────────────────┐
                          ▼                             ▼                             ▼
                ┌──────────────────┐         ┌──────────────────┐          ┌──────────────────┐
                │ 队友人格 coder   │         │ 队友人格 writer  │   ...    │ 队友人格 <id>    │
                │ 自己的模型/技能/ │         │ 自己的模型/技能/ │          │                  │
                │ memory(只读)    │         │ memory(只读)    │          │                  │
                └──────────────────┘         └──────────────────┘          └──────────────────┘
                          │                             │                             │
                          └────────── 结构化结果并回主导上下文 ◄─────────────────────┘
```

共享不变量(沿用上游隔离边界):`context_engine` / `skill_store` 每人格独立;
transport / tools / knowledge 经 `Arc<SharedRuntime>` 共享。队友 agent 由
`build_persona_agent()` 构建,天然继承这套隔离。

## 4. 组件与接口

### 4.1 `PersonaConfig.addressable`(`hakimi-core/src/persona.rs`)

```rust
/// 是否允许被其他人格作为队友寻址调用(team 工具)。默认开启。
#[serde(default = "default_true")]
pub addressable: bool,
```

- 新增 `fn default_true() -> bool { true }`(serde 默认函数)。
- `PersonaConfig::new()` 里 `addressable: true`。
- 旧的 `persona.yaml` 缺该字段时按默认 `true` 反序列化(向后兼容,开箱即用)。
- 主导人格能咨询的名册 = registry 中所有 `addressable == true` 且 **id != 自己** 的人格。

### 4.2 `TeamExecutor` trait(`hakimi-common`)

与现有 `DelegateExecutor` 并列定义,便于 `ToolContext` 在不依赖 `hakimi-core` 的前提下持有它:

```rust
#[async_trait]
pub trait TeamExecutor: Send + Sync {
    /// 列出可寻址队友:返回 (id, name, description) 三元组。
    async fn roster(&self) -> Vec<TeammateInfo>;

    /// 同步咨询单个队友;返回其最终结构化答复。
    async fn consult(&self, ctx: TeamCallContext) -> Result<String>;

    /// 并行咨询多个队友(fan-out);返回与输入等长的结果向量。
    async fn consult_many(&self, calls: Vec<TeamCallContext>) -> Result<Vec<String>>;
}

pub struct TeammateInfo { pub id: String, pub name: String, pub description: String }

pub struct TeamCallContext {
    pub teammate_id: String,
    pub task: String,
    pub context: String,
    /// 进度回调(复用 delegate 的气泡机制)。
    pub progress: Option<ToolProgressCallback>,
}
```

**深度与回环状态由执行器实例自身携带,不经工具/`ToolContext` 透传**:每个 agent 拿到的
`team_executor` 是一个"已定位"的实例(知道自己的 `depth` 与 `lineage`)。主导人格的实例位于
`depth=0, lineage=[lead_id]`;它的 `consult()` 在派生队友 agent 时,给队友注入一个 `depth+1`、
`lineage` 追加了该队友 id 的**子执行器实例**。如此工具层只需给出 teammate/task/context,
深度上限与回环判断在执行器内部完成,`ToolContext` 只需新增一个 `team_executor` 字段(见 4.3)。

### 4.3 `ToolContext.team_executor`(`hakimi-common`)

镜像 `delegate_executor`,新增:

```rust
pub team_executor: Option<Arc<dyn TeamExecutor>>,
```

`Default` 为 `None`(无队友环境下 `team` 工具优雅报"未启用团队")。

### 4.4 `PersonaTeamExecutor`(`hakimi-core/src/team.rs`)

实现 `TeamExecutor`。持有:

- registry 句柄 `Arc<RwLock<PersonaRegistry>>`(读 `addressable` 标志与人格 config,
  随 CRUD 即时生效);
- 构建队友 agent 所需资源:**优先复用** gateway 既有的预构建人格 base agent map
  (`GatewayPersonaAgents` = `Arc<RwLock<HashMap<String, AIAgent>>>`,克隆即得,廉价);
  map 中没有的(如未预构建的命名人格)走 `build_persona_agent(template, cfg, skills_dir, ctx_len)`
  按需构建 —— 与现有 `/api/agents/{id}/chat` 完全一致的取 agent 策略;
- 并发信号量(沿用 `MAX_CONCURRENT_DELEGATIONS` = 5)。

`consult` 流程:

1. 读 registry:teammate 必须存在且 `addressable`,否则返回明确错误(供模型改派)。
2. 深度护栏:`depth >= MAX_TEAM_DEPTH`(默认 2)直接拒绝并返回提示。
3. 回环护栏:`lineage.contains(teammate_id)` 拒绝(防 A→B→A)。
4. 取/建队友 agent(见上),设置队友的人格 system_prompt + 队友专属 skills,挂只读 memory。
   在队友 prompt 末尾追加一段**结构化返回**约定(复用 delegate 的
   `Status / Summary / Findings / ...` 风格,确保主导人格拿到可解析结果)。
5. 给队友 agent 注入一个 `depth+1`、`lineage` 追加了该队友 id 的**子执行器实例**,
   从而队友自己也能再咨询别人,直到深度上限。
6. 跑一个受限回合(沿用 delegate 的 `CHILD_MAX_ITERATIONS` / `DEFAULT_DELEGATION_TIMEOUT`
   / 最多 3 次重试),用信号量限并发。
7. 全程经 `progress` 回调发 `hakimi_delegate:` 气泡(标题用队友 `avatar + name + 截断task`)。
8. 返回队友 `final_response`。

`consult_many`:对多个 `TeamCallContext` 用 `tokio::spawn` + 信号量并行,`join_all` 收集,
失败项以 `"Teammate <id> failed: ..."` 占位(不让单个失败拖垮整批),与 delegate 批量一致。

### 4.5 `team` 工具(`hakimi-tools/src/builtin_team.rs`)

- `name() = "team"`,`toolset() = "collaboration"`,`emoji` 取协作类(如 🤝)。
- `description`:说明可把子任务委派给具名队友人格,并先用 `action:"list"` 看名册。
- schema:

```jsonc
{
  "type": "object",
  "properties": {
    "action":   { "type": "string", "enum": ["consult", "list"],
                  "description": "consult=咨询/委派队友;list=列出可用队友。默认 consult。" },
    "teammate": { "type": "string", "description": "目标队友人格 id(单个咨询)。" },
    "teammates":{ "type": "array", "items": {"type": "string"},
                  "description": "多个队友 id(并行 fan-out)。与 teammate 二选一。" },
    "task":     { "type": "string", "description": "交给队友的子任务/问题。" },
    "context":  { "type": "string", "description": "可选:共享上下文与约束。" }
  },
  "required": []
}
```

- `execute`:
  - `team_executor` 为 `None` → 返回"团队协作未在当前环境启用"。
  - `action="list"` → `roster()` 渲染为 `- <id> (<name>): <description>` 列表。
  - `action="consult"`:校验 `task` 非空、`teammate`/`teammates` 至少一个;
    从 `ToolContext` 读 `progress` 组装 `TeamCallContext`(depth/lineage 由执行器实例自带),
    调 `consult` 或 `consult_many`;单个返回队友答复,批量返回带 `## <id>` 分节的汇总。
- 名册也会在构建主导 agent 时**摘要注入其 system_prompt**(如"你的团队队友:coder(编码)、
  writer(文案)..."),让模型无需先 `list` 即知道能找谁;`list` 作为动态兜底(人格会变)。

### 4.6 接线

- `hakimi-common`:定义 `TeamExecutor` / `TeammateInfo` / `TeamCallContext`,`ToolContext` 加
  `team_executor` 字段。
- `hakimi-core`:实现 `PersonaTeamExecutor`;在构建人格 agent 时设置其 `ToolContext` 默认携带
  `team_executor`(类似现有 `delegate_executor` 的注入点)。
- `hakimi-tools`:注册 `TeamTool` 到工具注册表(随 `SharedRuntime.tool_registry` 对所有人格可见;
  无队友时优雅降级)。
- gateway `start_gateway` / `start_unified_server` 与 server `AppState`:各构建一个
  `PersonaTeamExecutor`(共享同一 `Arc<RwLock<PersonaRegistry>>` 与预构建 agent map),
  注入到派发出去的人格 agent。统一模式下 gateway 与 API 共享同一份,改人格即时生效。

## 5. 控制流(端到端)

1. 用户向主导人格发消息(gateway 或 WebUI)。
2. 主导人格在回合内判断需要队友,调 `team(action="consult", teammate="coder", task=..., context=...)`。
3. `TeamTool` 经 `ToolContext.team_executor` 调 `PersonaTeamExecutor::consult`(`depth=0`,
   `lineage=[lead_id]`)。
4. 执行器校验 → 取/建 coder agent → 跑受限回合(其 `team_executor` 注入 `depth=1`,
   `lineage=[lead_id, coder]`)→ 进度气泡实时上报。
5. coder 返回结构化答复 → 作为 `team` 工具结果并回主导上下文。
6. 主导人格据此继续(可再咨询其他队友,或汇总后回复用户)。
7. gateway 侧的出站投递、busy/queue、active_tasks 等均不受影响(team 是回合内的工具调用,
   不经 `MESSAGE_QUEUE` 出站队列)。

## 6. 安全与成本护栏

- **可寻址门控**:仅 `addressable` 人格可被咨询(默认开,可逐个关)。
- **深度上限** `MAX_TEAM_DEPTH = 2`:防止无限层级委派。
- **回环检测**:`lineage` 集合阻断 A→B→A。
- **并发信号量**:沿用 `MAX_CONCURRENT_DELEGATIONS = 5`,与 delegate 共享心智。
- **单次预算**:沿用 delegate 的迭代上限 / 超时 / 重试。
- **不自咨**:名册排除主导人格自身。
- **优雅降级**:无 `team_executor` 或无可用队友时,工具返回可读提示而非报错中断回合。

## 7. 可观察性

复用既有 `\u{001e}hakimi_delegate:{task_id}|{title}|{line}|{timestamp}` 进度协议:
队友的工具调用与阶段进度作为进度气泡实时流式显示(gateway 富文本气泡 + WebUI 均已支持);
`title` 用队友 `avatar + name + 截断task`,使多个队友的并行进度可区分。队友最终答复并入
主导回复正文。不引入新的展示通道(团队对话视图留作后续)。

## 8. WebUI(轻量)

- `PersonaConfigForm`:把预留的 `addressable` 占位开关接到 `PATCH /api/agents/{id}`
  的真实字段("允许被其他 Agent 作为队友调用",默认开)。
- `/api/agents` 的返回体补充 `addressable` 字段。
- 不新增团队视图;协作过程经进度气泡可见。

## 9. 测试计划

- **hakimi-tools(`TeamTool`)**:schema 校验;`action=list` 渲染;缺 `task` 报错;
  `teammate`/`teammates` 二选一校验;`team_executor=None` 降级;批量结果分节。
- **hakimi-core(`PersonaTeamExecutor`)**:
  - 不可寻址/不存在的 teammate 被拒;
  - 深度上限拒绝;回环检测拒绝;
  - 构建的队友 agent 携带正确 model/prompt/skills(用 mock template + 临时 registry);
  - `consult_many` 并行且单个失败不拖垮整批;
  - 名册排除自身、过滤 `addressable=false`。
- **hakimi-core/tests/integration.rs**:主导人格咨询队友的端到端(mock transport),
  断言队友答复并回主导上下文。
- **persona round-trip**:`addressable` 字段序列化/缺省反序列化(默认 true)向后兼容。

CI 门禁(权威):ubuntu nightly `fmt --all -- --check` + `clippy --workspace --all-targets
--all-features`(`-Dwarnings`)+ `test --workspace --all-features`。本机经 Docker
(`.superpowers/cargo.ps1`)预验。

## 10. 前向兼容与后续

- 本基座保留 `agent:<id>` 寻址的演进空间:`PersonaTeamExecutor` 的"把子任务投递给某人格并
  取回结果"正是 §5.2 异步路径所需的同步内核;后续异步层可让 gateway 把
  `send_message(target="agent:<id>")` 路由进同一执行内核(改为异步入站 + 回复路由)。
- 有状态团队会话:把 `consult` 的"无状态回合"升级为"按 (lead,teammate) 配对持久会话"。
- 团队编排:在 `team` 工具之上加"团队 = 一组人格 + 协调者 + 工作流"的一等配置与 WebUI 视图。

## 11. 涉及文件清单(实现时)

- `crates/hakimi-common/src/...`:`TeamExecutor` trait + 类型;`ToolContext.team_executor`。
- `crates/hakimi-core/src/persona.rs`:`addressable` 字段 + `default_true`。
- `crates/hakimi-core/src/team.rs`(新增):`PersonaTeamExecutor`。
- `crates/hakimi-core/src/lib.rs`:导出。
- `crates/hakimi-core/src/agent.rs` 或人格构建处:注入 `team_executor` 到 `ToolContext`。
- `crates/hakimi-tools/src/builtin_team.rs`(新增):`TeamTool`;在 tools `lib.rs` 注册。
- `crates/hakimi-cli/src/entry.rs`:gateway 双入口构建并注入 `PersonaTeamExecutor`。
- `crates/hakimi-server/src/server.rs` / `api.rs`:`AppState` 持有执行器;`/api/agents` 暴露
  `addressable`。
- `hakimi-webui/src/PersonaConfigForm.tsx` / `api.ts`:`addressable` 开关。
