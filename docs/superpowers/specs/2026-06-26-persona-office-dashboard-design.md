# 设计:人格办公室仪表板(Persona Office Dashboard)

- 日期:2026-06-26
- 分支:`feat/persona-office-dashboard`(从 `feat/persona-team-collaboration` 切出,依赖其 team 执行器)
- 相关:`docs/superpowers/specs/2026-06-24-persona-team-collaboration-design.md`(团队协作,提供咨询/组队事件源)

## 1. 目标

在 WebUI 新增一个卡通办公室仪表板,把每个人格当作"员工",实时、忠实地可视化所有人格的工作状态:谁在执行任务、谁空闲、A 去找 B 交付需求、ABC 组队、新人格入职。

把人格视为员工的六条行为(用户原始需求):

1. 办公室里每个人格坐在一个工位,每个工位一台电脑。
2. A 找 B 干活 → A 跑到 B 处交付需求的动画。
3. ABC 三人组队完成一件任务 → 三人带电脑聚坐。
4. 正在执行任务的人格:电脑屏幕亮 + 操纵键盘动作。
5. 空闲人格:屏幕在看电视/打游戏。
6. 新人格被创建 → 新员工入职,安排新座位。

## 2. 已确认的设计决策

- **数据源:全栈实时事件流。** 后端新增"人格活动事件总线 + SSE",忠实覆盖所有活动(网关/Telegram + WebUI)。
- **美术:扁平矢量 + 微俯视角。** 与现有控制台风格统一,SVG+CSS 动画,可随主题换色。
- **规模:中等开放式。** 自动按行排列工位,几个到 ~20 个人格都优雅,超出滚动;SVG/DOM 渲染。
- **交互:可交互导航。** 点工位→进入该人格对话/配置;悬停→状态详情卡。

## 3. 总体架构(两层)

```
  发布点(已有活动的地方)                      ActivityHub (hakimi-common 全局)
  ┌─────────────────────────┐                 ┌───────────────────────────────┐
  │ 人格 CRUD (api.rs)        │── publish ────▶│ broadcast::Sender<ActivityEvent>│
  │ 网关回合 (entry.rs)       │── publish ────▶│ + Mutex<HashMap<id,            │
  │ WebUI chat (api.rs stream)│── publish ────▶│        PersonaActivity>> 快照   │
  │ team 执行器 (core/team.rs)│── publish ────▶└──────────────┬────────────────┘
  └─────────────────────────┘                                │
                                          ┌────────────────────┴───────────────┐
                                          │ GET /api/activity/snapshot (初始态)  │
                                          │ GET /api/activity/stream   (SSE 增量) │
                                          └────────────────────┬───────────────┘
                                                               │  SSE
                                          ┌────────────────────▼───────────────┐
                                          │ WebUI 办公室视图 OfficeView          │
                                          │ useActivityStream → 状态归约          │
                                          │ officeLayout(座位) → PersonaDesk(精灵)│
                                          └─────────────────────────────────────┘
```

设计要点:`ActivityHub` 放在 `hakimi-common`(所有 crate 都依赖它),沿用仓库已有的全局单例模式(参考 `crates/hakimi-tools/src/builtin_send_message.rs` 的 `MESSAGE_QUEUE: LazyLock<Mutex<...>>`)。发布方调用 `hakimi_common::activity::publish(event)`,无需把句柄穿线到各处。

## 4. 事件模型与人格状态机

### 4.1 `ActivityEvent`(`#[serde(tag = "type")]` 标签枚举)

```rust
pub enum ActivityEvent {
    PersonaCreated { id: String, name: String, avatar: String },
    PersonaUpdated { id: String, name: String, avatar: String },
    PersonaDeleted { id: String },
    TurnStarted { persona_id: String, task_hint: Option<String>, model: Option<String> },
    TurnEnded   { persona_id: String },
    ConsultStarted { from_id: String, to_id: String, task_hint: Option<String> },
    ConsultEnded   { from_id: String, to_id: String },
    TeamFormed { team_id: String, lead_id: String, member_ids: Vec<String>, task_hint: Option<String> },
    TeamDisbanded { team_id: String },
}
```

- `task_hint` 是脱敏的简短任务摘要(截断,不含敏感内容),供详情卡显示;可为 `None`。
- 所有 id 为人格 id;未知 id 由前端懒加座位。

### 4.2 人格状态:基态 + 叠加态(前端归约 + 后端快照一致)

咨询是**同步阻塞**的:A 在自己的回合中调用 team 工具去找 B,期间 A 的回合仍在进行,所以 A 在 `ConsultEnded` 后必须回到 `working`(而非 `idle`)。为干净处理这点,状态用"基态 + 叠加态"建模:

