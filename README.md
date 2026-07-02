<h1 align="center">Hakimi Agent</h1>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.5.10-blue?style=for-the-badge" alt="Version">
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
<img width="1916" height="958" alt="AnythingAgentRecord" src="https://github.com/user-attachments/assets/64c1e6bb-2835-4a27-9e6c-fd5f49618695" />

<img width="1160" height="896" alt="image" src="https://github.com/user-attachments/assets/713b3a8f-1d5a-40bb-9e9f-7b771869ed12" />

---

## ✨ Recent Updates (v0.5.10)

**WebUI Chat Experience Enhanced:**
- ✅ **Tool Call Visualization** — Every tool execution now displays prominently in chat history with collapsible results
- ✅ **Fixed Content Overwrite** — Streaming responses no longer get replaced by final message, preserving complete conversation flow
- ✅ **Interactive Tool Results** — Click to expand/collapse tool outputs (file reads, searches, API calls) with syntax highlighting
- ✅ **Real-time Progress** — Live updates as tools execute, with clear visual separation from assistant responses
- 🎨 **Refined UI** — Smooth animations, better spacing, and improved readability for long conversations

**Example:** When you ask "analyze this codebase", you'll now see each file search, code analysis tool, and their outputs as separate expandable cards — no more mystery about what the agent is doing!

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

**v0.4.7 — 上下文管理优化 (Context Management Enhancement):**
- 🔄 **队列消息注入修复**：修复运行中上下文的排队消息注入逻辑，确保多消息场景下的正确处理
- 🗜️ **压缩标志重置**：上下文压缩后正确重置 `compressed_this_turn` 标志，避免重复压缩
- 🧹 **代码质量提升**：消除 entry.rs 中未使用变量和死代码警告，应用 rustfmt 格式化
- 🎯 **Agent 循环增强**：优化 loop_impl.rs 中的消息处理流程，提升稳定性

**v0.4.6 — 人格办公室仪表板 (Persona Office Dashboard):**
- 🏢 **办公室可视化**：把每个人格当作"员工"，实时展示所有人格的工作状态
- 🖥️ **个性化工位**：每个人格独立工位，执行任务时电脑屏幕亮起 + 键盘动作，空闲时看电视/打游戏
- 🤝 **协作动画**：A 找 B 干活时显示跑到 B 处交付需求的动画，多人组队时聚坐协作
- 📡 **实时事件流**：后端 ActivityHub + SSE 全栈实时推送（PersonaCreated/TurnStarted/TeamConsult/Idle 等）
- 🎨 **扁平矢量风格**：SVG + CSS 动画，微俯视角，可随主题换色，与现有 UI 风格统一
- 🔄 **自动布局**：按行自动排列工位，支持几个到 ~20 个人格，超出自动滚动
- 🖱️ **可交互导航**：点击工位进入该人格对话/配置，悬停显示状态详情卡
- 👔 **入职动画**：新人格创建时显示"新员工入职，安排新座位"动画

**v0.4.5 — Persona Team 协作系统 (Persona Team Collaboration):**
- 🤝 **具名人格协作**：主导人格可通过 `team` 工具将子任务委派给其他具名队友人格
- 🎯 **专业化分工**：每个队友使用自己的模型、技能、记忆和系统提示词独立作答
- 📋 **队友名册管理**：`team(action="list")` 枚举所有可寻址队友及其能力描述
- 🔒 **安全护栏**：内置深度上限、回环检测、并发信号量、超时预算机制
- ⚙️ **可配置开关**：`PersonaConfig.addressable` 控制人格是否可被当作队友（默认开启）
- 🔄 **同步无状态**：队友按子任务起干净回合，只读长期记忆，不写回自身会话/记忆
- 📊 **进度可视化**：复用现有 `hakimi_delegate:` 气泡机制，实时展示协作进度
- ✅ **WebUI 集成**：人格配置表单中的 `addressable` 开关已完整实现

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

## License

MIT License
