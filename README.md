<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.80-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1083-passing?style=for-the-badge&color=brightgreen" alt="Tests">
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
hakimi setup
hakimi doctor
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

**Production features:** 1083 tests · 20+ API error types auto-classified with recovery · Multi-key credential pool with circuit breakers · 3-tier context compression · Anthropic prompt caching

---

## Capabilities

### 🌟 What's New
- **v0.3.80 Reliable Self-Update State Restore**:
  - **Binary-Safe State Backup**: `hakimi --update` now backs up only user state (`memory`, `sessions`, `sessions.db*`, and `profiles`) instead of archiving the whole `~/.hakimi` directory.
  - **No Post-Verify Downgrade**: restoring pre-update memory/session state no longer overwrites the newly installed canonical binary under `~/.hakimi/bin/hakimi`.
  - **Regression Coverage**: added a state-restore test proving memory/session files are restored while the updated binary remains intact.
- **v0.3.79 Gateway `/usage` Display**:
  - **Hermes-Style Usage Surface**: gateway chats can now run `/usage` after a turn to see the active model, provider, API call count, and prompt/completion/total token usage.
  - **Rate-Limit Visibility**: the command includes the latest provider `x-ratelimit-*` snapshot when the active transport captured one, matching Hermes' rate-limit display path for remote operators.
  - **Regression Coverage**: added command-formatting tests for empty state, token counts, cache/reasoning buckets, and rate-limit snapshot rendering without live provider calls.
- **v0.3.78 Rate Limit Tracking**:
  - **Hermes-Style Header Parsing**: `hakimi-transports` now parses OpenAI/Nous-style `x-ratelimit-*` windows for requests and tokens per minute/hour, including numeric and duration reset values.
  - **Transport-Level Snapshots**: Chat Completions, Responses, Anthropic, and Gemini transports retain the latest rate-limit snapshot for future `/usage` and gateway status surfaces.
  - **Regression Coverage**: Added parser, formatting, warning, and tracker snapshot tests without calling live provider APIs.
- **v0.3.77 Think Scrubber Hardening**:
  - **Hermes-Style Tag Scrubbing**: `ThinkScrubber` now handles `<think>`, `<thinking>`, `<reasoning>`, `<thought>`, and `<REASONING_SCRATCHPAD>` tags case-insensitively, including tags split across SSE deltas.
  - **Clean Stored Responses**: streaming and non-streaming agent loops now store scrubbed `final_response` and assistant history while preserving hidden reasoning separately.
  - **Regression Coverage**: added state-machine and agent-loop tests for split tags, tag variants, inline closed pairs, non-streaming responses, and streaming accumulators.
- **v0.3.76 Doctor CLI / Gateway Diagnostics**:
  - **Hermes-Style Command Entry**: Added `hakimi doctor` while keeping the legacy `hakimi --doctor` flag, so setup diagnostics are reachable without starting the agent loop.
  - **Gateway `/doctor`**: Remote chats can now run setup diagnostics and receive a plain-text, chat-safe report instead of a placeholder command response.
  - **Regression Coverage**: Added parser coverage for top-level `doctor` / `setup` commands and ANSI-free diagnostic report formatting.
- **v0.3.75 Home Assistant Tools**:
  - **Smart Home REST Parity**: Added `ha_list_entities`, `ha_get_state`, `ha_list_services`, and `ha_call_service`, matching Hermes' Home Assistant tool surface through native async Rust.
  - **Guarded Service Calls**: Domain/service/entity IDs are validated before URL construction, and high-risk HA domains such as `shell_command`, `python_script`, `hassio`, and `rest_command` are blocked.
  - **Offline Regression Coverage**: Added validation, summarization, payload parsing, schema, blocked-domain, and service-response tests without requiring a live Home Assistant server.
- **v0.3.74 Image Describe Vision Alias**:
  - **Legacy Tool Now Works**: `image_describe` now reuses the `vision_analyze` pipeline instead of returning placeholder text, so older media workflows get the same base64 data-url payload as the dedicated vision tool.
  - **Hermes Parity Cleanup**: GAP_ANALYSIS no longer lists vision as both missing and complete; the remaining media gap is video analysis.
  - **Regression Coverage**: image_describe now has metadata, schema, validation, and local-file payload tests.
- **v0.3.73 Responses Stream Recovery**:
  - **Incomplete Means Continue**: OpenAI Responses `response.incomplete` SSE events now map to a `length` finish reason so Hakimi automatically requests a continuation instead of surfacing partial answers.
  - **Truncated Stream Retry**: streaming providers that close before a terminal `Done` or `Finished` event are classified as transport failures and retried through the existing backoff path.
  - **Shared LLM HTTP Timeouts**: CLI, server, and TUI transports now use a shared reqwest client with connect/read timeouts that keep long SSE streams alive while avoiding indefinite hangs.
