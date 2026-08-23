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

### Task P0.3: 添加 `hakimi doctor`

**Objective:** 快速诊断安装版/源码版/systemd/重复进程/端口状态，减少排查成本。

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

### Task P0.4: Release smoke test

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

### Task P1.1: 重构 GatewayStreamUiState ownership 注释与小结构

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

### Task P2.1: 设计 `AgentUiEvent` 内部事件协议

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

### Task P2.2: Tool progress 独立通道

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

### Task P3.2: SmartContextEngine 产品化设计

**Objective:** 设计 ContextProvider 插件化接口，按需加载 memory/session/skill/project context。

**Files:**
- Create: `.hermes/plans/hakimi-smart-context-engine.md`
- Audit existing `hakimi-context`, `hakimi-memory`, `hakimi-skills`

**Commit:** `docs(context): design provider-based smart context engine`

---

## Phase P4 — Runtime/UI 解耦

### Task P4.1: Agent Runtime / Renderer 边界设计

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
