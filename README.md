<h1 align="center">Hakimi Agent</h1>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.5.144-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1781-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/github/actions/workflow/status/Mouseww/hakimi-agent/ci.yml?branch=main&style=for-the-badge" alt="CI">
</p>

<p align="center">
  <strong>Production-grade AI Agent — rewritten in Rust for speed, safety, and multi-surface control</strong><br>
  <sub>Inspired by <a href="https://github.com/NousResearch/hermes-agent">Nous Research Hermes Agent</a> · built to match and surpass it</sub>
</p>

<p align="center">
  <a href="#one-click-install">Install</a> ·
  <a href="#why-hakimi">Why Hakimi</a> ·
  <a href="#features">Features</a> ·
  <a href="#unique-design">Unique Design</a> ·
  <a href="#commands">Commands</a> ·
  <a href="#hakimi-studio">Studio</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="README_CN.md">中文</a>
</p>

---

<p align="center">
  <img width="1916" height="958" alt="Hakimi Office View" src="https://github.com/user-attachments/assets/64c1e6bb-2835-4a27-9e6c-fd5f49618695" />
</p>

<p align="center">
  <img width="1160" height="896" alt="Hakimi chat / workspace" src="https://github.com/user-attachments/assets/713b3a8f-1d5a-40bb-9e9f-7b771869ed12" />
</p>

---

## One-Click Install

**Linux / macOS**

```bash
curl -sSL https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.sh | bash
```

