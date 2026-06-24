# Hakimi Agent

[![Version](https://img.shields.io/badge/version-0.4.2-blue.svg)](https://github.com/Mouseww/hakimi-agent/releases)
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.4.2-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1769-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/lines-44K+-orange?style=for-the-badge" alt="Lines">
</p>

<p align="center">
  <strong>Production-grade AI Agent framework, rewritten in Rust for speed and reliability</strong><br>
  <sub>Inspired by <a href="https://github.com/NousResearch/hermes-agent">Nous Research's Hermes Agent</a> — built from the ground up in Rust</sub>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#why-hakimi">Why Hakimi</a> ·
  <a href="#capabilities">Capabilities</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#compare">Compare</a> ·
  <a href="README_CN.md">中文</a>
</p>

---

## Install

**Linux / macOS:**
```bash
curl -sSL https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.ps1 | iex
```

**Build from source (any platform with Rust):**
```bash
cargo install hakimi-agent
```

**Quick setup:**
```bash
hakimi setup      # guided configuration wizard
hakimi doctor     # diagnose setup and connectivity
hakimi --serve    # start the embedded WebUI/API on 127.0.0.1:3005
```

**v0.3.282 — 智能上下文压缩优化 (Smart Context Compression Tuning):**
- 🗜️ **更早触发压缩**：压缩阈值从 70% 降低到 60%，避免接近 token 限制时才压缩
- 📉 **更激进的压缩策略**：
  - Tier 1 (Drop Tool Results): 保留 50% 最近消息（原 60%）
  - Tier 2 (Summarize Old Turns): 保留 30% 最近消息（原 40%）
  - Tier 3 (Sliding Window): 保留最近 10 条消息（不变）
- 🎯 **优化压缩梯度**：
  - Tier 1 触发: 60-75% 占用（原 70-85%）
  - Tier 2 触发: 75-90% 占用（原 85-95%）
  - Tier 3 触发: >90% 占用（原 >95%）
- ✅ **效果**：显著降低 context overflow 概率，为长对话腾出更多空间

**v0.3.277 — WebUI 流式输出优化 + 工具调用显示完善 (Streaming & Tool Display Polish):**
- 🚀 **真正的实时流式输出**：流式阶段使用纯文本显示（`textContent`），消除 Markdown 重复渲染导致的闪烁
- 🛠️ **工具调用完全隐藏**：预判 `incompleteLine` 中的工具调用前缀（`hakimi_tool:`、表情符号），永不显示在消息中
- 🎨 **完成后格式化**：流式结束时才渲染 Markdown，既快速又美观
- 📊 **性能提升**：减少 DOM 操作 90%+，流式输出更流畅
- 🔧 **底部状态栏**：工具调用状态仅显示在底部状态栏（紧凑单行，最多 3 条）
- ✅ **用户体验提升**：无闪烁、无"工具调用行出现后消失"现象

**v0.3.276 — WebUI Gateway 控制面板完整实现 (Gateway Control Panel Complete):**
- 🎨 **完整前端界面**：WebUI 新增"网关"标签页，提供完整的 Gateway 可视化管理
- 📊 **实时状态监控**：显示网关运行状态、已连接平台（Telegram、ClawBot）和连接详情
- ⚙️ **配置实时修改**：可在 WebUI 中直接修改网关配置（忙碌模式 queue/interrupt）
- 🔄 **一键重启**：提供安全的网关重启功能（通过 systemd），自动重连
- ✅ **功能验证完成**：Gateway 面板已完整实现，可在浏览器中管理所有网关功能

**v0.3.275 — 消息处理逻辑统一重构 (Message Processing Unification):**
- 🔄 **代码重构**：提取 `process_gateway_messages_loop()` 函数（1229 行），消除重复代码
- 🎯 **统一逻辑**：分离模式和统一模式共享相同的消息处理逻辑
- 📉 **代码简化**：entry.rs 从 10580 行减少到 10510 行，消除 1200+ 行重复代码
- ✅ **功能完整**：所有命令、Agent 调用、流式响应、工具调用状态更新全部保留
- 🛡️ **可维护性提升**：未来修复和功能添加只需修改一处代码

**v0.3.274 — 统一模式成为默认行为 (Unified Mode by Default):**
- 🚀 **默认统一模式**：`hakimi` 命令现在默认启动统一模式（WebUI + Gateway），无需额外参数
- 🔄 **重启功能实现**：WebUI Gateway 面板的重启按钮现在可用（通过 systemd 重启）
- ⚙️ **服务配置更新**：systemd 服务配置简化为 `hakimi --addr 127.0.0.1:3005`（自动启用统一模式）
- 📦 **向后兼容**：`--serve` 和 `--gateway` 参数仍可用于显式控制模式
- 🎯 **用户体验提升**：新用户无需理解复杂的模式选项，直接运行 `hakimi` 即可获得完整功能
- 🔧 **部署简化**：单个 systemd 服务 `hakimi.service` 替代原 `hakimi-unified.service`

**v0.3.273 — Phase 4: WebUI Gateway 管理界面 (Gateway Management UI):**
- 🎨 **完整前端界面**：新增 Gateway 标签页，提供可视化管理面板
- 📊 **状态监控面板**：实时显示 Gateway 运行状态、已连接平台、消息计数
- ⚙️ **配置编辑器**：可视化编辑繁忙模式、访问控制、用户白名单、过滤选项
- 💾 **一键保存**：配置修改自动持久化到 `~/.hakimi/config.yaml`
- 🔄 **重启控制**：WebUI 内一键重启 Gateway（统一模式）
- 🎯 **实时刷新**：手动刷新按钮获取最新 Gateway 状态
- 🖼️ **深色主题**：UI 样式与现有 WebUI 风格完全一致（268 行组件 + 300 行 CSS）
- ✅ **TypeScript 全栈**：前后端类型安全，完整的 API 类型定义
- 📦 **生产就绪**：前端已构建并集成到 WebUI，立即可用

**v0.3.272 — Phase 3C: WebUI Gateway 配置 API (Gateway Control Backend):**
- 🎯 **Gateway 配置 API**：实现 4 个 REST 端点供 WebUI 控制 Gateway
- 📡 **状态监控**：`GET /api/gateway/status` 获取运行状态、平台连接、消息计数
- ⚙️ **配置读写**：`GET /api/gateway/config` 和 `PATCH /api/gateway/config` 管理配置
- 🔄 **重启控制**：`POST /api/gateway/restart` 重启 Gateway（仅统一模式）
- 🔒 **认证保护**：所有 Gateway API 端点需要 Bearer Token 认证
- 💾 **配置持久化**：PATCH 更新自动保存到 `~/.hakimi/config.yaml`
- ✅ **测试验证**：所有端点通过 curl 测试（分离模式 + 统一模式）
- 🏗️ **下一步 Phase 4**：WebUI 前端界面（连接状态监控、配置编辑器、日志查看）

**v0.3.271 — Critical Fix: 修复 Gateway 幽灵任务 Bug (Phantom Task Fix):**
- 🐛 **修复幽灵任务**：修复 `active_tasks` 清理逻辑导致的"正在处理之前的消息"错误提示
- 🔧 **根本原因**：任务完成时的 `task_id` 匹配条件在并发场景下可能失败，导致记录永久残留
- 🎯 **修复方案**：重写清理逻辑，嵌套 `if` 代替 `let chains` 确保兼容性和正确性
- ✅ **影响范围**：所有使用 Gateway 的用户（Telegram Bot、Discord Bot 等）
- 🔒 **稳定性提升**：消除了导致 Gateway 假死的关键 bug

**v0.3.270 — Phase 2: Gateway 完整集成到统一服务器 (Full Gateway Integration):**
- 🎯 **Gateway 完整集成**：`hakimi --serve --gateway start` 现在完全整合 Gateway 和 WebUI
- 🔄 **共享资源**：Agent、SessionDB、Config 在 WebUI 和 Gateway 间共享（`Arc<Mutex<T>>`）
- 🚀 **并行运行**：Gateway 消息循环和 WebUI HTTP 服务器在独立 tokio 任务中运行
- 📡 **平台连接**：统一模式下 Gateway 自动连接所有配置的平台（Telegram、ClawBot 等）
- 🔒 **进程独占锁**：Gateway lock 文件防止多实例冲突
- 📤 **出站消息处理**：后台任务自动处理工具调用的出站消息队列
- ⚙️ **基础架构就绪**：为 Phase 3（Gateway 控制 API）铺路
- 🏗️ **TODO Phase 3**：Gateway 消息处理循环（目前为占位符，接收但不处理消息）

**v0.3.269 — Phase 1: 统一服务器架构 (Unified Server Architecture):**
- 🏗️ **统一模式**：`hakimi --serve --gateway start` 启动合并进程（WebUI + Gateway）
- 🎯 **向后兼容**：`--serve` 和 `--gateway` 单独使用时保持原有行为
- 📦 **共享资源基础**：扩展 `AppState` 结构，准备共享 Agent/SessionDB/Config
- 🚀 **MVP 发布**：统一模式当前运行 WebUI（Gateway 集成后续完善）
- 🎨 **CLI 参数优化**：添加统一模式文档，明确 `--gateway` 与 `--serve` 组合语义
- 🔧 **依赖更新**：`hakimi-server` 添加 `hakimi-gateway` 依赖，为后续集成铺路

**v0.3.268 — 修复 /clear 命令的多用户 bug (Critical Security Fix):**
- 🔐 **安全修复**：移除 `/clear` 命令中的 `agent.clear_messages()` 调用
- 🐛 **问题诊断**：Gateway 使用共享 Agent 架构，`agent.clear_messages()` 会影响所有聊天
- ✅ **修复方案**：只清空当前聊天的 `histories` 和 `usage` 数据，不操作全局 Agent
- 📝 **代码注释**：添加详细说明，防止未来重复引入此问题
- 🎯 **用户隔离**：保证不同用户/聊天的对话历史完全隔离

**v0.3.267 — 修复 /stop 命令队列清空功能:**
- 🛑 **队列清空**：`/stop` 命令现在会同时清空消息队列
- 📊 **详细反馈**：显示清空了多少条排队消息
- 🎯 **用户体验**：解决 queue 模式下 `/stop` 后队列消息继续执行的问题

**v0.3.266 — 消息队列功能 (参考 Hermes Agent):**
- 📬 **Queue 模式**：新消息加入队列，任务按顺序执行（默认）
- ⚡ **Interrupt 模式**：新消息立即中断当前任务（向后兼容）
- 🎛️ **配置项**：`gateways.busy_input_mode` (`"queue"` | `"interrupt"`)
- 💬 **用户提示**：排队时显示友好消息
- 🔄 **架构对齐**：参考 Hermes Agent 的并发处理机制

**v0.3.265 — 修复上下文压缩通知格式问题 (Hotfix):**
- 🐛 **修复 Unicode 转义**：`\\u{001e}` → `\u{001e}`，Telegram 现在能正确识别消息前缀
- 📦 **独立消息框**：压缩开始和完成通知现在分别显示在独立消息框中
- ✅ **完整格式**：`\u{001e}hakimi_tool:🗜️ ...` 与工具调用通知保持一致
- 🎯 **用户反馈**：感谢用户报告格式问题，快速修复部署

**v0.3.264 — 上下文压缩实时通知 (Context Compression Notifications):**
- 🗜️ **主动压缩通知**：当上下文接近限制时（`should_compress()` 触发），通过 `streaming_callback` 实时通知用户"正在自动压缩"
- ⚠️ **溢出恢复通知**：API 返回 `context_length_exceeded` 错误时，通知用户"上下文溢出，正在压缩并重试"
- ✅ **完成反馈**：压缩完成后发送"压缩完成，继续/重试任务"，让用户清楚任务未中断
- 🎯 **用户体验提升**：杜绝长任务假死错觉 — 用户始终知道 Agent 在做什么
- 📍 **实现位置**：`loop_impl.rs` 第 159-173 行（主动压缩）+ 196-214 行（溢出恢复）

**v0.3.263 — 修复 Router index=1 工具调用导致的空占位符 bug:**
- 过滤掉流式累积产生的空工具调用（某些 Router 后端从 index=1 开始计数）
- `process_tool_calls` 在处理前移除 `name.is_empty()` 的占位符工具调用
- 增强调试日志：显示原始/有效工具调用数量和详细信息
- 修复 `WebuiConfig` Clippy 警告（使用 `derive(Default)` 替代手动实现）
- 彻底解决"思考循环"：空工具名不再触发 guardrail 警告或错误提示

**v0.3.262 — SSE 格式自动检测:**
- `ChatCompletionsTransport` 使用 `SseEventStream::auto()` 实现自动格式检测
- 首个 SSE 事件动态决定解析器模式（Anthropic/OpenAI/Gemini）
- 消除因 `api_mode: chat_completions` 配置错误导致的"思考循环"
- Router 端点返回 Anthropic Messages API 格式时无缝工作

**v0.3.261 — WebUI password + session workdir binding:**
- WebUI password can now be configured in `config.yaml` (`webui.password`) or via Settings panel
- Sessions table extended with `workdir` field for workspace-session binding
- Password defaults to config, falls back to `HAKIMI_WEBUI_PASSWORD` env var
- All API routes protected by Bearer token middleware (empty password = no auth)

The WebUI Control Center can create, pause, resume, run-now, and delete persisted cron jobs via `/api/cron/jobs`, the `/clear` slash command now persists by deleting the current session transcript via `/api/sessions/{id}/messages`, and the mobile layout lets the conversation title toggle the session list so phones keep the chat area usable.

`hakimi --serve` ships the WebUI assets inside the release binary, so `/`, `/static/style.css`, `/static/hakimi.js`, `/static/composer.js`, `/static/workspace.js`, and `/static/favicon.svg` work from any current directory without copying a separate `static/` folder. The WebUI workspace browser treats `/` as the active working-directory root (not the OS filesystem root), while still rejecting `..` path escapes. Control-center modals honor native `hidden` state and can be dismissed via close button, overlay click, or Escape. When `HAKIMI_WEBUI_PASSWORD` is set, the WebUI prompts for the password on the first authenticated API call, stores it locally as a Bearer token, retries automatically, and renders send/auth errors inline instead of silently dropping messages. The embedded server persists WebUI sessions in `~/.hakimi/sessions.db` and initializes the schema on startup, so creating a chat session works immediately after launch. Streaming WebUI chat requests now carry the active `session_id`, restore that transcript before each turn, and persist both the user prompt and assistant reply back into the same session; the frontend also commits finalized streamed replies into its in-memory message list so a second send or session switch does not erase the previous response. The WebUI also exposes persisted cron jobs through `/api/cron/jobs`, supports session deletion from the sidebar, de-duplicates client-provided session titles during create/fork so repeated "New Chat" actions do not hit the SQLite title uniqueness constraint, and ships a polished skin system with Linear Dark, Obsidian, Midnight, Light, and System appearance choices persisted in localStorage. Theme switching now writes the resolved skin CSS variables directly at runtime, so color changes are immediate and resilient to cached/static stylesheet ordering. The refreshed UI uses glassy panels, richer message cards, focused composer chrome, theme swatches in Settings → Appearance, keeps theme switching local and instant, adds a mobile drawer sidebar with a compact top-bar menu, hides the workspace panel on narrow screens, and adapts messages/composer controls to safe-area mobile viewports. It also shows directory-style `SKILL.md` skills using their parent directory names instead of generic `SKILL` labels, and releases the composer immediately after completed streamed replies so follow-up messages remain responsive.

---

## Why Hakimi?

Python agent frameworks are slow, memory-hungry, and crash at runtime. Hakimi is built different — from the ground up in Rust with production reliability baked in.

| Metric | Python Agent | Hakimi (Rust) |
|--------|-------------|---------------|
| Startup | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Async model | asyncio + GIL | tokio native async |
| Tool safety | Runtime crashes | Compile-time guarantees |
| Tests | ~500 | 1767 |

**Not a wrapper. Not a demo. A real production system:**
- 20+ error types auto-classified with recovery strategies
- Hermes-style turn retry state for one-shot recovery guards
- Multi-key credential pool with circuit breakers
- 3-tier context compression (no manual window management)
- Contextual first-touch onboarding hints tracked in `onboarding.seen`
- Decision tree conversation history with backtracking
- Intent reasoning engine — predicts what tools you need
- Role adaptation — automatically switches between Coder, Researcher, Writer modes

---

## Capabilities

### 🌟 Core Features

**Smart Context Management**
- Three-tier compression: drop stale tool results → LLM summarization → sliding window
- No manual context window management — Hakimi handles it automatically
- Model-aware context windows: `model.context_length` overrides static metadata before compression and tool disclosure thresholds
- Intent classification into 10 categories with next-tool prediction

**Built-in Tools (63+)**
- **Files**: read, write, search, patch with safe-root sandbox
- **Shell**: terminal, background processes
- **Web**: search, extract, browser automation (Chromium with screenshot vision capture, Playwright cache/headless-shell discovery, raw CDP dispatch, CDP frame-tree inspection, cloud-provider readiness status, and provider CDP endpoint routing)
- **Desktop**: Hermes-style `computer_use` readiness surface with safe wait, macOS screenshot/list-app discovery, and guarded action schema
- **Code**: Python/JS/Bash execution with sandbox
- **Media**: vision analysis, video analysis, TTS, transcription with silence-hallucination filtering and oversized WAV chunking
- **Memory**: persistent memory + FTS5 full-text search + `hakimi knowledge` / TUI `/knowledge` / gateway `/knowledge` graph operations
- **Productivity**: todo, Kanban boards with profile routing, worker logs, event trails, diagnostics, notification subscriptions, swarm graph creation, dashboard read/write management, cron scheduler with interval/five-field cron expressions and home-channel fan-out delivery
- **Meta**: sub-agent delegation, Mixture-of-Agents reasoning, skills system, MCP plugins
- **Evaluation**: Hermes-compatible ShareGPT JSONL trajectory saving for completed and failed turns

**Multi-Platform Gateway**
- Telegram · Discord · Slack · Mattermost · Webhook · Microsoft Graph webhook · Signal · SMS/Twilio · Email/SMTP · WhatsApp Business Cloud · Home Assistant · Matrix · DingTalk · WeCom · Feishu/Lark · BlueBubbles/iMessage · QQBot outbound · WeChat (via iLink/ClawBot) · Weixin/iLink alias
- Config-driven multi-adapter fan-in: run chat and webhook gateways simultaneously
- Real-time streaming with progressive edits, native Telegram draft previews, flood-control backoff, per-platform preview policy, and UTF-8-safe overflow chunking for long replies
- Persistent lifecycle diagnostics record adapter, connect, route, filter, and edit events to `~/.hakimi/logs/gateway-events.log`; `/logs`, `/logs events`, and `/logs gateway` read recent logs without shelling out to `tail`
- Gateway `/undo [N]` rewinds recent in-memory chat turns and echoes the target prompt for editing before resend
- Gateway `/stop` immediately cancels the running task and clears any queued messages, supporting both `interrupt` and `queue` modes configured via `gateways.busy_input_mode`
- Gateway `/usage` shows last-turn token/cost/rate-limit data, best-effort OpenRouter-compatible `/v1/models` live pricing with a profile-scoped freshness cache and request fees, OpenRouter `/credits` plus `/key` quota/usage, Anthropic OAuth account windows, Codex usage windows, and a shared Nous rate-limit guard without exposing credentials
- Cron jobs scheduled from chat with `/cron add`, including `30m` / `2h` intervals, five-field cron syntax such as `*/15 * * * *` or `0 9 * * MON-FRI`, and delivery targets like `local`, `origin`, `all`, `platform`, `platform:home`, or `platform:#channel`
- Gateway `/voice on|off|tts|status|doctor` toggles spoken-response guidance and reports voice I/O readiness without polluting prompt cache or chat history
- Gateway `/update` sends the in-chat restart notice, then the restarted gateway proactively reports update success, current version, and release-note feature bullets after adapters connect
- TUI `/config [field]` shows sanitized runtime configuration, `/gateway [cmd]` inspects configured adapters, cached channel targets, and lifecycle events, `/sessions [cmd]` browses saved SQLite sessions, `/skills [cmd]` browses/searches local Skills Hub metadata, `/cron [cmd]` manages the persistent cron DB locally, `/undo [N]` prefills recent prompts for editing, `/checkpoints [cmd]` inspects the shared shadow-git checkpoint store without entering the model loop, and `/voice status` plus configurable Ctrl+B/Ctrl+letter push-to-talk share the same `voice.*` config, TTS/transcription tools, audio environment checks, PCM16 WAV recording artifact validation, oversized WAV chunked STT dispatch, local TTS playback launch, recorder-backed `voice_capture`, automatic transcript submission, continuous restart mode, second-press capture cancellation, three-no-speech auto-exit, and Hermes-style start/stop audio cues

**Extensibility**
- MCP (Model Context Protocol) client — stdio / HTTP / SSE transports, CLI/gateway catalog search and config snippets, and stdio server-initiated sampling with tool schema forwarding plus `tool_use` handoff
- HTTP plugin system with YAML templates
- HTTP API discovery — OpenAI-compatible `/v1/models`, `/v1/capabilities`, `/v1/skills`, `/v1/toolsets`, text `/v1/chat/completions` with completed SSE snapshots for `stream=true`, `/v1/responses` with SQLite-backed `previous_response_id` chaining plus completed SSE snapshots, pollable and cancellable `/v1/runs` with live lifecycle SSE events, and session lifecycle/messages/search discovery for external UI feature detection
- Dashboard admin API — `/api/status`, `/api/sessions` create/update/delete/fork plus message/search inspection, `/api/mcp/servers`, `/api/credentials/pool`, `/api/webhooks`, and Kanban `/api/kanban` board/task read-write management expose redacted operational state plus runtime-scoped admin writes for WebUI/admin panels
- Hakimi WebUI — Hermes-inspired React/Vite operator console with left-side session browsing, central `/api/chat` live turns, right-side runtime/tool/skill/control panels, Bearer token support, and runtime config editing through the existing HTTP API
- Skills Hub — install community skills with `/skills install`
- Static i18n foundation — `display.language`, `HAKIMI_LANGUAGE` / `HERMES_LANGUAGE`, Hermes-compatible language aliases, YAML catalog directory loading, English fallback, and named placeholders for static user-facing messages
- CLI Skin Engine — `hakimi skin list|inspect|set|path` plus gateway `/skin` discover built-in and `~/.hakimi/skins/*.yaml` themes, inherit missing values from `default`, persist `display.skin`, apply selected branding/colors/logo/hero to the CLI startup banner, and drive TUI thinking spinner faces/verbs/wings plus status, session, selection, completion, help, input, response, tool-prefix, tool emoji labels, running-tool progress, and tool-panel colors
- Isolated profiles — manage named workspaces, clone/export profile archives, install/update shareable `distribution.yaml` profile distributions, create `~/.hakimi/bin/<profile>` wrapper aliases, use gateway `/profile`, and bind `--profile` / sticky `active_profile` runs to profile-scoped config, memory, sessions, skills, cron, trajectories, gateway logs, and TUI defaults
- 10 curated MCP catalog entries: GitHub, filesystem, Brave Search, PostgreSQL, Puppeteer, memory, fetch, SQLite, sequential-thinking, and the Hermes-reviewed n8n bridge

### 🛡️ Production Safety

- **Secret redaction** — API keys, JWTs, tokens masked before output
- **Prompt injection detection** — scans skills, cron prompts, context files
- **SSRF protection** — blocks private/metadata URL fetches
- **Command safety guard** — blocks malicious shell patterns
- **Tool loop guardrails** — warns on repeated no-progress read-only calls and blocks runaway exact-call loops
- **One-time onboarding hints** — first-touch CLI/gateway tips persist under `onboarding.seen`
- **Write safe-root sandbox** — config-protected directories
- **Read credential guard** — protects config files
- **Shared shadow-git checkpoints** — `checkpoint` and gateway `/checkpoints` snapshots live under `~/.hakimi/checkpoints/store`, not the project `.git`
- **Tool output limits** — configurable `tools.output.max_bytes` boundary before tool results enter context

---

## Architecture

**20 crates, each with a single responsibility:**

```
hakimi-agent/
├── hakimi-core/          # Agent loop, error classifier, credential pool
├── hakimi-transports/    # OpenAI, Anthropic, Gemini, Bedrock transports + prompt caching/rate guards
├── hakimi-tools/         # 63+ built-in tools + plugin registry
├── hakimi-session/       # SQLite WAL + FTS5, decision tree history
├── hakimi-context/       # Context engine, compression, intent reasoning, roles
├── hakimi-knowledge/    # Knowledge graph (petgraph)
├── hakimi-skills/        # Skill system + meta-skill extraction
├── hakimi-cron/          # Persistent cron scheduler
├── hakimi-gateway/       # 19 runtime-exposed platform adapters
├── hakimi-mcp/           # MCP client (stdio/HTTP/SSE)
├── hakimi-cli/           # REPL CLI + setup wizard + doctor
└── hakimi-tui/           # ratatui terminal UI
```

### How It Works

```
User Message
    │
    ▼
┌─────────────────────────────────────┐
│  Intent Classification               │
│  → Which tools does this need?      │
├─────────────────────────────────────┤
│  Role Adaptation                     │
│  → Coder / Researcher / Writer...   │
├─────────────────────────────────────┤
│  Build Context                       │
│  → System prompt + knowledge graph  │
│  → Apply 3-tier compression         │
├─────────────────────────────────────┤
│  Credential Pool                     │
│  → Acquire API key (rotation-ready) │
│  → LLM call via SSE streaming       │
├─────────────────────────────────────┤
│  Tool Dispatch                       │
│  → Execute + guardrails check       │
│  → Error classification + recovery │
├─────────────────────────────────────┤
│  Decision Tree                       │
│  → Record response + backtrack Capable│
└─────────────────────────────────────┘
    │
    ▼
Response + Memory + Stats
```

---

## Compare

| Feature | Hermes (Python) | Hakimi (Rust) |
|---------|-----------------|---------------|
| Language | Python 3.11+ | Rust 2024 |
| Startup time | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Async model | asyncio + GIL | tokio native |
| Tool registration | Runtime AST | Compile-time trait |
| Error recovery | Basic retry | 20+ classifiers |
| Knowledge model | Flat file | Graph DB (petgraph) |
| Intent detection | None | 10-category classifier |
| Role adaptation | None | 8 roles auto-detected |
| Conversation model | Flat list | Decision tree |
| Tests | ~500 | 1767 |

---

## Development

```bash
# Build everything
cargo build --workspace

# Run tests
cargo test --workspace

# Lint
cargo clippy --workspace

# Debug logging
RUST_LOG=debug cargo run -p hakimi-cli
```

---

## Roadmap

- [x] Core agent loop + tool dispatch
- [x] OpenAI / Anthropic / Gemini transports + SSE streaming, plus non-streaming AWS Bedrock Converse
- [x] 63+ built-in tools
- [x] 19 runtime-exposed platform adapters
- [x] Gateway target directory + send_message channel resolution
- [x] MCP client + CLI/gateway server catalog
- [x] HTTP API model/capability discovery + text Chat Completions/Responses SSE snapshots + cancellable Runs with live lifecycle events
- [x] Dashboard admin API summaries + runtime writes + Kanban read/write management
- [x] Gateway `/usage` rate-limit, account-limit, live pricing with request fees, Nous shared rate guard, and offline OpenAI/Anthropic/Gemini/DeepSeek/MiniMax/Bedrock cost estimates
- [x] Plugin system + HTTP templates
- [x] Profile distributions with install/update/info and protected user data
- [x] CLI skin engine with built-in/user YAML themes, `display.skin` persistence, startup banner theming, and TUI spinner, status, completion, help, tool emoji/progress, and surface theming
- [x] ratatui TUI with local slash commands, sanitized config browser, and gateway status panel
- [x] Smart context compression (3-tier)
- [x] Error classifier + credential pool
- [x] Prompt caching (Anthropic)
- [x] Vision + video analysis
- [x] Knowledge graph memory with CLI/TUI/gateway operator commands
- [x] Intent reasoning engine
- [x] Decision tree backtracking
- [x] Role adaptation
- [x] Meta-skill auto-extraction
- [x] Browser automation (Chromium + Playwright cache discovery + CDP readiness probe + frame-tree inspection + cloud-provider readiness status + provider CDP endpoint routing)
- [x] Computer Use readiness surface
- [x] Kanban task boards + notification cursors + swarm graphs + dashboard read/write management
- [x] Gateway voice-response mode
- [x] TUI voice readiness and media-tool config parity
- [x] Voice environment diagnostics and STT silence-hallucination filtering
- [x] PCM16 WAV recording artifact validation for voice capture
- [x] Voice TTS playback text cleanup, MP3 cache planning, and local player launch
- [x] Voice capture tool with system recorder backends and STT dispatch
- [x] Oversized WAV chunking for captured-recording STT dispatch
- [x] TUI Ctrl+B continuous push-to-talk capture loop
- [x] TUI checkpoint viewer and manager slash command
- [x] TUI saved session browser slash command
- [x] TUI skill browser slash command
- [x] TUI cron job manager slash command
- [x] Voice capture second-press interrupt key
- [x] Voice capture start/stop audio cues
- [x] Voice capture continuous restart mode
- [x] Mixture-of-Agents reasoning via OpenRouter
- [x] OpenRouter, Anthropic, and Codex account usage display in gateway `/usage`
- [x] Basic Hakimi WebUI operator console
- [ ] WASM plugin runtime
- [ ] Web dashboard PTY terminal, session-scoped streaming, and full Kanban UI

---

## Changelog

### v0.3.266 (2026-06-18)
**消息队列功能** - 参考 Hermes Agent 实现并发消息处理

- 🎯 **新增配置项** `gateways.busy_input_mode`（默认 `"queue"`）
  - `"queue"` 模式：新消息排队等待当前任务完成
  - `"interrupt"` 模式：立即取消当前任务（原有行为）
- 🔄 **队列处理机制**：任务完成后自动处理队列中的下一条消息
- 💬 **用户友好提示**：队列模式下显示 "⏳ 正在处理之前的消息，已将新消息加入队列..."
- 🧩 **无缝集成**：支持文本和媒体附件的队列化处理
- 🐛 **向后兼容**：可通过配置切换回 interrupt 模式

**技术细节**：
- 添加 `QueuedMessage` 结构体和 per-chat 队列存储
- 实现基于 `task_key` 的消息队列管理（`platform:bot_id:chat_id`）
- 任务完成后通过 `gateway.route_message()` 递归触发下一条消息

---

## License

MIT License
