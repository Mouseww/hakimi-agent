<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.125-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1273-passing?style=for-the-badge&color=brightgreen" alt="Tests">
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

On Linux, the installer keeps the real binary under `~/.hakimi/bin/hakimi` and treats `/usr/local/bin/hakimi` as a symlink/launcher. Managed gateway installs use the canonical `~/.hakimi/bin/hakimi --gateway start` path when available and set a stable service PATH. Terminal/process tools also prefix the current PATH with Hakimi's managed bins:

```bash
~/.hakimi/bin:~/.cargo/bin:/usr/local/bin:/usr/bin:/bin
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

**Production features:** 1273 tests · 20+ API error types auto-classified with recovery · Multi-key credential pool with circuit breakers and terminal auth quarantine · 3-tier context compression · Anthropic prompt caching · Progressive MCP/plugin tool disclosure · Gateway ingress access policy · MCP sampling/createMessage · Skills guard, provenance, hub install policy, and platform gates · Rust-native backup/import · Gateway stream pacing

---

## Capabilities

### 🌟 What's New
- **v0.3.125 Skills platform-gated loading**:
  - **Hermes Skill Metadata Parity**: `SKILL.md` frontmatter can now declare `platforms` as a scalar or list, matching Hermes' platform-gated skill loading semantics.
  - **Rust-Native OS Matching**: Hakimi recognizes `macos`/`darwin`, `windows`/`win32`, `linux`, `termux`, and `android` aliases before a skill enters the runtime prompt.
  - **Prompt Hygiene**: incompatible skills are skipped after the safety scan and parse step, so OS-specific instructions do not leak into unrelated sessions.
- **v0.3.124 Skills Hub manifest install policy**:
  - **Hermes Skills Hub Slice**: top-level `hakimi skills browse|search|inspect|install|list|path` now works from a local `.hub/index.json` manifest and uses the same response path for gateway `/skills browse|search|inspect|install`.
  - **Trust Boundary**: community skills require explicit `--trust-community` before non-interactive install, while builtin/trusted skills can install directly after the safety scan.
  - **Safe Install Records**: installs validate bundle paths, scan `SKILL.md`, block traversal/symlink replacement, update `.hub/lock.json` provenance, and append `.hub/audit.log` without leaking skill contents.
- **v0.3.123 Rust-native backup/import**:
  - **Hermes Backup Parity**: top-level `hakimi backup [output]` creates a compressed archive of user state without shelling out to `tar`.
  - **Safe Import Boundary**: `hakimi import <archive> --force` restores into `~/.hakimi` while blocking path traversal and binary downgrade entries.
  - **State-Safe Exclusions**: backups skip binaries, nested backups, checkpoints, SQLite sidecars, PID files, symlinks, and dependency/cache directories while snapshotting `.db` files through SQLite's backup API when possible.
- **v0.3.122 Skills provenance metadata**:
  - **Hermes Hub Lock Parity**: skills loaded from a `.hub/lock.json` now inherit source, identifier, trust level, repository, and creator metadata without trusting the skill body to self-report provenance.
  - **Frontmatter Metadata**: Hakimi parses Hermes-style `metadata.hermes` provenance fields and explicit top-level `provenance` blocks, then normalizes blank/control-character-heavy labels before display.
  - **Operator Visibility**: skill summaries and gateway `/skills` responses show a compact `source/trust` label so operators can distinguish local, official, trusted, and community skills.
- **v0.3.121 Skills Guard**:
  - **Hermes Skills Guard Parity**: skill markdown is scanned before parsing so obvious prompt-injection, exfiltration, persistence, destructive, invisible-Unicode, and embedded-credential patterns cannot enter the runtime system prompt.
  - **Non-Leaking Diagnostics**: blocked skills report stable finding ids and line numbers in logs without echoing the suspicious skill content.
  - **Symlink Boundary**: the skills loader skips symlinked files and directories so a skill tree cannot redirect loading outside the configured skills directory.
- **v0.3.120 Credential pool terminal auth quarantine**:
  - **Hermes Credential Pool Parity**: provider failures with terminal 401 OAuth reasons such as `token_revoked`, `token_invalidated`, `invalid_grant`, and `refresh_token_reused` now mark the credential `dead`.
  - **No Cooldown Re-Entry**: dead credentials stay out of round-robin, fill-first, random, and least-used rotation until explicit re-auth or token replacement revives them.
  - **Health Visibility**: pool stats now distinguish temporarily exhausted credentials from permanently dead ones and preserve the last provider status/reason for diagnostics.
- **v0.3.119 Gateway stream pacing**:
  - **Hermes Stream Consumer Parity**: gateway streaming now honors configurable edit cadence and buffered-character flush thresholds instead of a fixed 450 ms loop.
  - **Configurable Progressive Edits**: `gateways.streaming.edit_interval_ms` defaults to `800`, and `buffer_threshold_chars` defaults to `24`; set the threshold to `0` for interval-only edits.
  - **Boundary-Safe Flushes**: tool, media, delegate-progress, and shutdown boundaries flush pending assistant text before opening the next message bubble.
- **v0.3.118 Gateway fresh-final streaming**:
  - **Hermes Stream Consumer Parity**: gateway streaming tracks how long the first preview bubble has been visible and can finish long streamed answers as a fresh final message.
  - **Configurable Completion Timestamp**: `gateways.streaming.fresh_final_after_seconds` defaults to `60`; set it to `0` to keep legacy edit-in-place behavior for all completions.
  - **Telegram Preview Cleanup**: Telegram implements `deleteMessage` for best-effort stale preview cleanup after the fresh final message is sent.
- **v0.3.117 MCP sampling/createMessage**:
  - **Hermes MCP Sampling Parity**: stdio MCP clients can advertise sampling support and answer server-initiated `sampling/createMessage` requests.
  - **Transport-Backed Sampling**: sampling requests reuse Hakimi's configured LLM transport and active model instead of introducing a separate provider path.
  - **Protocol-Safe Fallbacks**: unsupported client-side server requests now return JSON-RPC errors instead of being misread as logs or stray responses.
- **v0.3.116 Gateway MCP Server Listing**:
  - **Hermes Control-Plane Visibility**: gateway `/mcp` and `/mcp list` now report the real configured MCP servers instead of a fixed placeholder.
  - **Config-First Boundary**: `/mcp add|remove` now tells operators that server changes are managed through the `mcp_servers` config and require a gateway restart.
  - **Safe Inventory Output**: the listing shows server names, launch command, argument count, and env-var count without printing environment values.
- **v0.3.115 Gateway ingress access policy · Gateway MCP server listing**:
  - **Hermes Gateway Safety Parity**: inbound gateway messages now pass a config-driven allowlist before any slash command or agent turn runs.
  - **Config-First Authorization**: `gateways.allowed_users`, `gateways.telegram.allowed_users`, role Telegram allowlists, and `gateways.clawbot.allowed_users` are merged into one ingress policy.
  - **Safe Compatibility**: empty allowlists preserve the previous allow-all behavior, while `gateways.allow_all` provides an explicit override for deployments that want open access.
- **v0.3.114 Terminal Shell Hooks**:
  - **Hermes Shell-Hook Slice**: `terminal` now supports opt-in `HAKIMI_PRE_TOOL_HOOK` and `HAKIMI_POST_TOOL_HOOK` commands for local pre/post command execution hooks.
  - **Hermes-Compatible Payloads**: hooks receive JSON on stdin with `hook_event_name`, `tool_name`, `tool_input`, `session_id`, `cwd`, and `extra.task_id`; post hooks also receive the rendered tool output.
  - **Blocking Guardrails**: pre hooks can return either `{"action":"block","message":"..."}` or `{"decision":"block","reason":"..."}` to stop unsafe terminal calls before execution.
- **v0.3.113 Progressive Tool Disclosure**:
  - **Hermes Tool Search Parity**: large MCP/plugin tool surfaces can now collapse behind `tool_search`, `tool_describe`, and `tool_call` bridge tools instead of sending every deferred schema on each turn.
  - **Core Tool Safety**: built-in Hakimi tools such as terminal, file, memory, cron, browser, media, and knowledge tools stay directly visible and are never deferred.
  - **Configurable Thresholds**: `tools.tool_search.enabled`, `threshold_pct`, `search_default_limit`, and `max_search_limit` support Hermes-style `auto`/`on`/`off` behavior for CLI and server agents.
- **v0.3.112 Plugin List Usability**:
  - **Hermes CLI Parity**: `hakimi plugins list` now supports `--plain` and `--json`, matching Hermes' latest compact and machine-readable listing workflow.
  - **Metadata-Aware HTTP Plugins**: plugin configs can declare optional top-level `version` and `description`, and the list output surfaces those values instead of a fixed generic label.
  - **Tool Visibility Controls**: `--tools` and `--no-tools` let operators choose between concise inventory output and tool-level inspection.
- **v0.3.111 Read-File Credential Guard**:
  - **Hermes File-Safety Parity**: `read_file` now refuses known Hakimi credential stores before reading, including `config.yaml`, OAuth caches, MCP token files, project `.env*` files, and `cache/bws_cache.json`.
  - **Symlink-Aware Defense**: existing paths are canonicalized before matching so simple symlinks cannot bypass the guard.
  - **Windows Path Fix**: absolute path detection now uses `Path::is_absolute()`, so `C:\...` paths are not accidentally resolved under the current workdir.
- **v0.3.110 Delegation Blocked Tools**:
  - **Hermes Subagent Safety Parity**: delegated child agents no longer receive `delegate_task`, `clarify`, `memory`, `send_message`, or `code_exec`.
  - **Toolset-Safe Filtering**: explicit child `toolsets` still work, but the denylist is applied after selection so sensitive tools cannot be re-enabled accidentally.
  - **Regression Coverage**: added offline registry-filter tests for default and explicit-toolset delegation paths.
- **v0.3.109 Context File Injection Guard**:
  - **Hermes Prompt-Builder Safety Parity**: `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `SOUL.md`, and `.cursor/rules/*.mdc` are scanned before they enter the system prompt.
  - **Non-Leaking Blocking**: suspicious context files are replaced with a concise blocked placeholder that reports stable finding ids without exposing the original content.
  - **Shared Scanner**: the prompt-builder path reuses Hakimi's existing Rust-native prompt-injection detector, keeping context loading, file safety, and cron safety aligned.
