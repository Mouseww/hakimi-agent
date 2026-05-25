<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.60-blue?style=for-the-badge" alt="Version">
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

After install, the installer automatically adds `~/.hakimi/bin` to your shell PATH when possible and offers to launch the setup wizard. You can also run it manually at any time:

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

### 🌟 What's New
- **v0.3.60 Gateway Concurrent Input Routing**:
  - **No More Silent Blocking**: Telegram/Gateway handlers no longer hold the shared `AIAgent` mutex across the full LLM/tool loop, so a second message sent while a task is running is accepted immediately instead of appearing ignored.
  - **Supplement-or-New-Task Hinting**: Overlapping messages run through an isolated turn agent with the latest chat snapshot and an explicit system hint that the text may be supplemental context for the active request or a separate task.
  - **Safe History Merge**: Finished turns append only their own new messages back into chat history, avoiding late-finishing tasks overwriting newer conversation state.
- **v0.3.59 Self-Improvement Review Notices**:
  - **Hermes-Style Memory Feedback**: Successful `memory` tool writes now emit a compact standalone status bubble like `💾 Self-improvement review: User profile updated` after user profile changes.
  - **Clean Bubble Separation**: Self-improvement review notices use their own structured Gateway side-channel, so they do not get appended to the assistant's main streamed reply.
- **v0.3.58 UTF-8 Safe Tool Notices**:
  - **No More Chinese Panic**: Tool argument summaries now truncate by Unicode scalar values instead of raw byte offsets, preventing crashes like `end byte index ... is not a char boundary` when Chinese options or proxy/API setup prompts are summarized.
  - **Regression Coverage**: Added tests for Chinese tool notice truncation and newline normalization so compact `⚙️ ...` status bubbles stay safe for multilingual text.
- **v0.3.57 Installer Setup & Stream Text Polish**:
  - **First-Run Setup**: `hakimi --setup` is wired into the CLI and the shell installer now offers to run it immediately after install instead of showing help.
  - **PATH Auto-Configuration**: `install.sh` adds `~/.hakimi/bin` to `.bashrc`, `.zshrc`, or fish config when possible, and also attempts safe symlinks into existing PATH directories.
  - **Coalesced Gateway Streaming**: Progressive Telegram/Gateway content deltas are appended exactly as received and coalesced into burst updates before editing the message, preventing accidental token spaces and the sluggish one-character-per-edit effect.
  - **Workspace Install Fix**: Source fallback builds the executable `hakimi` crate and includes the server/knowledge crates in workspace membership.
- **v0.3.56 Gateway Bubble Boundary Fix**:
  - **Tool Boundaries Freeze Prose**: Telegram/Gateway streaming now treats every tool notice as a hard semantic boundary. The explanation before a tool stays in its own assistant bubble, the tool call is sent as a compact standalone bubble, and later assistant prose starts in a fresh bubble.
  - **No Final Re-Merge**: Removed the final whole-response edit that previously overwrote the initial placeholder with the complete transcript, which could visually recombine prose and tool notices into one oversized message.
  - **Queue Drain Safety**: The streaming UI task is now awaited after the callback is cleared, ensuring late content/tool events flush before the gateway finishes the turn.
- **v0.3.55 Streaming Layout Preservation**:
  - **Smart Continuation Merge**: Automatic continuation now merges truncated response segments with a layout-preserving append routine, preventing `hello` + `world` from becoming `helloworld` while keeping intentional Markdown and line breaks intact.
  - **Telegram Newline Safety**: Gateway streaming and Telegram send/edit paths now normalize CRLF/CR into LF without trimming or folding content, so multi-line assistant replies remain multi-line during progressive edits.
- **v0.3.53 Separated Tool Status Bubbles**:
  - **Clean Tool Notices**: Tool dispatch status is now rendered as a compact one-line message like `⚙️ patch (path: ...)` instead of `tool` / `tool_call` flavored markup.
  - **Bubble Separation**: Gateway streaming treats tool notices as structured side-channel events and sends them as standalone Telegram messages, so descriptive assistant prose remains in the main response bubble instead of being mixed with tool logs.
- **v0.3.52 Real-time Tool Status & Concurrent Borrow Fix**:
  - **Tool Stream Announce**: When an LLM calls a tool (like `delegate_task`), the message now instantly updates to show `⚙️ **tool_call**: {tool_name}` alongside `⏳ Processing...`, eliminating the "hanging" feeling during long-running tasks.
  - **Concurrent Borrow Checker Fix**: Fixed the immutable borrow conflict inside `process_tool_calls` when dispatching concurrent tools by isolating the `ToolRegistry` arc.
- **v0.3.49 Advanced Tool Execution**:
  - **Execute Code Injection (`execute_code`)**: Embedded Python REPL sandbox now natively injects the `hermes_tools` library, bridging the tool pipeline (read/write/patch/search/terminal) directly into the Python environment!
  - **PTY Terminal Support**: Added full pseudo-terminal (`pty: true`) backing via standard linux `script` for interactive commands, ensuring smooth flow without input deadlocks.
- **v0.3.48 Full-Stack Reliability & Features**:
  - **Embedded Cron Scheduler**: 100% isolated background Daemon polling `cron.db` and delegating scheduled tasks.
  - **MCP Out-of-the-Box**: Verified and activated the native integration of Model Context Protocol (`mcp_servers` configuration in `config.yaml`).
  - **Deadlock-Free CI**: Complete elimination of race conditions and deadlocks in the async test suites.
- **Interactive Commands Menu:** Auto-configured command menu in Telegram for easier navigation (e.g., `/help`, `/stop`, `/clear`).
- **Stop Command:** Added `/stop` command to halt ongoing tasks or streaming generation.
- **Gateway Progressive Streaming**: Resolved lingering typing indicators (`⏳`) ensuring fluid platform interaction without delayed edits.
- **Enhanced Memory Injection**: Fixed move-semantics that occasionally dropped configuration states when reading `~/.hakimi/memory` across concurrent `tokio` tasks.
- **Improved Code Quality**: Resolved all CI linting errors, including `clippy::too_many_arguments` in `hakimi-core` and various nested `if` statements across the codebase.

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
