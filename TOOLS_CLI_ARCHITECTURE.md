# Hermes Agent — Tool System & CLI Architecture Analysis

## Purpose
Analysis of the Python codebase for rewriting in Rust. Covers tool registration/dispatch, CLI architecture, config system, and Rust migration notes.

---

## 1. Tool Registration Pattern

### 1.1 The Registry (`tools/registry.py`)

**Singleton pattern** — module-level instance:
```python
# tools/registry.py line 518
registry = ToolRegistry()
```

**ToolEntry** holds per-tool metadata:
- `name` — unique tool name (e.g. `"terminal"`, `"read_file"`)
- `toolset` — group name (e.g. `"file"`, `"memory"`, `"browser"`, `"mcp-<server>"`)
- `schema` — OpenAI function-calling JSON schema (`{"name", "description", "parameters"}`)
- `handler` — `Callable[[dict, **kwargs], str]` — takes args dict, returns JSON string
- `check_fn` — `Callable[[], bool]` — availability check (e.g. Docker installed?)
- `requires_env` — list of env var names needed
- `is_async` — bool, handler is a coroutine
- `emoji` — display emoji
- `max_result_size_chars` — optional output truncation limit
- `dynamic_schema_overrides` — `Callable[[], dict]` for runtime schema patches

### 1.2 Self-Registration at Import Time

Each tool file calls `registry.register(...)` at **module level** (top-level statement):
```python
# tools/file_tools.py
registry.register(name="read_file", toolset="file", schema=READ_FILE_SCHEMA,
                  handler=_handle_read_file, check_fn=_check_file_reqs,
                  emoji="📖", max_result_size_chars=100_000)
```

**No decorator pattern** — raw function calls at module scope.

### 1.3 Discovery (`discover_builtin_tools()`)

```python
def discover_builtin_tools(tools_dir=None) -> List[str]:
```

1. Scans `tools/*.py` (sorted)
2. Excludes `__init__.py`, `registry.py`, `mcp_tool.py`
3. **AST inspection** — parses each file, checks for top-level `registry.register(...)` call
4. Imports matching modules via `importlib.import_module()`
5. Each import triggers the module-level `register()` calls

**Import chain** (circular-import safe):
```
tools/registry.py  (no deps)
       ↑
tools/*.py  (import registry at module level)
       ↑
model_tools.py  (imports registry + triggers discovery)
       ↑
run_agent.py, cli.py, batch_runner.py
```

### 1.4 Handler Signature

Handlers receive:
- `args: dict` — the tool call arguments (from model JSON)
- `**kwargs` — extra context (`task_id`, `store`, etc.)

Return: `str` (JSON) — always `json.dumps(...)`.

Helper functions:
- `tool_error(message, **extra) -> str` — `{"error": "..."}`
- `tool_result(data=None, **kwargs) -> str` — arbitrary JSON

### 1.5 MCP Dynamic Registration

MCP tools (`tools/mcp_tool.py`) register dynamically at runtime:
- Creates per-server toolset (`"mcp-<server_name>"`)
- Registers tools from MCP server responses
- Supports deregistration + re-registration on `notifications/tools/list_changed`
- `registry.deregister(name)` removes a tool cleanly
- Generation counter bumped on every mutation for cache invalidation

### 1.6 check_fn TTL Cache

- `check_fn` results cached for 30 seconds (monotonic clock)
- Thread-safe with `threading.Lock`
- `invalidate_check_fn_cache()` clears cache on config changes

---

## 2. Tool Dispatch

### 2.1 Schema Retrieval (`get_definitions()`)

```python
def get_definitions(self, tool_names: Set[str], quiet=False) -> List[dict]:
```

- Returns OpenAI-format: `[{"type": "function", "function": {...}}]`
- Filters by `check_fn` (cached)
- Applies `dynamic_schema_overrides` at call time
- Used by `model_tools.get_tool_definitions()` to build the tools array for API calls

### 2.2 Dispatch (`dispatch()`)

