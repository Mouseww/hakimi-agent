# Hakimi Studio Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task after user approval.

**Goal:** 交付跨端（Desktop/Web/Mobile-PWA）AI 开发工作台，Hakimi Core 单引擎，支持工作区浏览、多 Provider、SubAgent 组队、Skills/MCP、SSH、Cron，以及服务器托管下的多端会话接力。

**Architecture:** Local-first Execution Node（Tauri 或 Server Worker）跑 `hakimi-core`；可选 `hakimi-hub` 中继多端事件；UI Shell 同构 React，只发 command/渲染 event。

**Tech Stack:** Rust (Axum, tokio), Tauri 2, React 19 + Vite + Monaco + xterm.js, SQLite, Docker。

---

## Phase 0 — Protocol & Skeleton ✅ (2026-07-24)

### Task 0.1: 协议文档与模块占位 ✅

**Objective:** 落盘设计文档并在 workspace 登记新 crate 占位。

**Files:**
- 已有: `docs/hakimi-studio/DESIGN.md`（v0.2 决策已锁定）
- Create: `docs/hakimi-studio/protocol.md`
- Create: `crates/hakimi-studio-api/`（protocol / event_bus / runtime）
- Modify: workspace `Cargo.toml` members

**Done:** `cargo test -p hakimi-studio-api` 3 passed

### Task 0.2: 进程内 Studio Runtime ✅

**Objective:** 不经网络，在库内完成 create session → submit → 订阅事件。

**实现要点:**
- `EventBus`：broadcast + per-session seq + 有界 replay
- `StudioRuntime`：session CRUD、`chat.submit` 队列、`chat.preempt` 抢占、mock agent 流式 delta
- 测试：`message.delta` + `session.ended`；busy 时非 preempt 入队

### Task 0.3: 本地 HTTP+WS 服务 ✅

**Objective:** `hakimi-server` 暴露 Studio 端点。

**Files:**
- Create: `crates/hakimi-server/src/studio.rs`
- Modify: `api.rs` merge `/v1/studio` + `/v1/studio/health`
- Modify: `Cargo.toml`（`hakimi-studio-api` + axum `ws`）

**端点:**
- `GET /v1/studio/health` — JSON liveness
- `GET /v1/studio` — WebSocket（JSON Command in / EventEnvelope out）

**Done:** `cargo check -p hakimi-server` 通过；Phase 1.5 已接 `CoreAgentHost` → 真实 `hakimi-core`

## Phase 1 — Workspace IDE + Providers + SSH v1 🚧

### Done (2026-07-24)

| Item | Status |
|------|--------|
| `hakimi-workspace` crate (path jail + list/read/write/create/delete/grep) | ✅ tests 3/3 |
| Protocol `workspace.*` commands/events | ✅ |
| StudioRuntime workspace dispatch | ✅ |
| Studio WebUI 三栏 (tree \| editor \| chat) over `/v1/studio` WS | ✅ built into static |
| Default view = Studio; Office 可切换 | ✅ |
| `fallback_models` agent field + builder (unblocked server compile) | ✅ |

### Phase 1.5 — Wire Studio Chat → real `hakimi-core` ✅ (2026-07-24)

| Item | Status |
|------|--------|
| `AgentHost` trait + `MockAgentHost` (unit tests keep mock) | ✅ `agent_host.rs` |
| `StudioRuntime` injects `Arc<dyn AgentHost>`; spawn_run → host.run_turn | ✅ |
| `CoreAgentHost` in `hakimi-server`: clone agent + request-local stream callback | ✅ |
| Stream markers → `ToolStarted` / `ToolCompleted` / `MessageDelta` | ✅ |
| Preempt: `Notify` + `interrupt` AtomicBool | ✅ |
| `StudioState::with_shared_agent` wired in `build_router` | ✅ |
| Tests: `hakimi-studio-api` 3 + `hakimi-workspace` 3; `cargo check -p hakimi-server` | ✅ |

**Pattern (anti 真假猴王 / SSE hang):** never attach streaming callback to the shared `AppState` agent; clone per turn, clear callbacks after turn ends.

### Remaining / deferred