- **v0.3.108 Configurable LLM Context Compression**:
  - **Hermes Compression Runtime Parity**: `compression.engine: llm` now selects Hakimi's LLM-backed compressor instead of silently using the smart local engine.
  - **Configurable Summary Model**: `compression.model` can choose a cheaper/faster summarization model; when empty, Hakimi uses the active chat model.
  - **Shared Runtime Wiring**: CLI and server construction now share the same context-engine factory for `smart`, `simple`, and `llm`.
- **v0.3.107 MCP Error Sanitization**:
  - **Hermes MCP Safety Parity**: MCP transport and adapter errors now strip credential-like text before surfacing failures to the agent.
  - **Remote Transport Coverage**: StreamableHTTP and SSE error bodies and JSON parse snippets are redacted before they enter `anyhow` context.
  - **Tool Error Coverage**: MCP `isError` tool results and adapter call failures are sanitized with the shared runtime redactor.
- **v0.3.106 Browser Dialog Handling**:
  - **Hermes Browser Dialog Parity**: optional Chromium automation now exposes `browser_dialog` for alert, confirm, prompt, and beforeunload dialogs.
  - **Pending Dialog Visibility**: `browser_snapshot` surfaces `pending_dialogs` when a native JavaScript dialog is blocking the page.
  - **Shared Registration**: CLI, TUI, and server browser feature builds register the same dialog responder.
