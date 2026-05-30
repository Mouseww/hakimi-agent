# GAP ANALYSIS: Hermes Agent vs Hakimi Agent

Generated: 2026-05-21

---

## COMPLETE in Hakimi (match found)

### Core Tools
- **read_file** — File reading with line numbers and pagination
- **write_file** — File writing with auto-directory creation
- **patch** — Find-and-replace edits in files
- **search_files** — Content search (regex) and file search (glob)
- **terminal** — Shell command execution (foreground + background)
- **Terminal shell hooks** — Opt-in `HAKIMI_PRE_TOOL_HOOK` / `HAKIMI_POST_TOOL_HOOK` commands receive Hermes-style terminal tool payloads and can block unsafe pre-tool calls
- **process** — Background process management (list, poll, log, wait, kill, write, submit)
- **web_search** — Web search via DuckDuckGo scraping
- **todo** — Task planning and tracking
- **memory** — Persistent memory (file-backed `MEMORY.md`/`USER.md`)
- **session_search** — FTS5 search across past session transcripts
- **delegate_task** — Subagent spawning with isolated context and toolset filtering
- **skill_manage** — Skill loading and management from markdown files
- **send_message** — Cross-platform messaging via gateway
- **code_exec** — Code execution tool (similar to execute_code)
- **web_extract** — URL content extraction with HTML cleaning, readability fallback, markdown/raw output
- **image_generate** — AI image generation with OpenAI/FAL backends and local file output
- **text_to_speech** — OpenAI-compatible + Edge TTS with local audio file output
- **transcribe_audio** — OpenAI-compatible speech-to-text for local audio files and remote audio URLs
- **Home Assistant tools** — `ha_list_entities`, `ha_get_state`, `ha_list_services`, `ha_call_service` via HA REST API with guarded service calls
- **video_analyze** — Video analysis request payloads for HTTP/HTTPS, `file://`, and local video files with MIME detection and size guardrails
- **Browser automation (basic)** — Optional `browser` feature with shared Chromium session controls: `browser_navigate`, `browser_snapshot`, `browser_click`, `browser_type`, `browser_scroll`, `browser_back`, `browser_press`, `browser_get_images`, `browser_console`, `browser_dialog`, and `browser_screenshot`

### Runtime Environment
- **Linux install/gateway path hygiene** — The real binary stays under `~/.hakimi/bin/hakimi`, `/usr/local/bin/hakimi` is maintained as a symlink/launcher, and managed systemd gateway units prefer the canonical binary path with a stable service PATH (`~/.hakimi/bin:~/.cargo/bin:/usr/local/bin:/usr/bin:/bin`).
- **Terminal PATH diagnostics** — Terminal/process commands prefix the current PATH with Hakimi's managed bins, and foreground terminal failures distinguish missing explicit paths, PATH misses, non-executable binaries, and systemd/Hakimi vs interactive shell PATH drift.

### Agent Loop
- **Core conversation loop** — Message → LLM → tool dispatch → loop until done
- **Iteration budget** — Max iterations cap (configurable, default 90)
- **Interrupt handling** — AtomicBool-based interrupt checking
- **Streaming support** — `execute_streaming()` on transport trait with `StreamAccumulator`
- **Builder pattern** — `AIAgent::builder()` construction

### Transports
- **Chat Completions** — OpenAI-compatible API
- **Anthropic** — Anthropic Messages API
- **Gemini** — Google Gemini native API

### Context Management
- **ContextEngine trait** — Pluggable context engine abstraction
- **ContextCompressor** — Threshold-based compression trigger
- **SmartContextEngine** — 3-tier compression (drop tool results → summarize → sliding window)
- **LlmCompressor runtime wiring** — `compression.engine: llm` uses a configured summary model with local fallback, question tracking, and large tool-output pruning
- **SimpleContextEngine** — Basic truncation-based compression
- **StreamingContextScrubber** — Removes `<memory-context>` blocks during streaming
- **Token usage tracking** — `update_from_response()` with Usage struct

### Session & Storage
- **SQLite session store** — WAL mode, busy timeout, foreign keys
- **FTS5 full-text search** — Message content indexing
- **Message CRUD** — Save, retrieve, search messages
- **Session metadata** — ID, source, user, model, timestamps, message counts, token counts
- **Auto-generated session titles** — First user message names untitled persisted sessions with collision-safe, Unicode-safe titles
- **Rust-native backup/import** — `hakimi backup` and `hakimi import` archive and restore user state with traversal guards, binary/cache exclusions, symlink skipping, and SQLite snapshot support

### Memory
- **MemoryProvider trait** — `system_prompt_block()`, `prefetch()`, `handle_tool_call()`
- **FileMemoryProvider** — Reads `~/.hermes/memory/` directory files into system prompt

### Skills
- **SkillLoader** — Loads `.md` files with YAML frontmatter from a directory
- **SkillStore** — In-memory skill storage
- **Skill struct** — Name, content, frontmatter metadata
- **Skills Hub manifest install policy** — `hakimi skills browse|search|inspect|install|list|path` and gateway `/skills browse|search|inspect|install` use a local `.hub/index.json` manifest, require explicit community trust, scan SKILL.md before install, and record lock/audit provenance
- **Skills platform-gated loading** — `SKILL.md` frontmatter `platforms` scalar/list metadata is parsed and incompatible OS-specific skills are skipped before runtime prompt injection
- **Skills template preprocessing** — Runtime skill bodies resolve `${HERMES_SKILL_DIR}` / `${HAKIMI_SKILL_DIR}` and session-id aliases before prompt injection; trusted callers can opt into bounded inline-shell expansion
- **Skills usage telemetry** — Runtime skill activation records non-sensitive use/view counters in `.usage.json`, and `hakimi skills usage` plus gateway `/skills usage` expose the sidecar for operator inspection
- **Skills bundled sync/update** — `hakimi skills sync --source <dir>` seeds bundled skills with `.bundled_manifest` origin hashes, updates only unmodified synced copies, preserves user edits/deletions, and exposes the same summary/JSON path through gateway `/skills sync`
- **Skills Hub source indexes** — `hakimi skills sources list|add|refresh|remove` registers local or HTTPS hub indexes, refreshes them into `.hub/index-cache`, and merges refreshed caches with `.hub/index.json` for browse/search/inspect/install while keeping HTTPS and trust boundaries explicit

### MCP
- **McpClient** — stdio transport, JSON-RPC 2.0, with Hermes-style Node command fallback for narrowed PATH environments, credential-stripped remote error surfaces, and gateway `/mcp list` inventory over configured servers
- **McpToolAdapter** — Adapts MCP tools to Hakimi's Tool trait
- **Protocol support** — initialize, tools/list, tools/call, StreamableHTTP, SSE

### Cron
- **CronScheduler** — In-memory job scheduling
- **CronJob** — Name, schedule, prompt, enabled flag, last/next run
- **Interval parsing** — `30m`, `2h` syntax
- **Tick-based execution** — `next_tick()` returns due job IDs
- **Cron prompt injection scanning** — Strict create/store/run-time scan plus looser assembled-skill scan mirroring Hermes cron security

### Gateway
- **PlatformAdapter trait** — connect, send_message, disconnect, take_receiver
- **Gateway** — Central message routing, adapter registration
- **Gateway ingress access policy** — Config-driven allowlist merges global gateway users, Telegram user IDs, role allowlists, and ClawBot sender IDs before command/agent handling
- **Gateway fresh-final streaming** — Configurable `gateways.streaming.fresh_final_after_seconds` sends long streamed completions as a fresh final message and lets Telegram clean up stale preview bubbles
- **Gateway stream pacing** — Configurable `gateways.streaming.edit_interval_ms` and `buffer_threshold_chars` control progressive edit cadence and force pending-text flushes before tool/media/delegate boundaries
- **Gateway silence-narration filter** — Configurable outbound guard drops bare loop-prone silence narration such as `*(silent)*`, `.`, `...`, `…`, `🔇`, `silent`, `no response`, and `no reply` before chat adapters send it
- **Telegram adapter** — Telegram Bot API integration
- **Discord adapter** — Discord bot with embeds
- **Slack adapter** — Slack bot with blocks

