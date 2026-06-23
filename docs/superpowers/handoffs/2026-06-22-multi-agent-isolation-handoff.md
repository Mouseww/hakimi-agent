# 交接:Hakimi 多 Agent 空间隔离 + WebUI 重设计(继续 P3 / P4 / P5)

- 日期:2026-06-22
- 分支:`feat/shared-runtime` · PR:https://github.com/Mouseww/hakimi-agent/pull/1
- 目标:一个实例 N 个隔离 Agent(人格),每人格独立 `memory/skills/prompt/model/channel 绑定`,共用 `transport/工具/知识`;并重设计 WebUI 管理它们。

## 权威文档(读这些,勿重复)
- 设计 spec(含隔离架构 + WebUI Layout A):`docs/superpowers/specs/2026-06-22-multi-agent-isolation-and-webui-design.md`
- P1 实现计划(可作 P3/P4/P5 计划的范本):`docs/superpowers/plans/2026-06-22-p1-shared-runtime-extraction.md`

## 已完成 + CI 验证(均在 `feat/shared-runtime`)
- **P1 SharedRuntime 抽取**(`ced0a2e`、`edcfd17`)— CI 绿。把 `AIAgent` 的 transport/tool_registry/knowledge/embedding 抽到 `pub shared: Arc<SharedRuntime>`。
- **P2a 人格模型+存储**(`71a6e31`):`crates/hakimi-core/src/persona.rs`(`PersonaConfig`/`RegistryIndex`/load/save)+ `RuntimeHome::agents_dir/persona_dir/agents_registry_path`。
- **P2b 路由 registry + 活体 Agent**(`4dce0c5`、`63ee77b`):`persona_registry.rs`(`PersonaRegistry`:load/get/`resolve_for_channel`/create/update/delete/persist + `DEFAULT_PERSONA_ID`)、`persona_runtime.rs`(`build_persona_agent`:clone 模板共享 SharedRuntime + 覆盖 model/prompt/独立 context_engine/skills)。CI 绿。
- **P2c AppState 接入 registry**(`ab7c1f4`):`server.rs` 加 `persona_registry: Arc<tokio::sync::RwLock<PersonaRegistry>>`,3 处构造点加载(server.rs / entry.rs / api.rs test)。**CI 绿**(run 27963649709)。现有端点行为不变(仍走 `state.agent` = 默认人格);registry 已就位待 P3/P4 使用。
- **P3 Gateway 路由**(`eba0e75`):gateway 消息循环改用 `registry.resolve_for_channel(platform, bot_id)` 取人格并派发到该人格 agent;per-chat histories 改用 `persona_id:chat_id` key 下沉到人格(读/写/`/clear`/`/undo` 一致)。计划:`docs/superpowers/plans/2026-06-22-p3-gateway-persona-routing.md`。新增 `gateway_history_key` + `build_gateway_persona_agents`;`process_gateway_messages_loop` 增 `persona_registry`/`persona_agents` 两参;`start_gateway` + `start_unified_server` 双入口接线(统一模式与 AppState 共享同一 registry Arc)。默认人格(id=`default`)保持 legacy 行为(复用 `agent_arc` + `config.roles[default]` prompt + root memory);命名人格独立 model/prompt/context/skills(`agents/<id>/skills`)/memory(`agents/<id>/memory`),无预构建 agent 时按 legacy 兜底。**本地 Docker 验证通过**(fmt + clippy `-Dwarnings` + test);**CI 待推送确认**(本机 push 需交互式 GCM,由用户推送)。
- **P4 Agent 维度 API**(`74aefe6` core 访问器 + `415398e` server):`crates/hakimi-server/src/api.rs` 新增 7 个端点(均挂已鉴权 `/api` 下),操作共享 `Arc<RwLock<PersonaRegistry>>`:`GET/POST /api/agents`、`GET/PATCH/DELETE /api/agents/{id}`、`POST /api/agents/{id}/chat`(非流式)、`GET /api/bindings`。计划:`docs/superpowers/plans/2026-06-22-p4-agent-dimension-api.md`。CRUD 走 `registry.create/update/delete`(落盘 + 重建 binding_index → 统一模式 gateway 路由即时生效);PATCH 字段合并;新增 `PersonaRegistry::agents_dir()`;`/chat` 按需构建人格 agent(默认人格直接克隆模板,命名人格走 `build_persona_agent`)。现有 `/api/chat`、`/api/sessions`、`/v1/*` 不变(向后兼容)。**本地 Docker 验证通过**(fmt + clippy `-Dwarnings` + 4 端点测试);**CI 待推送确认**。
- **P5 WebUI Layout A**(`79103bd`):重设计 React 操作台 `hakimi-webui/`(顶层工程,React19 + Vite + TS)。计划:`docs/superpowers/plans/2026-06-23-p5-webui-layout-a.md`。topbar 下新增窄人格栏 `PersonaRail`,主区按 `view`(`chat`/`config`/`instance`)切换:`PersonaConfigForm`(身份/模型/推理强度/系统提示/技能 chips/绑定/默认)、`InstanceSettings`(绑定总览表 + 并入的 SettingsPanel/GatewayPanel)。`api.ts` 加 `agents`(CRUD)/`agentChat`/`bindings`;聊天走 `/api/agents/{id}/chat`。右面板 `control`/`gateway` tab 迁入实例设置。补 lucide 图标声明,顺带修 GatewayPanel 既有 `set-state-in-effect` lint。**本机验证通过**(`npm run lint` 干净 + `npm run build` = tsc + vite)。**重要落地缺口**:仅改 React app,**未接入二进制嵌入链路** —— 运行的二进制仍 `include_str!` 嵌 `crates/hakimi-webui/static/`(手写 vanilla JS),React 改动不会自动 ship(经与用户确认,本期范围如此)。

