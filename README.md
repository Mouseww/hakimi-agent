<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.1.0-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-391-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/lines-29.1K++-orange?style=for-the-badge" alt="Lines">
</p>

<h1 align="center">🐙 Hakimi Agent</h1>

<p align="center">
  <b>A high-performance AI agent framework written in Rust</b><br>
  <sub>Modular • Async-native • Multi-platform • Tool-rich • Extensible</sub>
</p>

<p align="center">
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-architecture">Architecture</a> •
  <a href="#-tools">Tools</a> •
  <a href="#-platforms">Platforms</a> •
  <a href="#-configuration">Configuration</a> •
  <a href="#-extending">Extending</a>
</p>

---

## What is Hakimi?

Hakimi is a Rust rewrite of the [Hermes Agent](https://github.com/NousResearch/hermes-agent) Python framework — rebuilt from the ground up for **performance**, **safety**, and **extensibility**. It provides a complete AI agent runtime with:

- **17 built-in tools** (file ops, shell, search, web, memory, code execution, ...)
- **2 LLM transports** (OpenAI-compatible + Anthropic Messages API)
- **3 platform adapters** (Telegram, Discord, Slack)
- **SSE streaming** with real-time token delivery
- **MCP client** for external tool servers
- **Plugin system** for HTTP-based and native tool extensions
- **ratatui TUI** for a rich terminal experience
- **SQLite** session storage with full-text search
- **SmartContextEngine** with 3-tier compression (summarize, truncate, sliding window)
- **MCP & Plugin systems** wired into CLI for extensibility

## Quick Start

```bash
# Clone and build
git clone https://github.com/Mouseww/hakimi-agent.git
cd hakimi-agent
cargo build --release

# Set your API key
export OPENAI_API_KEY="sk-..."

# Run the CLI
./target/release/hakimi-cli

# Or single-query mode
./target/release/hakimi-cli --query "What is the Rust borrow checker?"

# Or the TUI
./target/release/hakimi-tui
```

On first run, Hakimi creates `~/.hakimi/config.yaml` with sensible defaults. Edit it to customize your model, provider, and agent behavior.

## Architecture

Hakimi is a **Cargo workspace** with 13 crates, each with a single responsibility.
The context engine (`hakimi-context`) features a **SmartContextEngine** with 3-tier
compression — summarization, truncation, and sliding window — to keep conversations
within token limits without losing critical information.

```
hakimi-agent/
├── crates/
│   ├── hakimi-common/      # Shared types: Message, ToolCall, Usage, Error
│   ├── hakimi-config/      # YAML config loading, profiles, env expansion
│   ├── hakimi-session/     # SQLite WAL + FTS5 full-text search
│   ├── hakimi-context/     # Context engine, compression, prompt building
│   ├── hakimi-core/        # AIAgent builder, conversation loop, retry logic
│   ├── hakimi-transports/  # LLM providers (OpenAI, Anthropic) + SSE streaming
│   ├── hakimi-tools/       # 17 built-in tools + registry
│   ├── hakimi-cron/        # Cron scheduler for recurring tasks
│   ├── hakimi-gateway/     # Platform adapters (Telegram, Discord, Slack)
│   ├── hakimi-mcp/         # MCP (Model Context Protocol) client
│   ├── hakimi-plugin/      # Plugin loader (HTTP tools, native libs, WASM)
│   ├── hakimi-cli/         # Interactive REPL CLI
│   └── hakimi-tui/         # ratatui-based terminal UI
```

### Core Loop

```
User Message
    │
    ▼
┌─────────────────────────────────────────────┐
│  AIAgent.run_conversation()                 │
│                                             │
│  1. Build system prompt + context           │
│  2. Call LLM via Transport (streaming)      │
│  3. If tool_calls → dispatch & loop         │
│  4. If text response → return               │
│  5. Retry on transient errors (backoff)     │
│  6. Compress context if near limit          │
│     └─ SmartContextEngine 3-tier compression │
└─────────────────────────────────────────────┘
    │
    ▼
Final Response + Usage Stats
```

## Tools

17 built-in tools organized by toolset:

### 📁 File (`file`)
| Tool | Description |
|------|-------------|
| `read_file` | Read files with line numbers, offset, and limit |
| `write_file` | Write files, auto-create parent directories |
| `search_files` | Regex search via ripgrep (with grep fallback) |
| `patch` | Find-and-replace in files with uniqueness validation |

### 💻 Shell (`shell`)
| Tool | Description |
|------|-------------|
| `terminal` | Execute shell commands with timeout |
| `process` | Manage background processes (start/stop/log) |

### 🌐 Web (`web`)
| Tool | Description |
|------|-------------|
| `web_search` | Web search (DuckDuckGo or API) |

### 🧠 Memory (`memory`)
| Tool | Description |
|------|-------------|
| `memory` | Persistent agent memory (add/replace/remove) |
| `session_search` | Full-text search across past sessions |

### ✅ Productivity (`productivity`)
| Tool | Description |
|------|-------------|
| `todo` | Task management (create/update/list) |

### 🐍 Code (`code`)
| Tool | Description |
|------|-------------|
| `code_exec` | Execute Python/JavaScript/Bash snippets |

### 📨 Communication (`communication`)
| Tool | Description |
|------|-------------|
| `send_message` | Queue messages for platform delivery |

### 🤝 Meta (`meta`)
| Tool | Description |
|------|-------------|
| `delegate_task` | Spawn sub-tasks (planned: child agents) |
| `skill_manage` | CRUD operations on reusable skill files |

### 👁️ Media (`media`)
| Tool | Description |
|------|-------------|
| `image_describe` | Image analysis (requires vision model) |

## Platforms

### Telegram
```yaml
gateway:
  telegram:
    token: "your-bot-token"
```
- Long polling with auto-reconnect
- Markdown formatting with plain-text fallback
- Auto-splits messages > 4096 chars
- Photo message support

### Discord
```yaml
gateway:
  discord:
    token: "your-bot-token"
    channel_id: "123456789"
```
- REST API (no WebSocket gateway required)
- Rich embed support
- Rate limit handling with retry

### Slack
```yaml
gateway:
  slack:
    token: "xoxb-your-bot-token"
    channel_id: "C0123456789"
```
- Block Kit support for rich formatting
- Timestamp-based message cursor

## Configuration

`~/.hakimi/config.yaml`:

```yaml
model:
  default: "anthropic/claude-sonnet-4-20250514"
  provider: "openrouter"
  base_url: "https://openrouter.ai/api"

api_key: "sk-your-key"

agent:
  max_turns: 90
  max_retries: 3

display:
  streaming: true
```

Environment variables (override config):
- `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `OPENROUTER_API_KEY`
- `HAKIMI_MODEL` — model override
- `HAKIMI_BASE_URL` — API endpoint override

## Transports

| Transport | API | Streaming | Status |
|-----------|-----|-----------|--------|
| `ChatCompletionsTransport` | OpenAI-compatible (`/v1/chat/completions`) | ✅ SSE | Production |
| `AnthropicTransport` | Anthropic Messages API (`/v1/messages`) | ✅ SSE | Production |

Both transports support:
- Non-streaming (blocking) mode
- SSE streaming with real-time token delivery
- Tool calling (function calling)
- Usage tracking (prompt/completion/cached tokens)
- Error classification and retry logic

## MCP (Model Context Protocol)

Hakimi includes a full MCP client for connecting to external tool servers:

```rust
use hakimi_mcp::McpClient;

// Connect to an MCP server via stdio
let mut client = McpClient::connect_stdio("npx", &["@modelcontextprotocol/server-filesystem"]).await?;
client.initialize().await?;

// Discover tools
let tools = client.list_tools().await?;

// Call a tool
let result = client.call_tool("read_file", json!({"path": "/tmp/test.txt"})).await?;
```

## Plugins

Extend Hakimi with custom tools via the plugin system:

### HTTP Tool Plugins

Create a YAML file in `~/.hakimi/plugins/`:

```yaml
name: my_api
tools:
  - name: get_weather
    endpoint: "https://api.weather.com/v1/current?city={city}"
    method: GET
    description: "Get current weather for a city"
    parameters:
      type: object
      properties:
        city:
          type: string
          description: "City name"
      required: ["city"]
```

### Native Plugins (planned)

Load `.so`/`.dylib` dynamic libraries that implement the `Plugin` trait.

## Development

```bash
# Build everything
cargo build --workspace

# Run all tests (310 tests)
cargo test --workspace

# Run with debug logging
RUST_LOG=debug cargo run -p hakimi-cli

# Run the TUI
cargo run -p hakimi-tui

# Check for warnings
cargo clippy --workspace
```

### Test Coverage

```
310 tests passing across 13 crates
├── hakimi-common:      22 tests
├── hakimi-config:       6 tests
├── hakimi-context:     25 tests (SmartContextEngine)
├── hakimi-core:         16 tests + 21 integration tests
├── hakimi-cron:          4 tests
├── hakimi-gateway:      24 tests (Telegram, Discord, Slack)
├── hakimi-mcp:          13 tests
├── hakimi-plugin:        4 tests
├── hakimi-session:      36 tests (SQLite WAL + FTS5)
├── hakimi-tools:         95 tests
├── hakimi-transports:    43 tests (ChatCompletions, Anthropic, Streaming)
├── hakimi-cli:            1 test
└── hakimi-tui:            0 tests
```

## Roadmap

- [x] Core agent loop with tool dispatch
- [x] OpenAI-compatible + Anthropic transports
- [x] SSE streaming
- [x] 17 built-in tools
- [x] Telegram / Discord / Slack adapters
- [x] MCP client
- [x] Plugin system (HTTP tools)
- [x] ratatui TUI
- [x] SQLite session storage with FTS5
- [x] Context compression (SmartContextEngine — 3-tier)
- [x] MCP & Plugin systems wired into CLI
- [ ] Skill system (load SKILL.md files)
- [ ] Delegated sub-agents
- [ ] WASM plugin runtime
- [ ] Web UI dashboard
- [ ] Voice input/output
- [ ] Multi-agent orchestration

## Recent Changes

- **SmartContextEngine** — 3-tier context compression (summarize, truncate, sliding window)
- **hakimi-session** — jumped from 0 to 36 tests with full SQLite/FTS5 coverage
- **MCP & Plugin systems** — now wired into CLI (`hakimi-cli`)
- **hakimi-tools** — expanded to 95 tests (up from 42+)
- **hakimi-common** — expanded to 22 tests (up from 3)
- **Total tests** — 310 passing (up from 212)
- **Total lines** — 18,700+ Rust LOC (up from 16,478)

## Comparison with Hermes (Python)

| Feature | Hermes (Python) | Hakimi (Rust) |
|---------|-----------------|---------------|
| Language | Python 3.11+ | Rust 2024 |
| Async | asyncio + threading bridge | tokio native async |
| Memory model | threading.RLock | Arc\<RwLock\> |
| Session store | SQLite via Python | rusqlite (bundled) |
| Tool registration | Runtime AST scanning | Compile-time trait impl |
| Startup time | ~2s | ~50ms |
| Memory usage | ~150MB idle | ~15MB idle |
| Streaming | Generator-based | SSE + futures Stream |

## License

MIT License — see [LICENSE](LICENSE) for details.

---

<p align="center">
  <b>Built with 🦀 Rust and ❤️ by the Hakimi contributors</b><br>
  <sub>Inspired by <a href="https://github.com/NousResearch/hermes-agent">Hermes Agent</a> by Nous Research</sub>
</p>
