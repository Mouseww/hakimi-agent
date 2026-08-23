# Hakimi 技术优化实施计划

> **For Hermes:** Use subagent-driven-development / heartbeat-driven continuous work to implement this plan task-by-task.

**Goal:** 把 Hakimi 从“功能叠加型 Agent”升级为“事件协议 + 状态机 + 回归测试驱动”的稳定 Agent Runtime。

**Architecture:** 先稳住 Gateway streaming 与发布质量，再整理 CLI/TUI/Gateway 入口，随后推进统一事件协议、工具进度独立通道、TUI 默认体验与 SmartContextEngine 产品化。所有阶段遵循：先审计当前状态 → 写回归测试 → 最小实现 → CI 验证 → release。

**Tech Stack:** Rust 2024 workspace, cargo test/fmt/clippy, GitHub Actions Release, systemd Gateway, Telegram/Clawbot/Teams adapters。

---

## 已锁定决策

| 决策 | 内容 |
|---|---|
| 优先级 | P0 稳定性 > P1 入口体验 > P2 事件协议 > P3 TUI/Context > P4 Runtime 解耦 |
| 实施方式 | heartbeat cronjob 每轮自主推进一小项，技术修复自动批准 |
| 验证策略 | 优先 GitHub CI；本机磁盘紧张时避免全量 release build |
| 代码栈 | 纯 Rust，不用 Python 实现 Hakimi 功能 |
| 本机正式二进制 | `/root/.hakimi/bin/hakimi` |
| systemd | `hakimi.service` 只跑 Gateway：`/root/.hakimi/bin/hakimi --gateway start` |
| WebUI | 已移除运行态；不要恢复旧 WebUI；未来另做 |
| 文档 | 新功能必须同步 README；技术修复可不强制 README，但需计划/测试记录 |

---

## Phase P0 — 立即稳定性加固

### Task P0.1: 为首句重复写永久回归测试

**Objective:** 防止 v0.5.128-v0.5.142 期间的首句重复问题再次出现。

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs` tests module or create focused test module if可行

**Steps:**
1. 新增测试：`gateway_tool_boundary_does_not_duplicate_first_sentence`。
2. 构造事件序列：Text("爸爸") → Text("稍等...") → render_pending(NewMessage) → finish_tool_boundary() → Text("喵～结果")。
3. 断言第一段只出现一次，不允许 `爸爸稍等...爸爸稍等...`。
4. Run: `cargo test -p hakimi-cli gateway_tool_boundary -- --nocapture`。
5. Run: `cargo fmt --all`。
6. Commit: `test(gateway): add regression test for first-sentence duplication`。

**Acceptance:** 测试能覆盖工具边界后的首句重复风险。

---

### Task P0.2: 补充 Gateway streaming 边界测试矩阵

**Objective:** 覆盖连续工具、媒体、delegate、overflow chunk、平台不支持 edit 等边界。

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs` tests

**Test cases:**
- consecutive tool boundaries keep clean state
- media boundary starts fresh message
- delegate boundary starts fresh message
- overflow chunks still produce ordered NewMessage
- final delivery does not resend duplicate complete response
- UTF-8 中文/emoji split remains char-safe

**Verify:**
- `cargo test -p hakimi-cli gateway_ -- --nocapture`
- `cargo fmt --all`

**Commit:** `test(gateway): expand streaming boundary regression coverage`

---

### Task P0.3: 添加 `hakimi doctor` ✅ 2026-08-23