- **基态**(由 Turn 事件驱动):`idle` ↔ `working`(`TurnStarted`/`TurnEnded`)。
- **叠加态**(由 Consult/Team 事件驱动,瞬时):`consulting { target }`(`ConsultStarted`/`ConsultEnded`)、`in_team { team_id }`(`TeamFormed`/`TeamDisbanded`)。

**显示状态**(`PersonaActivity.state`)取优先级:`in_team` > `consulting` > 基态(`working`/`idle`)。叠加态结束后回落到当时的基态(故 A 咨询结束后若回合未结束则仍显示 `working`)。hub 内部需同时记基态与叠加态以正确计算显示状态;前端 reducer 与之一致。

### 4.3 `PersonaActivity`(快照表的值)

```rust
pub struct PersonaActivity {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub state: PersonaState,        // idle | working | consulting | in_team
    pub task_hint: Option<String>,
    pub model: Option<String>,
    pub team_id: Option<String>,
    pub updated_at: String,         // rfc3339,用于详情卡计时(后端 stamp)
}
```

## 5. 后端组件

### 5.1 `crates/hakimi-common/src/activity.rs`(新)

- `ActivityEvent`、`PersonaState`、`PersonaActivity` 类型。
- 全局 `ActivityHub`:`LazyLock` 持有 `tokio::sync::broadcast::Sender<ActivityEvent>`(容量有上限,慢消费者丢旧事件)+ `Mutex<HashMap<String, PersonaActivity>>` 快照表。
- `pub fn publish(event: ActivityEvent)`:更新快照表(按事件迁移状态)+ `send` 广播(忽略无订阅者错误)。
- `pub fn subscribe() -> broadcast::Receiver<ActivityEvent>`、`pub fn snapshot() -> Vec<PersonaActivity>`。
- 快照状态迁移在 hub 内集中实现(单一真相源),前端归约与之保持一致。

### 5.2 发布点

- **人格 CRUD**(`crates/hakimi-server/src/api.rs`:`create_agent`/`update_agent`/`delete_agent`)→ `PersonaCreated/Updated/Deleted`。
- **网关回合**(`crates/hakimi-cli/src/entry.rs`:`process_gateway_messages_loop` 的 turn agent 起止处,已有 `persona_id` 与回合边界)→ `TurnStarted/TurnEnded`。
- **WebUI 流式 chat**(`crates/hakimi-server/src/api.rs`:`agent_chat_stream` 起止)→ `TurnStarted/TurnEnded`。
- **team 执行器**(`crates/hakimi-core/src/team.rs`):`consult` 起止 → `ConsultStarted/ConsultEnded`(from = 当前 lineage 末端的发起者,to = teammate);`consult_many`(N>1)进入时 `TeamFormed`、结束时 `TeamDisbanded`(`team_id` 由执行器生成一个 uuid,起止配对);`from_id` 由执行器的 lineage 末端提供。

> 接口微调:`consult` 当前不知道"发起者 id"。执行器的 `lineage` 末端即发起者(`for_lead(lead_id)` 注入了 `[lead_id]`),`PersonaTeamExecutor` 可据此得到 `from_id`,无需改 `TeamCallContext`。

### 5.3 端点(`api.rs`,挂在已鉴权 `/api` 下)

- `GET /api/activity/snapshot` → `{ personas: [PersonaActivity...] }`。
- `GET /api/activity/stream` → SSE,每个 `ActivityEvent` 一条 `data:`(JSON)。复用现有 SSE 基础设施风格;断线由前端重连。

## 6. 前端组件(`hakimi-webui`)

- `OfficeView.tsx`:顶层视图容器;调用 `useActivityStream`,渲染地板 + 工位 + 临时动画层。
- `useActivityStream.ts`:先 `GET snapshot` 再开 SSE(沿用 `streamAgentChat` 的 fetch+ReadableStream 思路或 `EventSource`);把事件归约成 `Map<id, PersonaActivity>` + 组队分组;指数退避重连,重连后重取 snapshot 对齐。
- `officeLayout.ts`(纯函数,易测):输入人格列表 + 组队分组,输出每个人格的座位(行列网格,**稳定座位**:已就座的人格保持原位,新人格分配下一个空位,删除留空位可复用),并计算组队聚拢坐标。
- `PersonaDesk.tsx`:单个工位 + 角色精灵;按 `state` 渲染:`working`=亮屏+打字;`idle`=游戏/电视屏;`consulting`/`in_team` 见动画层。点击→选中人格并切到对话视图;悬停→详情卡(name/avatar/state/task_hint/model/计时)。
- 动画层:`ConsultStarted` → 生成临时"跑动"精灵,从 from 座位沿路径跑到 to 座位(`ConsultEnded` 或超时后移除);`TeamFormed` → 成员工位聚拢 + 高亮环,`TeamDisbanded` 复位。
- 精灵:扁平矢量(头/身/手 + 显示器),人格 `avatar`(emoji)作为角色面牌;主题色用现有 `App.css` CSS 变量。
- 导航:`App.tsx` 增加 `view = 'office'`,在人格栏/顶栏加入口;i18n 文案补充。

