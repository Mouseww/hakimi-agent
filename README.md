# 🐙 Hakimi Agent

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.213-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1649-passing?style=for-the-badge&color=brightgreen" alt="Tests">
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
```

---

## Why Hakimi?

Python agent frameworks are slow, memory-hungry, and crash at runtime. Hakimi is built different — from the ground up in Rust with production reliability baked in.

| Metric | Python Agent | Hakimi (Rust) |
|--------|-------------|---------------|
| Startup | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Async model | asyncio + GIL | tokio native async |
| Tool safety | Runtime crashes | Compile-time guarantees |
| Tests | ~500 | 1643 |

**Not a wrapper. Not a demo. A real production system:**
- 20+ error types auto-classified with recovery strategies
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

**Built-in Tools (62+)**
- **Files**: read, write, search, patch with safe-root sandbox
- **Shell**: terminal, background processes
- **Web**: search, extract, browser automation (Chromium with screenshot vision capture and Playwright cache/headless-shell discovery)
- **Desktop**: Hermes-style `computer_use` readiness surface with safe wait, macOS screenshot/list-app discovery, and guarded action schema
- **Code**: Python/JS/Bash execution with sandbox
- **Media**: vision analysis, video analysis, TTS, transcription with silence-hallucination filtering and oversized WAV chunking
- **Memory**: persistent memory + FTS5 full-text search
- **Productivity**: todo, Kanban boards with profile routing, worker logs, event trails, diagnostics, notification subscriptions, swarm graph creation, cron scheduler
- **Meta**: sub-agent delegation, Mixture-of-Agents reasoning, skills system, MCP plugins
- **Evaluation**: Hermes-compatible ShareGPT JSONL trajectory saving for completed and failed turns

**Multi-Platform Gateway**
- Telegram · Discord · Slack · Mattermost · Webhook · Microsoft Graph webhook · Signal · SMS/Twilio · Email/SMTP · WhatsApp Business Cloud · Home Assistant · Matrix · DingTalk · WeCom · Feishu/Lark · BlueBubbles/iMessage · QQBot outbound · WeChat (via iLink/ClawBot) · Weixin/iLink alias
- Config-driven multi-adapter fan-in: run chat and webhook gateways simultaneously
- Real-time streaming with progressive edits, per-platform preview policy, and UTF-8-safe overflow chunking for long replies
- Persistent lifecycle diagnostics record adapter, connect, route, filter, and edit events to `~/.hakimi/logs/gateway-events.log`; `/logs`, `/logs events`, and `/logs gateway` read recent logs without shelling out to `tail`
- Gateway `/undo [N]` rewinds recent in-memory chat turns and echoes the target prompt for editing before resend
- Cron jobs scheduled from chat with `/cron add`
- Gateway `/voice on|off|tts|status|doctor` toggles spoken-response guidance and reports voice I/O readiness without polluting prompt cache or chat history
- TUI `/config [field]` shows sanitized runtime configuration, `/gateway [cmd]` inspects configured adapters, cached channel targets, and lifecycle events, `/sessions [cmd]` browses saved SQLite sessions, `/skills [cmd]` browses/searches local Skills Hub metadata, `/cron [cmd]` manages the persistent cron DB locally, `/undo [N]` prefills recent prompts for editing, `/checkpoints [cmd]` inspects the shared shadow-git checkpoint store without entering the model loop, and `/voice status` plus configurable Ctrl+B/Ctrl+letter push-to-talk share the same `voice.*` config, TTS/transcription tools, audio environment checks, PCM16 WAV recording artifact validation, oversized WAV chunked STT dispatch, local TTS playback launch, recorder-backed `voice_capture`, automatic transcript submission, continuous restart mode, second-press capture cancellation, three-no-speech auto-exit, and Hermes-style start/stop audio cues

**Extensibility**
- MCP (Model Context Protocol) client — stdio / HTTP / SSE transports, CLI/gateway catalog search and config snippets, and stdio server-initiated sampling with tool schema forwarding plus `tool_use` handoff
- HTTP plugin system with YAML templates
- HTTP API discovery — OpenAI-compatible `/v1/models`, `/v1/capabilities`, `/v1/skills`, `/v1/toolsets`, text `/v1/chat/completions` with completed SSE snapshots for `stream=true`, `/v1/responses` with SQLite-backed `previous_response_id` chaining plus completed SSE snapshots, pollable and cancellable `/v1/runs` with live lifecycle SSE events, and session lifecycle/messages/search discovery for external UI feature detection
- Dashboard admin API — `/api/status`, `/api/sessions` create/update/delete/fork plus message/search inspection, `/api/mcp/servers`, `/api/credentials/pool`, and `/api/webhooks` expose redacted operational state plus runtime-scoped admin writes for WebUI/admin panels
- Skills Hub — install community skills with `/skills install`
- Static i18n foundation — `display.language`, `HAKIMI_LANGUAGE` / `HERMES_LANGUAGE`, Hermes-compatible language aliases, YAML catalog directory loading, English fallback, and named placeholders for static user-facing messages
- CLI Skin Engine — `hakimi skin list|inspect|set|path` plus gateway `/skin` discover built-in and `~/.hakimi/skins/*.yaml` themes, inherit missing values from `default`, and persist `display.skin`
- Isolated profiles — manage named workspaces, clone/export profile archives, install/update shareable `distribution.yaml` profile distributions, create `~/.hakimi/bin/<profile>` wrapper aliases, and use gateway `/profile`
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
├── hakimi-transports/    # OpenAI, Anthropic, Gemini transports + prompt caching
├── hakimi-tools/         # 62+ built-in tools + plugin registry
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
| Tests | ~500 | 1643 |

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
- [x] OpenAI / Anthropic / Gemini transports + SSE streaming
- [x] 62+ built-in tools
- [x] 19 runtime-exposed platform adapters
- [x] Gateway target directory + send_message channel resolution
- [x] MCP client + CLI/gateway server catalog
- [x] HTTP API model/capability discovery + text Chat Completions/Responses SSE snapshots + cancellable Runs with live lifecycle events
- [x] Dashboard admin API summaries + runtime writes
- [x] Plugin system + HTTP templates
- [x] Profile distributions with install/update/info and protected user data
- [x] CLI skin engine with built-in/user YAML themes and `display.skin` persistence
- [x] ratatui TUI with local slash commands, sanitized config browser, and gateway status panel
- [x] Smart context compression (3-tier)
- [x] Error classifier + credential pool
- [x] Prompt caching (Anthropic)
- [x] Vision + video analysis
- [x] Knowledge graph memory
- [x] Intent reasoning engine
- [x] Decision tree backtracking
- [x] Role adaptation
- [x] Meta-skill auto-extraction
- [x] Browser automation (Chromium + Playwright cache discovery)
- [x] Computer Use readiness surface
- [x] Kanban task boards + notification cursors + swarm graphs
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
- [ ] WASM plugin runtime
- [ ] Web dashboard

---

## License

MIT License
