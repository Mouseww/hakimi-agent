# Hermes Agent — Architecture Document for Rust Rewrite

**Source:** `/usr/local/lib/hermes-agent/` (Python, ~15k LOC in `run_agent.py` alone)
**Date:** 2026-05-20

---

## 1. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Entry Points                                                    │
│  cli.py · gateway/ · batch_runner.py · acp_adapter/ · tui_gateway│
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  AIAgent  (run_agent.py — ~15,700 lines)                         │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  run_conversation() — the core agent loop                │   │
│  │  while budget > 0:                                       │   │
│  │    response = transport.call(model, messages, tools)     │   │
│  │    if response.tool_calls:                               │   │
│  │      for tc in response.tool_calls:                      │   │
│  │        result = handle_function_call(tc)                 │   │
│  │        messages.append(tool_result)                      │   │
│  │    else: return response.content                         │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Subsystems:                                                     │
│  ┌──────────────┐ ┌──────────────┐ ┌────────────────────────┐  │
│  │  Transport    │ │  Tools       │ │  Context Management    │  │
│  │  Layer        │ │  Registry    │ │  (compression, memory) │  │
│  └──────────────┘ └──────────────┘ └────────────────────────┘  │
│  ┌──────────────┐ ┌──────────────┐ ┌────────────────────────┐  │
│  │  Error/Retry  │ │  Prompt      │ │  Session Store         │  │
│  │  Engine       │ │  Builder     │ │  (SQLite)              │  │
│  └──────────────┘ └──────────────┘ └────────────────────────┘  │
│  ┌──────────────┐ ┌──────────────┐ ┌────────────────────────┐  │
│  │  Display/     │ │  Cron        │ │  Credential Pool       │  │
│  │  Streaming    │ │  Scheduler   │ │  & Provider Routing    │  │
│  └──────────────┘ └──────────────┘ └────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### Message Format

All internal messages follow OpenAI format:
```json
{"role": "system|user|assistant|tool", "content": "...", "tool_calls": [...], "tool_call_id": "..."}
```
Reasoning content stored in `assistant_msg["reasoning"]`.

---

## 2. Module-by-Module Analysis

### 2.1 `run_agent.py` — AIAgent (Core Agent Loop)

**Purpose:** The central orchestrator. Manages the conversation loop, tool execution, streaming, error recovery, context compression triggers, and budget tracking.

**Key Classes:**

```rust
// IterationBudget — thread-safe iteration counter
struct IterationBudget {
    max_total: usize,
    used: AtomicUsize,  // was Mutex<usize> in Python
}

// AIAgent — ~60 constructor parameters
struct AIAgent {
    // Identity & routing
    model: String,
    provider: String,                // "openrouter", "anthropic", etc.
    api_mode: ApiMode,               // ChatCompletions | CodexResponses | AnthropicMessages | BedrockConverse
    base_url: String,
    api_key: String,
    
    // Budget & control
    max_iterations: usize,
    iteration_budget: Arc<IterationBudget>,
    tool_delay: Duration,
    
    // Toolsets
    enabled_toolsets: Option<Vec<String>>,
    disabled_toolsets: Option<Vec<String>>,
    
    // Session context
    session_id: String,
    platform: Option<String>,       // "cli", "telegram", "discord"
    user_id: Option<String>,
    chat_id: Option<String>,
    
    // Callbacks (trait objects in Rust)
    tool_progress_callback: Option<Box<dyn Fn(&str, &str)>>,
    tool_start_callback: Option<Box<dyn Fn(&str)>>,
    stream_delta_callback: Option<Box<dyn Fn(&str)>>,
    clarify_callback: Option<Box<dyn Fn(&str, &[String]) -> String>>,
    // ... ~12 more callbacks
    
    // Subsystems (initialized in __init__)
    transport: Box<dyn ProviderTransport>,
    context_engine: Box<dyn ContextEngine>,
    memory_manager: MemoryManager,
    tool_guardrails: ToolCallGuardrailController,
    subdirectory_hints: SubdirectoryHintTracker,
    session_db: Option<Arc<SessionDB>>,
    credential_pool: Option<CredentialPool>,
    
    // State
    messages: Vec<Message>,          // conversation history
    _interrupt_requested: Arc<AtomicBool>,
    _executing_tools: bool,
}
```

**Public API:**
- `chat(message: &str) -> String` — simple interface
- `run_conversation(user_message, system_message?, history?) -> ConversationResult` — full interface