### Plugin System
- **Plugin trait** — name, version, description, tools, init
- **PluginLoader** — Directory-based discovery, HTTP tool plugins
- **Plugin CLI/templates** — `hakimi plugins list|templates|init|path` plus gateway `/plugins` inspection for HTTP plugins; `plugins list` supports Hermes-style `--plain` and `--json` output
- **Progressive tool disclosure** — Hermes-style `tool_search`, `tool_describe`, and `tool_call` bridge tools defer MCP/plugin schemas once their token estimate crosses the configured context threshold while core Hakimi tools stay directly visible

### Retry & Error
- **Jittered backoff** — Exponential backoff with random jitter
- **should_retry()** — Transport/IO errors retryable, tool/config errors not
- **HakimiError enum** — Transport, Tool, Config, Session, Context, Io, Json, Other
- **Responses stream recovery** — Incomplete Responses SSE maps to continuation, and truncated streams retry before surfacing partial output
- **Output-token budget recovery** — Provider errors with `available_tokens` lower only the retry `max_tokens` budget, preserving the current prompt/context instead of forcing context compression
- **Credential pool terminal auth quarantine** — 401 OAuth terminal reasons mark credentials `dead`, keep them out of rotation without TTL re-entry, and preserve last status/reason for diagnostics until explicit re-auth
- **Think scrubber** — Stateful Hermes-style removal of reasoning/thinking blocks from streaming and non-streaming assistant content

### Config
- **YAML config** — model, terminal, agent, compression, display, delegation, mcp_servers, gateway ingress policy, gateway silence-narration filtering
- **Profile support** — `--profile` CLI flag
- **Defaults** — Sensible defaults via `serde(default)`

### CLI
- **Interactive REPL** — Input loop with slash commands
- **Slash commands** — /help, /quit, /clear, /model, /config, /resume, /history, /tools, /skills, /status, /usage
- **Single-query mode** — `--query` flag
- **YOLO mode** — `--yolo` auto-accept
- **Serve mode** — `--serve` HTTP API server

### Server
- **REST API** — Health, chat, sessions, tools, config endpoints (Axum)

### TUI
- **Ratatui TUI** — Terminal UI with chat panel, tools activity panel, status bar
- **TUI `/history [N]` command** — Reviews recent user/assistant turns locally without sending the command to the model
- **TUI `/copy [N]` clipboard command** — Copies the latest or Nth-latest assistant response through native Windows/macOS/WSL/Wayland/X11 clipboard writers plus OSC 52 terminal fallback
- **Spinner animation** — Thinking indicator
- **Key handling** — Ctrl+C quit, input editing, scrolling

### Prompt Building
- **System prompt assembly** — Identity, platform hints, skills, memory, environment hints
- **Platform-specific formatting** — Telegram, Discord, Slack markdown hints
- **Context file injection guard** — AGENTS.md, CLAUDE.md, .cursorrules, SOUL.md, and `.cursor/rules/*.mdc` are scanned before system prompt injection; suspicious content is replaced with a non-leaking blocked placeholder

### Delegation
- **CoreDelegateExecutor** — Spawns child agents with filtered tool registries
- **Toolset filtering** — Only includes tools from specified toolsets
- **Timeout** — Default 60s delegation timeout

### Knowledge (stub)
- **KnowledgeGraph** — Graph store with node/edge types (crate exists but minimal)

---

## MISSING from Hakimi

### Critical Priority

#### 1. Browser Automation (remaining advanced suite)
- **What**: Advanced browser suite beyond the basic Chromium controls already present: vision, CDP attach, cloud/browser-provider routing
- **Hermes location**: `tools/browser_tool.py`, `tools/browser_camofox.py`, `tools/browser_cdp_tool.py`, `tools/browser_dialog_tool.py`, `tools/browser_supervisor.py`, `tools/browser_providers/`
- **Details**: Hakimi now covers navigate, snapshot, click, type, scroll, back, press, image listing, console/error capture, page-context expression evaluation, JavaScript dialog accept/dismiss, and screenshot through the optional Rust-native Chromium feature. Remaining parity is multi-backend support (Browserbase/Browser Use/Camofox/CDP) and vision routing.
- **Priority**: **Critical** — Core capability for web interaction beyond search

#### 2. Credential Pool / Multi-Credential Failover
- **What**: Persistent multi-credential pool for same-provider failover with round-robin and fill-first strategies
- **Hermes location**: `agent/credential_pool.py`
- **Details**: Hakimi supports API-key pools, round-robin/fill-first/random/least-used strategies, temporary exhaustion, and Hermes-style terminal auth quarantine for 401 OAuth reasons such as `token_revoked`, `invalid_grant`, and `refresh_token_reused`. Remaining parity is persisted OAuth singleton syncing/refresh, write-side re-auth clearing, and live integration with richer recovery loops.
- **Priority**: **Critical** — Production reliability for high-traffic deployments

#### 3. Error Classifier (Rich Taxonomy)
- **What**: Structured API error classification with priority-ordered recovery strategies
- **Hermes location**: `agent/error_classifier.py`
- **Details**: 20+ FailoverReason enums (auth, billing, rate_limit, overloaded, context_overflow, model_not_found, thinking_signature, etc.). Each maps to a recovery action (retry, rotate, fallback, compress, abort). Hakimi only has basic Transport/IO retry.
- **Priority**: **Critical** — Production-grade error handling

#### 4. ~~Prompt Caching (Anthropic-specific)~~ ✅ DONE
- **What**: Anthropic prompt caching with TTL-aware cache breakpoints
- **Hermes location**: `agent/prompt_caching.py`
- **Status**: ✅ Done in v0.3.107 — `hakimi-transports/src/prompt_caching.rs` supports `system_and_3` and `prefix_and_2`, TTL-aware 5m/1h `cache_control`, tool/schema/message breakpoints, and Anthropic beta header wiring.

### High Priority

#### 8. Clarify Tool
- **What**: Agent can present structured multiple-choice or open-ended questions to the user
- **Hermes location**: `tools/clarify_tool.py`
- **Details**: CLI: arrow-key navigation. Gateway: numbered list. Max 4 choices + "Other" option.
- **Priority**: **High** — Important for interactive workflows

#### 9. ~~Home Assistant Integration (4 tools)~~ ✅ DONE
- **What**: Smart home control via Home Assistant REST API
- **Hermes location**: `tools/homeassistant_tool.py`
- **Status**: ✅ Done in v0.3.75 — `ha_list_entities`, `ha_get_state`, `ha_list_services`, and `ha_call_service` use `HASS_TOKEN` / `HASS_URL`, validate path components, block high-risk HA domains, and return compact JSON summaries

#### 10. Computer Use (macOS Desktop Control)
- **What**: Background macOS desktop control via cua-driver
- **Hermes location**: `tools/computer_use_tool.py`, `tools/computer_use/`
- **Details**: Screenshots, mouse, keyboard, scroll, drag. Does NOT steal user's cursor/focus. Works with any tool-capable model.
- **Priority**: **High** — Desktop automation capability

#### 11. Mixture-of-Agents (MoA)
- **What**: Multi-model collaboration for enhanced reasoning on complex tasks
- **Hermes location**: `tools/mixture_of_agents_tool.py`
- **Details**: Reference models generate parallel responses, aggregator synthesizes. Uses claude-opus-4.6, gemini-3-pro, gpt-5.4-pro, deepseek-v3.2.
- **Priority**: **High** — Advanced reasoning capability