- **v0.3.105 MCP Node Command Resolution**:
  - **Hermes MCP Parity**: stdio MCP servers launched as bare `node`, `npm`, or `npx` commands now recover when a narrowed server `PATH` omits common Node install directories.
  - **Stable Node Fallbacks**: resolution checks the inherited PATH first, then Hakimi-managed `~/.hakimi/node/bin`, `~/.local/bin`, and `/usr/local/bin` on Unix.
  - **Shebang-Safe Spawn**: fallback launches prepend the resolved command directory to the child PATH so `npx` can still re-exec `node`.
- **v0.3.104 Browser Console + Eval**:
  - **Hermes Browser Parity**: optional Chromium automation now includes `browser_console` for captured console output, JavaScript errors, and page-context expression evaluation.
  - **Rust-Native Capture**: a lightweight per-page recorder tracks `console.log`/`warn`/`error` calls and uncaught JS errors without adding a Python browser runtime.
  - **Shared Registration**: CLI and TUI browser feature builds expose the same shared console/eval tool.
- **v0.3.103 Browser Image Listing**:
  - **Hermes Browser Parity**: optional Chromium automation now includes `browser_get_images`, matching Hermes' current-page image extraction surface.
  - **Vision Handoff**: returns non-data image URLs with alt text and natural dimensions so agents can choose images for follow-up vision analysis.
  - **Shared Registration**: CLI and TUI browser feature builds expose the same shared browser image-listing tool.