**Key Methods (private):**
- `_build_system_prompt()` — assembles identity + platform hints + memory + skills + context files
- `_execute_tool_calls(tool_calls)` — parallel/sequential tool dispatch with guardrails
- `_handle_api_error(error, attempt)` — error classification + retry/failover/compress
- `_should_compress()` / `_trigger_compression()` — context window management
- `_fire_stream_delta(delta)` — streaming callback with think-block scrubbing

**Dependencies:** Everything. This is the god object.

**Rust Changes:**
- Replace `threading.Lock` with `Arc<Mutex<T>>` or `tokio::sync::Mutex`
- Callbacks → `Box<dyn Fn>` trait objects or channel-based event system
- The 60-param constructor → builder pattern
- `async fn run_conversation()` — the whole loop should be async (tokio)
- Replace `json.loads/dumps` with `serde_json`

---

### 2.2 `agent/transports/` — Provider Transport Layer

**Purpose:** Abstract provider-specific API format differences. Each transport converts between OpenAI-format messages/tools and the provider's native format, then normalizes responses back.

**Key Types:**

```rust
// agent/transports/types.py
struct ToolCall {
    id: Option<String>,
    name: String,
    arguments: String,  // JSON
    provider_data: Option<serde_json::Value>,
}

struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    cached_tokens: u64,
}

struct NormalizedResponse {
    content: Option<String>,
    tool_calls: Vec<ToolCall>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    reasoning: Option<String>,
    reasoning_details: Option<Vec<serde_json::Value>>,
    provider_data: Option<serde_json::Value>,
}

enum ApiMode {
    ChatCompletions,
    CodexResponses,
    AnthropicMessages,
    BedrockConverse,
}
```

**Transport Trait:**
```rust
trait ProviderTransport: Send + Sync {
    fn api_mode(&self) -> ApiMode;
    fn convert_messages(&self, messages: &[Message]) -> serde_json::Value;
    fn convert_tools(&self, tools: &[ToolDefinition]) -> serde_json::Value;
    fn build_request(&self, model: &str, messages: &[Message], tools: &[ToolDefinition], params: &RequestParams) -> serde_json::Value;
    fn normalize_response(&self, raw: &serde_json::Value) -> Result<NormalizedResponse>;
    fn map_finish_reason(&self, raw: &str) -> String;
}
```

**Implementations:** `ChatCompletionsTransport`, `CodexResponsesTransport`, `AnthropicMessagesTransport`, `BedrockConverseTransport`

**Rust Changes:**
- Transport registry → `HashMap<ApiMode, Box<dyn ProviderTransport>>`
- Use `serde_json::Value` for provider-native format interop
- Client construction stays outside transport (on AIAgent)

---

### 2.3 `tools/registry.py` — Tool Registry

**Purpose:** Central singleton that collects tool schemas, handlers, and metadata. Each tool file calls `registry.register()` at module import time. `model_tools.py` queries the registry.

**Key Classes:**

```rust
struct ToolEntry {
    name: String,
    toolset: String,
    schema: serde_json::Value,       // JSON Schema for the tool
    handler: ToolHandler,            // fn(HashMap<String, Value>) -> Result<String>
    check_fn: Option<Box<dyn Fn() -> bool>>,
    requires_env: Vec<String>,
    is_async: bool,
    description: String,
    emoji: String,
    max_result_size_chars: Option<usize>,
    dynamic_schema_overrides: Option<Box<dyn Fn() -> serde_json::Value>>,
}

struct ToolRegistry {
    tools: RwLock<HashMap<String, ToolEntry>>,
    toolset_checks: RwLock<HashMap<String, Box<dyn Fn() -> bool>>>,
    toolset_aliases: RwLock<HashMap<String, String>>,
    generation: AtomicU64,           // cache invalidation counter
}
```

