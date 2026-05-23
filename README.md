<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.29-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1035-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/lines-44K+-orange?style=for-the-badge" alt="Lines">
</p>

<h1 align="center">🐙 Hakimi Agent</h1>

<p align="center">
  <b>A Rust-native AI Agent framework — 40x faster startup, 90% less memory than Python</b><br>
  <sub>Production-grade architecture from <a href="https://github.com/NousResearch/hermes-agent">Nous Research's Hermes Agent</a>, rewritten from scratch in Rust</sub>
</p>

<p align="center">
  <a href="#install">Install</a> •
  <a href="#overview">Overview</a> •
  <a href="#capabilities">Capabilities</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#benchmark">Benchmark</a> •
  <a href="#roadmap">Roadmap</a> •
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

**Any platform (with Rust):**
```bash
cargo install hakimi-agent
```

After install, run the interactive setup wizard:

```bash
hakimi --setup
```

The wizard walks you through LLM provider, API key, model, platform adapters, and MCP server configuration — all saved to `~/.hakimi/config.yaml`.

---

## Overview

Hakimi is a Rust rewrite of [Hermes Agent](https://github.com/NousResearch/hermes-agent) — the production AI agent framework by Nous Research, serving thousands of users. Not a demo, not a wrapper — a ground-up Rust implementation of the complete architecture.

**Performance vs Python agent frameworks:**

| Metric | Python Agent | Hakimi (Rust) |
|--------|-------------|---------------|
| Startup | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Concurrency | asyncio + thread bridge | tokio native async (no GIL) |
| Tool registration | Runtime AST scanning | Compile-time trait (zero overhead) |
| Type safety | Runtime crashes | Compile-time guarantees |

**Production features:** 771 tests · 20+ API error types auto-classified with recovery · Multi-key credential pool with circuit breakers · 3-tier context compression · Anthropic prompt caching

---

## Capabilities

### 🧠 Hakimi-Original Features

These features do not exist in the original Hermes Agent — they are unique to Hakimi:

**Knowledge Graph Memory** (`hakimi-knowledge`)
- petgraph-based directed graph with 10 node types (Entity, Concept, Fact, Preference, Person, Location, Skill, Tool, Event, Note) and 12 edge types
- BFS neighbor queries, shortest path, subgraph extraction, fuzzy search
- File persistence with auto-save, wired into the MemoryProvider trait
- Replaces flat memory with structured, queryable knowledge

**Intent Reasoning** (`hakimi-context`)
- Classifies user messages into 10 intent categories (InformationSeeking, TaskExecution, Debugging, Planning, Research, etc.)
- Rule-based keyword + pattern matching — no ML dependency, zero latency
- Confidence scoring, secondary intents, predicted next tool actions
- Context-aware: uses recent tool history to refine predictions

**Decision Tree Backtracking** (`hakimi-session`)
- Conversations stored as a branching tree, not a flat list
- Backtrack to any decision point and explore alternative paths
- Compare outcomes across branches with `PathComparison`
- JSON serialization for persistence and replay

**Role Adaptation** (`hakimi-context`)
- 8 role profiles: Coder, Researcher, Writer, Analyst, Tutor, Assistant, DevOps, Reviewer
- Auto-detects appropriate role from message content and tool context
- Per-role tool filtering and prioritization (coder gets terminal/patch first, researcher gets web_search)
- Role transitions with history tracking

**Meta-Skill Extraction** (`hakimi-skills`)
- Analyzes past sessions for 6 pattern types: ToolSequence, ErrorFixCycle, SearchRefine, FileEditCycle, DelegatePattern, ConfigPattern
- Auto-generates reusable YAML skill files from extracted patterns
- Pattern merging and confidence scoring

### 🛠️ 30 Built-in Tools

- **Files**: read_file, write_file, search_files, patch
- **Shell**: terminal, process (background process management)
- **Web**: web_search, web_extract
- **Memory**: memory (persistent), session_search (FTS5 full-text)
- **Code**: code_exec (Python/JS/Bash)
- **Browser**: browser_navigate, browser_snapshot, browser_click, browser_type, browser_screenshot (Chromium automation)
- **Media**: vision_analyze (image analysis), image_generate
- **Productivity**: todo, clarify, checkpoint (shadow git snapshots)
- **Safety**: file_safety (path protection), secret_redaction, prompt_injection_detection
- **Meta**: delegate_task (sub-agent delegation), skill_manage, send_message

### 🔌 Transports

| Transport | API | Streaming | Status |
|-----------|-----|-----------|--------|
| ChatCompletions | OpenAI-compatible (`/v1/chat/completions`) | ✅ SSE | Production |
| Anthropic | Messages API (`/v1/messages`) | ✅ SSE + Prompt Caching | Production |
| Gemini | Google Gemini native API | ✅ SSE | Production |
| Bedrock | AWS Converse API | ✅ | Planned |

### 🌐 8 Platform Adapters

Telegram · Discord · Slack · DingTalk · WeCom · Signal · Matrix · Webhook

### 🧠 Smart Context Compression

Three-tier compression — no manual context window management:
- **Tier 1**: Drop old tool call results
- **Tier 2**: LLM-powered summarization of middle conversation turns
- **Tier 3**: Sliding window preserving recent context

### 🔐 Credential Pool & Error Recovery

```yaml
credential_pools:
  openrouter:
    strategy: round_robin
    credentials:
      - api_key: "sk-key-1"
        priority: 10
      - api_key: "sk-key-2"
        priority: 5
```

20+ error types auto-classified: auth failure → rotate key; rate limit → exponential backoff; context overflow → trigger compression; model not found → fallback model.

### 🔧 MCP (Model Context Protocol)

Full MCP client with stdio / HTTP / SSE transports. Built-in catalog of 9 popular servers (filesystem, GitHub, Brave Search, PostgreSQL, Puppeteer, memory, fetch, SQLite, sequential-thinking).

### 📦 Plugin System

```yaml
# ~/.hakimi/plugins/weather.yaml
name: weather
tools:
  - name: get_weather
    endpoint: "https://wttr.in/{city}?format=j1"
    method: GET
    description: "Get weather for a city"
```

4 ready-to-use templates bundled. `hakimi plugins list` to browse, `hakimi plugins init <name>` to scaffold.

---

## Architecture

**20 crates, each with a single responsibility**:

```
hakimi-agent/
├── crates/
│   ├── hakimi-common/      # Shared types, 20+ error classifications
│   ├── hakimi-config/      # YAML config, credential pool, env expansion
│   ├── hakimi-session/     # SQLite WAL + FTS5, decision tree backtracking
│   ├── hakimi-context/     # Context engine, compression, intent reasoning, role adaptation
│   ├── hakimi-core/        # Agent loop, error classifier, credential pool, guardrails
│   ├── hakimi-transports/  # LLM transports (OpenAI, Anthropic, Gemini) + prompt caching
│   ├── hakimi-tools/       # 25 built-in tools + registry
│   ├── hakimi-knowledge/   # Knowledge graph memory (petgraph)
│   ├── hakimi-skills/      # Skill system + meta-skill extraction
│   ├── hakimi-cron/        # Cron scheduler (SQLite persistent)
│   ├── hakimi-gateway/     # 8 platform adapters
│   ├── hakimi-mcp/         # MCP client (stdio/HTTP/SSE) + server catalog
│   ├── hakimi-plugin/      # Plugin loader
│   ├── hakimi-i18n/        # Internationalization
│   ├── hakimi-batch/       # Parallel batch processing
│   ├── hakimi-server/      # HTTP REST API (Axum)
│   ├── hakimi-cli/         # REPL CLI + setup wizard + doctor
│   └── hakimi-tui/         # ratatui terminal UI
```

### Core Loop

```
User Message
    │
    ▼
┌──────────────────────────────────────────────────┐
│  AIAgent.run_conversation()                      │
│                                                  │
│  1. Classify intent → predict needed tools       │
│  2. Adapt role → filter/prioritize tools         │
│  3. Build system prompt + knowledge context      │
│  4. Acquire API key from credential pool         │
│     → Call LLM via Transport (SSE streaming)     │
│  5. If tool_calls → dispatch & loop              │
│  6. If text response → return                    │
│  7. Error classifier → auto-recovery             │
│  8. Guardrails → loop detection / circuit break  │
│  9. Record decision tree node                    │
└──────────────────────────────────────────────────┘
    │
    ▼
Response + Token Usage Stats + Knowledge Updates
```

---

## Benchmark

| Feature | Hermes (Python) | Hakimi (Rust) |
|---------|-----------------|---------------|
| Language | Python 3.11+ | Rust 2024 |
| Async model | asyncio + thread bridge | tokio native async |
| Memory model | threading.RLock | `Arc<RwLock>` |
| Tool registration | Runtime AST scanning | Compile-time trait impl |
| Startup time | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Streaming | Generator-based | SSE + futures Stream |
| Error recovery | Basic retry | 20+ classifiers + auto-strategy |
| Credential mgmt | Single key | Multi-key pool + rotation + circuit breaker |
| Knowledge model | Flat memory file | Graph database (petgraph) |
| Intent detection | None | 10-category classifier |
| Role adaptation | None | 8 roles with auto-detection |
| Conversation model | Flat message list | Decision tree with backtracking |
| Skill extraction | Manual | Automatic pattern extraction |
| Tests | ~500 | 1035 |

---

## Development

```bash
# Build everything
cargo build --workspace

# Run all tests (1035 tests)
cargo test --workspace

# Debug logging
RUST_LOG=debug cargo run -p hakimi-cli

# Clippy linting
cargo clippy --workspace
```

---

## Roadmap

- [x] Core agent loop + tool dispatch
- [x] OpenAI / Anthropic / Gemini transports + SSE streaming
- [x] 25 built-in tools
- [x] 8 platform adapters
- [x] MCP client (stdio/HTTP/SSE) + server catalog
- [x] Plugin system + templates
- [x] ratatui TUI
- [x] SQLite session storage + FTS5
- [x] Smart context compression (3-tier)
- [x] Error classifier (20+ types) + credential pool
- [x] Prompt caching (Anthropic)
- [x] Vision analysis + checkpoint rollback
- [x] Profiles system + i18n + batch processing
- [x] Install script + cargo install + CI/CD
- [x] **Browser automation** (Chromium via chromiumoxide)
- [x] Setup wizard + doctor diagnostics
- [x] **Knowledge graph memory** (petgraph)
- [x] **Intent reasoning engine**
- [x] **Decision tree backtracking**
- [x] **Role adaptation**
- [x] **Meta-skill auto-extraction**
- [ ] WASM plugin runtime
- [ ] Web dashboard
- [ ] Voice input/output

---

## License

MIT License — see [LICENSE](LICENSE)

---

<p align="center">
  <b>Built with 🦀 Rust and ❤️</b><br>
  <sub>Inspired by <a href="https://github.com/NousResearch/hermes-agent">Hermes Agent</a> by Nous Research</sub>
</p>