```python
def dispatch(self, name: str, args: dict, **kwargs) -> str:
```

- Looks up `ToolEntry` by name
- Async handlers bridged via `_run_async()`
- All exceptions caught → `{"error": "..."}` JSON string

### 2.3 Toolset System (`toolsets.py`)

- `_HERMES_CORE_TOOLS` — canonical list of ~50 tool names shared across CLI and all platforms
- `TOOLSETS` dict — named groups with `tools` list and `includes` for composition
- Example: `"hermes-cli"` toolset includes `_HERMES_CORE_TOOLS` plus extras
- Toolset resolution expands includes recursively

---

## 3. CLI Architecture (`cli.py`)

### 3.1 Structure

**`HermesCLI` class** (line 2278, ~11k LOC total file):
- `__init__` takes: model, toolsets, provider, api_key, base_url, max_turns, verbose, compact, resume, checkpoints, pass_session_id, ignore_rules
- Uses **Rich** for display (Console, panels, tables)
- Uses **prompt_toolkit** for input (TUI layout with `Application`, `TextArea`, `KeyBindings`)
- Fixed input area at bottom, scrollable output above

### 3.2 Input System

- prompt_toolkit `Application` with `HSplit` layout
- `FileHistory` for persistent command history
- Autocomplete via prompt_toolkit completions
- Key bindings for Ctrl+L (redraw), Shift+Enter (newline), Ctrl+Enter
- `patch_stdout` for clean async output

### 3.3 Display System

- `KawaiiSpinner` (`agent/display.py`) — animated faces during API calls
- `┊` activity feed for tool results
- Skin engine (`hermes_cli/skin_engine.py`) — data-driven theming
- Config keys: `display.compact`, `display.streaming`, `display.skin`, `display.show_reasoning`, `display.tool_progress`, etc.

### 3.4 Command Dispatch (`process_command()`)

```python
def process_command(self, command: str) -> bool:
```

1. Lowercases command, resolves aliases via `hermes_cli.commands.resolve_command()`
2. Giant if/elif chain on canonical name:
   - `quit/exit` → return False
   - `help` → show_help()
   - `tools` → _handle_tools_command()
   - `config` → show_config()
   - `clear` → new_session()
   - `model` → model switch
   - `resume` → session restore
   - etc.
3. Returns `True` to continue, `False` to exit

### 3.5 Core Loop

```
while True:
    user_input = prompt_toolkit application.run()
    if user_input.startswith("/"):
        process_command(user_input)
    else:
        response = agent.chat(user_input)
        display_response(response)
```

---

## 4. Config System

### 4.1 Two Config Files

| File | Purpose | Format |
|------|---------|--------|
| `~/.hermes/config.yaml` | All settings (model, toolsets, terminal, display, etc.) | YAML |
| `~/.hermes/.env` | API keys and secrets only | dotenv |

### 4.2 Config Loading (`hermes_cli/config.py`)

**`load_config()`** (line 4068):
1. Ensures `~/.hermes/` exists with secure permissions (0700)
2. Reads `config.yaml` via `yaml.safe_load()`
3. Deep-merges user config onto `DEFAULT_CONFIG`
4. Normalizes keys (`max_turns` legacy → `agent.max_turns`, etc.)
5. Expands `${ENV_VAR}` references
6. **Caches** on `(mtime_ns, size)` — returns deep copy on cache hit
7. Thread-safe via `threading.RLock`

**`save_config()`**: Atomic write via `atomic_replace()`, clears cache.

**`cfg_get(cfg, *keys, default=None)`** — safe nested dict traversal.

### 4.3 DEFAULT_CONFIG (cli.py, line 303)

```python
{
    "model": {"default": "", "base_url": "", "provider": "auto"},
    "terminal": {"env_type": "local", "cwd": ".", "timeout": 60, ...},
    "browser": {"inactivity_timeout": 120, "engine": "auto"},
    "compression": {"enabled": True, "threshold": 0.50},
    "agent": {"max_turns": 90, "verbose": False, "system_prompt": "", ...},
    "display": {"compact": False, "streaming": True, "skin": "default", ...},
    "auxiliary": {"vision": {...}, "web_extract": {...}},
    "delegation": {"max_iterations": 45, "model": "", ...},
    ...
}
```