**Windows (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.ps1 | iex
```

**From source (any platform with Rust)**

```bash
cargo install --git https://github.com/Mouseww/hakimi-agent --locked
# or after clone:
cargo build --release -p hakimi-agent
```

**First run**

```bash
hakimi setup      # guided config wizard (providers, keys, gateway)
hakimi doctor     # diagnose install, systemd, gateway processes, port 3005, config/session DB, connectivity
hakimi            # local TUI (same as `hakimi tui`)
hakimi "prompt"   # one-shot CLI print mode
hakimi chat       # interactive CLI entrypoint (compatibility placeholder until REPL wiring lands)
hakimi gateway start  # multi-platform gateway (Telegram, Discord, …)
hakimi --gateway start # legacy gateway alias
# WebUI runtime has been removed; use Gateway/CLI/TUI/Studio surfaces instead
```

Release binaries (CLI + bundled `hakimi-tui` launcher): GitHub Releases on tags `v*`.
Use `hakimi tui --smoke` to verify the packaged TUI through the main `hakimi` launcher, or `hakimi-tui --smoke` to probe the bundled TUI binary directly without entering raw terminal mode.
Desktop Studio packs (deb / AppImage / MSI / DMG): Actions **Desktop** workflow artifacts, or attached on the same tag release when packaging succeeds.

---

## Why Hakimi?

| | Typical Python agent | **Hakimi (Rust)** |
|--|----------------------|-------------------|
| Startup | ~2s | **~50ms** |
| Idle memory | ~150MB | **~15MB** |
| Async | asyncio + GIL | **tokio native** |
| Tool safety | runtime crashes | **compile-time traits** |
| Error recovery | basic retry | **20+ classifiers + recovery** |
| Context | manual / fragile | **3-tier smart compression** |
| Surfaces | CLI or chat | **CLI · TUI · WebUI · Gateway · Studio desktop** |

**Not a thin wrapper.** Hakimi is a production agent runtime: credential pools with circuit breakers, secret redaction, SSRF guards, path jails, multi-device Studio protocol, and release pipelines for both CLI and desktop.

---

## Features

### Agent core

- Full agent loop in **Rust only** (not TypeScript) with SSE / chunk streaming
- **63+ tools**: files, shell, web, browser/CDP, computer-use readiness, code exec, vision/TTS/STT, todo, cron, memory, knowledge graph, MCP, delegation
- **Sub-agents & teams**: `delegate_task`, named personas, multi-level delegation with depth limits
- **Smart context**: drop stale tool noise → LLM summary → sliding window; model-aware context length
- **Intent + roles**: classify intent, adapt Coder / Researcher / Writer modes
- **Memory**: short/long/working tiers, FTS5, session search (discovery / scroll / browse)
- **Checkpoints**: shared shadow-git store under `~/.hakimi/checkpoints` (not your project `.git`)

### Control surfaces

| Surface | What it is |
|---------|------------|
| **CLI** | REPL, setup, doctor, skills, plugins, profiles |
| **TUI** | ratatui UI, slash commands, voice PTT, skins |
| **WebUI** | Removed runtime; use TUI, Gateway, Studio desktop, or the optional Studio backend surfaces instead |
| **Gateway** | Telegram · Discord · Slack · Signal · WhatsApp · Feishu · WeCom · Matrix · Email · … |
| **Studio** | Local-first workbench: workspace IDE, multi-device handoff, Hub relay, desktop shell |

### Gateway highlights

- Streaming with progressive edits, flood control, UTF-8-safe chunking
- Busy input: queue or interrupt (`gateways.busy_input_mode`)
- Slash commands: `/cron`, `/usage`, `/stop`, `/undo`, `/voice`, `/update`, …
- Optional `hide_tool_details` — keep ⚙️ progress indicators, hide raw STDOUT/JSON dumps
- Cron: intervals + five-field expressions, deliver to origin / home / all channels

### Safety

- Secret redaction (API keys, JWTs) before model/output surfaces
- Prompt-injection heuristics on skills / cron / context files
- SSRF blocklist, dangerous shell patterns, tool-loop guardrails
- Write safe-root sandbox + Studio **path deny policy** (`.env`, `.git`, keys, …)
- Controller / Viewer roles for multi-device Studio sessions

### Extensibility

- **MCP** client (stdio / HTTP / SSE) + catalog snippets
- **HTTP plugins** (YAML) and **WASM** plugin path (evolving)
- **Skills Hub**: install community skills
- OpenAI-compatible discovery: `/v1/models`, `/v1/chat/completions`, `/v1/runs`, …
- Isolated **profiles** (`--profile`) for config / memory / sessions / cron

---

## Unique Design

Ideas that define Hakimi vs “just another agent wrapper”:

1. **Office View** — personas as desks in a shared office: live state, tool progress on monitors, team handoff animation, SSE-driven without polling.
2. **Studio Protocol** — seq-numbered events, gap detection + session reset, single active runner, Controller/Viewer roles, pure Hub relay (no provider keys on the hub).
3. **Local-first execution** — default run on your machine; switch location via Hub worker dispatch when you need remote runners.
4. **Queue + preempt** — dual input model: queue follow-ups or preempt the current run.
5. **Workspace jail + checkpoints** — path-jailed FS, auto pre-write snapshots, worktree isolation for agent sessions.
6. **Desktop = shell** — `hakimi-desktop` embeds backend + WebUI; optional Tauri window is UI only, agent loop stays in Rust.
7. **Hermes alignment** — gateway slash commands, skin engine, voice, session/search shapes — then go further on multi-device and Studio packaging.

Design docs: [`docs/hakimi-studio/`](docs/hakimi-studio/) · [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)

---

## Commands

### Everyday

```bash
hakimi                 # local TUI (default local experience)
hakimi tui             # local TUI explicitly
hakimi "…prompt…"      # one-shot CLI print mode
hakimi chat            # interactive CLI entrypoint (REPL wiring pending; use TUI today)
hakimi setup           # wizard
hakimi doctor          # install/systemd/gateway/config diagnostics
hakimi gateway start   # messaging platforms
hakimi --gateway start # legacy gateway alias
hakimi tui --smoke  # verify packaged TUI via the main launcher
hakimi-tui --smoke  # verify the TUI binary directly without raw terminal mode
# TUI keys: F1 or /help opens command help,
#           Tab completes slash commands or toggles the tools panel,
#           Shift+Tab always toggles the tools panel, Esc clears the current input/completion hint,
#           ↑↓/Ctrl+P/Ctrl+N/PageUp/PageDown scroll; Ctrl+L jumps back to latest,
#           Home/End or Ctrl+A/Ctrl+E jump input start/end,
#           Ctrl+Left/Alt+Left or Alt+B jumps to previous word,
#           Ctrl+Right/Alt+Right or Alt+F jumps to next word,
#           Ctrl+U/Ctrl+K clear before/after cursor,
#           Ctrl+W or Alt+Backspace deletes the previous word; Alt+D or Ctrl+Delete deletes the next word,
#           Ctrl+D deletes under cursor or exits when empty,
#           Ctrl+C quits.
# Type /shortcuts inside the TUI to show the same key reference without the full command help.
# Type /about inside the TUI to show version and local-surface summary without calling the model.
# Type /status inside the TUI to show local session/model/state counters without calling the model.
# Type /usage inside the TUI to show local token/API counters without calling the model.
# Type /doctor inside the TUI to show local TUI readiness diagnostics; use `hakimi doctor` for install/systemd checks.
# Type /logs inside the TUI to show recent local TUI log lines without calling the model.
# Type /model inside the TUI to show the current model; /model NAME explains safe restart-based switching.
# Type /providers inside the TUI to show the current provider/model transport snapshot and supported API modes.
# Type /memory inside the TUI to show read-only persistent memory status and paths.
# Type /skin inside the TUI to show the active skin; /skin NAME explains restart-based theme switching.
# Type /tips inside the TUI to show practical TUI usage tips.
# Type /commands (or /help local) inside the TUI to show the local slash-command catalog without calling the model.
# hakimi serve / --serve are retained only to report that the old WebUI runtime was removed
```

### Studio / desktop

```bash
# Headless Studio backend (WebUI static + /v1/studio WS)
cargo run -p hakimi-desktop -- --bind 127.0.0.1:3015

# Smoke (listen, print URL, exit)
cargo run -p hakimi-desktop -- --once

# Native window (Tauri 2; needs webkit2gtk-4.1 on Linux)
cargo run -p hakimi-desktop --features gui
```

Docs: [`docs/hakimi-studio/DESKTOP.md`](docs/hakimi-studio/DESKTOP.md)

### Ops / products

```bash
hakimi plugin list|install|info …
hakimi skills …
hakimi knowledge …
hakimi skin list|set …
hakimi cron …          # or manage via WebUI / gateway /cron
```

### Gateway (in-chat)

| Command | Purpose |
|---------|---------|
| `/stop` | Cancel run + clear queue |
| `/undo [N]` | Rewind turns / prefill edit |
| `/usage` | Tokens, cost, rate limits |
| `/cron …` | Schedule jobs |
| `/voice …` | TTS / STT mode |
| `/update` | Self-update path (release-aware) |

### Dev

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
RUST_LOG=debug cargo run -p hakimi-cli
```

CI packaging (cloud, not local EL9 GUI):

- **CI** — fmt / clippy / tests  
- **Desktop** — headless + GUI packs on Ubuntu 22.04 / Windows / macOS  
- **Release** — tag `v*` → multi-target CLI binaries (+ desktop assets when Desktop attaches)

---

## Hakimi Studio

Local-first **AI development workbench**:

| Piece | Role |
|-------|------|
| `hakimi-studio-api` | Protocol, EventBus, runtime, agent host |
| `hakimi-workspace` | Path jail, worktrees, checkpoints |
| `hakimi-hub` | Pure relay or embedded runtime |
| `hakimi-server` | Runner + Studio WS + Hub worker client |
| `hakimi-desktop` | Local binary / optional Tauri GUI |
| WebUI Studio panels | Files, chat, devices, ecosystem, cron, checkpoints |

```
Device A (Controller) ──WS──┐
Device B (Viewer)    ──WS──┼── Runtime ── Agent (Rust) ── Tools / Workspace
Hub (relay only)     ──WS──┘       ▲
                                   └── optional remote worker_dispatch
```

Permissions: [`docs/hakimi-studio/PERMISSIONS.md`](docs/hakimi-studio/PERMISSIONS.md)  
Checkpoint: [`docs/hakimi-studio/CHECKPOINT.md`](docs/hakimi-studio/CHECKPOINT.md)  
Protocol: [`docs/hakimi-studio/protocol.md`](docs/hakimi-studio/protocol.md)

---

## Architecture

```
hakimi-agent/
├── hakimi-core/           # Loop, errors, credentials, delegation
├── hakimi-transports/     # OpenAI / Anthropic / Gemini / Bedrock …
├── hakimi-tools/          # Built-in tools + registry
├── hakimi-session/        # SQLite WAL + FTS5
├── hakimi-context/        # Compression, request planning, intent, roles
├── hakimi-knowledge/      # Graph memory
├── hakimi-skills/         # Skills + meta extraction
├── hakimi-cron/           # Persistent scheduler
├── hakimi-gateway/        # Platform adapters
├── hakimi-mcp/            # MCP client
├── hakimi-cli/ · hakimi-tui/
├── hakimi-server/         # Unified serve + Studio + hub worker
├── hakimi-studio-api/ · hakimi-workspace/ · hakimi-hub/
└── hakimi-desktop/        # Studio desktop shell
```

**Turn pipeline (simplified)**

```
Message → Intent / Role → Context (compress → request-local plan)
        → Credential pool → LLM stream → Tool dispatch + guards
        → Session / memory
```

---

## Compare

| | Hermes (Python) | **Hakimi** |
|--|-----------------|------------|
| Language | Python 3.11+ | **Rust 2024** |
| Startup / RAM | ~2s / ~150MB | **~50ms / ~15MB** |
| Tool model | runtime | **compile-time traits** |
| Multi-device Studio | — | **seq events, handoff, Hub relay** |
| Office / persona desks | — | **first-class WebUI** |
| Desktop pack | — | **Tauri 2 CI matrix** |
| Gateway breadth | strong | **parity + more adapters** |

---

## Configuration (quick)

| Item | Notes |
|------|--------|
| Config dir | `~/.hakimi/` (or profile-scoped) |
| WebUI password | `HAKIMI_WEBUI_PASSWORD` → Bearer prompt |
| Language | `display.language` / `HAKIMI_LANGUAGE` |
| Hide tool dumps | `gateways.hide_tool_details` (keeps ⚙️ progress) |
| Busy input | `gateways.busy_input_mode`: `queue` \| `interrupt` |

Run `hakimi setup` for the full wizard.

---

## License

MIT License

---

**中文说明** → [README_CN.md](README_CN.md) · **Studio 设计** → [docs/hakimi-studio/DESIGN.md](docs/hakimi-studio/DESIGN.md)