## 7. 数据流

后端发布 → SSE → `useActivityStream` 归约 → `OfficeView` 用 `officeLayout` 布座 → `PersonaDesk` 渲染状态;瞬时 `Consult*` 事件驱动动画层的跑动精灵;`Team*` 事件重组工位分组。

## 8. 错误处理与降级

- SSE 退避重连;重连后重取 snapshot 重新对齐(防止漏事件导致状态漂移)。
- snapshot 保证无实时事件时也能渲染初始办公室。
- 事件引用未知人格(竞态)→ 前端懒加一个座位(随后 snapshot 校正)。
- `prefers-reduced-motion` → 关闭跑动/打字/闪烁,用静态状态徽标替代。
- 活动端点缺失/连接失败 → 回退到 `GET /api/agents` 的静态花名册(全部按 idle 展示)+ 顶部提示"实时不可用"。
- broadcast 慢消费者丢事件 → 不致命;下次 snapshot 或后续事件自愈。

## 9. 测试计划

- **后端(hakimi-common / server / core / cli):**
  - `ActivityHub` 发布/订阅:`publish` 后 `subscribe` 收到事件;快照表按事件正确迁移状态(turn→working→idle、consult→consulting、teamformed→in_team→复位)。
  - 各发布点发出预期事件:CRUD → Created/Deleted;team `consult` → ConsultStarted/Ended(from/to 正确);`consult_many(N>1)` → TeamFormed/Disbanded。
  - SSE 端点冒烟:订阅后 publish 一个事件,客户端能收到对应 JSON;snapshot 端点返回当前态。
- **前端(hakimi-webui):**
  - `officeLayout` 纯函数:稳定座位分配、补空位、组队聚拢坐标。
  - 事件归约 reducer:事件序列 → 正确的 `Map<id,state>` 与组队分组(含状态优先级)。
  - `PersonaDesk` 各状态渲染(idle/working/consulting/in_team)与点击/悬停回调。

CI 门禁(权威):ubuntu nightly `fmt --all -- --check` + `clippy --workspace --all-targets --all-features`(`-Dwarnings`)+ `test --workspace --all-features`。前端 `tsc` + `eslint` + `vite build` 本地跑,并重建提交 `crates/hakimi-webui/static/` 嵌入产物(前端 CI 不覆盖)。详见记忆 [[hakimi-toolchain-fmt-clippy-divergence]] 与 [[hakimi-ci-over-local-builds]]。

## 10. 范围与分期

- **阶段1(后端,可独立验证):** `hakimi-common` 的 `ActivityHub` + 各发布点 + `/api/activity/snapshot` 与 `/api/activity/stream`。验证:在网关/对话/组队活动时 `curl` SSE 看到事件流。
- **阶段2(前端):** 办公室视图 + 布局引擎 + 精灵 + 动画 + 交互导航,由活动流驱动;重建嵌入式 bundle。
- **暂不做:** 拖拽排座/手动座位、50+ 园区缩放、复杂 idle 花样、音效、活动历史回放。

## 11. 涉及文件清单(实现时)

- `crates/hakimi-common/src/activity.rs`(新)+ `lib.rs` 导出。
- `crates/hakimi-server/src/api.rs`:两个端点 + 路由;CRUD 与 `agent_chat_stream` 发布点。
- `crates/hakimi-cli/src/entry.rs`:网关回合发布 `TurnStarted/Ended`。
- `crates/hakimi-core/src/team.rs`:`consult`/`consult_many` 发布 `Consult*`/`Team*`。
- `hakimi-webui/src/`:`OfficeView.tsx`、`useActivityStream.ts`、`officeLayout.ts`、`PersonaDesk.tsx`(+精灵)、`api.ts`(活动类型 + snapshot/stream)、`App.tsx`(新视图 + 入口)、`i18n.tsx`、`App.css`;重建 `crates/hakimi-webui/static/`。