### 4.4 Config Expansion in CLI (`load_cli_config()`)

Additional layer in `cli.py` that:
- Loads from `~/.hermes/config.yaml` or `./cli-config.yaml` (project fallback)
- Handles `HERMES_IGNORE_USER_CONFIG=1`
- Bridges terminal config to env vars
- Expands `${ENV_VAR}` in values

### 4.5 Profile Support

- `HERMES_HOME` env var overrides `~/.hermes`
- `get_hermes_home()` — single source of truth
- `get_default_hermes_root()` — root dir for profile operations
- `active_profile` file tracks current profile
- `get_subprocess_home()` — per-profile HOME for subprocesses

---

## 5. Constants (`hermes_constants.py`)

Import-safe, zero-dependency module:

| Function | Purpose |
|----------|---------|
| `get_hermes_home()` | `~/.hermes` or `$HERMES_HOME` |
| `get_default_hermes_root()` | Root dir for profile ops |
| `display_hermes_home()` | User-friendly path string |
| `get_config_path()` | `get_hermes_home() / "config.yaml"` |
| `get_env_path()` | `get_hermes_home() / ".env"` |
| `get_skills_dir()` | `get_hermes_home() / "skills"` |
| `get_subprocess_home()` | Per-profile HOME for subprocesses |
| `is_container()` | Docker/Podman detection |
| `is_wsl()` | WSL detection |
| `is_termux()` | Termux detection |
| `parse_reasoning_effort()` | Effort level → config dict |
| `apply_ipv4_preference()` | Socket monkey-patch for IPv4 |

---

## 6. Rust Migration — What Changes

### 6.1 Tool System → Trait-Based

**Current (Python):**
```python
registry.register(name="memory", toolset="memory", schema=MEMORY_SCHEMA,
                  handler=lambda args, **kw: memory_tool(...),
                  check_fn=check_memory_requirements, emoji="🧠")
```

**Rust equivalent:**
```rust
trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn toolset(&self) -> &str;
    fn schema(&self) -> &serde_json::Value;  // OpenAI JSON schema
    fn emoji(&self) -> &str { "⚡" }
    fn check_available(&self) -> bool { true }
    fn max_result_size(&self) -> Option<usize> { None }

    // Sync dispatch — most tools
    fn execute(&self, args: &serde_json::Value, ctx: &ToolContext) -> ToolResult;

    // Optional async override
    async fn execute_async(&self, args: &serde_json::Value, ctx: &ToolContext) -> ToolResult {
        // Default: call sync execute
    }
}
```

**Registration:** Inventory-style macro or explicit `register_tool!` macro:
```rust
// Option A: inventory crate (link-time collection)
#[distributed_slice(TOOLS)]
fn register_memory() -> Box<dyn Tool> { Box::new(MemoryTool) }

// Option B: explicit registry
fn register_all_tools(registry: &mut ToolRegistry) {
    registry.register(Box::new(MemoryTool));
    registry.register(Box::new(FileTools::read_file()));
    // ...
}
```

**Discovery:** Compile-time (inventory/linkme) instead of AST scanning + importlib.

**Dispatch:** `HashMap<String, Arc<dyn Tool>>` with `registry.get(name)?.execute(args, ctx)`.

**Schema:** Serialize with `serde_json` — define as Rust structs or use `schemars` for auto-generation from `#[derive(JsonSchema)]` on args structs.

### 6.2 CLI → clap + ratatui/crossterm

**Current:** prompt_toolkit TUI with Rich display.

**Rust options:**
- **clap** — CLI argument parsing (replaces `argparse` / manual arg parsing)
- **ratatui** (or **crossterm**) — TUI framework for the interactive REPL
- **indicatif** — spinners/progress bars (replaces KawaiiSpinner)
- **colored** / **owo-colors** — terminal colors (replaces Rich)