#### 12. Kanban Multi-Agent Coordination (9 tools)
- **What**: Durable SQLite-backed board for multi-agent task collaboration
- **Hermes location**: `tools/kanban_tools.py`, `hermes_cli/kanban.py`, `hermes_cli/kanban_db.py`
- **Details**: kanban_show, kanban_list, kanban_complete, kanban_block, kanban_heartbeat, kanban_comment, kanban_create, kanban_link, kanban_unblock. Dispatcher spawns workers.
- **Priority**: **High** — Multi-agent orchestration

#### 13. Gateway Platform Adapters (17+ missing)
- **What**: All gateway platforms beyond Telegram/Discord/Slack
- **Hermes location**: `gateway/platforms/`
- **Missing**: whatsapp, signal, matrix, mattermost, email, sms, dingtalk, wecom, weixin, feishu, qqbot, bluebubbles, yuanbao, webhook, api_server, homeassistant, msgraph_webhook
- **Priority**: **High** — Platform reach

#### 14. Bedrock Transport
- **What**: AWS Bedrock Converse API native integration
- **Hermes location**: `agent/bedrock_adapter.py`, `agent/transports/bedrock.py`
- **Details**: Native Converse API, AWS credential chain (IAM, SSO, env, instance metadata), dynamic model discovery, guardrails support, cross-region inference profiles.
- **Priority**: **High** — AWS ecosystem integration

#### 15. Plugin System — Memory Providers (8+ backends)
- **What**: Pluggable memory backends with dedicated providers
- **Hermes location**: `plugins/memory/`, `agent/memory_manager.py`, `agent/memory_provider.py`
- **Missing providers**: honcho, mem0, supermemory, byterover, hindsight, holographic, openviking, retaindb
- **Details**: MemoryManager orchestrates providers. Lifecycle hooks: sync_turn, prefetch, shutdown, post_setup. Only one external provider at a time.
- **Priority**: **High** — Advanced memory/context persistence

#### 16. Plugin System — Model Provider Plugins
- **What**: Inference backend plugins (openrouter, anthropic, gmi, etc.)
- **Hermes location**: `plugins/model-providers/`
- **Details**: ProviderProfile-based registration. Auto-coercion via source-text heuristic. Full authoring guide.
- **Priority**: **High** — Provider ecosystem extensibility

#### 17. ACP Adapter (IDE Integration)
- **What**: Agent Client Protocol server for VS Code / Zed / JetBrains integration
- **Hermes location**: `acp_adapter/`
- **Details**: Exposes Hermes via ACP for IDE integration. Session management, tool dispatch, auth, permissions.
- **Priority**: **High** — Developer workflow integration

#### 18. Profiles System
- **What**: Multiple isolated Hermes instances with separate config, memory, sessions, skills
- **Hermes location**: `hermes_cli/profiles.py`, `hermes_cli/profile_distribution.py`
- **Details**: `hermes profile create/delete/use`. Each profile is a full HERMES_HOME. Clone support. Wrapper aliases. `-p` flag.
- **Priority**: **High** — Multi-context workflows

#### 19. Setup Wizard
- **What**: Interactive first-run configuration wizard
- **Hermes location**: `hermes_cli/setup.py`
- **Details**: Modular sections: Model & Provider, Terminal Backend, Agent Settings, Messaging Platforms, Tools configuration.
- **Priority**: **High** — User onboarding

#### 20. Cron — Persistent File-Based with Full CLI
- **What**: Persistent cron job store with file-based locking, CLI management, slash commands
- **Hermes location**: `cron/jobs.py`, `cron/scheduler.py`, `hermes_cli/cron.py`, `tools/cronjob_tools.py`
- **Details**: File-based tick lock for multi-process safety. `hermes cron list/add/edit/pause/resume/run/remove/status/tick`. Standalone `hakimi cron list/status/add/edit/pause/resume/run/remove/tick`, gateway `/cron status/add/edit/list/pause/resume/run/remove`, and `cronjob create/update/list/pause/resume/run/remove` are now covered in Hakimi; scheduled and standalone tick runs now assemble attached skills with Hermes-style prompt scanning, `[SILENT]` suppression, overlap-safe due-job claiming, explicit `platform:chat_id` gateway delivery, and Hermes-style repeat limits with completed-run tracking and automatic cleanup at the limit. Deeper delivery expansion remains.
- **Priority**: **High** — Remaining work is home-channel/all/plugin delivery expansion

### Medium Priority

#### 21. ~~Vision Analysis (vision_analyze tool)~~ ✅ DONE
- **What**: Image analysis from URLs with custom prompts using vision-capable models
- **Hermes location**: `tools/vision_tools.py`
- **Status**: ✅ Done in v0.3.74 — `vision_analyze` and legacy `image_describe` both produce structured base64 data-url vision request payloads for URLs and local files

#### 22. ~~Video Analysis~~ ✅ DONE
- **What**: Video analysis and understanding (opt-in toolset)
- **Hermes location**: `tools/vision_tools.py` (`video_analyze`)
- **Status**: ✅ Done in v0.3.81 — `video_analyze` accepts HTTP/HTTPS, `file://`, and local paths, supports mp4/webm/mov/avi/mkv/mpeg/mpg, and returns structured video-capable model request blocks with raw/base64 size checks

#### 23. RL Training Tools (10 tools)
- **What**: Reinforcement learning training via Tinker-Atropos
- **Hermes location**: `tools/rl_training_tool.py`, `environments/`
- **Details**: rl_list_environments, rl_select_environment, rl_get_current_config, rl_edit_config, rl_start_training, rl_check_status, rl_stop_training, rl_get_results, rl_list_runs, rl_test_inference
- **Priority**: **Medium** — Specialized ML workflow

#### 24. MCP — HTTP/SSE Transports + Sampling
- **What**: MCP support beyond stdio: HTTP/StreamableHTTP, SSE transports, server-initiated sampling
- **Hermes location**: `tools/mcp_tool.py`
- **Details**: Hakimi now supports stdio, StreamableHTTP, SSE, configurable timeouts, automatic SSE reconnection, narrowed-PATH Node recovery, credential stripping in remote MCP errors, and stdio server-initiated `sampling/createMessage` backed by the configured Hakimi LLM transport. Remaining parity is richer HTTP/SSE server-initiated flow handling, sampling tool-use loops, and the fuller background event-loop architecture.
- **Priority**: **Medium** — Remote MCP server support

#### 25. Context Engine Plugin System
- **What**: Pluggable context engine replacement via plugin system
- **Hermes location**: `agent/context_engine.py`, `plugins/context_engine/`
- **Details**: Abstract base class with lifecycle hooks (on_session_start, update_from_response, should_compress, compress, on_session_end). Third-party engines can replace built-in compressor.
- **Priority**: **Medium** — Hakimi has the trait but no plugin discovery for context engines

#### 26. ~~LLM-Based Context Compression~~ ✅ DONE
- **What**: Uses auxiliary LLM (cheap/fast) to summarize middle turns with structured templates
- **Hermes location**: `agent/context_compressor.py`, `agent/auxiliary_client.py`
- **Status**: ✅ Done in v0.3.108 — `compression.engine: llm` now selects `LlmCompressor`, uses `compression.model` or the active model for structured summarization, preserves local fallback, tracks resolved/pending questions, prunes large tool outputs, and is wired through both CLI and server construction.

#### 27. Tool Guardrails
- **What**: Pure tool-call loop detection, idempotency tracking, and turn-halt decisions
- **Hermes location**: `agent/tool_guardrails.py`
- **Details**: Tracks per-turn tool-call observations. Detects infinite loops, repeated identical calls. Returns decisions for warning/synthetic-result/halt.
- **Priority**: **Medium** — Safety and cost control