- **v0.3.72 Cron Prompt Injection Guard**:
  - **Hermes-Style Cron Scanning**: user-authored cron prompts are checked for injection, secret-exfiltration, destructive command, and invisible Unicode patterns before they are persisted or manually triggered.
  - **Runtime Defense-in-Depth**: due cron jobs are re-scanned immediately before auto-execution; unsafe jobs are disabled and a gateway notification is queued instead of running in auto-approved cron context.
  - **Shared Prompt Security**: broad prompt-injection detection now lives in `hakimi-common`, so core file safety and cron security reuse the same baseline scanner.
- **v0.3.71 Cron Run Trigger**:
  - **Gateway `/cron run`**: operators can now trigger an existing scheduled job from Telegram/Discord/Slack with `/cron run <job-id>`, matching Hermes' "run on the next scheduler tick" behavior.
  - **Shared Tool Semantics**: the built-in `cronjob` tool now supports `action="run"` instead of advertising an unsupported action.
  - **Safer Persistent Update**: `hakimi-cron` updates `enabled` and `next_run` in-place, avoiding a full row rewrite when manually triggering a job.
- **v0.3.70 Gateway Cron Controls**:
  - **Real `/cron` Management in Gateway Chats**: operators can now run `/cron list`, `/cron pause <job-id>`, `/cron resume <job-id>`, and `/cron remove <job-id>` directly from Telegram/Discord/Slack instead of dropping to the host shell.
  - **Shared SQLite Cron State**: gateway commands now operate on the same persistent `~/.hakimi/cron.db` store used by Hakimi's Rust-native cron subsystem, keeping state consistent across restarts.
  - **Parity Status Clarified**: docs and gap analysis now reflect the real boundary: basic gateway cron lifecycle control is done, while add/edit flows, delivery wiring, and skill loading remain follow-up parity work.
- **v0.3.69 Speech Transcription Tooling**:
  - **`transcribe_audio` Built-in Tool**: Hakimi can now transcribe local audio files or remote audio URLs through an OpenAI-compatible `/audio/transcriptions` API.
  - **Shared Voice Runtime Config**: `voice.provider`, `voice.base_url`, `voice.api_key`, `voice.model`, `voice.voice`, and the new `voice.transcription_model` now flow into media tools instead of relying only on environment variables.
  - **Clearer Voice Roadmap**: speech-to-text parity is now covered by a real tool, while CLI push-to-talk remains a separate remaining gap.
- **v0.3.68 Real Telegram Stop/Restart + Reliable Self Update**:
  - **Real `/stop` Cancellation**: Telegram `/stop` now cancels the active per-chat gateway turn instead of only clearing the streaming callback, so long-running LLM/tool operations stop promptly.
  - **Telegram `/restart` Command**: `/restart` is parsed, handled by gateway mode, and exposed through Telegram Bot commands to restart the managed Hakimi gateway service.
  - **Reliable `hakimi --update`**: self-update now resolves GitHub's latest release via API, downloads the exact tag asset, installs to the `hakimi` binary found on `PATH`, and verifies `--version` after replacement to prevent staying on stale versions like 0.3.58.
  - **WeChat Typing Indicator**: ClawBot/iLink stores `typing_ticket` from getupdates and maps gateway `typing` actions to iLink `sendtyping`, so WeChat shows “对方正在输入...” while Hakimi is working.
  - **Mouseww/Rust Identity**: the default system prompt now identifies Hakimi as Mouseww's high-performance Rust-native Agent.

- **v0.3.67 One-Command Gateway Setup & Lifecycle**:
  - **Platform Multi-Select Setup**: `hakimi --setup` now lets operators select gateway platforms in one flow and writes real `gateways:` / `roles.default.gateways:` YAML instead of leaving platform tokens as comments.
  - **One-Step ClawBot Configuration**: the setup flow can configure WeChat ClawBot/iLink native mode, token storage, and Telegram QR-login notifications without hand-editing YAML.
  - **Managed Gateway Install**: `hakimi --gateway install` creates/updates the systemd service, enables it on boot, and starts it; `--gateway status` inspects it, while `--gateway restart` remains a fast lifecycle command that does not load model credentials.
  - **Top-Level Telegram Config Works**: gateway startup now honors `gateways.telegram.bot_token` in addition to env vars and role-scoped config, so setup-generated configs work immediately.