**Status:** 本轮已扩展 doctor 诊断：安装版二进制、PATH shim、版本、systemd hakimi.service、旧 WebUI service、重复 Gateway 进程、端口 3005、config/session DB、网络连通性；README 已同步。验证：`cargo test -p hakimi-cli doctor -- --nocapture` 20 passed，`cargo run -p hakimi-agent -- doctor` 可正常输出。

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs`
- Maybe modify CLI args enum definitions in same file
- Update: `README.md` if command is user-facing

**Output should include:**
```text
Hakimi Doctor
- Binary: /root/.hakimi/bin/hakimi
- PATH shim: /usr/local/bin/hakimi -> /root/.hakimi/bin/hakimi
- Version: 0.5.xxx
- Systemd hakimi.service: active/inactive
- ExecStart: ...
- Duplicate gateway processes: none/list
- WebUI service: disabled/missing/active
- Port 3005: free/listener
- Config: /root/.hakimi/config.yaml exists
- Session DB: /root/.hakimi/sessions.db exists
- MCP health summary if cheaply available
```

**Implementation constraints:**
- Rust only。
- 不要依赖 shell `grep/sed` 作为核心逻辑；可用 `std::process::Command` 调 `systemctl`, `ss`, `pgrep`，失败时降级。
- 输出中文/英文都可，但要简洁。

**Verify:**
- `cargo test -p hakimi-cli doctor`
- `cargo run -p hakimi-agent -- doctor` 或 CI 中可编译即可

**Commit:** `feat(cli): add hakimi doctor diagnostics`

---

### Task P0.4: Release smoke test ✅ 2026-08-23

**Status:** 已添加 `scripts/release-smoke-test.sh` 并接入 Release workflow 的 Linux x86_64-unknown-linux-gnu 矩阵，在正式 release build 前执行 Gateway streaming 回归测试、`cargo fmt --all -- --check`、`cargo test -p hakimi-agent --no-default-features`，并在 `GITHUB_REF_NAME` 存在时校验 tag 版本与 `hakimi-agent` Cargo 版本一致。

**Objective:** 发版前自动验收二进制版本、Gateway 状态机测试、CLI 单次模式。

**Files:**
- Create: `scripts/release-smoke-test.sh`
- Modify: `.github/workflows/release.yml` or CI workflow if合适

**Script checks:**
1. `cargo test -p hakimi-cli gateway_`
2. `cargo fmt --all --check` 或 CI existing fmt
3. `cargo test -p hakimi-agent --no-default-features` if feasible;否则 targeted tests
4. Verify Cargo.toml version matches tag when `GITHUB_REF_NAME` available

**Disk constraint:** 不要求本机全量 release build；CI 执行。

**Commit:** `ci: add release smoke tests for gateway streaming`

---

## Phase P1 — 入口体验与状态机整理

### Task P1.1: 重构 GatewayStreamUiState ownership 注释与小结构 ✅ 2026-08-23

**Status:** 已为 `GatewayStreamUiState` 字段补充 ownership 注释，明确 `last_rendered_at_boundary` 只由 `render_pending(NewMessage)` 设置；已将 boundary 后的段状态清理提取为 `reset_segment_after_boundary()`，并扩展测试断言 boundary 只清 active segment、不覆盖 boundary marker。验证：`cargo fmt --all`，`cargo test -p hakimi-cli gateway_stream -- --nocapture`，`cargo test -p hakimi-cli gateway_tool_boundary -- --nocapture`，`cargo test -p hakimi-cli tool_boundary_forces_next_content_into_new_message -- --nocapture`。

**Objective:** 明确 `last_rendered_at_boundary` 只由 NewMessage 设置，boundary 只清空段状态。

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs`

**Steps:**
1. 添加 struct 字段注释，说明 ownership。
2. 将 boundary 清理逻辑提取为 `reset_segment_after_boundary()`。
3. 保持行为不变，测试先行。
4. Run targeted tests。

**Commit:** `refactor(gateway): clarify streaming boundary ownership`

---

### Task P1.2: CLI/TUI/Gateway 子命令入口整理方案与兼容层

**Status:** 已完成入口兼容层：`hakimi` 默认启动本地 TUI，`hakimi tui` 显式 TUI，`hakimi "prompt"` 单次 CLI，`hakimi gateway start|install|status|restart` 为 Gateway 入口，`--gateway start` 继续兼容，`hakimi serve` / `--serve` 明确报错旧 WebUI 已移除。本轮补充了无参默认 TUI 与 legacy `--serve` 解析回归测试，并同步 README 中 WebUI removed 表述。验证：`cargo test -p hakimi-cli top_level_doctor_and_setup_commands_parse_like_hermes -- --nocapture`，`cargo fmt --all`。

**Objective:** 让启动语义清晰：`hakimi` 默认 TUI，`hakimi "prompt"` 单次，`hakimi gateway start` Gateway。

**Files:**
- Modify: `crates/hakimi-cli/src/entry.rs`
- Modify README if user-facing changed

**Desired commands:**
| Command | Behavior |
|---|---|
| `hakimi` | 默认 TUI（若 TUI crate 已可用），否则给出明确安装/构建提示 |
| `hakimi "prompt"` | 单次 CLI print mode |
| `hakimi chat` | 交互式 CLI REPL |
| `hakimi tui` | 显式 TUI |
| `hakimi gateway start` | 前台 Gateway |
| `hakimi gateway install/status/restart` | systemd 管理 |
| `hakimi serve` / `--serve` | 明确报错 WebUI removed |