## 剩余工作(按序)
- ~~**P3 Gateway 路由**~~ — 已完成(见上,`eba0e75`)。
- ~~**P4 Agent 维度 API**~~ — 已完成(见上,`74aefe6` + `415398e`)。
  - **遗留给后续的钩子**:gateway 的 `persona_agents`(预构建 base agents `HashMap`)在 `start_gateway`/`start_unified_server` 启动时构建一次,binding 解析读的是共享 registry(改绑定即时生效),但**新建人格的 agent 实例需在 CRUD 后重建/插入该 map** 才能在 gateway 侧免重启热生效(对未在 map 中的命名人格按 legacy 兜底)。WebUI 的 `/api/agents/{id}/chat` 按需构建,不受此限。考虑把 `persona_agents` 升级为 `Arc<RwLock<…>>` 放进 `AppState`,在 create/delete 时同步。另:P4 `/chat` 非流式且不落 session DB;`/api/agents/{id}/sessions|memory|skills` 与 `/chat/stream`、人格级 sessions.db/cron.db 隔离均为后续。
- ~~**P5 WebUI**~~ — 已完成(见上,`79103bd`)。
  - **遗留给后续**:把 React `dist/` 接入二进制嵌入(改 `api.rs` 的 `include_str!` 指向 `dist/` 或加 build.rs/构建步骤),否则重设计不 ship;人格作用域 sessions/memory/skills 子资源 UI、流式人格 chat、客户端路由(`/agents/:id`)、§4.3 协作(`agent:<id>`)占位开关均未做。
- **前向兼容钩子**(spec §5.2):`agent:<id>` 寻址(多 Agent 互聊)留作后续单独 spec,本期只在 `send_message` target 解析预留;无 channel 的人格在 WebUI 内已天然可用。

## 环境 gotchas(关键,务必照做)
- **本机无 Rust 工具链**,所有 cargo 经 Docker:`& "D:\projects\hakimi-agent\.superpowers\cargo.ps1" <args>`(镜像 `hakimi-rust:nightly` = `rustlang/rust:nightly` + clippy/rustfmt;target/registry 用命名卷;已设 `RUSTFLAGS=-Dwarnings`)。`.superpowers/` 已 gitignore(内含 cargo.ps1 / Dockerfile / pr-body.md)。
- **未装 `gh`**:建/查 PR 走 GitHub REST API + `git credential fill` 取 token(公开仓库只读 API 免 token);CI 状态用 `GET /repos/Mouseww/hakimi-agent/actions/runs?head_sha=<sha>` 轮询。
- **`git push` 需交互式 GCM 登录**(自动化非交互会失败);用户自行推或在其终端完成登录。会话需关掉「auto 自动批准」模式,改手动批准命令(否则会撞到安全分类器临时不可用)。
- **CI 是权威门禁**(ubuntu nightly:`fmt --all -- --check` + `clippy --workspace --all-targets --all-features` + `test --workspace --all-features`)。hakimi-core 改动可本地快速验证;server/cli 冷编译慢,优先推送让 CI 验证。
- **main 历史遗留已在本分支修**:v0.3.282 改压缩阈值 70%→60% 但漏改测试(`c88445a` 修 smart_engine 测试),另修了历史 fmt 偏差(`bba2f97`)。
- 行尾 LF/CRLF 警告无害。隔离边界:`context_engine`/`skill_store` 是每人格;transport/tools/knowledge 共享。

## 建议下一会话使用的技能
- `superpowers:subagent-driven-development` 或 `superpowers:executing-plans` 继续执行(已在 `feat/shared-runtime`,无需再开 worktree)
- 各阶段先 `superpowers:writing-plans` 出 P3/P4/P5 细化计划(以 P1 计划为范本)
- P5 用 `frontend-design`
- 大改动后 `superpowers:requesting-code-review`