- **v0.3.66 Non-Blocking ClawBot QR Login**:
  - **Gateway Isolation**: native iLink QR login now runs in the background, so missing/expired WeChat login state no longer prevents Telegram from reaching `gateway listening for messages`.
  - **Telegram QR Image**: configure `login_notify_platform: "telegram"`, `login_notify_bot_id: "telegram_bot"`, and `login_notify_chat_id: "<chat-id>"` to receive the WeChat QR code as a Telegram photo instead of copying a URL from logs.
  - **Login Completion Notice**: after scanning succeeds, Hakimi sends a compact confirmation and persists the iLink token under `token_store` for future restarts.
- **v0.3.65 Gateway Restart Mode**:
  - **CLI Restart Shortcut**: `hakimi --gateway restart` restarts the managed systemd gateway service and exits, while plain `hakimi --gateway` still starts gateway mode in the foreground.
  - **Service Override**: set `HAKIMI_GATEWAY_SERVICE=<service-name>` when the systemd unit is not named `hakimi`.
  - **Backward Compatibility**: `--gateway start` is accepted explicitly for scripts that prefer a named mode.
- **v0.3.64 Native WeChat iLink / ClawBot Protocol**:
  - **Official iLink Mode**: `gateways.clawbot.mode: "ilink_native"` now talks directly to `https://ilinkai.weixin.qq.com` with QR login, `getupdates` long polling, and native `sendmessage` envelopes.
  - **Persistent Context Tokens**: bot tokens, update cursors, and per-user `context_token` values are stored under `~/.hakimi/clawbot`, so replies include the required iLink context instead of disappearing silently.
  - **Mode Compatibility**: the original generic `http_bridge` remains the default, while `weclawbot_api` supports Cp0204/WeClawBot-API outbound message/typing endpoints.
  - **Config + Env Overrides**: `CLAWBOT_MODE=ilink_native` can enable the native path without changing YAML.
- **v0.3.63 WeChat ClawBot Gateway**:
  - **ClawBot Adapter**: Hakimi can now connect to WeChat through a configurable ClawBot HTTP bridge.
  - **Multi-Platform Gateway Fan-in**: gateway mode now merges receivers from all registered platforms so Telegram and ClawBot can run together.
  - **Flexible Bridge Schema**: ClawBot polling accepts common aliases such as `messages`, `data`, `chat_id`, `conversation_id`, `text`, and `content`.
  - **Config + Env Overrides**: configure `gateways.clawbot` or role-scoped `roles.default.gateways.clawbot`; `CLAWBOT_BASE_URL` / `CLAWBOT_TOKEN` can enable it at runtime.

- **v0.3.62 Delegate Progress Bubbles**:
  - **One Bubble per Delegate/Child Agent**: `delegate_task` now streams progress into stable Telegram bubbles instead of going silent until the final result.
  - **Live Container Updates**: each child agent gets a titled container and the gateway edits that same message with timestamped progress lines.
  - **No Mixed Output**: delegate progress is routed separately from assistant prose and normal tool-call status, preserving clean chat bubbles.
- **v0.3.61 Processing Placeholder Recovery**:
  - **No Stuck `✨ Processing...`**: Gateway now tracks whether any assistant prose actually rendered through the streaming callback; if providers return final text without content deltas, Hakimi edits the initial placeholder with the final response instead of leaving it visible.
  - **Error Bubble Cleanup**: errors now overwrite the same placeholder message, so Telegram users see the actual failure instead of a permanent loading state.
  - **Regression Coverage**: added a focused unit test for the no-stream-content fallback path.
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

### 🛠️ 35 Built-in Tools

- **Files**: read_file, write_file, search_files, patch
- **Shell**: terminal, process (background process management)
- **Web**: web_search, web_extract
- **Home Assistant**: ha_list_entities, ha_get_state, ha_list_services, ha_call_service
- **Memory**: memory (persistent), session_search (FTS5 full-text)
- **Code**: code_exec (Python/JS/Bash)
- **Browser**: browser_navigate, browser_snapshot, browser_click, browser_type, browser_screenshot (Chromium automation)
- **Media**: vision_analyze (image analysis), image_describe (legacy alias), image_generate, text_to_speech, transcribe_audio
- **Productivity**: todo, clarify, checkpoint (shadow git snapshots)
- **Safety**: file_safety (path protection), secret_redaction, prompt_injection_detection
- **Meta**: delegate_task (sub-agent delegation), skill_manage, send_message

### 🔌 Gateway Platforms

Hakimi can run as a long-lived gateway bot and fan-in messages from multiple adapters at the same time.

**WeChat via ClawBot / iLink:**