- Monaco editor (currently highlight.js readonly)
- Provider settings UI
- SSH tool v1


**Objective:** 工作区根路径 jail + list/read/write/edit/grep。

**Files:**
- Create: `crates/hakimi-workspace/**`
- Test: path jail 拒绝 `../`

### Task 1.2: Studio Web 三栏布局

**Objective:** 文件树 + Monaco + Chat。

**Files:**
- Create under `hakimi-webui` or `studio-web/`: `WorkspaceLayout.tsx`, `FileTree.tsx`, `EditorPane.tsx`, `ChatPanel.tsx`

### Task 1.3: Provider 设置 UI + 后端

**Objective:** 多 Provider CRUD，密钥不进前端日志。

**Files:**
- Settings API + UI modal
- 复用 `hakimi-config` / transports

### Task 1.4: SSH tool v1

**Objective:** `ssh_exec` 工具 + profile 存储。

**Files:**
- `crates/hakimi-tools/src/builtin_ssh.rs`
- registry 注册
- 单元测试用 mock（不连真机）

### Task 1.5: 会话 resume 列表

**Objective:** UI 列出本地 session 并 attach 历史。

---

## Phase 2 — Multi-device Relay ✅ (2026-07-24 skeleton)

### Task 2.1: `hakimi-hub` 骨架 ✅

**Objective:** Axum 服务：device register、WS fan-out、health。

**Files:**
- `crates/hakimi-hub/**` — binary + lib (`HubState`, `/v1/studio` WS, `/health`)
- `deploy/studio-hub.compose.yml` + `deploy/Dockerfile.hub`

**Done:** `cargo test -p hakimi-hub` 2 passed; hub is relay-only (no tools / no provider keys).

### Task 2.2: 事件窗口与 after_seq ✅

**Objective:** 有界 replay；超窗 reset。

- `EventBus::replay_after` → `ReplayResult::Ok | Gap`
- Gap → `session.reset` event with `last_seq` + `window_oldest_seq`
- Test: `replay_detects_gap_when_window_slides`

### Task 2.3: Runner 注册与 Active Runner ✅

**Objective:** 单 session 单 runner；viewer 只读订阅；controller 可 submit。

- `devices` map on Hello; `devices.list` command
- `session.attach` role → `controllers` set; viewer denied with `viewer_readonly`
- `runner.handoff` updates `active_runner_device_id` + promotes target to controller
- Tests: `multi_device_viewer_cannot_submit`, `devices_list_after_hello`

### Task 2.4: 端到端接力验收 (deferred UI)

**Objective:** 两个浏览器：A 开跑，B 看到流并继续一句。

```bash
cargo run -p hakimi-hub -- --bind 0.0.0.0:3010
# or: docker compose -f deploy/studio-hub.compose.yml up -d
# WS: ws://host:3010/v1/studio  hello → session.create / attach / chat.submit
```

**Remaining Phase 2 polish:**
- ~~Per-connection device identity~~ ✅ `handle_command_as(actor)` on runtime + hub/server WS
- ~~Remote worker protocol~~ ✅ pure-relay mode (`--mode relay` / `HAKIMI_HUB_MODE=relay`)
  - `worker_publish` (worker → hub fan-out)
  - `worker_dispatch` (hub → active runner connection)
  - embedded mode retained for local smoke (`--mode embedded`)
- ~~Studio WebUI multi-device attach UI~~ ✅ Devices strip + handoff + role switch + `?session=&role=`

### Dual-client smoke (2026-07-24) ✅

```bash
cargo run -p hakimi-hub -- --bind 127.0.0.1:3010 --mode embedded
python3 /tmp/hub_smoke.py   # A create+submit, B viewer deny, handoff → B submit
```

Result: `viewer_readonly` enforced per-connection; handoff promotes B; health reports `devices: 2`, `executes_tools: false`.

---

## Phase 3 — Agent Fleet & Ecosystem UI ✅ (shell + cron)

### Task 3.1–3.3 shell ✅

**Files:**
- `hakimi-webui/src/studio/StudioEcosystemPanel.tsx` — Fleet / Skills / MCP cards (reads existing REST)
- Studio status bar **Hub** toggle