**Public API:**
- `register(name, toolset, schema, handler, ...)` — register a tool
- `deregister(name)` — remove a tool
- `get_definitions(tool_names) -> Vec<ToolDefinition>` — filtered schemas
- `dispatch(name, args) -> String` — execute a tool handler
- `get_entry(name) -> Option<ToolEntry>`
- `discover_builtin_tools()` — scan tools/*.py, import self-registering modules

**Rust Changes:**
- Tool handler: `async fn(Box<dyn Any>) -> Result<String, ToolError>` (or use an enum for args)
- `discover_builtin_tools()` → build-time registration via proc macros or a `register_tools!` macro, OR runtime plugin loading via `libloading`
- `_check_fn_cached()` TTL cache → `moka::Cache` or manual TTL map
- `threading.RLock` → `tokio::sync::RwLock`

---

### 2.4 `model_tools.py` — Tool Orchestration

**Purpose:** Thin layer over the registry that provides the public API consumed by `run_agent.py`. Handles toolset resolution, schema filtering, async bridging, argument coercion, and result size enforcement.

**Public API:**
- `get_tool_definitions(enabled_toolsets, disabled_toolsets, quiet) -> Vec<ToolDef>`
- `handle_function_call(name, args, task_id) -> String`
- `TOOL_TO_TOOLSET_MAP: HashMap<String, String>`
- `check_toolset_requirements() -> HashMap<String, bool>`

**Key Logic:**
- Module-level `discover_builtin_tools()` call at import
- `_run_async(coro)` — sync→async bridge with persistent event loops per thread
- `coerce_tool_args(tool_name, args)` — type coercion for LLM output (string→int, etc.)
- Memoized `get_tool_definitions()` keyed on `(enabled, disabled, generation, config_mtime)`

**Rust Changes:**
- Async bridging is unnecessary — Rust is natively async with tokio
- `coerce_tool_args` → straightforward serde deserialization with `#[serde(try_from)]`
- Memoization → `moka::Cache` or manual cache with generation-based invalidation

---

### 2.5 `toolsets.py` — Toolset Definitions

**Purpose:** Defines named groups of tools (e.g. "web", "terminal", "browser", "skills"). Toolsets compose from other toolsets.

**Key Data:**

```rust
struct ToolsetDef {
    description: String,
    tools: Vec<String>,
    includes: Vec<String>,  // composed from other toolsets
}

static TOOLSETS: Lazy<HashMap<&str, ToolsetDef>> = ...;
static HERMES_CORE_TOOLS: &[&str] = &[
    "web_search", "web_extract", "terminal", "process",
    "read_file", "write_file", "patch", "search_files",
    "vision_analyze", "image_generate",
    // ... ~40 tools total
];
```

**Public API:**
- `get_toolset(name) -> Vec<String>` — resolved tool names
- `resolve_toolset(name) -> Vec<String>` — recursively resolves includes
- `get_all_toolsets() -> HashMap<String, ToolsetDef>`
- `validate_toolset(name) -> bool`

**Rust Changes:**
- Pure data, trivially portable
- `LazyLock<HashMap>` or `phf` for static initialization

---

### 2.6 `hermes_state.py` — SessionDB (SQLite State Store)

**Purpose:** Persistent session storage with FTS5 full-text search. Stores session metadata, full message history, and model configuration.

**Schema:**
```sql
sessions(id, source, user_id, model, model_config, system_prompt,
         parent_session_id, started_at, ended_at, end_reason,
         message_count, tool_call_count, input_tokens, output_tokens,
         cache_read_tokens, cache_write_tokens, reasoning_tokens,
         billing_provider, billing_base_url, billing_mode,
         estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
         title, api_call_count, handoff_state, handoff_platform)

messages(id, session_id, role, content, tool_call_id, tool_calls,
         tool_name, timestamp, token_count, finish_reason, reasoning,
         reasoning_content, reasoning_details, codex_reasoning_items,
         codex_message_items)

messages_fts -- FTS5 virtual table on content+tool_name+tool_calls
messages_fts_trigram -- CJK trigram search table
```

**Key Class:**
```rust
struct SessionDB {
    conn: Mutex<rusqlite::Connection>,  // WAL mode, thread-safe
}

impl SessionDB {
    fn new(path: &Path) -> Result<Self>;
    fn create_session(&self, ...) -> String;
    fn save_message(&self, session_id: &str, msg: &Message);
    fn get_session_messages(&self, session_id: &str) -> Vec<Message>;
    fn search_messages(&self, query: &str, limit: usize) -> Vec<SearchResult>;
    fn update_session_totals(&self, session_id: &str, ...);
    fn get_recent_sessions(&self, source: &str, limit: usize) -> Vec<SessionMeta>;
}
```

**Rust Changes:**
- `sqlite3` → `rusqlite` with bundled SQLite (includes FTS5)
- WAL fallback logic preserved
- `threading.Lock` → `Mutex` (rusqlite Connection is not `Send` by default, need `bundled` feature)

---

### 2.7 `agent/memory_manager.py` — Memory System

**Purpose:** Orchestrates pluggable memory providers for persistent recall across sessions. Single integration point in AIAgent.

**Key Classes:**

```rust
// Abstract provider trait
trait MemoryProvider: Send + Sync {
    fn name(&self) -> &str;
    fn is_available(&self) -> bool;
    fn initialize(&mut self, session_id: &str, config: &MemoryConfig);
    fn system_prompt_block(&self) -> String;
    fn prefetch(&self, query: &str) -> String;
    fn sync_turn(&self, user_msg: &str, assistant_msg: &str);
    fn get_tool_schemas(&self) -> Vec<ToolDefinition>;
    fn handle_tool_call(&self, name: &str, args: &serde_json::Value) -> String;
    fn shutdown(&mut self);
}

struct MemoryManager {
    providers: Vec<Box<dyn MemoryProvider>>,
}
```

**Additional utilities:**
- `StreamingContextScrubber` — stateful state machine that strips `<memory-context>` blocks from streamed text
- `sanitize_context(text)` — one-shot regex-based scrub
- `build_memory_context_block(memories)` — format memories for injection

**Rust Changes:**
- ABC → trait object
- Plugin providers loaded dynamically via `libloading` or compiled in

---

### 2.8 `agent/context_compressor.py` — Context Window Compression

**Purpose:** Automatic context window compression when approaching token limits. Uses a cheap auxiliary model to summarize middle turns while protecting head and tail context.

**Key Class:**
```rust
struct ContextCompressor {
    // ContextEngine implementation
    context_length: usize,
    threshold_percent: f64,       // 0.75
    protect_first_n: usize,       // 3 messages
    protect_last_n: usize,        // 6 messages
    compression_count: usize,
    last_prompt_tokens: usize,
    last_completion_tokens: usize,
    auxiliary_client: AuxiliaryClient,
}

// Also implements ContextEngine trait
trait ContextEngine: Send + Sync {
    fn name(&self) -> &str;
    fn update_from_response(&mut self, usage: &Usage);
    fn should_compress(&self, prompt_tokens: Option<usize>) -> bool;
    fn compress(&self, messages: &mut Vec<Message>, current_tokens: usize) -> Result<()>;
    fn on_session_start(&mut self);
    fn on_session_end(&mut self);
}
```

**Key constants:**
- `_MIN_SUMMARY_TOKENS = 2000`
- `_SUMMARY_RATIO = 0.20`
- `_SUMMARY_TOKENS_CEILING = 12_000`
- `_IMAGE_TOKEN_ESTIMATE = 1600`

**Rust Changes:**
- `call_llm()` for summarization → async HTTP via `reqwest`
- Message mutation → `&mut Vec<Message>` with careful ownership

---

### 2.9 `agent/prompt_builder.py` — System Prompt Assembly

**Purpose:** Stateless functions that assemble the system prompt from identity, platform hints, skills index, context files, and security scanning.

**Key Functions:**
- `build_system_prompt(...)` — main assembly
- `build_skills_system_prompt(skills_dirs)` — scan skills/, build index
- `build_context_files_prompt(cwd)` — load AGENTS.md, SOUL.md, .cursorrules
- `build_environment_hints(platform, ...)` — platform-specific hints
- `load_soul_md()` — load ~/.hermes/SOUL.md
- `_scan_context_content(content, filename)` — prompt injection detection

**Constants:**
- `DEFAULT_AGENT_IDENTITY` — base identity string
- `PLATFORM_HINTS: HashMap<String, String>` — per-platform formatting guidance
- `MEMORY_GUIDANCE`, `SESSION_SEARCH_GUIDANCE`, `SKILLS_GUIDANCE` — tool-use guidance

**Rust Changes:**
- Pure functions, trivially portable
- File I/O → `std::fs` or `tokio::fs`
- Regex scanning → `regex` crate

---

### 2.10 `agent/error_classifier.py` — Error Classification

**Purpose:** Structured taxonomy of API errors with recovery hints. The retry loop consults this for every API failure.

**Key Types:**
```rust
enum FailoverReason {
    Auth, AuthPermanent,
    Billing, RateLimit,
    Overloaded, ServerError,
    Timeout,
    ContextOverflow, PayloadTooLarge, ImageTooLarge,
    ModelNotFound, ProviderPolicyBlocked,
    FormatError,
    ThinkingSignature, LongContextTier, OauthLongContextBetaForbidden,
    LlamaCppGrammarPattern,
    Unknown,
}

struct ClassifiedError {
    reason: FailoverReason,
    status_code: Option<u16>,
    provider: Option<String>,
    model: Option<String>,
    message: String,
    retryable: bool,
    should_compress: bool,
    should_rotate_credential: bool,
    should_fallback: bool,
}
```

**Public API:**
- `classify_api_error(error, provider, model) -> ClassifiedError`

**Rust Changes:**
- Pattern matching on HTTP status codes + message substrings
- `reqwest::Error` → classify from status + body

---

### 2.11 `agent/retry_utils.py` — Retry Backoff

**Purpose:** Jittered exponential backoff for decorrelated retries.

```rust
fn jittered_backoff(attempt: u32, base_delay: f64, max_delay: f64, jitter_ratio: f64) -> Duration
```

**Rust Changes:** Pure function, trivial. Use `rand` for jitter.

---

### 2.12 `agent/think_scrubber.py` — Streaming Think Block Scrubber

**Purpose:** Stateful state machine that strips `<think>`, `<thinking>`, `<reasoning>`, `<thought>`, `<REASONING_SCRATCHPAD>` blocks from streamed assistant text.

```rust
struct StreamingThinkScrubber {
    in_block: bool,
    buf: String,
    last_emitted_ended_newline: bool,
}

impl StreamingThinkScrubber {
    fn feed(&mut self, text: &str) -> String;  // visible portion
    fn flush(&mut self) -> String;              // end-of-stream
    fn reset(&mut self);
}
```

**Rust Changes:** Pure state machine, trivially portable.

---

### 2.13 `agent/display.py` — CLI Presentation

**Purpose:** Kawaii spinner, tool preview formatting, diff display.

```rust
struct KawaiiSpinner {
    faces: Vec<String>,
    current: usize,
    // animation state
}

struct LocalEditSnapshot {
    paths: Vec<PathBuf>,
    before: HashMap<PathBuf, Option<String>>,
}
```

**Rust Changes:**
- ANSI escape codes → same approach
- `prompt_toolkit` integration → `crossterm` or `ratatui` for TUI

---

### 2.14 `agent/tool_guardrails.py` — Tool Call Loop Detection

**Purpose:** Pure, side-effect-free controller that tracks per-turn tool-call patterns and returns decisions (warn, halt, allow).

```rust
#[derive(Clone)]
struct ToolCallGuardrailConfig {
    warnings_enabled: bool,
    hard_stop_enabled: bool,
    exact_failure_warn_after: u32,
    same_tool_failure_warn_after: u32,
    no_progress_warn_after: u32,
    // ...
}

enum ToolGuardrailDecision {
    Allow,
    Warn(String),
    Halt(String),
}

struct ToolCallGuardrailController {
    config: ToolCallGuardrailConfig,
    observations: Vec<ToolCallObservation>,
}

impl ToolCallGuardrailController {
    fn observe(&mut self, tool_name: &str, args: &serde_json::Value, result: &str);
    fn decide(&self) -> ToolGuardrailDecision;
}
```

**Rust Changes:** Pure logic, trivially portable.

---

### 2.15 `agent/subdirectory_hints.py` — Progressive Context Discovery

**Purpose:** As the agent navigates subdirectories, discovers and loads AGENTS.md, CLAUDE.md, .cursorrules from those directories.

```rust
struct SubdirectoryHintTracker {
    working_dir: PathBuf,
    loaded_dirs: HashSet<PathBuf>,
}

impl SubdirectoryHintTracker {
    fn check_tool_call(&mut self, tool_name: &str, tool_args: &serde_json::Value) -> Option<String>;
}
```

**Rust Changes:** Pure logic with `std::fs` reads.

---

### 2.16 `agent/prompt_caching.py` — Anthropic Cache Control

**Purpose:** Applies `cache_control` breakpoints to messages for Anthropic models. Two strategies: `system_and_3` and `prefix_and_2`.

**Rust Changes:** Pure function operating on `Vec<Message>`, trivially portable.

---

### 2.17 `agent/model_metadata.py` — Model Metadata & Token Estimation

**Purpose:** Fetches model metadata (context lengths, pricing) from OpenRouter and other sources. Provides token estimation utilities.

**Key Functions:**
- `fetch_model_metadata(model) -> ModelMetadata`
- `estimate_tokens_rough(text) -> usize`
- `estimate_messages_tokens_rough(messages) -> usize`
- `get_model_context_length(model) -> usize`
- `is_local_endpoint(base_url) -> bool`

**Rust Changes:**
- HTTP calls → `reqwest` with caching (`moka` TTL cache)
- Provider prefix stripping → string parsing

---

### 2.18 `agent/usage_pricing.py` — Cost Estimation

**Purpose:** Estimates API call costs from token usage and model pricing data.

```rust
struct CanonicalUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
}

struct CostResult {
    amount_usd: Option<Decimal>,
    status: CostStatus,
    source: CostSource,
    label: String,
}

fn estimate_usage_cost(usage: &CanonicalUsage, model: &str, provider: &str) -> CostResult;
```

**Rust Changes:** Use `rust_decimal` for precise currency math.

---

### 2.19 `agent/auxiliary_client.py` — Auxiliary LLM Client

**Purpose:** Shared client router for side tasks (compression, search, vision). Resolves the best available backend with fallback chain.

**Resolution order:** Main provider → OpenRouter → Nous Portal → Custom endpoint → Anthropic → Direct API-key providers → None

**Rust Changes:**
- `OpenAI()` SDK → `reqwest` HTTP client with OpenAI-compatible JSON
- Credential resolution chain → config-driven provider enum

---

### 2.20 `agent/transports/codex_responses_adapter.py` — Codex Responses API

**Purpose:** Format conversion for OpenAI Responses API (Codex, xAI). Stateless functions.

**Rust Changes:** Pure serde serialization/deserialization.

---

### 2.21 `cron/scheduler.py` + `cron/jobs.py` — Cron Scheduler

**Purpose:** File-based cron job system. Jobs stored in `~/.hermes/cron/jobs.json`, output in `~/.hermes/cron/output/{job_id}/{timestamp}.md`.

**Key Functions:**
```rust
// jobs.rs
fn load_jobs() -> Vec<CronJob>;
fn save_jobs(jobs: &[CronJob]);
fn get_due_jobs() -> Vec<CronJob>;
fn mark_job_run(job_id: &str, success: bool, output: &str);
fn advance_next_run(job_id: &str);

// scheduler.rs
fn tick() -> Result<()>;  // check for due jobs, run them
fn run_job(job: &CronJob) -> Result<String>;  // spawn AIAgent for the job
```

**Key Types:**
```rust
struct CronJob {
    id: String,
    name: String,
    prompt: String,
    schedule: Schedule,        // cron expression or one-shot datetime
    enabled: bool,
    state: String,             // "scheduled", "paused", "running"
    skills: Vec<String>,
    delivery: Option<DeliveryConfig>,
    enabled_toolsets: Option<Vec<String>>,
    next_run: Option<DateTime>,
    last_run: Option<DateTime>,
}
```

**Rust Changes:**
- `croniter` → `cron` crate for expression parsing
- `jobs.json` → same file-based storage or migrate to SQLite
- `fcntl` file locking → `fs2` crate
- Spawn AIAgent in a tokio task

---

### 2.22 `hermes_constants.py` — Shared Constants

**Purpose:** Import-safe module with no dependencies. Provides `get_hermes_home()`.

**Rust Changes:**
- `get_hermes_home()` → `fn hermes_home() -> PathBuf` reading `HERMES_HOME` env var, defaulting to `~/.hermes`

---

## 3. Cross-Cutting Concerns for Rust Rewrite

### 3.1 Async Runtime

**Recommendation:** `tokio` as the async runtime.

The Python codebase has complex sync→async bridging (`_run_async()` with per-thread persistent event loops). In Rust, everything can be natively async. The agent loop, tool dispatch, HTTP calls, and streaming all become `async fn`.

### 3.2 Error Handling

**Recommendation:** `thiserror` for domain errors, `anyhow` for application-level errors.

```rust
#[derive(Debug, thiserror::Error)]
enum AgentError {
    #[error("API error: {reason:?}")]
    ApiError { reason: FailoverReason, status: Option<u16>, message: String },
    #[error("Tool error: {0}")]
    ToolError(String),
    #[error("Budget exhausted")]
    BudgetExhausted,
    #[error("Interrupted")]
    Interrupted,
    #[error("Context overflow — needs compression")]
    ContextOverflow,
}
```

### 3.3 Serialization

**Recommendation:** `serde` + `serde_json` for all JSON handling. Messages, tool definitions, and API payloads all serialize/deserialize via serde.

### 3.4 HTTP Client

**Recommendation:** `reqwest` with `rustls` TLS. Build a thin OpenAI-compatible client wrapper that supports streaming via SSE.

### 3.5 SQLite

**Recommendation:** `rusqlite` with `bundled` feature (includes FTS5). Single `Connection` behind a `Mutex` (matching the Python WAL-mode pattern).

### 3.6 Concurrency

- Tool parallelism → `tokio::task::JoinSet` for async tools, `tokio::task::spawn_blocking` for sync tools
- Iteration budget → `AtomicUsize`
- Registry → `RwLock` for concurrent reads, exclusive writes
- Streaming scrubbers → owned per-agent, no sharing needed

### 3.7 Plugin System

**Options:**
1. **Compile-time:** Register tools via proc macros (`#[hermes_tool]`)
2. **Runtime:** Load `.so`/`.dylib` plugins via `libloading`
3. **Hybrid:** Built-in tools compile-time registered, external plugins via dynamic loading

**Recommendation:** Start with compile-time, add dynamic loading later.

### 3.8 Configuration

**Recommendation:** `serde_yaml` for `config.yaml`, `dotenvy` for `.env` files. Config struct with `#[serde(default)]` fields.

---

## 4. Suggested Rust Crate Structure

```
hakimi-agent/
├── Cargo.toml
├── crates/
│   ├── agent-core/          # AIAgent, agent loop, iteration budget
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── agent.rs       # AIAgent struct + run_conversation
│   │   │   ├── budget.rs      # IterationBudget
│   │   │   └── error.rs       # AgentError
│   │   └── Cargo.toml
│   │
│   ├── transports/            # Provider transport layer
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types.rs       # NormalizedResponse, ToolCall, Usage
│   │   │   ├── base.rs        # ProviderTransport trait
│   │   │   ├── chat_completions.rs
│   │   │   ├── anthropic.rs
│   │   │   ├── codex.rs
│   │   │   └── bedrock.rs
│   │   └── Cargo.toml
│   │
│   ├── tools/                 # Tool registry + built-in tools
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── registry.rs    # ToolRegistry, ToolEntry
│   │   │   ├── toolsets.rs    # Toolset definitions
│   │   │   ├── orchestration.rs # get_tool_definitions, handle_function_call
│   │   │   ├── coerce.rs      # Argument type coercion
│   │   │   └── builtin/       # One file per tool
│   │   │       ├── terminal.rs
│   │   │       ├── web_search.rs
│   │   │       ├── read_file.rs
│   │   │       ├── write_file.rs
│   │   │       ├── patch.rs
│   │   │       ├── search_files.rs
│   │   │       ├── vision.rs
│   │   │       ├── browser.rs
│   │   │       └── ...
│   │   └── Cargo.toml
│   │
│   ├── context/               # Context management
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── compressor.rs  # ContextCompressor
│   │   │   ├── engine.rs      # ContextEngine trait
│   │   │   ├── memory.rs      # MemoryManager + MemoryProvider trait
│   │   │   ├── prompt.rs      # PromptBuilder (system prompt assembly)
│   │   │   └── scrubber.rs    # StreamingThinkScrubber, StreamingContextScrubber
│   │   └── Cargo.toml
│   │
│   ├── session/               # Session persistence
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── db.rs          # SessionDB (rusqlite)
│   │   │   └── search.rs      # FTS5 search
│   │   └── Cargo.toml
│   │
│   ├── cron/                  # Scheduler
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── jobs.rs        # Job storage
│   │   │   └── scheduler.rs   # tick(), run_job()
│   │   └── Cargo.toml
│   │
│   ├── providers/             # LLM client wrappers
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── openai.rs      # OpenAI-compatible client
│   │   │   ├── anthropic.rs   # Anthropic native client
│   │   │   └── auxiliary.rs   # AuxiliaryClient with fallback chain
│   │   └── Cargo.toml
│   │
│   └── common/                # Shared types and utilities
│       ├── src/
│       │   ├── lib.rs
│       │   ├── config.rs      # Config loading
│       │   ├── constants.rs   # hermes_home(), paths
│       │   ├── message.rs     # Message type (OpenAI format)
│       │   ├── error_classifier.rs
│       │   ├── retry.rs       # jittered_backoff
│       │   ├── guardrails.rs  # ToolCallGuardrailController
│       │   └── display.rs     # Spinner, preview formatting
│       └── Cargo.toml
│
├── src/
│   └── main.rs                # CLI entry point
└── tests/
```

---

## 5. Data Flow Summary

```
User Message
    │
    ▼
AIAgent.run_conversation()
    │
    ├─► build_system_prompt() ──► prompt_builder + memory + skills + context files
    │
    ├─► get_tool_definitions() ──► registry.get_definitions() ──► toolset filtering
    │
    └─► LOOP:
         │
         ├─► transport.build_request(messages, tools, params)
         │
         ├─► HTTP POST to provider ──► streaming SSE
         │
         ├─► transport.normalize_response(raw)
         │       │
         │       ├─► NormalizedResponse { content, tool_calls, usage }
         │       └─► error? ──► error_classifier ──► retry/failover/compress
         │
         ├─► context_engine.update_from_response(usage)
         │
         ├─► IF tool_calls:
         │       ├─► guardrails.observe() ──► check for loops
         │       ├─► handle_function_call(name, args) for each
         │       │       ├─► coerce_tool_args()
         │       │       ├─► registry.dispatch(name, args)
         │       │       └─► subdirectory_hints.check_tool_call()
         │       └─► messages.append(tool_results)
         │
         ├─► IF context_engine.should_compress():
         │       └─► context_engine.compress(messages)
         │
         └─► IF no tool_calls: return response.content
```

---

## 6. Key Design Decisions for Rust

| Decision | Python Approach | Rust Recommendation |
|---|---|---|
| Async runtime | asyncio + sync bridges | tokio (native async) |
| Threading | threading.Lock, ThreadPoolExecutor | tokio tasks, Arc<Mutex/RwLock> |
| HTTP | openai SDK, requests | reqwest + custom SSE parser |
| JSON | json.loads/dumps everywhere | serde_json (zero-copy where possible) |
| SQLite | sqlite3 stdlib | rusqlite (bundled) |
| Config | PyYAML | serde_yaml or toml |
| Regex | re module | regex crate |
| Errors | try/except + string matching | Result<T, AgentError> + thiserror |
| Plugin system | importlib + registry.register() | proc macros or libloading |
| Token estimation | char-count / 4 | tiktoken-rs or char-count / 4 |
| Cron expressions | croniter | cron crate |
| File locking | fcntl | fs2 crate |
| Decimal math | Decimal (stdlib) | rust_decimal |
| Streaming | SSE via httpx/aiohttp | reqwest + eventsource-stream |
| Callbacks | callable parameters | Box<dyn Fn> or mpsc channels |

---

## 7. Migration Strategy

**Phase 1: Core types and agent loop**
- `common/` — Message, Error, Config, Constants
- `transports/` — NormalizedResponse, ToolCall, all 4 transports
- `agent-core/` — AIAgent with run_conversation loop
- `providers/` — OpenAI-compatible HTTP client

**Phase 2: Tool system**
- `tools/` — Registry, toolsets, orchestration
- Port built-in tools one by one (terminal, file ops, web search)

**Phase 3: Context management**
- `context/` — Compressor, memory, prompt builder, scrubbers

**Phase 4: Persistence and scheduling**
- `session/` — SessionDB with FTS5
- `cron/` — Job storage and scheduler

**Phase 5: Entry points**
- CLI (replaces cli.py)
- Gateway adapter (replaces gateway/)
- TUI (replaces ui-tui/)

---

## 8. Complexity Hotspots (What Will Be Hardest)

1. **`run_agent.py` itself** — 15,700 lines of interleaved logic. The streaming path alone has ~20 special cases per provider. Expect this to be the single largest translation effort.

2. **Tool dispatch + async bridging** — Python's `_run_async()` handles 3 different contexts (main thread, worker thread, already-in-async). Rust's native async eliminates this but the tool handlers themselves need careful async design.

3. **Provider-specific quirks** — Anthropic thinking blocks, Codex reasoning items, Gemini thought signatures, OpenRouter metadata. Each adds special-case handling in the transport layer.

4. **Streaming normalization** — Each provider streams SSE differently. The Python code has provider-specific delta parsing spread across multiple files.

5. **Dynamic schema generation** — Tools like `execute_code` and `discord` rebuild their JSON schemas at runtime based on available tools and bot permissions. This needs a flexible schema builder in Rust.

6. **Plugin ecosystem** — Python's importlib makes plugin loading trivial. Rust's `libloading` is more complex and platform-specific. Consider WASM plugins as a cross-platform alternative.