```yaml
gateways:
  clawbot:
    enabled: true
    mode: "ilink_native"   # http_bridge | weclawbot_api | ilink_native
    bot_id: "clawbot"
    base_url: "https://ilinkai.weixin.qq.com"
    token: ""              # optional existing bot_token; otherwise QR login
    token_store: "~/.hakimi/clawbot"
    channel_version: "1.0.2"
    app_client_version: "2.4.3"
    login_notify_platform: "telegram"     # optional: send QR login image to Telegram
    login_notify_bot_id: "telegram_bot"   # optional: target Telegram adapter bot_id
    login_notify_chat_id: "<telegram-chat-id>"
```

On first `hakimi --gateway`, native iLink mode starts the WeChat QR login in the background instead of blocking other adapters. If `login_notify_chat_id` is configured, Hakimi routes the QR URL through Telegram `sendPhoto`, so the operator receives an image to scan directly in chat. Hakimi persists the returned bot token, update cursor, and per-chat `context_token` under `token_store`, then receives inbound messages through `POST /ilink/bot/getupdates` and replies through `POST /ilink/bot/sendmessage`.

**Gateway lifecycle:**

```bash
hakimi setup              # multi-select Telegram / WeChat ClawBot and write real gateway config
hakimi doctor             # diagnose config, dependencies, and API connectivity
hakimi --gateway install  # create/update systemd service, enable boot start, and start now
hakimi --gateway          # foreground gateway mode (same as --gateway start)
hakimi --gateway start    # explicit foreground gateway mode
hakimi --gateway restart  # restart the managed systemd service and exit
hakimi --gateway status   # show managed service status and exit
```

By default the lifecycle shortcuts target `hakimi.service`. If your unit uses another name, set `HAKIMI_GATEWAY_SERVICE=<service-name>` before running `hakimi --gateway install`, `hakimi --gateway restart`, or `hakimi --gateway status`.

Inside gateway chats, `/cron` now supports `list`, `pause <job-id>`, `resume <job-id>`, `run <job-id>`, and `remove <job-id>` against the shared SQLite-backed `cron.db`, so operators can manage scheduled jobs without leaving Telegram/Discord/Slack.

**Legacy generic ClawBot HTTP bridge:**

```yaml
gateways:
  clawbot:
    enabled: true
    mode: "http_bridge"
    bot_id: "clawbot"
    base_url: "http://127.0.0.1:5700"
    token: ""
    poll_path: "/messages"
    send_path: "/send_message"
    edit_path: "/edit_message"
    poll_interval_ms: 1000
    poll_limit: 50
```

`mode: "weclawbot_api"` targets Cp0204/WeClawBot-API (`/bots/{bot_id}/messages` and `/typing`) for outbound WeChat pushes.

Environment overrides are also supported:

```bash
CLAWBOT_MODE=ilink_native CLAWBOT_BASE_URL=https://ilinkai.weixin.qq.com CLAWBOT_TOKEN=[REDACTED] hakimi --gateway
```

The legacy bridge accepts common inbound aliases such as `messages`/`data`, `chat_id`/`conversation_id`, and `text`/`content`; outbound sends include `chat_id`, `conversation_id`, `to`, `text`, and `content` for broad ClawBot compatibility.

### 🔌 Transports

| Transport | API | Streaming | Status |
|-----------|-----|-----------|--------|
| ChatCompletions | OpenAI-compatible (`/v1/chat/completions`) | ✅ SSE | Production |
| Anthropic | Messages API (`/v1/messages`) | ✅ SSE + Prompt Caching | Production |
| Gemini | Google Gemini native API | ✅ SSE | Production |
| Bedrock | AWS Converse API | ✅ | Planned |

### 🌐 8 Platform Adapters

Telegram · Discord · Slack · DingTalk · WeCom · Signal · Matrix · Webhook

Telegram now uploads generated local images directly and delivers generated TTS files as native audio messages, so `image_generate` / `text_to_speech` results can reach gateway users without manually copying file paths. For voice input flows, Hakimi now also exposes `transcribe_audio` for local files and remote audio URLs; CLI push-to-talk remains future work.

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
│   ├── hakimi-tools/       # 35 built-in tools + registry
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
| Tests | ~500 | 1083 |

---

## Development

```bash
# Build everything
cargo build --workspace

# Run all tests (1083 tests)
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
- [x] 35 built-in tools
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
- [ ] CLI voice mode (push-to-talk capture + playback)

---

## License

MIT License — see [LICENSE](LICENSE)

---

<p align="center">
  <b>Built with 🦀 Rust and ❤️</b><br>
  <sub>Inspired by <a href="https://github.com/NousResearch/hermes-agent">Hermes Agent</a> by Nous Research</sub>
</p>