- **v0.3.102 Browser Navigation Controls**:
  - **Hermes Browser Parity**: optional Chromium automation now includes `browser_scroll`, `browser_back`, and `browser_press` alongside navigate/snapshot/click/type/screenshot.
  - **Shared Registration**: CLI and TUI browser feature builds register the same shared browser session controls.
  - **Regression Coverage**: added offline schema and metadata coverage for the new browser tools without launching Chromium locally.
- **v0.3.101 Output Token Budget Recovery**:
  - **Hermes Context-Halving Parity**: provider errors that report `available_tokens` now lower only the retry `max_tokens` budget instead of treating the prompt as too large.
  - **Prompt Preservation**: long Anthropic turns can retry with a safe output cap while keeping the current context intact.
  - **Regression Coverage**: added parser and retry-parameter tests for canonical Anthropic output-cap errors, natural-language variants, and prompt-overflow non-matches.
- **v0.3.100 Web URL Redaction Parity**:
  - **Hermes URL Passthrough**: ordinary `http`, `https`, `ws`, `wss`, and `ftp` URLs now remain intact, including OAuth callbacks, magic links, pre-signed URLs, and request-target query strings the agent may need to follow.
  - **High-Confidence Secret Masking**: provider keys, JWTs, database connection-string passwords, private keys, bearer tokens, and pure form-urlencoded secret fields are still redacted before tool output is surfaced.
  - **Regression Coverage**: added deterministic coverage for URL passthrough, URL-embedded provider tokens, URL-embedded JWTs, and form-body redaction without live provider calls.
- **v0.3.99 TUI OSC 52 Clipboard Fallback**:
  - **Hermes Terminal Clipboard Parity**: `/copy [N]` now falls back to OSC 52 terminal clipboard output when native clipboard writers are unavailable.
  - **Remote Terminal Friendly**: SSH, tmux, and terminal-emulator workflows can still receive copied assistant responses when platform tools like `pbcopy`, `wl-copy`, or PowerShell are missing.
  - **Regression Coverage**: added deterministic coverage for the OSC 52 payload wrapper without invoking a live clipboard.
- **v0.3.98 Plugin CLI + Gateway Management**:
  - **Hermes-Style Plugin Commands**: `hakimi plugins list|templates|init|path` now exposes the existing HTTP plugin loader from the CLI.
  - **Template Scaffolding**: bundled HTTP plugin templates are embedded in the binary and scaffold safely into `~/.hakimi/plugins` without overwriting existing configs.
  - **Gateway Parity**: `/plugins list|templates|path` shares the same management surface in Telegram/Discord/Slack/Webhook chats.