#### 28. ~~File Safety / Path Security~~ ✅ DONE
- **What**: Write-denied paths, path traversal protection, symlink resolution
- **Hermes location**: `agent/file_safety.py`, `tools/path_security.py`
- **Status**: ✅ Done in v0.3.111 — `read_file` now applies a shared Hakimi credential read guard before opening files, covering `config.yaml`, OAuth/token stores, project `.env*` files, `mcp-tokens/`, and Hermes' latest `cache/bws_cache.json` pattern. Existing paths are canonicalized before matching, and Windows absolute paths are resolved with `Path::is_absolute()`.

#### 29. ~~Secret Redaction~~ ✅ DONE
- **What**: Regex-based secret masking for logs and tool output
- **Hermes location**: `agent/redact.py`
- **Status**: ✅ Done in v0.3.100 — `hakimi-common::redact_sensitive_text()` masks provider keys, bearer tokens, private keys, JWTs, database connection-string passwords, high-confidence URL-embedded tokens, pure form-urlencoded secret fields, and JSON/env secret carriers while preserving ordinary Web URLs for OAuth callbacks, magic links, pre-signed URLs, and request targets; terminal/process/code_exec/command-plugin output boundaries redact stdout, stderr, diagnostics, stored commands, and plugin errors before surfacing them.

#### 30. ~~Prompt Injection Detection~~ ✅ DONE
- **What**: Scans context files (AGENTS.md, .cursorrules, SOUL.md) for injection patterns before system prompt injection
- **Hermes location**: `agent/prompt_builder.py` (`_CONTEXT_THREAT_PATTERNS`)
- **Status**: ✅ Done in v0.3.109 — `hakimi-context::build_context_files_prompt()` now scans project context files with the shared prompt-injection detector before injecting them into the system prompt. Matching files are replaced by a concise blocked placeholder that reports finding ids without leaking the original content.

#### 31. ~~Cron Prompt Injection Scanning~~ ✅ DONE
- **What**: Scans user-authored cron prompts before persistence/manual trigger and again before auto execution; uses looser assembled-skill scan for skill-loaded cron prompts
- **Hakimi implementation**: `hakimi-cron/src/lib.rs`, `hakimi-cron/src/persistence.rs`, `hakimi-tools/src/builtin_cronjob.rs`, `hakimi-cli/src/entry.rs`
- **Hermes location**: `tools/cronjob_tools.py` (`_scan_cron_prompt`, `_scan_cron_skill_assembled`), `cron/scheduler.py` (`CronPromptInjectionBlocked`)
- **Status**: ✅ Done in v0.3.72 — unsafe jobs are blocked, disabled on scheduled execution, and reported through gateway queue

#### 32. i18n (Internationalization)
- **What**: Lightweight i18n for static user-facing messages
- **Hermes location**: `agent/i18n.py`
- **Details**: Locale YAML catalogs. Dotted key paths. Fallback to English. Used for approval prompts, gateway replies, restart notices.
- **Priority**: **Medium** — Multi-language support

#### 33. Onboarding Hints
- **What**: Contextual first-touch hints instead of blocking questionnaires
- **Hermes location**: `agent/onboarding.py`
- **Details**: One-time hints triggered by behavior forks. Tracked in config.yaml under `onboarding.seen.<flag>`.
- **Priority**: **Medium** — User experience

#### 34. ~~Doctor Diagnostics~~ ✅ DONE
- **What**: CLI command to diagnose setup issues
- **Hermes location**: `hermes_cli/doctor.py`
- **Status**: ✅ Done in v0.3.76 — `hakimi doctor`, `hakimi --doctor`, and gateway `/doctor` run diagnostics for dependencies, config, env vars, paths, and API connectivity; gateway output is ANSI-free for chat surfaces

#### 35. Batch Runner
- **What**: Parallel batch processing across multiple prompts from a dataset
- **Hermes location**: `batch_runner.py`
- **Details**: Dataset loading, parallel processing with multiprocessing, checkpointing for fault tolerance, trajectory saving, tool usage statistics.
- **Priority**: **Medium** — Evaluation/benchmarking workflows

#### 36. Trajectory Saving
- **What**: Save conversation trajectories in structured format (from/value pairs)
- **Hermes location**: `agent/trajectory.py`
- **Details**: For training data generation and debugging. Controlled by `save_trajectories` flag.
- **Priority**: **Medium** — ML training pipeline

#### 37. Checkpoint Manager (Filesystem Snapshots)
- **What**: Transparent shadow-git snapshots before file-mutating operations
- **Hermes location**: `tools/checkpoint_manager.py`
- **Details**: Auto-snapshots of working directories. Single shared git store with deduplication. Rollback to any previous checkpoint. NOT visible to LLM — transparent infrastructure.
- **Priority**: **Medium** — Safety net for file operations

#### 38. Skin Engine (CLI Theming)
- **What**: Data-driven CLI theming system
- **Hermes location**: `hermes_cli/skin_engine.py`
- **Details**: Customize banner colors, spinner faces/verbs/wings, tool prefix, response box, branding text. Config-driven via `display.skin`.
- **Priority**: **Medium** — CLI customization

#### 39. Gateway Streaming Consumer
- **What**: Bridges sync agent callbacks to async platform delivery with progressive message editing
- **Hermes location**: `gateway/stream_consumer.py`
- **Details**: Hakimi now has progressive gateway edits, tool/media/delegate side-channel segmentation, final delivery de-duplication, Hermes-style fresh-final completion via `gateways.streaming.fresh_final_after_seconds` with Telegram stale-preview cleanup, configurable edit interval/buffer threshold, and a default-on silence-narration filter for loop-prone bare tokens. Remaining parity is native draft transport, overflow chunking, flood-control backoff, and per-platform display policy.
- **Priority**: **Medium** — Real-time streaming UX on messaging platforms

#### 40. Usage Pricing / Account Usage Tracking
- **What**: Token usage pricing calculation and account usage aggregation
- **Hermes location**: `agent/usage_pricing.py`, `agent/account_usage.py`
- **Details**: Per-model cost estimation, account usage aggregation, and provider usage surfaces. Rate-limit header parsing/tracking, gateway `/usage`, and offline per-turn cost estimates are implemented; provider account usage APIs, persisted aggregation, and live model pricing discovery are still missing.
- **Priority**: **Medium** — Cost visibility

#### 41. Model Metadata / Auto-Discovery
- **What**: Model context length metadata, auto-discovery from providers
- **Hermes location**: `agent/model_metadata.py`, `agent/models_dev.py`
- **Details**: `get_model_context_length()`, `MINIMUM_CONTEXT_LENGTH`. Provider-specific model catalogs.
- **Priority**: **Medium** — Correct context window sizing

### Low Priority

#### 42. Observability Plugin
- **What**: Metrics, traces, and logs plugin
- **Hermes location**: `plugins/observability/`
- **Priority**: **Low** — Production monitoring

#### 43. Achievements Plugin
- **What**: Gamified achievement tracking
- **Hermes location**: `plugins/hermes-achievements/`
- **Priority**: **Low** — Engagement feature

#### 44. Spotify Plugin
- **What**: Spotify integration
- **Hermes location**: `plugins/spotify/`
- **Priority**: **Low** — Entertainment

#### 45. Google Meet Plugin
- **What**: Google Meet integration
- **Hermes location**: `plugins/google_meet/`
- **Priority**: **Low** — Meeting integration

#### 46. Disk Cleanup Plugin
- **What**: Disk space management
- **Hermes location**: `plugins/disk-cleanup/`
- **Priority**: **Low** — Maintenance