**Compatibility:** 保留 `--gateway start`，但 help 引导新子命令。

**Commit:** `feat(cli): clarify tui cli gateway command entrypoints`

---

## Phase P2 — 统一事件协议与工具进度

### Task P2.1: 设计 `AgentUiEvent` 内部事件协议 ✅ 2026-08-23

**Status:** 已新增 `.hermes/plans/hakimi-agent-ui-event-protocol.md`，明确 AgentUiEvent 分层、事件类型草案、语义规则、与现有 `StreamEvent` / `GatewayStreamUiEvent` 的映射、渐进迁移计划和测试要求。验证：`cargo test -p hakimi-common ui_event -- --nocapture`（当前无专门测试，编译通过）；`cargo fmt --all`。

**Objective:** 将 Text/Tool/Media/Delegate/Final/Error 统一为显式事件，先写设计文档和类型草案。

**Files:**
- Create: `.hermes/plans/hakimi-agent-ui-event-protocol.md`
- Maybe create Rust type in suitable crate after audit

**Events:**
```rust
enum AgentUiEvent {
    TextDelta(String),
    ToolCallStarted(ToolCallMeta),
    ToolCallProgress(ToolProgress),
    ToolCallFinished(ToolResultMeta),
    MediaGenerated(MediaRef),
    DelegateProgress(DelegateEvent),
    MessageBoundary(BoundaryReason),
    FinalAnswer(String),
    Error(String),
}
```

**Commit:** `docs: design agent ui event protocol`

---

### Task P2.2: Tool progress 独立通道 ✅ 2026-08-23

**Status:** 本轮将 Gateway `hakimi_tool` / `hakimi_review` 进度统一格式化为独立时间戳事件：`⚙️ HH:MM ...`，保持通过 `GatewayStreamUiEvent::Tool` 单独发送，不写入 assistant prose；新增回归测试覆盖进度格式化、tool boundary 后最终助手文本不含工具噪声且不复发首句重复。验证：`cargo fmt --all`，`cargo test -p hakimi-cli gateway_tool_progress -- --nocapture`，`cargo test -p hakimi-cli gateway_ -- --nocapture`。

**Objective:** 工具调用进度显示为独立进度事件，不污染 assistant prose。

**Files:**
- Audit first: find streaming callback/tool progress code
- Modify relevant Gateway rendering code

**Acceptance:**
- 工具进度可见：`⚙️ HH:MM terminal ...`
- 最终助手消息不包含工具噪声
- 不引发首句重复

**Commit:** `feat(gateway): separate tool progress from assistant text`

---

## Phase P3 — TUI 与 SmartContextEngine

### Task P3.1: TUI 默认体验最小可用

**Status:** 进行中；已补 `hakimi-tui --smoke` 非交互启动探针，用于验证 release 包内 TUI 二进制、配置加载和模型解析，不进入 raw terminal mode；README 已同步 smoke 用法；Release workflow 已在 Linux x86_64-unknown-linux-gnu release build 后执行 `target/.../release/hakimi tui --smoke`，验证主 `hakimi` 二进制能发现同目录 bundled `hakimi-tui`。已补充 TUI 欢迎语可发现性：首屏提示 `/help`、`Ctrl+C`、`/quit`，并添加回归测试。本轮将无参 `hakimi` 默认 TUI 路径前置到 agent 构造前，避免无参启动先加载/校验 CLI agent 配置；补充默认 TUI/legacy gateway/one-shot 入口选择回归断言。新增 TUI 输入编辑 UTF-8 边界回归修复，Backspace/Delete/左右移动按 Unicode 字符边界处理，避免中文/emoji 输入时切到非法 byte offset。本轮补充 readline 风格 Ctrl+A/Ctrl+E 输入光标跳转，并在 README 记录 TUI 快捷键。本轮补充 TUI 输入框真实终端光标渲染，按 Unicode 显示宽度定位，并覆盖非法 byte offset clamp 回归，避免中文/emoji 输入时屏幕光标与内部编辑位置错位。本轮补充 readline 风格 Ctrl+W 删除光标前一个词，按 UTF-8 字符边界处理中文/emoji，并同步 README 快捷键说明。验证：`cargo test -p hakimi-cli top_level_doctor_and_setup_commands_parse_like_hermes -- --nocapture`，`cargo run -p hakimi-agent -- tui --smoke`，`cargo test -p hakimi-tui input_editing_handles_utf8_char_boundaries -- --nocapture`，`cargo test -p hakimi-tui status_bar_surfaces_help_hint -- --nocapture`，`cargo test -p hakimi-tui ctrl_ -- --nocapture`，`cargo test -p hakimi-tui input_cursor_ -- --nocapture`，`cargo test -p hakimi-tui previous_word_start -- --nocapture`，`cargo test -p hakimi-tui ctrl_w_ -- --nocapture`，`cargo fmt --all`。