- **v0.3.97 Session Title Generation**:
  - **Hermes-Style Auto Titles**: persisted sessions now receive a concise title from the first user message when no manual title exists.
  - **Collision-Safe Titles**: generated titles preserve user-set names and add a short session suffix when another session already owns the same title.
  - **Unicode-Safe Truncation**: title generation now truncates by characters instead of bytes, avoiding invalid UTF-8 boundaries.
- **v0.3.96 TUI `/history` Conversation Review**:
  - **Hermes-Style History Command**: Hakimi TUI now supports `/history [N]` and `/hist [N]` to review recent user/assistant turns without sending the command to the model.
  - **Bounded Local Review**: optional numeric limits show the latest N visible conversation messages while skipping tool/system noise.
  - **Parser + Gateway Clarity**: `/history` is part of the shared slash-command parser, and gateway chats explain that full history review belongs to the local TUI/chat surface.
- **v0.3.95 Tool Output Secret Redaction**:
  - **Hermes-Style Redactor**: shared Rust-native redaction now masks API keys, bearer tokens, private keys, JWTs, database URLs, high-confidence URL-embedded tokens, and form-urlencoded secret fields before tool output is surfaced.
  - **Output Boundary Coverage**: terminal, process, code execution, and command-plugin results now redact secrets in stdout/stderr, stored commands, diagnostics, and plugin errors.
  - **Regression Coverage**: added offline tests for redaction patterns and tool-output boundaries without calling live providers.
- **v0.3.94 TUI `/copy` Clipboard Parity**:
  - **Hermes-Style Copy Command**: Hakimi TUI now supports `/copy [N]` to copy the latest or Nth-latest assistant response to the local system clipboard.
  - **Cross-Platform Clipboard Backends**: the command tries native clipboard writers on Windows, macOS, WSL, Wayland, and X11 without adding a runtime dependency.
  - **Parser + Gateway Clarity**: `/copy` is now part of the shared slash-command parser, while gateway chats explain that local clipboard copying belongs to the TUI surface.
- **v0.3.93 Telegram Command Menu**:
  - **Complete Bot Menu**: Telegram `setMyCommands` now exposes the gateway's quality and operations commands, including `/usage`, `/doctor`, `/logs`, `/providers`, `/platforms`, `/mcp`, `/browser`, `/backup`, and `/dump`.
  - **Better Help Surface**: Gateway `/help` now uses grouped, operator-focused command sections instead of a short legacy list.
  - **Regression Coverage**: added Telegram menu coverage for key quality and operations commands.
- **v0.3.92 Terminal Workdir Fallback**:
  - **Empty Workdir Handling**: terminal tool calls now treat `workdir: ""` and whitespace-only workdirs as omitted, falling back to the tool context workdir instead of passing an empty path to `current_dir`.
  - **Regression Coverage**: added targeted coverage for empty workdir resolution and execution.
- **v0.3.91 Linux Runtime Pathing**:
  - **Canonical Gateway Binary**: managed systemd installs now prefer `~/.hakimi/bin/hakimi --gateway start` and keep `/usr/local/bin/hakimi` as a symlink/launcher.
  - **Stable Tool PATH**: terminal and process tools prefix Hakimi and Cargo managed bins before system paths, reducing systemd vs interactive shell drift.
  - **Clear Command Diagnostics**: terminal failures now distinguish missing command paths, PATH misses, and non-executable binaries.
- **v0.3.89 Cron Repeat Semantics**:
  - **Hermes-Style Repeat Limits**: cron jobs now persist `repeat` limits and completed-run counts, with `--repeat N` support for gateway and standalone `hakimi cron add`.
  - **Automatic Completion Cleanup**: scheduled and standalone tick execution now increments repeat completion after each run and removes jobs when the configured limit is reached.
  - **Regression Coverage**: added offline coverage for repeat persistence, claim filtering, scheduler cleanup, and CLI/gateway repeat parsing.
