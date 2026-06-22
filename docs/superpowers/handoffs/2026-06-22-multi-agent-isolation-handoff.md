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

## 剩余工作(按序)
- **P3 Gateway 路由**:gateway 消息循环改用 `registry.resolve_for_channel(platform, bot_id)` 取人格并派发到该人格 agent;per-chat histories 下沉到人格。入口:`crates/hakimi-cli/src/entry.rs` 的 gateway 消息处理(原单 `agent_arc` 处;`task_key = platform:bot_id:chat_id`)。
- **P4 Agent 维度 API**:`/api/agents`(GET/POST)、`/api/agents/{id}`(GET/PATCH/DELETE)、`/api/agents/{id}/chat`、`/api/bindings`,用 `state.persona_registry`。位置:`crates/hakimi-server/src/api.rs`(`build_router` ~1903 + handlers)。契约见 spec §3.6。现有端点保持指向默认人格(向后兼容)。
- **P5 WebUI**:Layout A(左侧人格栏)+ 人格配置表单 + 实例设置/绑定总览。前端 `hakimi-webui/`(React19 + Vite + Tailwind4)。3 个已确认 mockup 概念见 spec §4。
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