#### 47. Voice Mode (Push-to-Talk)
- **What**: Audio recording and playback for CLI with STT dispatch
- **Hermes location**: `tools/voice_mode.py`
- **Details**: sounddevice capture, WAV encoding, STT via transcription_tools, TTS playback. Hakimi now has `text_to_speech` + `transcribe_audio`, but still lacks interactive CLI capture/playback.
- **Priority**: **Low** — Niche CLI feature

#### 49. Curator
- **What**: Conversation curation and quality tracking
- **Hermes location**: `agent/curator.py`, `agent/curator_backup.py`, `hermes_cli/curator.py`
- **Priority**: **Low** — Quality assurance

#### 50. ~~Think Scrubber~~ ✅ DONE
- **What**: Removes reasoning/thinking blocks from responses before display and persisted assistant history
- **Hermes location**: `agent/think_scrubber.py`
- **Status**: ✅ Done in v0.3.77 — stateful tag scrubber handles `<think>`, `<thinking>`, `<reasoning>`, `<thought>`, and `<REASONING_SCRATCHPAD>` across streaming delta boundaries; non-streaming responses are scrubbed before final_response/session storage

#### 51. ~~Title Generator~~ ✅ DONE
- **What**: Auto-generates session titles from conversation content
- **Hermes location**: `agent/title_generator.py`
- **Status**: ✅ Done in v0.3.97 — persisted sessions now derive a concise title from the first user message when no manual title exists, preserve existing titles, avoid duplicate-title conflicts with a short session suffix, and truncate Unicode safely

#### 52. KawaiiSpinner / Display System
- **What**: Animated spinner faces during API calls, activity feed for tool results
- **Hermes location**: `agent/display.py`
- **Details**: `KawaiiSpinner` with configurable faces. `┊` activity feed.
- **Priority**: **Low** — CLI aesthetics (Hakimi has basic spinner in TUI)

#### 53. Nous Rate Guard
- **What**: Rate limiting specific to Nous provider
- **Hermes location**: `agent/nous_rate_guard.py`
- **Priority**: **Low** — Provider-specific

#### 54. ~~Shell Hooks (terminal pre/post slice)~~ ✅ DONE
- **What**: Pre/post command execution hooks
- **Hermes location**: `agent/shell_hooks.py`
- **Status**: ✅ Done in v0.3.114 for the terminal-tool execution boundary — `HAKIMI_PRE_TOOL_HOOK` and `HAKIMI_POST_TOOL_HOOK` execute local hook commands with Hermes-style JSON payloads on stdin; pre hooks can return either canonical `action:block` or Claude-Code-style `decision:block` JSON to stop terminal execution before the command runs. Full Hermes plugin-manager hook registration and consent allowlist remain future extension work.

#### 55. ~~Clipboard Integration~~ ✅ DONE
- **What**: Copy output to clipboard
- **Hermes location**: `cli.py`, `hermes_cli/commands.py`, `website/docs/reference/slash-commands.md`
- **Status**: ✅ Done in v0.3.99 — TUI `/copy [N]` copies the latest or Nth-latest assistant response to the local clipboard using native platform writers plus Hermes-style OSC 52 fallback; gateway chats surface a clear local-only notice instead of pretending remote clipboard access exists

#### 56. PTY Bridge
- **What**: Pseudo-terminal bridge for interactive CLI tools
- **Hermes location**: `hermes_cli/pty_bridge.py`
- **Priority**: **Low** — Advanced terminal

#### 57. Web Server / Dashboard
- **What**: Web-based dashboard with embedded PTY terminal
- **Hermes location**: `hermes_cli/web_server.py`
- **Details**: React dashboard with xterm.js, WebSocket PTY, REST API.
- **Priority**: **Low** — Web UI (Hakimi has basic REST server)

#### 58. Feishu/Lark Document Tools
- **What**: Feishu document and drive integration
- **Hermes location**: `tools/feishu_doc_tool.py`, `tools/feishu_drive_tool.py`
- **Priority**: **Low** — Enterprise Chinese platform

#### 59. URL Safety / Tirith Security
- **What**: URL safety checking and security policy enforcement
- **Hermes location**: `tools/url_safety.py`, `tools/tirith_security.py`
- **Priority**: **Low** — Security

#### 60. Image Routing / Generation Registry
- **What**: Multi-provider image generation routing with model registry
- **Hermes location**: `agent/image_gen_provider.py`, `agent/image_gen_registry.py`, `agent/image_routing.py`
- **Priority**: **Low** — Advanced image gen

---

## PARTIALLY IMPLEMENTED in Hakimi

### 2. Cron System
- **Status**: SQLite 持久化、file lock、cronjob tool `create|list|update|pause|resume|remove|run`、gateway `/cron status|list|add|edit|pause|resume|run|remove`、独立 CLI `hakimi cron status|list|add|edit|pause|resume|run|remove|tick`、prompt injection 扫描、cron 扩展元数据持久化、skill-loaded scheduled runs、standalone tick 执行、`[SILENT]` 投递抑制、gateway 创建任务的显式 `platform:chat_id` 定向投递，以及 repeat 上限/完成次数追踪与到达上限自动清理已落地
- **What's missing**: home-channel/all/plugin delivery expansion
- **Hermes reference**: `cron/jobs.py`, `cron/scheduler.py`, `tools/cronjob_tools.py`

### 3. MCP Client
- **Status**: stdio, StreamableHTTP, and SSE transports work; SSE has reconnect backoff; Node-based stdio servers recover from narrowed PATH; remote transport/adapter error messages are credential-stripped before surfacing to the agent; stdio MCP servers can issue `sampling/createMessage` through Hakimi's configured LLM transport
- **What's missing**: sampling tool-use loops, richer HTTP/SSE server-initiated flow handling, thread-safe background event loop
- **Hermes reference**: `tools/mcp_tool.py`

### 4. Skills System
- **Status**: Basic loader from markdown files with YAML frontmatter plus a Hermes-style safety scan that blocks dangerous prompt-injection, exfiltration, persistence, destructive, invisible-Unicode, and embedded-credential patterns before skill content enters the runtime system prompt. The loader skips symlinked skill paths, carries Hermes-style skill provenance from `metadata.hermes`, explicit `provenance` frontmatter, and `.hub/lock.json`, supports Hermes-style `platforms` frontmatter gates, preprocesses skill templates with skill-dir/session variables plus opt-in inline-shell expansion, has a Skills Hub workflow for local and refreshed multi-source indexes with explicit community trust, safe bundle-path checks, lock updates, and audit logging, records runtime skill activation in a non-sensitive `.usage.json` sidecar, can sync bundled skill trees with `.bundled_manifest` origin hashes while preserving user edits/deletions, and injects loaded skill slash-command invocations as normal user messages from CLI query and gateway chats.
- **What's missing**: Full live GitHub tree / well-known / skills.sh adapters, richer remote freshness policy
- **Hermes reference**: `agent/skill_commands.py`, `agent/skill_preprocessing.py`, `agent/skill_utils.py`, `agent/skill_provenance.py`, `tools/skills_guard.py`, `tools/skills_hub.py`, `tools/skills_sync.py`, `tools/skill_usage.py`

### 5. Gateway
- **Status**: 8 platforms plus config-driven ingress access policy, fresh-final streaming, configurable stream pacing, and outbound silence-narration filtering. Gateway messages are checked against global, Telegram, role, and ClawBot allowlists before slash-command or agent execution; empty allowlists preserve the existing open-gateway behavior.
- **What's missing**: 12+ other platforms, gateway hooks system, channel directory, pairing, mirror, delivery abstraction, restart/drain, shutdown forensics, runtime footer, display config, session context management, sticker cache, native draft transport, and flood-control backoff
- **Hermes reference**: `gateway/` (entire directory)