- **v0.3.88 Cron Gateway Delivery**:
  - **Origin Chat Persistence**: gateway `/cron add` now stores the creating platform/chat as an explicit `deliver` target, so scheduled output returns to the chat that created the job.
  - **Explicit Delivery Routing**: scheduler delivery now honors `local` as no-delivery and only queues explicit `platform:chat_id` targets, with comma-separated multi-target deduplication.
  - **Regression Coverage**: added offline coverage for gateway-created delivery metadata, target parsing, and local-only suppression.
- **v0.3.87 Cron Tick Execution**:
  - **Standalone Tick Entry**: top-level `hakimi cron tick` now claims due jobs from the shared SQLite store and executes them once through the same delegated cron task path used by the gateway scheduler.
  - **At-Most-Once Claiming**: gateway and standalone ticks share a persistent tick lock and advance `next_run` before execution, matching Hermes' overlap-safe scheduler semantics.
  - **Regression Coverage**: added offline coverage for locked due-job claiming, tick-lock contention, tick command parsing, and bounded tick output previews.
- **v0.3.86 Cron Status Surface**:
  - **Hermes-Style Status Entry**: `/cron status` and top-level `hakimi cron status` now summarize the shared SQLite cron store without starting an agent session.
  - **Operator Counts**: status output reports total, active, paused, and due-now jobs, plus the next scheduled job and gateway scheduler hint.
  - **Regression Coverage**: added offline coverage for gateway and standalone cron status formatting against persistent cron state.
- **v0.3.85 Cron Skill-Loaded Runs**:
  - **Runtime Skill Assembly**: scheduled cron jobs now honor persisted `skills` metadata by loading matching Hakimi skill content into the delegated cron task before execution.
  - **Assembled Prompt Guard**: skill-loaded cron prompts use the looser Hermes assembled-skill scanner, allowing security runbooks while still blocking explicit prompt-injection directives.
  - **Silent Delivery Guard**: cron jobs can return exactly `[SILENT]` or an empty response to suppress automatic gateway delivery when there is nothing new to report.
- **v0.3.84 Standalone Cron CLI**:
  - **Hermes-Style `hakimi cron` Entry**: top-level `hakimi cron` now manages the same persistent job store as gateway `/cron`, covering list/add/edit/pause/resume/run/remove without starting an agent session.
  - **Shared Safety Path**: CLI add/edit reuse the existing schedule parser, prompt-injection scan, SQLite persistence, and next-run recomputation instead of duplicating cron logic.
  - **Regression Coverage**: added top-level parsing and persistent-store delegation coverage for `hakimi cron add` and `hakimi cron edit`.
- **v0.3.83 Gateway Cron Add/Edit**:
  - **Hermes-Style Remote Scheduling**: gateway chats can now create jobs with `/cron add <schedule> <prompt>` or `/cron add <cron expr> | <prompt>`, then adjust them with `/cron edit <job-id> schedule|prompt|name <value>`.
  - **Real Tool Update Path**: the built-in `cronjob` tool now implements `action="update"` instead of only advertising it, including prompt scanning and next-run recomputation on schedule changes.
  - **Persistent Cron Metadata**: `skills`, `enabled_toolsets`, `context_from`, and `deliver` fields round-trip through the SQLite store for skill-loaded and gateway-delivered cron runs.
- **v0.3.82 Usage Pricing Estimates**:
  - **Hermes-Style Cost Surface**: gateway `/usage` now shows an estimated per-turn USD cost next to token counts and rate-limit data.
  - **Native Pricing Snapshot**: `hakimi-common` includes an offline pricing catalog for common OpenAI, Anthropic, Gemini, DeepSeek, and MiniMax routes, including cached-token handling where provider semantics expose it.
  - **Regression Coverage**: added pricing tests for cached OpenAI tokens, Anthropic cache read/write buckets, provider-prefixed models, included subscription routes, unknown models, and unavailable cache pricing.
