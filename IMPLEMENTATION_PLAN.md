# Hakimi Agent — Rust Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Rewrite Hermes Agent in Rust as "Hakimi Agent" — a high-performance, async-native AI agent framework.

**Architecture:** Workspace of 10 crates. tokio async runtime. Trait-based tool system. serde for all serialization. SQLite for session storage.

**Tech Stack:** Rust 1.95, tokio, serde, reqwest, rusqlite, clap, ratatui, tracing

---

## Crate Dependency Graph

```
hakimi-common          (no deps — shared types)
    ↑
hakimi-config          (config loading)
hakimi-session         (SQLite store)
hakimi-transports      (provider adapters)
hakimi-tools           (tool registry)
hakimi-context         (compression, memory, prompts)
hakimi-cron            (scheduler)
    ↑
hakimi-core            (agent loop — depends on all above)
    ↑
hakimi-gateway         (platform adapters)
hakimi-cli             (binary entry point)
```

---

## Phase 1: Foundation (Parallel — 3 subagents)

### Task 1: hakimi-common — Shared Types
- Message types (OpenAI format)
- ToolCall, ToolResult, Usage
- Error types
- Config structs

### Task 2: hakimi-config — Configuration System
- YAML config loading with serde
- Profile support
- Environment variable expansion

### Task 3: hakimi-session — Session Store
- SQLite with WAL mode
- FTS5 full-text search
- Session CRUD, message CRUD

## Phase 2: Core Systems (Parallel — 3 subagents)

### Task 4: hakimi-transports — Provider Transport Layer
- ProviderTransport trait
- ChatCompletions transport
- Streaming support with think-block scrubbing

### Task 5: hakimi-tools — Tool Registry
- Tool trait definition
- ToolRegistry with RwLock
- Built-in tools (read_file, write_file, terminal, search_files)

### Task 6: hakimi-context — Context Management
- ContextEngine trait
- ContextCompressor
- PromptBuilder
- MemoryProvider trait

## Phase 3: Integration (Parallel — 2 subagents)

### Task 7: hakimi-core — Agent Loop
- AIAgent struct with builder pattern
- run_conversation() async loop
- Error classification and retry
- Budget tracking

### Task 8: hakimi-cron + hakimi-gateway
- Cron scheduler
- Gateway platform trait
- Telegram adapter skeleton

## Phase 4: CLI

### Task 9: hakimi-cli — Entry Point
- clap argument parsing
- Interactive REPL with ratatui
- Command dispatch