**Objective:** WebUI 移除后，本地无参 `hakimi` 有高级感 TUI/明确入口。

**Files:**
- Audit: `crates/hakimi-tui/`
- Modify CLI entry wiring
- README update

**Acceptance:**
- `hakimi tui` 可启动
- `hakimi` 默认行为明确
- Gateway 不受影响

**Commit:** `feat(tui): wire tui as default local experience`

---

### Task P3.2: SmartContextEngine 产品化设计 ✅ 2026-08-23

**Status:** 已新增 `.hermes/plans/hakimi-smart-context-engine.md`，审计现有 `hakimi-context` / `hakimi-skills` / `hakimi-session` 后，设计了 `ContextProvider` 插件化接口、provider 初始清单、request-local planner、telemetry、与现有 `SmartContextEngine` 三层压缩的渐进迁移关系。验证：`cargo test -p hakimi-context smart_context -- --nocapture`，`cargo fmt --all`。

**Objective:** 设计 ContextProvider 插件化接口，按需加载 memory/session/skill/project context。

**Files:**
- Create: `.hermes/plans/hakimi-smart-context-engine.md`
- Audit existing `hakimi-context`, `hakimi-memory`, `hakimi-skills`

**Commit:** `docs(context): design provider-based smart context engine`

---

## Phase P4 — Runtime/UI 解耦

### Task P4.1: Agent Runtime / Renderer 边界设计 ✅ 2026-08-24

**Status:** 已新增 `.hermes/plans/hakimi-runtime-renderer-boundary.md`，审计现有 `hakimi-core` stream callback、`hakimi-cli` GatewayStreamUiState、`hakimi-tui` AgentEvent、`hakimi-studio-api` EventBus/StudioRuntime 后，明确 runtime 是 run/session/cancel/queue 的唯一事实源，Gateway/TUI/Studio/Future WebUI 只消费事件并维护 renderer-local preview state。文档记录了 ownership 表、RuntimeEvent 草案、渐进迁移计划和待决策 blocker。验证：`cargo fmt --all`，`cargo test -p hakimi-studio-api event_bus -- --nocapture`。

**Objective:** 明确 Core Runtime 是唯一真相源，Gateway/TUI/Future WebUI 都只订阅事件并渲染。

**Files:**
- Create: `.hermes/plans/hakimi-runtime-renderer-boundary.md`

**Architecture:**
```text
hakimi-core        Agent loop / tools / memory / context
hakimi-runtime     session execution / queue / cancellation / event stream
hakimi-gateway     Telegram / Teams / Clawbot renderers
hakimi-cli         CLI/TUI frontend
hakimi-server      optional HTTP API only
```

**Commit:** `docs(runtime): define runtime renderer boundary`

---

## Heartbeat 执行规则

每轮 heartbeat 必须：

1. `cd /root/hakimi-agent`
2. 先审计当前状态：
   - `git status --short`
   - `git log --oneline -5`
   - `gh run list --limit 5` if relevant
   - 检查本计划中下一个未完成任务是否已被别人完成
3. 只做一个小任务或一个测试子项。
4. 优先运行 targeted tests，不在磁盘紧张时全量 build。
5. Rust 改动后跑 `cargo fmt --all`。
6. Commit + push。
7. 如果涉及用户可见功能，README 同步更新。
8. 中文汇报：完成了什么、验证结果、下轮做什么。

## Stop Conditions

- 所有 P0/P1 完成后，可降低 heartbeat 频率。
- 如果 CI 红，下一轮优先修 CI。
- 如果出现架构歧义，只记录 blocker 并暂停该项，转做下一个无歧义 P0/P1 任务。