**Command dispatch:** Enum-based matching instead of if/elif chain:
```rust
enum Command {
    Help, Quit, Clear, Tools, Config, Model(String), Resume(String), ...
}

impl Command {
    fn parse(input: &str) -> Option<Command> { ... }
}
```

### 6.3 Config → serde + toml/yaml

**Current:** YAML config loaded with `yaml.safe_load()`, deep-merged with defaults, cached on mtime.

**Rust:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermesConfig {
    model: ModelConfig,
    terminal: TerminalConfig,
    agent: AgentConfig,
    display: DisplayConfig,
    // ...
}

impl Default for HermesConfig {
    fn default() -> Self { /* hardcoded defaults */ }
}
```

- **serde** for deserialization
- **serde_yaml** or **toml** for format (YAML is current; TOML is Rustier)
- **notify** crate for file watching (replaces mtime polling)
- **dirs** crate for `~/.hermes` path resolution

### 6.4 Async Model

Python uses sync loops with `threading` for concurrency. Rust should use:
- **tokio** runtime for async tool execution
- `tokio::sync::RwLock` for registry (replaces `threading.RLock`)
- `tokio::task::spawn_blocking` for blocking I/O tools

### 6.5 Key Architectural Differences

| Aspect | Python | Rust |
|--------|--------|------|
| Tool discovery | AST scan + importlib | Compile-time (inventory/linkme) |
| Registration | Module-level `register()` calls | `impl Tool` + registry macro |
| Schema | Hand-written dicts | `#[derive(JsonSchema)]` or serde structs |
| Handler dispatch | `Callable[[dict], str]` | `fn execute(&self, &Value, &Ctx) -> Result<String>` |
| Config loading | yaml.safe_load + deep_merge + mtime cache | serde::Deserialize + merge crate + notify |
| CLI input | prompt_toolkit (Python TUI) | ratatui/crossterm (Rust TUI) |
| Concurrency | threading + asyncio bridge | tokio async runtime |
| Error handling | try/except → JSON error strings | `Result<T, ToolError>` → JSON |
| Dynamic tools (MCP) | Runtime register/deregister | Same, but behind `Arc<RwLock<HashMap>>` |

### 6.6 Critical Migration Risks

1. **MCP dynamic registration** — needs careful locking with `Arc<RwLock<ToolRegistry>>`
2. **Dynamic schema overrides** — closures that modify schema at call time
3. **50+ tool files** — each needs porting; some are complex (terminal: 2349 lines, browser: 3640 lines)
4. **Plugin system** — Python plugins loaded dynamically; Rust may need WASM or native dynamic libs
5. **Platform adapters** (Telegram, Discord, Slack, etc.) — 20+ gateway platforms
6. **Skin engine** — data-driven theming system

---

## 7. File Inventory Summary

| Component | File(s) | LOC | Complexity |
|-----------|---------|-----|------------|
| Tool Registry | `tools/registry.py` | 563 | Medium |
| Tool Implementations | `tools/*.py` (~50 files) | ~30k total | High |
| Toolsets | `toolsets.py` | 855 | Low |
| CLI | `cli.py` | 13,540 | Very High |
| Config | `hermes_cli/config.py` | 5,193 | High |
| Constants | `hermes_constants.py` | 345 | Low |
| MCP Integration | `tools/mcp_tool.py` | ~3,000 | High |
| Commands | `hermes_cli/commands.py` | ~500 | Medium |

---

## 8. Recommended Rust Crate Stack

```
tokio           — async runtime
clap            — CLI argument parsing
ratatui         — TUI framework
crossterm       — terminal backend
serde / serde_json — serialization
serde_yaml      — config loading (or toml)
schemars        — JSON Schema generation
reqwest         — HTTP client
tracing         — logging
dirs            — home directory
notify          — file watching
inventory       — tool registration
serde_json      — JSON tool schemas
tokio::sync     — RwLock for registry
```