### 7. Plugin System
- **Status**: Plugin trait + directory loader + HTTP tool plugin, embedded HTTP plugin templates, CLI `hakimi plugins list|templates|init|path`, gateway `/plugins list|templates|path`, and Hermes-style `plugins list --plain|--json` output with optional tool metadata
- **What's missing**: Memory provider plugins (8 backends), model provider plugins, context engine plugins, kanban plugin, observability plugin, achievements plugin, Spotify plugin, disk-cleanup plugin. Plugin discovery from pip entry points.
- **Hermes reference**: `plugins/` (entire directory)

### 8. CLI Commands
- **Status**: 38 个 slash 命令可解析；gateway 已具备 `/cron` 管理、`/plugins`、`/mcp list`、`/memory`、`/checkpoints`、`/logs`、`/platforms`、`/providers` 等基础响应；顶层 CLI 已覆盖 `doctor`、`setup`、`cron`、`plugins`
- **What's missing**: 大量命令仍停留在占位文本或只读视图，尤其是 `/profile`、`/setup`、`/mcp`、`/kanban` 等尚未形成与 Hermes 对齐的完整管理闭环
- **Hermes reference**: `hermes_cli/commands.py` (central COMMAND_REGISTRY)

### 10. Delegation
- **Status**: Child agent spawning with toolset filtering and Hermes-style blocked-tool stripping for leaf subagents
- **What's missing**: Subagent approval callbacks, parallel batch delegation, per-child timeout configuration, ThreadPoolExecutor with initializer for TLS callbacks
- **Hermes reference**: `tools/delegate_tool.py`

### 11. Session Store
- **Status**: SQLite with WAL, FTS5, message CRUD, and Rust-native full state backup/import
- **What's missing**: Session resume with full history restoration, session search with LLM summarization of results, richer session export/dump formats, session lifecycle events (start/end callbacks)
- **Hermes reference**: `hermes_state.py`, `hermes_cli/dump.py`

### 12. Knowledge Graph
- **Status**: Crate exists with basic types (KnowledgeGraph, NodeType, EdgeType)
- **What's missing**: Actual graph operations, provider integration, store implementation, agent integration
- **Hermes reference**: No direct equivalent in Hermes — this is a Hakimi-original feature that needs completion

### 13. REST API Server
- **Status**: Basic endpoints (health, chat, sessions, tools, config)
- **What's missing**: WebSocket streaming, authentication/authorization, rate limiting, session-scoped agents, PTY terminal endpoint, media handling, webhook callbacks
- **Hermes reference**: `gateway/platforms/api_server.py`, `hermes_cli/web_server.py`

### 14. TUI
- **Status**: Basic Ratatui TUI with chat, tools panel, status bar
- **What's missing**: Slash command autocomplete, session picker, skill browser, config editor, theme/skin support, checkpoint viewer, cron job management, gateway status panel
- **Hermes reference**: `ui-tui/` (Ink/React), `tui_gateway/`, `hermes_cli/curses_ui.py`

### 15. Error Handling
- **Status**: Basic HakimiError enum with retry for Transport/IO errors; credential pool now distinguishes temporary exhausted credentials from terminal dead OAuth credentials
- **What's missing**: 20+ specific error categories, runtime credential rotation on auth/billing errors, context overflow → compression trigger, model fallback on 404, provider-specific error handling (thinking_signature, long_context_tier, llama_cpp_grammar_pattern), failover reason tracking
- **Hermes reference**: `agent/error_classifier.py`

### 16. Usage Pricing / Rate Limit Tracking
- **Status**: `hakimi-transports::RateLimitTracker` parses OpenAI/Nous-style `x-ratelimit-*` request/token windows, formats detailed/compact displays, and Chat Completions, Responses, Anthropic, and Gemini transports retain the latest snapshot. `hakimi-common::estimate_usage_cost()` adds Hermes-style static pricing estimates for common OpenAI, Anthropic, Gemini, DeepSeek, and MiniMax routes; gateway `/usage` shows token counts, estimated cost, pricing snapshot version, and rate limits.
- **What's missing**: Provider account usage APIs, persisted aggregation, OpenRouter/provider live pricing discovery, and reconciliation with actual billed costs.
- **Hermes reference**: `agent/rate_limit_tracker.py`, `agent/usage_pricing.py`, `agent/account_usage.py`
---

## Summary Statistics

| Category | Hermes Features | Hakimi Complete | Hakimi Partial | Hakimi Missing |
|----------|----------------|-----------------|----------------|----------------|
| Core Tools | 40+ | 27 | 1 | 13+ |
| Transports | 4 | 4 | 0 | 0 |
| Gateway Platforms | 20+ | 8 | 1 | 12+ |
| CLI Commands | 50+ | 16 | 0 | 34+ |
| Agent Internals | 25+ | 18 | 4 | 2+ |
| Plugins | 10+ | 0 | 1 | 9+ |
| MCP Features | Full | Full | 0 | 0 |
| Cron Features | Full | Full | 0 | 0 |
| Skills Features | Full | 6 | 1 | 1 |
| Security Features | 6 | 6 | 0 | 0 |

**Total unique Hermes features identified: ~150+**
**Fully present in Hakimi: ~72** (up from ~30)
**Partially implemented: ~9**
**Missing entirely: ~72+**

### Top 10 Critical Gaps (by impact)
1. Browser advanced automation (vision, CDP attach, cloud backends)
2. Gateway platform breadth (12 missing platforms — webhook/signal/matrix/wecom/dingtalk added)
3. Plugin ecosystem (memory providers, model providers, context engines)
4. CLI command completeness (33+ missing commands)
5. Bedrock transport
6. ACP adapter / IDE integration
7. Kanban multi-agent coordination
8. Remote MCP sampling + richer server-initiated flows
9. Observability / usage pricing and account usage display
10. Voice mode (push-to-talk capture + playback)

---

## IMPLEMENTATION STATUS (Updated: 2026-05-29)

### Phase 1: Critical Gaps — ALL COMPLETE ✅
| # | Feature | File(s) | Tests | Status |
|---|---------|---------|-------|--------|
| 1 | Error Classifier | `hakimi-core/src/error_classifier.rs` | 62 | ✅ 20+ FailoverReasons, RecoveryAction, classify(), wired into loop_impl |
| 2 | Credential Pool | `hakimi-core/src/credential_pool.rs` | 49 | ✅ RoundRobin/FillFirst/Random strategies, exhaustion detection, rotation |
| 3 | Prompt Caching | `hakimi-transports/src/prompt_caching.rs` | 11 | ✅ CacheControl, TTL (5m/1h), breakpoints on system/tools/messages |
| 4 | Vision Analysis | `hakimi-tools/src/builtin_vision_analyze.rs`, `hakimi-tools/src/builtin_image_describe.rs` | 16 | ✅ Real vision payload generation, base64 encoding, and legacy `image_describe` alias |
| 5 | Clarify Tool | `hakimi-tools/src/builtin_clarify.rs` | 8 | ✅ Multiple-choice + open-ended, structured JSON output |