### Task 3.4: Cron 管理 UI ✅
- `StudioCronPanel.tsx` + `api.cronJobs/create/pause/resume/run/delete`
- Integrated into ecosystem strip (4-column)

### Task 3.5: worktree 隔离 ✅ (library + docs)
- `hakimi-workspace`: `ensure_worktree` / `list_worktrees` / `remove_worktree`
- Default isolation ON; git worktree preferred, dir fallback
- Doc: `docs/hakimi-studio/WORKTREE.md`

---

## Phase 4 — Desktop Packaging & PWA 🚧

### Task 4.1: Tauri 2 壳 `hakimi-desktop` ✅ (skeleton)
- Crate: `crates/hakimi-desktop`
- Default: embedded Axum backend (WebUI static + `/v1/studio` WS)
- Feature `gui`: Tauri 2 window (requires webkit2gtk-4.1; EL9 has 4.0 only → headless CI)
- `tauri.conf.json` product **Hakimi Studio**, targets deb/appimage/msi/dmg
- Doc: `docs/hakimi-studio/DESKTOP.md`
- Verified: `cargo test -p hakimi-desktop` + `cargo run -- --once`

### Task 4.2: CI 产出 Windows MSI / macOS DMG / Linux AppImage ✅ (workflow)
- Workflow: `.github/workflows/desktop.yml`
- Matrix: ubuntu-22.04 / windows-latest / macos-latest
- Jobs: `headless` (binary + `--once`), `gui` (`--features gui` + `cargo tauri build`), `release-assets` on `v*` tags
- Main CI installs WebKit 4.1 so `--all-features` can compile desktop `gui`
- Trigger: path filters + `workflow_dispatch` + tags
- Doc: `docs/hakimi-studio/DESKTOP.md`

### Task 4.3: WebUI PWA manifest + offline shell ✅
- `public/manifest.webmanifest`, `public/sw.js`
- `index.html` manifest + theme-color
- `main.tsx` registers service worker under `/static/sw.js`

### Task 4.4: 权限档位 + 危险操作确认 ✅
- Server: Controller required for workspace write/create/delete + checkpoint create/restore (when session_id set)
- Client: `dangerConfirm.ts` — typed confirm helpers
- Doc: `docs/hakimi-studio/PERMISSIONS.md`
- Wired: cron delete, runner handoff; restore uses `RESTORE` phrase
- Still open: path allowlists / policy file

---

## Phase 5 — Polish (partial)

### Checkpoint / Rewind ✅ (file snapshots + UI + auto)
- Library: `Workspace::create/list/restore_checkpoint`
- Protocol: `checkpoint_create|list|restore` + events
- Runtime: auto-checkpoint before `workspace_write` / `workspace_delete` (best-effort)
- WebUI: status bar **CP** → `StudioCheckpointPanel` (list/create/restore + typed `RESTORE`)
- Doc: `docs/hakimi-studio/CHECKPOINT.md`
- Still open: full action-graph Rewind, ACP

### Hub production worker client ✅
- `hakimi-server/src/hub_worker.rs` — reconnecting WS client
- Env: `HAKIMI_HUB_URL`, optional `HAKIMI_HUB_TOKEN` / `HAKIMI_HUB_DEVICE_ID`
- Flow: hello(server runner) → `worker_dispatch` → `handle_command_as` → `worker_publish`
- Spawned from `StudioState::with_host` when URL set

### Still open
- Full agent-action Rewind graph
- Configurable path allow/deny globs per session (hardcoded deny list shipped)
- ACP
- 成本面板
- iOS/Android 壳
- 文档站与演示
- Watch Desktop workflow artifacts on first cloud run

---

## 建议默认决策（若用户未特别指定）

| 项 | 默认 |
|----|------|
| 品牌 | Hakimi Studio |
| 执行 | 本机优先 |
| Hub | self-host Docker |
| 并发输入 | 队列 + 可抢占 |
| UI 主路径 | Workspace IDE；Office View 可切换 |
| 协议 | JSON Phase1 |
| 移动 | PWA 先于原生 |

---

## 验收对照 DESIGN §12

实现每阶段结束时勾选 S1–S8 相关项；Phase 2 完成前 **S7 必须绿**。