- **v0.3.81 Video Analysis Requests**:
  - **Hermes-Style `video_analyze` Tool**: Added a Rust-native media tool that accepts HTTP/HTTPS, `file://`, or local video paths and prepares video-capable model request blocks.
  - **Format and Size Guardrails**: Supports mp4, webm, mov, avi, mkv, mpeg, and mpg inputs, detects MIME types, and rejects oversized raw/base64 payloads before model dispatch.
  - **Offline Regression Coverage**: Added schema, MIME detection, file URL, structured payload, and size-limit tests without requiring live provider calls.
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
  - **Hermes Parity Cleanup**: GAP_ANALYSIS no longer lists vision as both missing and complete.
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
  - **Parity Status Clarified**: docs and gap analysis now reflect the real boundary: basic gateway cron lifecycle control is done, while delivery wiring, standalone CLI management, and skill loading remain follow-up parity work.
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

### 🛠️ 41 Built-in Tools

- **Files**: read_file, write_file, search_files, patch
- **Shell**: terminal, process (background process management)
- **Web**: web_search, web_extract
- **Home Assistant**: ha_list_entities, ha_get_state, ha_list_services, ha_call_service
- **Memory**: memory (persistent), session_search (FTS5 full-text)
- **Code**: code_exec (Python/JS/Bash)
- **Browser**: browser_navigate, browser_snapshot, browser_click, browser_type, browser_scroll, browser_back, browser_press, browser_get_images, browser_console, browser_dialog, browser_screenshot (Chromium automation)
- **Media**: vision_analyze (image analysis), video_analyze (video analysis request), image_describe (legacy alias), image_generate, text_to_speech, transcribe_audio
- **Productivity**: todo, clarify, checkpoint (shadow git snapshots)
- **Safety**: file_safety (path protection and read-file credential guard), secret_redaction, prompt_injection_detection
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

Inside gateway chats, `/cron` now supports `list`, `status`, `add --repeat N`, `edit`, `pause <job-id>`, `resume <job-id>`, `run <job-id>`, and `remove <job-id>` against the shared SQLite-backed `cron.db`; jobs created from a gateway chat keep that `platform:chat_id` as their delivery target, while host operators can run `hakimi cron tick` to execute due jobs once with the same tick lock used by the gateway scheduler. Repeat-limited jobs track completed runs and are removed automatically when the limit is reached.

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

20+ error types auto-classified: auth failure -> rotate key; terminal OAuth failure -> quarantine credential; rate limit -> exponential backoff; context overflow -> trigger compression; model not found -> fallback model.

### 🔧 MCP (Model Context Protocol)

Full MCP client with stdio / HTTP / SSE transports. Stdio Node-based servers also recover from narrowed PATH environments by resolving `node`, `npm`, and `npx` from Hakimi-managed, user-local, and `/usr/local/bin` fallback directories. Remote MCP transport and tool error paths redact credential-like text before exposing failures to the agent. Built-in catalog of 9 popular servers (filesystem, GitHub, Brave Search, PostgreSQL, Puppeteer, memory, fetch, SQLite, sequential-thinking).

### 📦 Plugin System

```yaml
# ~/.hakimi/plugins/weather.yaml
name: weather
version: "1.0"
description: "Weather lookup plugin backed by wttr.in"
tools:
  - name: get_weather
    endpoint: "https://wttr.in/{city}?format=j1"
    method: GET
    description: "Get weather for a city"
```

Bundled HTTP plugin templates are embedded in the CLI. Use `hakimi plugins templates` to browse, `hakimi plugins init weather [name]` to scaffold into `~/.hakimi/plugins`, and `hakimi plugins list --plain` / `hakimi plugins list --json` or gateway `/plugins list` to inspect loaded plugins.

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
│   ├── hakimi-tools/       # 40 built-in tools + registry
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
| Tests | ~500 | 1273 |

---

## Development

```bash
# Build everything
cargo build --workspace

# Run all tests (1273 tests)
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
- [x] 40 built-in tools
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