### Phase 2: High Gaps — ALL COMPLETE ✅
| # | Feature | File(s) | Tests | Status |
|---|---------|---------|-------|--------|
| 6 | MCP HTTP/SSE | `hakimi-mcp/src/http_transport.rs`, `sse_transport.rs` | 19 | ✅ StreamableHTTP, SSE, auto-reconnect, per-server timeouts |
| 7 | File Safety + Secret Redaction | `hakimi-core/src/file_safety.rs`, `hakimi-common/src/redact.rs`, `hakimi-tools/src/{builtin_terminal,builtin_process,builtin_code_exec,plugin}.rs` | 27 | ✅ WriteDeniedPaths, PathSecurity, shared SecretRedactor, PromptInjectionDetector, and forced redaction for shell/process/code/plugin output |
| 8 | Tool Guardrails | `hakimi-core/src/guardrails.rs` | 12 | ✅ Loop detection, idempotency tracking, halt decisions |
| 9 | LLM Context Compression | `hakimi-context/src/{compressor.rs,factory.rs}`, CLI/server construction | 25 | ✅ Config-selectable `llm` engine, summary model selection, Resolved/Pending tracking, tool output pruning, and local fallback |
| 10 | Profiles | `hakimi-cli/src/profiles.rs` | 10 | ✅ ~/.hakimi/profiles/, create/delete/use, separate config/memory/sessions |
| 11 | Setup Wizard | `hakimi-cli/src/setup_wizard.rs` | 15 | ✅ Model/Provider selection, API key input, platform config |
| 12 | Doctor | `hakimi-cli/src/doctor.rs`, `hakimi-cli/src/entry.rs` | 17 | ✅ Dependencies, config, env vars, API connectivity checks, `hakimi doctor`, gateway `/doctor` |

### Phase 3: Medium Gaps — ALL COMPLETE ✅
| # | Feature | File(s) | Tests | Status |
|---|---------|---------|-------|--------|
| 13 | Gateway Adapters | `hakimi-gateway/src/{webhook,signal,matrix,wecom,dingtalk}.rs` | 19 | ✅ 5 new PlatformAdapter implementations |
| 14 | Cron Persistence + Prompt Guard | `hakimi-cron/src/{lib.rs,persistence.rs}`, `hakimi-tools/src/builtin_cronjob.rs`, `hakimi-cli/src/entry.rs` | 36 | ✅ SQLite storage, FileLock, per-job toolset/config/delivery metadata, `cronjob update`, gateway `/cron status/add/edit`, standalone `hakimi cron status/tick` management, strict/assembled cron prompt scanner, skill-loaded scheduled runs, explicit gateway delivery targets |
| 15 | Checkpoint Manager | `hakimi-tools/src/builtin_checkpoint.rs` | 20 | ✅ Shadow git snapshots, rollback, diff, transparent to LLM |
| 16 | i18n | `hakimi-i18n/src/lib.rs` | 10 | ✅ Locale YAML catalogs, dotted key paths, English fallback |
| 17 | Batch Runner | `hakimi-batch/src/lib.rs` | 8 | ✅ Dataset loading, parallel processing, checkpointing, trajectory saving |
| 18 | Gateway Media Delivery | `hakimi-core/src/loop_impl.rs`, `hakimi-cli/src/entry.rs`, `hakimi-gateway/src/telegram.rs` | 4 | ✅ `MEDIA:` / `IMAGE:` tool results now stream through gateway side-channel; Telegram uploads local images and generated TTS audio directly |
| 19 | Responses Stream Recovery | `hakimi-transports/src/responses.rs`, `hakimi-core/src/loop_impl.rs` | 1 | ✅ `response.incomplete` continues as `length`, missing terminal stream events retry through classified transport recovery |
| 20 | Home Assistant Tools | `hakimi-tools/src/builtin_homeassistant.rs`, CLI/server/TUI registration | 11 | ✅ `ha_list_entities`, `ha_get_state`, `ha_list_services`, `ha_call_service` with REST auth, validation, blocked domains, and compact summaries |
| 21 | Think Scrubber | `hakimi-transports/src/scrubber.rs`, `hakimi-core/src/loop_impl.rs` | 18 | ✅ Hermes-style stateful reasoning tag scrubbing for streaming and non-streaming responses |
| 22 | Rate Limit Tracking + Gateway Usage + Cost Estimates | `hakimi-transports/src/rate_limit.rs`, `hakimi-common/src/usage_pricing.rs`, transport adapters, `hakimi-cli/src/entry.rs` | 17 | ✅ OpenAI/Nous-style `x-ratelimit-*` parsing, detailed/compact formatting, hot-bucket warnings, latest snapshot retained by Chat/Responses/Anthropic/Gemini transports, and gateway `/usage` renders last-turn tokens/API calls, Hermes-style estimated cost, pricing snapshot version, plus rate-limit display |
| 23 | Video Analysis | `hakimi-tools/src/builtin_video_analyze.rs`, CLI/server/TUI registration | 10 | ✅ `video_analyze` prepares structured video-capable request payloads for URLs, `file://`, and local files with MIME detection and payload-size guardrails |
| 24 | TUI `/copy` Clipboard | `hakimi-tui/src/clipboard.rs`, `hakimi-tui/src/app.rs`, `hakimi-cli/src/lib.rs` | 10 | ✅ Hermes-style `/copy [N]` copies recent assistant responses through native clipboard backends plus OSC 52 terminal fallback and exposes the command in shared slash parsing |
| 25 | TUI `/history` Review | `hakimi-tui/src/app.rs`, `hakimi-cli/src/lib.rs`, `hakimi-cli/src/entry.rs` | 3 | ✅ Hermes-style `/history [N]` / `/hist [N]` reviews recent user/assistant messages locally and gives gateway users a clear surface-boundary notice |
| 26 | Session Title Generation | `hakimi-session/src/message_ops.rs`, `hakimi-session/src/session_ops.rs` | 4 | ✅ First user messages auto-title untitled persisted sessions, preserve manual titles, avoid duplicate generated titles, and truncate Unicode safely |
| 27 | Plugin CLI/Templates | `hakimi-cli/src/entry.rs`, `hakimi-cli/src/lib.rs`, `templates/plugin-*.yaml` | 8 | ✅ `hakimi plugins list|templates|init|path` and gateway `/plugins` expose HTTP plugin discovery, safe template scaffolding, metadata-aware list output, and `--plain`/`--json` formats |
| 28 | Output Token Budget Recovery | `hakimi-core/src/error_classifier.rs`, `hakimi-core/src/loop_impl.rs` | 7 | ✅ Anthropic-style `available_tokens` errors now retry with a safe temporary `max_tokens` cap instead of compressing context when the prompt itself still fits |
| 29 | Browser Navigation Controls | `hakimi-tools/src/builtin_browser.rs`, `hakimi-cli/src/entry.rs`, `hakimi-tui/src/main.rs` | 6 | ✅ Optional Chromium browser tooling now includes Hermes-style `browser_scroll`, `browser_back`, and `browser_press` in CLI and TUI feature builds |
| 30 | Browser Image Listing | `hakimi-tools/src/builtin_browser.rs`, `hakimi-cli/src/entry.rs`, `hakimi-tui/src/main.rs` | 3 | ✅ Optional Chromium browser tooling now includes Hermes-style `browser_get_images` with image URL, alt text, and natural dimensions |
| 31 | Browser Console + Eval | `hakimi-tools/src/builtin_browser.rs`, `hakimi-cli/src/entry.rs`, `hakimi-tui/src/main.rs` | 2 | ✅ Optional Chromium browser tooling now includes Hermes-style `browser_console` for captured console messages, JavaScript errors, and page-context expression evaluation |
| 32 | MCP Node Command Resolution | `hakimi-mcp/src/client.rs` | 5 | ✅ Stdio MCP `node`/`npm`/`npx` launch now falls back to Hakimi-managed, user-local, and `/usr/local/bin` Node locations when PATH is narrowed |
| 33 | Browser Dialog Handling | `hakimi-tools/src/builtin_browser.rs`, `hakimi-cli/src/entry.rs`, `hakimi-tui/src/main.rs`, `hakimi-server/src/main.rs` | 2 | ✅ Optional Chromium browser tooling now surfaces pending native JavaScript dialogs in `browser_snapshot` and exposes `browser_dialog` to accept or dismiss them |
| 34 | MCP Error Sanitization | `hakimi-mcp/src/{redaction.rs,http_transport.rs,sse_transport.rs,adapter.rs}` | 6 | ✅ Remote MCP HTTP/SSE response snippets, parse contexts, adapter failures, and `isError` tool results redact credential-like text before reaching the agent |
| 35 | Context File Injection Guard | `hakimi-context/src/prompt_builder.rs` | 4 | ✅ Context files that feed the system prompt are scanned and blocked before injection when they contain prompt-injection patterns |
| 36 | Delegation Blocked Tools | `hakimi-core/src/delegate.rs` | 3 | ✅ Child agent registries strip `delegate_task`, `clarify`, `memory`, `send_message`, and `code_exec` after optional toolset filtering |
| 37 | Read-File Credential Guard | `hakimi-common/src/file_safety.rs`, `hakimi-tools/src/builtin_read_file.rs` | 7 | ✅ `read_file` blocks Hakimi credential stores, MCP token files, profile credential stores, project `.env*`, and `cache/bws_cache.json` before file content reaches the agent |
| 38 | Progressive Tool Disclosure | `hakimi-common/src/tool.rs`, `hakimi-tools/src/{tool_search.rs,registry.rs}`, `hakimi-core/src/{agent.rs,loop_impl.rs}` | 8 | ✅ MCP/plugin tool schemas can collapse behind `tool_search`/`tool_describe`/`tool_call`; core tools never defer; CLI/server honor `tools.tool_search` config |
| 39 | Terminal Shell Hooks | `hakimi-tools/src/builtin_terminal.rs` | 4 | ✅ Opt-in terminal pre/post hook commands receive Hermes-style JSON payloads; pre hooks can block execution with canonical or Claude-Code-style JSON |
| 40 | Gateway Ingress Access Policy | `hakimi-cli/src/entry.rs`, `hakimi-config/src/config.rs` | 7 | ✅ Config-driven global, Telegram, role, and ClawBot allowlists gate inbound gateway messages before command/agent handling |
| 41 | Gateway MCP Server Listing | `hakimi-cli/src/entry.rs` | 2 | ✅ Gateway `/mcp` and `/mcp list` render configured MCP servers with safe command/arg/env counts while keeping add/remove config-file managed |
| 42 | MCP Sampling createMessage | `hakimi-mcp/src/{protocol.rs,sampling.rs,client.rs}`, `hakimi-cli/src/entry.rs` | 7 | ✅ Stdio MCP clients advertise sampling support and answer server-initiated `sampling/createMessage` through Hakimi's configured LLM transport and active model, with JSON-RPC errors for unsupported client requests |
| 43 | Gateway Fresh-Final Streaming | `hakimi-cli/src/entry.rs`, `hakimi-config/src/config.rs`, `hakimi-gateway/src/{lib.rs,telegram.rs}` | 2 | ✅ Long-lived gateway stream previews can finish as fresh final messages through `gateways.streaming.fresh_final_after_seconds`; Telegram deletes stale previews best-effort |
| 44 | Gateway Stream Pacing | `hakimi-cli/src/entry.rs`, `hakimi-config/src/config.rs` | 4 | ✅ Gateway progressive edits honor `gateways.streaming.edit_interval_ms` and `buffer_threshold_chars`, and flush pending assistant text before tool/media/delegate boundaries |
| 45 | Credential Pool Terminal Auth Quarantine | `hakimi-core/src/credential_pool.rs` | 7 | ✅ Terminal 401 OAuth reasons mark credentials `dead`, prevent cooldown re-entry, expose dead/exhausted stats separately, and support explicit revive after re-auth |
| 46 | Skills Guard | `hakimi-skills/src/{safety.rs,loader.rs}` | 6 | ✅ Skill markdown is scanned before parsing; dangerous injection/exfiltration/persistence/destructive/invisible-Unicode/credential patterns and symlinked skill paths are blocked before prompt injection |
| 47 | Skills Provenance Metadata | `hakimi-skills/src/{skill.rs,loader.rs,store.rs}`, `hakimi-cli/src/entry.rs` | 3 | ✅ Skill loading preserves Hermes-style `metadata.hermes`, explicit `provenance` frontmatter, and `.hub/lock.json` source/trust records, then surfaces normalized provenance labels in summaries and gateway `/skills` |
| 48 | Rust-Native Backup/Import | `hakimi-cli/src/backup.rs`, `hakimi-cli/src/entry.rs` | 7 | ✅ `hakimi backup [output]` and `hakimi import <archive> --force` archive/restore `~/.hakimi` user state without external tar, skip binary/cache/sidecar/symlink entries, snapshot SQLite DBs, and block path traversal on import |
| 49 | Skills Hub Manifest Install Policy | `hakimi-skills/src/hub.rs`, `hakimi-cli/src/skills.rs`, `hakimi-cli/src/entry.rs` | 7 | ✅ Local `.hub/index.json` skills can be browsed/searched/inspected/installed from CLI and gateway; community installs require explicit trust, SKILL.md is scanned, bundle paths are guarded, and lock/audit provenance is recorded |
| 50 | Skills Platform Gates | `hakimi-skills/src/{skill.rs,loader.rs}` | 4 | ✅ Skill frontmatter `platforms` accepts scalar/list values, recognizes Hermes OS aliases, and skips incompatible skills before prompt injection |
| 51 | Skills Template Preprocessing | `hakimi-skills/src/{preprocessing.rs,loader.rs,store.rs}` | 7 | ✅ Runtime skill bodies substitute Hermes/Hakimi skill-dir and session-id variables before prompt injection; trusted callers can opt into bounded inline-shell expansion with skill-directory CWD |
| 52 | Skills Usage Telemetry | `hakimi-skills/src/{usage.rs,store.rs}`, `hakimi-cli/src/skills.rs` | 6 | ✅ Runtime skill activation writes `.usage.json` use/view counters best-effort, and CLI/gateway `skills usage` renders text or JSON reports without exposing skill content |
| 53 | Skills Bundled Sync/Update | `hakimi-skills/src/sync.rs`, `hakimi-cli/src/skills.rs` | 7 | ✅ `hakimi skills sync --source <dir>` seeds bundled SKILL.md trees, writes `.bundled_manifest` origin hashes, updates only unmodified synced copies, preserves user edits/deletions, and exposes CLI/gateway text or JSON summaries |
| 54 | Skills Slash-Command Invocation | `hakimi-skills/src/slash.rs`, `hakimi-core/src/agent.rs`, `hakimi-cli/src/entry.rs` | 3 | ✅ Loaded skills can be invoked as `/skill-name optional instruction`; CLI query and gateway chats inject the selected full skill content as a user message while preserving built-in command priority |
| 55 | Skills Hub Source Indexes | `hakimi-skills/src/hub.rs`, `hakimi-cli/src/skills.rs` | 6 | ✅ `hakimi skills sources list/add/refresh/remove` manages local/HTTPS index sources, caches refreshed catalogs under `.hub/index-cache`, merges cached entries into discovery, deduplicates by identifier, and blocks unsafe remote source hosts |
| 56 | Gateway Silence-Narration Filter | `hakimi-gateway/src/lib.rs`, `hakimi-config/src/config.rs`, `hakimi-cli/src/entry.rs` | 8 | ✅ Outbound gateway routing drops bare silence narration before adapter send/edit/get-id paths, defaults on through `gateways.filter_silence_narration`, supports Hakimi/Hermes env overrides, and preserves media deliveries |

### Summary
- **Total tests**: 1310 (latest CI target; local compilation intentionally not run in automation)
- **Build**: Clean (0 errors)
- **Stubs/todos/unimplemented**: 0 across all gap files
- **Cargo workspace**: 19 crates, edition 2024
