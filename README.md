<h1 align="center">Hakimi Agent</h1>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.5.50-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1769-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/lines-44K+-orange?style=for-the-badge" alt="Lines">
</p>

<p align="center">
  <strong>Production-grade AI Agent framework, rewritten in Rust for speed and reliability</strong><br>
  <sub>Inspired by <a href="https://github.com/NousResearch/hermes-agent">Nous Research's Hermes Agent</a> — built from the ground up in Rust</sub>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#why-hakimi">Why Hakimi</a> ·
  <a href="#capabilities">Capabilities</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#compare">Compare</a> ·
  <a href="README_CN.md">中文</a>
</p>

---
<img width="1916" height="958" alt="AnythingAgentRecord" src="https://github.com/user-attachments/assets/64c1e6bb-2835-4a27-9e6c-fd5f49618695" />

<img width="1160" height="896" alt="image" src="https://github.com/user-attachments/assets/713b3a8f-1d5a-40bb-9e9f-7b771869ed12" />

---

## ✨ Recent Updates (v0.5.50)

**🎯 Team 工具返回优化（完整版）：**
- ✅ **直接返回结果** — team 工具调用返回 teammate 的实际输出，Agent 不再需要 read_file
- 🔧 **过滤用户展示** — Gateway 层自动过滤 `hakimi_tool_result:team` 消息，用户不会看到冗余的工具结果
- 📊 **简洁折叠块** — 工具调用列表以折叠块形式显示，自动去除 `[工具调用详情]` 标题行
- 🚀 **用户体验优化** — 聊天记录清晰紧凑：开始 → 工具列表（折叠） → 完成，工具结果留给 Agent 内部使用

**Previous Updates (v0.5.49):**

**🎯 Team 工具返回优化：**
- ✅ **直接返回结果** — team 工具调用返回 teammate 的实际输出，Agent 不再需要 read_file
- 🔧 **过滤用户展示** — Gateway 层自动过滤 `hakimi_tool_result:team` 消息，用户不会看到冗余的工具结果
- 📊 **更好的上下文** — 主 Agent 能立即获取子任务完整结果，无需额外步骤
- 🚀 **用户体验改进** — 聊天记录更清晰，只显示有意义的协作过程（开始/进度/完成），工具结果留给 Agent 内部使用

**Previous Updates (v0.5.48):**

**📊 子 Agent 工具调用记录折叠显示：**
- ✅ **折叠块内部显示工具列表** — 子 agent 完成时，在 Telegram 的折叠块（spoiler）内显示逐行的工具调用记录
- 🛠️ **收集工具调用历史** — 在 delegate.rs 中使用 `Arc<Mutex<Vec<String>>>` 收集所有工具调用
- 📋 **格式化协作消息** — 识别 `[工具调用详情]` 标记，自动转换为 Telegram 的 `||spoiler||` 语法
- 🎯 **实时进度 + 最终汇总** — 保留实时工具通知，同时在完成时提供完整工具列表
- 🚀 **用户体验优化** — 不再只显示空折叠块，而是在折叠内容中显示所有工具调用详情

**Previous Updates (v0.5.47):**

**🔧 子 Agent 工具调用可见性修复：**
- ✅ **显示工具计数** — 子 agent 完成时显示"完成，返回结果（使用了 N 个工具）"
- 🛠️ **工具调用追踪** — 在 delegate.rs 中添加 AtomicUsize 计数器，与 team.rs 保持一致
- 📊 **实时工具通知** — 子 agent 执行工具时通过 streaming callback 转发 `\u{001e}hakimi_tool:` 事件
- 🎯 **对齐 Persona Team 行为** — delegate_task 和 persona 团队协作现在使用相同的进度报告机制

**Previous Updates (v0.5.46):**

**🎯 Telegram 流式输出格式稳定性修复：**
- ✅ **智能未闭合语法检测** — 新增 `sanitize_for_streaming()` 函数，实时检测并移除未闭合的 Markdown 语法
- 🔧 **消除 UI 闪烁** — 流式更新时自动截断未完成的 `**粗体**`、`` `代码` ``、`` ```代码块``` `` 等语法，防止 Telegram 解析失败
- 📊 **渐进式渲染** — 已完成的 Markdown 格式正常显示，未完成部分作为纯文本，输出完成后格式完整
- 🚀 **用户体验优化** — 彻底解决流式输出过程中"有格式 ↔ 无格式"反复切换导致的视觉不稳定问题
- ⚡ **零性能损耗** — 使用 `saturating_sub()` 避免整数溢出，逻辑高效且安全

**Previous Updates (v0.5.36):**

**🔌 Teams Webhook Gateway 注册修复：**
- ✅ **自动注册 Adapter** — 在统一模式启动时自动注册 TeamsWebhookAdapter 到 Gateway
- 🎯 **配置驱动** — 读取 `config.yaml` 中的 `gateways.teams_webhook.hmac_secret` 和 `default_workflow_url`
- 🔧 **完整双向通信** — Teams webhook 收到消息后可以正常通过 Gateway 发送 AI 回复
- 💡 **统一架构** — Teams Webhook 与 Telegram、Discord、Slack 等平台一致的 Gateway adapter 架构

**Previous Updates (v0.5.35):**

**🎨 Telegram Markdown 稳定渲染修复：**
- ✅ **智能清理 sanitize_for_markdown()** — 替代 v0.5.27 过度激进的 `escape_markdown()`
- 🔧 **选择性转义** — 只转义会导致解析错误的字符（括号、方括号、表格分隔符），保留格式化标记（`*`、`` ` ``、`**`）
- 📊 **表格支持** — 将 `|` 替换为 Unicode 盒绘字符 `│`，避免解析错误同时保持表格视觉效果
- 🎯 **解决 UI 闪烁** — 彻底消除流式输出过程中"格式化 ↔ 纯文本"反复切换的问题
- 💎 **保留所有格式** — 粗体、斜体、代码块、链接等 Markdown 功能完全可用

**Previous Updates (v0.5.34):**

**🔄 Teams Webhook 完整回复功能：**
- ✅ **Gateway 路由集成** — 后台任务通过 Gateway.route_message() 发送回复到 Teams
- 🎯 **自动匹配 chat_id** — 后台任务构造 `teams_{channel_id}` 格式的 chat_id，匹配 adapter 的映射
- 🔧 **优雅降级** — Gateway 不可用时（WebUI-only 模式）记录详细日志，不会崩溃
- 📝 **完整日志追踪** — 发送成功/失败都记录 info/warn 日志，方便排查问题
- 🏗️ **统一架构** — 统一模式下，Teams webhook 和其他平台使用相同的 Gateway 路由机制

**Previous Updates (v0.5.33):**

**💬 Teams Webhook 友好即时响应 + 日志增强：**
- 🎨 **立即返回 Adaptive Card** — 不再返回空 202，而是返回友好的"✅ 收到消息，正在处理..."卡片
- 📝 **详细后台日志** — 后台任务处理开始/完成/结果都记录 info 日志，方便排查问题
- 🌐 **提取 service_url** — 为后续实现 Bot Framework API 回复做准备（当前先记录日志）
- 🔧 **TODO 标记** — 明确标记了两种回复方式：Bot Framework API 或 Power Automate Webhook

**Previous Updates (v0.5.32):**

**⚡ Teams Webhook 异步非阻塞修复：**
- 🚀 **立即返回 202 Accepted** — Teams webhook 处理改为异步模式，收到请求后立即返回，彻底解决 10 秒超时问题
- 🔄 **后台任务处理** — 用 `tokio::spawn` 创建独立任务处理 AI 消息，不阻塞事件循环
- 🎯 **并发友好** — Agent 处理其他任务时，新 Teams 请求不再等待锁释放
- 📝 **回复机制 TODO** — 标记了 Power Automate Workflow URL 回调实现（下个版本完成）

**Previous Updates (v0.5.31):**

**🐛 macOS CI Build Fix:**
- ✅ **Axum API Compatibility** — Fixed `RawBody` usage → `Request<Body>` for Axum 0.8 API changes
- 🔓 **Module Visibility** — Made `teams_webhook` module public in `hakimi-gateway` lib.rs
- 🧹 **Simplified Config** — Removed incomplete config validation in Teams webhook handler (TODO for future)
- 🚀 **CI Green** — All platforms (Linux x64/ARM64, macOS x64/ARM64, Windows x64/ARM64) now build successfully

**Previous Updates (v0.5.30):**

**🏢 Teams Webhook Integration (3005 Port Reuse):**
- ♻️ **Unified Port Deployment** — Teams Webhook endpoints integrated into WebUI server (3005 port), eliminating need for separate service
- 🔌 **Simple Reverse Proxy Setup** — Works with existing Nginx configuration, no new domains or ports required
- 🚀 **Two New Endpoints** — `POST /webhooks/teams/inbound` for messages, `GET /webhooks/teams/health` for health checks
- 💬 **Adaptive Card Responses** — Returns formatted Adaptive Cards directly from the WebUI handler
- 🔒 **Config-Based HMAC** — Reads Teams webhook secret from `config.yaml` gateway section
- 📦 **Cleaner Architecture** — Removed standalone `teams-webhook-server` binary, consolidated into main server

**Previous Updates (v0.5.29):**

**🏢 Microsoft Teams Webhook Integration:**
- 🔌 **No Azure Bot Required** — Direct integration via Teams Outgoing Webhooks + Power Automate Workflows
- 🔒 **HMAC Signature Verification** — Secure inbound message authentication with SHA-256 HMAC
- 🎨 **Adaptive Card Builder** — Rich card formatting with titles, facts, buttons, and custom layouts
- ⚡ **10-Second Response** — Async task processing with immediate receipt acknowledgment
- 📡 **Bidirectional Channels** — Inbound via HTTP POST `/teams/inbound`, outbound via Workflows webhook URLs
- 🗺️ **Multi-Channel Routing** — Channel ID → Workflows URL mapping for project-specific notifications
- 📚 **Complete Documentation** — Full setup guide at `docs/integrations/teams-webhook.md`

**Previous Updates (v0.5.28):**

**🎮 QQ Bot & ClawBot (WeChat) Support:**
- 🤖 **QQ Bot Integration** — Added QQ Bot to setup wizard, requires AppID + Token from QQ Open Platform
- 💬 **ClawBot (WeChat) Support** — Added WeChat integration via ClawBot server (endpoint + optional token)
- 🔧 **Multi-Platform Setup** — Expanded platform adapter options from 3 to 5 (Telegram, QQ, ClawBot, Discord, Slack)
- ✨ **Better User Experience** — Interactive prompts guide users through QQ AppID/Token and ClawBot endpoint configuration

**Previous Updates (v0.5.27):**

**💎 Stable Telegram Markdown UI:**
- 🎨 **Automatic Markdown Escaping** — All outbound text now escapes special characters (`_`, `*`, `[`, `]`, `(`, `)`, `` ` ``) to prevent Telegram parse errors
- 🚀 **Eliminated UI Flicker** — Removed fallback-to-plain-text retry logic that caused mid-stream format toggling
- ✨ **Always Beautiful** — Messages, media captions, and drafts render with consistent Markdown styling throughout the entire conversation
- 🔧 **Applied Everywhere** — Covers `send_message`, `send_message_get_id`, `edit_message`, `send_remote_media`, and `send_local_media`

**Previous Updates (v0.5.26):**

**🎯 Clean Teammate Task Box Output:**
- 🧹 **Suppressed Intermediate Output** — Teammate task boxes no longer flood the chat with detailed tool invocations
- 📊 **Tool Usage Summary** — Task completion now shows "完成，返回结果（使用了 N 个工具）" for non-zero tool usage
- ✨ **Task Box Shows Only** — Start marker → Final tool count → Completion status
- 🎨 **Cleaner UX** — Follows user's high standards for minimal, focused output (no redundant verbose logs)

**Previous Updates (v0.5.25):**

**🧠 Advanced Context Compression System:**
- 🎯 **Three-Phase Compression** — Tool output pruning → Boundary protection → LLM structured summarization (inspired by Hermes Agent)
- ✨ **Smart Boundaries** — Protects head (system prompt + first N messages) + dynamic tail (token budget-based)
- 🔧 **Tool Call Integrity** — Aligns boundaries to avoid splitting tool call/result pairs
- 📊 **Anti-Thrashing** — Skips compression if last 2 attempts saved <10% each
- 🏗️ **Iterative Summary** — Updates previous summaries instead of starting from scratch
- 🚀 **Foundation for Progressive Compression** — Ready for multi-level (40%/60%/80%) thresholds and intelligent routing

**Previous Updates (v0.5.24):**

**Critical `/stop` Command Fix:**
- 🐛 **Fixed Non-Functional /stop** — `/stop` command now correctly cancels the active running task
- 🎯 **Root Cause** — Previous implementation cancelled its own token instead of the running task's token
- ✅ **New Behavior** — Finds and cancels the active task from `active_tasks` registry, shows status feedback
- 💡 **User Feedback** — Now shows "⏹️ 已停止当前任务。" or "ℹ️ 当前没有正在运行的任务。" depending on state

**Previous Updates (v0.5.23):**

**Team Tool Output Cleanup:**
- 🎯 **Suppressed Verbose Results** — Team tool now returns compact completion confirmations instead of full agent responses
- ✨ **Clean Main Chat** — Tool results show "✓ teammate completed: task" format, preventing response duplication in chat
- 🔧 **Combined with v0.5.22** — Task boxes show minimal progress + main chat no longer cluttered with full teammate outputs

**Previous Updates (v0.5.22):**

**Team Collaboration UI Optimization:**
- 🎯 **Clean Task Box Display** — Teammate agent task boxes now show only start/tool calls/completion, suppressing verbose text output
- ✨ **Focused Progress Updates** — Task boxes display essential progress markers without cluttering the chat with full agent responses
- 🔧 **Improved Multi-Agent UX** — Cleaner delegation visualization keeps the main conversation readable during complex workflows

**Previous Updates (v0.5.20):**

**Team Execution Modes — Sequential & Staged Collaboration:**
- ✅ **Sequential Mode** — Tasks run one after another, each receiving previous results as context (`mode: "sequential"`)
- ✅ **Stages Mode** — Multi-phase workflows: parallel execution within each stage, sequential between stages (`stages` parameter)
- ✅ **Parallel Mode** — Existing concurrent behavior preserved as default (`mode: "parallel"`)
- 🎯 **Dependency Support** — Agent can now orchestrate complex workflows with task dependencies

**Before v0.5.20:**
```json
{
  "tasks": [...]  // ❌ All tasks always run in parallel, no dependency support
}
```

**After v0.5.20:**
```json
// Sequential: later tasks depend on earlier results
{"mode": "sequential", "tasks": [
  {"teammate": "researcher", "task": "Find solution"},
  {"teammate": "coder", "task": "Implement based on research"}
]}

// Stages: mixed parallel/sequential
{"stages": [
  {"tasks": [{"teammate": "researcher", "task": "Research"}]},
  {"tasks": [  // These run in parallel
    {"teammate": "backend", "task": "Backend"},
    {"teammate": "frontend", "task": "Frontend"}
  ]},
  {"tasks": [{"teammate": "reviewer", "task": "Review all"}]}
]}
```

**Impact:**
- Agent can handle complex dependency chains (research → implement → test)
- Mixed workflows supported (parallel development after sequential planning)
- Previous results automatically injected as context for dependent tasks

### Previous Updates (v0.5.19)

**Team Task Division — Smart Multi-Agent Collaboration:**
- ✅ **Individual Task Assignment** — Each teammate now receives a **different sub-task** tailored to their expertise
- ✅ **New `tasks` Parameter** — Structured task division: `[{teammate: "researcher", task: "Search solutions"}, {teammate: "coder", task: "Implement fix"}]`
- 🔧 **Deprecation Warning** — Old `teammates` array (same task for all) marked as DEPRECATED
- 🎯 **True Division of Labor** — No more duplicate work — agents collaborate with proper specialization

**Before v0.5.19:**
```json
{
  "teammates": ["researcher", "coder"],
  "task": "Complete this PR"  // ❌ Both receive the same task
}
```

**After v0.5.19:**
```json
{
  "tasks": [
    {"teammate": "researcher", "task": "Find best practices", "context": "Focus on security"},
    {"teammate": "coder", "task": "Implement the changes", "context": "Use TypeScript"}
  ]
}
```

**Impact:**
- Agent orchestrator can divide complex tasks into specialized sub-tasks
- Each teammate works on what they're best at, no redundant effort
- Better parallel collaboration and faster delivery

### Previous Updates (v0.5.18)

**SSE Keepalive Fix — No More WebUI Timeout:**
- ✅ **15-Second Keepalive** — SSE connection sends keepalive comments every 15 seconds
- ✅ **Fixed Network Error** — Long-running tasks no longer timeout with "network error"
- 🔧 **Stable Connections** — Prevents browser and proxy timeout on idle connections
- 🎯 **Better UX** — Users can send complex requests without worrying about timeout

**Technical Details:**
- Added `keep_alive()` to `sse_response_from_rx` function
- Sends `: keepalive\n\n` comments every 15 seconds (SSE standard)
- Matches pattern from run SSE endpoint but with shorter interval for chat interactivity
- Compatible with all SSE-supporting browsers and proxies

**Before v0.5.18:**
- Long agent tasks (team collaboration, complex analysis) would timeout
- WebUI showed "network error" after ~30-60 seconds of silence
- Users had to retry or break tasks into smaller pieces

**After v0.5.18:**
- All tasks complete successfully regardless of duration
- Stable connection maintained throughout entire agent response
- Seamless experience for complex multi-step operations

### Previous Updates (v0.5.17)

**Enhanced Agent Delegation Proactivity — Smarter Team Collaboration:**
- ✅ **Proactive Tool Description** — Agent now actively considers delegation in 4 explicit scenarios
- ✅ **Scenario-Based Guidance** — Clear triggers: domain expertise gaps, parallel work, divide-and-conquer, specialized skills
- 🤝 **Empowered Teammates** — Enhanced collaboration contract emphasizes value and actionable guidance
- 🎯 **Cultural Shift** — "Delegation is a strength, not a weakness — leverage your team early and often"

**Key Changes:**
- Rewrote `team` tool description with PROACTIVE framing instead of passive "use when better suited"
- Enhanced `TEAM_RESULT_CONTRACT` to emphasize teammate expertise and thorough guidance
- Added explicit encouragement for early and frequent delegation

**Impact:**
- Agent more likely to use team collaboration proactively
- Better recognition of when to delegate vs. handle directly
- Improved multi-agent workflows and specialization

### Previous Updates (v0.5.16)

**UTF-8 Streaming Fix — No More Garbled Chinese Characters:**
- ✅ **Proper UTF-8 Boundary Handling** — HTTP chunk boundaries no longer split multi-byte characters
- ✅ **Fixed Garbled Output** — Chinese text like "我来帏查看服务器的息" now displays correctly as "我来查看服务器的信息"
- 🔧 **Smart Buffer Management** — Incomplete UTF-8 sequences preserved in carry buffer until next chunk arrives
- 🎯 **Zero Data Loss** — Complete characters always decoded correctly, no replacement characters (�)

**Technical Details:**
- Implemented `find_last_complete_utf8_boundary()` to detect incomplete multi-byte sequences
- Handles all UTF-8 character types: 1-byte (ASCII), 2-byte, 3-byte (CJK), 4-byte (emoji)
- Scans backward from buffer end to find last complete character
- Carries forward incomplete sequences to next chunk for proper decoding

**Before v0.5.16:**
- Chinese characters appeared garbled during streaming: "配置信息" → "配帏信息"
- Replacement characters (�) appeared randomly in CJK text
- Emoji and special Unicode characters broke into fragments

**After v0.5.16:**
- All text streams correctly regardless of chunk boundaries
- Perfect display of Chinese, Japanese, Korean, emoji, and all Unicode
- Seamless streaming experience with zero character corruption

### Previous Updates (v0.5.15)

**WebUI Config Persistence — Settings Now Survive Restart:**
- ✅ **Config Auto-Save** — WebUI settings changes automatically persist to `~/.hakimi/config.yaml`
- ✅ **No More Lost Settings** — Model configs, API keys, and all settings survive restart
- 🔧 **Smart Persistence** — Only saves in unified mode (default), WebUI-only mode stays memory-only
- 📝 **Logging** — Success/failure logged for debugging config save operations

**Before v0.5.15:**
- WebUI settings only stored in memory
- Restarting hakimi lost all configuration changes
- Had to manually edit config.yaml

**After v0.5.15:**
- Change settings in WebUI → Automatically saved to config.yaml
- Restart preserves all your configuration
- WebUI becomes the primary config interface

### Previous Updates (v0.5.14)

**Critical WebUI UX Fixes — Three Key Issues Resolved:**
- ✅ **Copy Message Feedback** — Copy button now shows visual feedback (opacity change) so users know it worked
- ✅ **Tool Call Panel Position** — Fixed rendering order: tool calls now appear after message content, not stacked at the top
- ✅ **Tool Calls Persistence** — Full backend integration: tool calls persist to database and survive page refresh
- 🎯 **Backend API Enhanced** — Added ToolCallInfo struct and tool_calls field to SessionMessageInfo
- 🔄 **Frontend Mapping** — Automatic conversion from backend tool_calls to frontend toolCalls format

**Before v0.5.14:**
- Copy button seemed broken (no feedback)
- Tool panels stacked messily at message top: `⚙️ read_file ⚙️ terminal [content below]`
- Refreshing the page lost all tool call history

**After v0.5.14:**
- Copy gives instant feedback
- Clean flow: `[content] → [delegate progress] → [tool calls]`
- Tool calls persist forever, visible in historical sessions

**v0.5.13 — WebUI Tool Call Display Fix:**
- ✅ **Fixed Protocol Mismatch** — Frontend now correctly detects backend control messages (changed \x01 → \x1e)
- ✅ **Clean Message Display** — Tool markers no longer leak into assistant responses
- ✅ **Structured Tool Panels** — Tool calls appear in collapsible cards with clear visual separation
- ✅ **Better Readability** — Long tool results are folded by default, click to expand when needed
- 🎯 **Double Filter** — stripToolMarkers() provides fallback cleanup for any protocol edge cases

**Example:** Before this fix, you'd see messy raw markers like `hakimi_tool:⚙️ read_file` mixed into the response text. Now tool calls appear as clean, expandable panels while the assistant's prose stays pristine.

**v0.5.12 — Model Tiers & Auto-Dispatch:**
- ✅ **Three-Tier Model System** — Configure Light/Primary/Reasoning models for different task complexities
- ✅ **Automatic Task Routing** — Smart dispatcher analyzes task complexity and routes to appropriate model tier
- ✅ **WebUI Configuration** — Full control panel in Settings for model tiers and auto-dispatch options
- ✅ **Cost & Performance Optimization** — Use lighter models for simple tasks, save powerful models for complex work
- 🎯 **Two-Stage Execution** — Optional mode: plan with reasoning model, execute with primary model
- 📊 **Dispatch Decision Visibility** — See which tier handles each request and why

**Example:** Simple file reads go to your fast 7B model, standard coding to your 32B model, and complex architecture planning to your reasoning model — all automatically!

**v0.5.11 — WebUI Chat Experience Enhanced:**
- ✅ **Tool Call Visualization** — Every tool execution now displays prominently in chat history with collapsible results
- ✅ **Fixed Content Overwrite** — Streaming responses no longer get replaced by final message, preserving complete conversation flow
- ✅ **Interactive Tool Results** — Click to expand/collapse tool outputs (file reads, searches, API calls) with syntax highlighting
- ✅ **Real-time Progress** — Live updates as tools execute, with clear visual separation from assistant responses
- 🎨 **Refined UI** — Smooth animations, better spacing, and improved readability for long conversations

**Example:** When you ask "analyze this codebase", you'll now see each file search, code analysis tool, and their outputs as separate expandable cards — no more mystery about what the agent is doing!

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

**Build from source (any platform with Rust):**
```bash
cargo install hakimi-agent
```

**Quick setup:**
```bash
hakimi setup      # guided configuration wizard
hakimi doctor     # diagnose setup and connectivity
hakimi --serve    # start the embedded WebUI/API on 127.0.0.1:3005
```

**v0.4.7 — 上下文管理优化 (Context Management Enhancement):**
- 🔄 **队列消息注入修复**：修复运行中上下文的排队消息注入逻辑，确保多消息场景下的正确处理
- 🗜️ **压缩标志重置**：上下文压缩后正确重置 `compressed_this_turn` 标志，避免重复压缩
- 🧹 **代码质量提升**：消除 entry.rs 中未使用变量和死代码警告，应用 rustfmt 格式化
- 🎯 **Agent 循环增强**：优化 loop_impl.rs 中的消息处理流程，提升稳定性

**v0.4.6 — 人格办公室仪表板 (Persona Office Dashboard):**
- 🏢 **办公室可视化**：把每个人格当作"员工"，实时展示所有人格的工作状态
- 🖥️ **个性化工位**：每个人格独立工位，执行任务时电脑屏幕亮起 + 键盘动作，空闲时看电视/打游戏
- 🤝 **协作动画**：A 找 B 干活时显示跑到 B 处交付需求的动画，多人组队时聚坐协作
- 📡 **实时事件流**：后端 ActivityHub + SSE 全栈实时推送（PersonaCreated/TurnStarted/TeamConsult/Idle 等）
- 🎨 **扁平矢量风格**：SVG + CSS 动画，微俯视角，可随主题换色，与现有 UI 风格统一
- 🔄 **自动布局**：按行自动排列工位，支持几个到 ~20 个人格，超出自动滚动
- 🖱️ **可交互导航**：点击工位进入该人格对话/配置，悬停显示状态详情卡
- 👔 **入职动画**：新人格创建时显示"新员工入职，安排新座位"动画

**v0.4.5 — Persona Team 协作系统 (Persona Team Collaboration):**
- 🤝 **具名人格协作**：主导人格可通过 `team` 工具将子任务委派给其他具名队友人格
- 🎯 **专业化分工**：每个队友使用自己的模型、技能、记忆和系统提示词独立作答
- 📋 **队友名册管理**：`team(action="list")` 枚举所有可寻址队友及其能力描述
- 🔒 **安全护栏**：内置深度上限、回环检测、并发信号量、超时预算机制
- ⚙️ **可配置开关**：`PersonaConfig.addressable` 控制人格是否可被当作队友（默认开启）
- 🔄 **同步无状态**：队友按子任务起干净回合，只读长期记忆，不写回自身会话/记忆
- 📊 **进度可视化**：复用现有 `hakimi_delegate:` 气泡机制，实时展示协作进度
- ✅ **WebUI 集成**：人格配置表单中的 `addressable` 开关已完整实现

The WebUI Control Center can create, pause, resume, run-now, and delete persisted cron jobs via `/api/cron/jobs`, the `/clear` slash command now persists by deleting the current session transcript via `/api/sessions/{id}/messages`, and the mobile layout lets the conversation title toggle the session list so phones keep the chat area usable.

`hakimi --serve` ships the WebUI assets inside the release binary, so `/`, `/static/style.css`, `/static/hakimi.js`, `/static/composer.js`, `/static/workspace.js`, and `/static/favicon.svg` work from any current directory without copying a separate `static/` folder. The WebUI workspace browser treats `/` as the active working-directory root (not the OS filesystem root), while still rejecting `..` path escapes. Control-center modals honor native `hidden` state and can be dismissed via close button, overlay click, or Escape. When `HAKIMI_WEBUI_PASSWORD` is set, the WebUI prompts for the password on the first authenticated API call, stores it locally as a Bearer token, retries automatically, and renders send/auth errors inline instead of silently dropping messages. The embedded server persists WebUI sessions in `~/.hakimi/sessions.db` and initializes the schema on startup, so creating a chat session works immediately after launch. Streaming WebUI chat requests now carry the active `session_id`, restore that transcript before each turn, and persist both the user prompt and assistant reply back into the same session; the frontend also commits finalized streamed replies into its in-memory message list so a second send or session switch does not erase the previous response. The WebUI also exposes persisted cron jobs through `/api/cron/jobs`, supports session deletion from the sidebar, de-duplicates client-provided session titles during create/fork so repeated "New Chat" actions do not hit the SQLite title uniqueness constraint, and ships a polished skin system with Linear Dark, Obsidian, Midnight, Light, and System appearance choices persisted in localStorage. Theme switching now writes the resolved skin CSS variables directly at runtime, so color changes are immediate and resilient to cached/static stylesheet ordering. The refreshed UI uses glassy panels, richer message cards, focused composer chrome, theme swatches in Settings → Appearance, keeps theme switching local and instant, adds a mobile drawer sidebar with a compact top-bar menu, hides the workspace panel on narrow screens, and adapts messages/composer controls to safe-area mobile viewports. It also shows directory-style `SKILL.md` skills using their parent directory names instead of generic `SKILL` labels, and releases the composer immediately after completed streamed replies so follow-up messages remain responsive.

---

## Why Hakimi?

Python agent frameworks are slow, memory-hungry, and crash at runtime. Hakimi is built different — from the ground up in Rust with production reliability baked in.

| Metric | Python Agent | Hakimi (Rust) |
|--------|-------------|---------------|
| Startup | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Async model | asyncio + GIL | tokio native async |
| Tool safety | Runtime crashes | Compile-time guarantees |
| Tests | ~500 | 1767 |

**Not a wrapper. Not a demo. A real production system:**
- 20+ error types auto-classified with recovery strategies
- Hermes-style turn retry state for one-shot recovery guards
- Multi-key credential pool with circuit breakers
- 3-tier context compression (no manual window management)
- Contextual first-touch onboarding hints tracked in `onboarding.seen`
- Decision tree conversation history with backtracking
- Intent reasoning engine — predicts what tools you need
- Role adaptation — automatically switches between Coder, Researcher, Writer modes

---

## Capabilities

### 🌟 Core Features

**Smart Context Management**
- Three-tier compression: drop stale tool results → LLM summarization → sliding window
- No manual context window management — Hakimi handles it automatically
- Model-aware context windows: `model.context_length` overrides static metadata before compression and tool disclosure thresholds
- Intent classification into 10 categories with next-tool prediction

**Built-in Tools (63+)**
- **Files**: read, write, search, patch with safe-root sandbox
- **Shell**: terminal, background processes
- **Web**: search, extract, browser automation (Chromium with screenshot vision capture, Playwright cache/headless-shell discovery, raw CDP dispatch, CDP frame-tree inspection, cloud-provider readiness status, and provider CDP endpoint routing)
- **Desktop**: Hermes-style `computer_use` readiness surface with safe wait, macOS screenshot/list-app discovery, and guarded action schema
- **Code**: Python/JS/Bash execution with sandbox
- **Media**: vision analysis, video analysis, TTS, transcription with silence-hallucination filtering and oversized WAV chunking
- **Memory**: persistent memory + FTS5 full-text search + `hakimi knowledge` / TUI `/knowledge` / gateway `/knowledge` graph operations
- **Productivity**: todo, Kanban boards with profile routing, worker logs, event trails, diagnostics, notification subscriptions, swarm graph creation, dashboard read/write management, cron scheduler with interval/five-field cron expressions and home-channel fan-out delivery
- **Meta**: sub-agent delegation, Mixture-of-Agents reasoning, skills system, MCP plugins
- **Evaluation**: Hermes-compatible ShareGPT JSONL trajectory saving for completed and failed turns

**Multi-Platform Gateway**
- Telegram · Discord · Slack · Mattermost · Webhook · Microsoft Graph webhook · Signal · SMS/Twilio · Email/SMTP · WhatsApp Business Cloud · Home Assistant · Matrix · DingTalk · WeCom · Feishu/Lark · BlueBubbles/iMessage · QQBot outbound · WeChat (via iLink/ClawBot) · Weixin/iLink alias
- Config-driven multi-adapter fan-in: run chat and webhook gateways simultaneously
- Real-time streaming with progressive edits, native Telegram draft previews, flood-control backoff, per-platform preview policy, and UTF-8-safe overflow chunking for long replies
- Persistent lifecycle diagnostics record adapter, connect, route, filter, and edit events to `~/.hakimi/logs/gateway-events.log`; `/logs`, `/logs events`, and `/logs gateway` read recent logs without shelling out to `tail`
- Gateway `/undo [N]` rewinds recent in-memory chat turns and echoes the target prompt for editing before resend
- Gateway `/stop` immediately cancels the running task and clears any queued messages, supporting both `interrupt` and `queue` modes configured via `gateways.busy_input_mode`
- Gateway `/usage` shows last-turn token/cost/rate-limit data, best-effort OpenRouter-compatible `/v1/models` live pricing with a profile-scoped freshness cache and request fees, OpenRouter `/credits` plus `/key` quota/usage, Anthropic OAuth account windows, Codex usage windows, and a shared Nous rate-limit guard without exposing credentials
- Cron jobs scheduled from chat with `/cron add`, including `30m` / `2h` intervals, five-field cron syntax such as `*/15 * * * *` or `0 9 * * MON-FRI`, and delivery targets like `local`, `origin`, `all`, `platform`, `platform:home`, or `platform:#channel`
- Gateway `/voice on|off|tts|status|doctor` toggles spoken-response guidance and reports voice I/O readiness without polluting prompt cache or chat history
- Gateway `/update` sends the in-chat restart notice, then the restarted gateway proactively reports update success, current version, and release-note feature bullets after adapters connect
- TUI `/config [field]` shows sanitized runtime configuration, `/gateway [cmd]` inspects configured adapters, cached channel targets, and lifecycle events, `/sessions [cmd]` browses saved SQLite sessions, `/skills [cmd]` browses/searches local Skills Hub metadata, `/cron [cmd]` manages the persistent cron DB locally, `/undo [N]` prefills recent prompts for editing, `/checkpoints [cmd]` inspects the shared shadow-git checkpoint store without entering the model loop, and `/voice status` plus configurable Ctrl+B/Ctrl+letter push-to-talk share the same `voice.*` config, TTS/transcription tools, audio environment checks, PCM16 WAV recording artifact validation, oversized WAV chunked STT dispatch, local TTS playback launch, recorder-backed `voice_capture`, automatic transcript submission, continuous restart mode, second-press capture cancellation, three-no-speech auto-exit, and Hermes-style start/stop audio cues

**Extensibility**
- MCP (Model Context Protocol) client — stdio / HTTP / SSE transports, CLI/gateway catalog search and config snippets, and stdio server-initiated sampling with tool schema forwarding plus `tool_use` handoff
- HTTP plugin system with YAML templates
- HTTP API discovery — OpenAI-compatible `/v1/models`, `/v1/capabilities`, `/v1/skills`, `/v1/toolsets`, text `/v1/chat/completions` with completed SSE snapshots for `stream=true`, `/v1/responses` with SQLite-backed `previous_response_id` chaining plus completed SSE snapshots, pollable and cancellable `/v1/runs` with live lifecycle SSE events, and session lifecycle/messages/search discovery for external UI feature detection
- Dashboard admin API — `/api/status`, `/api/sessions` create/update/delete/fork plus message/search inspection, `/api/mcp/servers`, `/api/credentials/pool`, `/api/webhooks`, and Kanban `/api/kanban` board/task read-write management expose redacted operational state plus runtime-scoped admin writes for WebUI/admin panels
- Hakimi WebUI — Hermes-inspired React/Vite operator console with left-side session browsing, central `/api/chat` live turns, right-side runtime/tool/skill/control panels, Bearer token support, and runtime config editing through the existing HTTP API
- Skills Hub — install community skills with `/skills install`
- Static i18n foundation — `display.language`, `HAKIMI_LANGUAGE` / `HERMES_LANGUAGE`, Hermes-compatible language aliases, YAML catalog directory loading, English fallback, and named placeholders for static user-facing messages
- CLI Skin Engine — `hakimi skin list|inspect|set|path` plus gateway `/skin` discover built-in and `~/.hakimi/skins/*.yaml` themes, inherit missing values from `default`, persist `display.skin`, apply selected branding/colors/logo/hero to the CLI startup banner, and drive TUI thinking spinner faces/verbs/wings plus status, session, selection, completion, help, input, response, tool-prefix, tool emoji labels, running-tool progress, and tool-panel colors
- Isolated profiles — manage named workspaces, clone/export profile archives, install/update shareable `distribution.yaml` profile distributions, create `~/.hakimi/bin/<profile>` wrapper aliases, use gateway `/profile`, and bind `--profile` / sticky `active_profile` runs to profile-scoped config, memory, sessions, skills, cron, trajectories, gateway logs, and TUI defaults
- 10 curated MCP catalog entries: GitHub, filesystem, Brave Search, PostgreSQL, Puppeteer, memory, fetch, SQLite, sequential-thinking, and the Hermes-reviewed n8n bridge

### 🛡️ Production Safety

- **Secret redaction** — API keys, JWTs, tokens masked before output
- **Prompt injection detection** — scans skills, cron prompts, context files
- **SSRF protection** — blocks private/metadata URL fetches
- **Command safety guard** — blocks malicious shell patterns
- **Tool loop guardrails** — warns on repeated no-progress read-only calls and blocks runaway exact-call loops
- **One-time onboarding hints** — first-touch CLI/gateway tips persist under `onboarding.seen`
- **Write safe-root sandbox** — config-protected directories
- **Read credential guard** — protects config files
- **Shared shadow-git checkpoints** — `checkpoint` and gateway `/checkpoints` snapshots live under `~/.hakimi/checkpoints/store`, not the project `.git`
- **Tool output limits** — configurable `tools.output.max_bytes` boundary before tool results enter context

---

## Architecture

**20 crates, each with a single responsibility:**

```
hakimi-agent/
├── hakimi-core/          # Agent loop, error classifier, credential pool
├── hakimi-transports/    # OpenAI, Anthropic, Gemini, Bedrock transports + prompt caching/rate guards
├── hakimi-tools/         # 63+ built-in tools + plugin registry
├── hakimi-session/       # SQLite WAL + FTS5, decision tree history
├── hakimi-context/       # Context engine, compression, intent reasoning, roles
├── hakimi-knowledge/    # Knowledge graph (petgraph)
├── hakimi-skills/        # Skill system + meta-skill extraction
├── hakimi-cron/          # Persistent cron scheduler
├── hakimi-gateway/       # 19 runtime-exposed platform adapters
├── hakimi-mcp/           # MCP client (stdio/HTTP/SSE)
├── hakimi-cli/           # REPL CLI + setup wizard + doctor
└── hakimi-tui/           # ratatui terminal UI
```

### How It Works

```
User Message
    │
    ▼
┌─────────────────────────────────────┐
│  Intent Classification               │
│  → Which tools does this need?      │
├─────────────────────────────────────┤
│  Role Adaptation                     │
│  → Coder / Researcher / Writer...   │
├─────────────────────────────────────┤
│  Build Context                       │
│  → System prompt + knowledge graph  │
│  → Apply 3-tier compression         │
├─────────────────────────────────────┤
│  Credential Pool                     │
│  → Acquire API key (rotation-ready) │
│  → LLM call via SSE streaming       │
├─────────────────────────────────────┤
│  Tool Dispatch                       │
│  → Execute + guardrails check       │
│  → Error classification + recovery │
├─────────────────────────────────────┤
│  Decision Tree                       │
│  → Record response + backtrack Capable│
└─────────────────────────────────────┘
    │
    ▼
Response + Memory + Stats
```

---

## Compare

| Feature | Hermes (Python) | Hakimi (Rust) |
|---------|-----------------|---------------|
| Language | Python 3.11+ | Rust 2024 |
| Startup time | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Async model | asyncio + GIL | tokio native |
| Tool registration | Runtime AST | Compile-time trait |
| Error recovery | Basic retry | 20+ classifiers |
| Knowledge model | Flat file | Graph DB (petgraph) |
| Intent detection | None | 10-category classifier |
| Role adaptation | None | 8 roles auto-detected |
| Conversation model | Flat list | Decision tree |
| Tests | ~500 | 1767 |

---

## Development

```bash
# Build everything
cargo build --workspace

# Run tests
cargo test --workspace

# Lint
cargo clippy --workspace

# Debug logging
RUST_LOG=debug cargo run -p hakimi-cli
```

---

## Roadmap

- [x] Core agent loop + tool dispatch
- [x] OpenAI / Anthropic / Gemini transports + SSE streaming, plus non-streaming AWS Bedrock Converse
- [x] 63+ built-in tools
- [x] 19 runtime-exposed platform adapters
- [x] Gateway target directory + send_message channel resolution
- [x] MCP client + CLI/gateway server catalog
- [x] HTTP API model/capability discovery + text Chat Completions/Responses SSE snapshots + cancellable Runs with live lifecycle events
- [x] Dashboard admin API summaries + runtime writes + Kanban read/write management
- [x] Gateway `/usage` rate-limit, account-limit, live pricing with request fees, Nous shared rate guard, and offline OpenAI/Anthropic/Gemini/DeepSeek/MiniMax/Bedrock cost estimates
- [x] Plugin system + HTTP templates
- [x] Profile distributions with install/update/info and protected user data
- [x] CLI skin engine with built-in/user YAML themes, `display.skin` persistence, startup banner theming, and TUI spinner, status, completion, help, tool emoji/progress, and surface theming
- [x] ratatui TUI with local slash commands, sanitized config browser, and gateway status panel
- [x] Smart context compression (3-tier)
- [x] Error classifier + credential pool
- [x] Prompt caching (Anthropic)
- [x] Vision + video analysis
- [x] Knowledge graph memory with CLI/TUI/gateway operator commands
- [x] Intent reasoning engine
- [x] Decision tree backtracking
- [x] Role adaptation
- [x] Meta-skill auto-extraction
- [x] Browser automation (Chromium + Playwright cache discovery + CDP readiness probe + frame-tree inspection + cloud-provider readiness status + provider CDP endpoint routing)
- [x] Computer Use readiness surface
- [x] Kanban task boards + notification cursors + swarm graphs + dashboard read/write management
- [x] Gateway voice-response mode
- [x] TUI voice readiness and media-tool config parity
- [x] Voice environment diagnostics and STT silence-hallucination filtering
- [x] PCM16 WAV recording artifact validation for voice capture
- [x] Voice TTS playback text cleanup, MP3 cache planning, and local player launch
- [x] Voice capture tool with system recorder backends and STT dispatch
- [x] Oversized WAV chunking for captured-recording STT dispatch
- [x] TUI Ctrl+B continuous push-to-talk capture loop
- [x] TUI checkpoint viewer and manager slash command
- [x] TUI saved session browser slash command
- [x] TUI skill browser slash command
- [x] TUI cron job manager slash command
- [x] Voice capture second-press interrupt key
- [x] Voice capture start/stop audio cues
- [x] Voice capture continuous restart mode
- [x] Mixture-of-Agents reasoning via OpenRouter
- [x] OpenRouter, Anthropic, and Codex account usage display in gateway `/usage`
- [x] Basic Hakimi WebUI operator console
- [ ] WASM plugin runtime
- [ ] Web dashboard PTY terminal, session-scoped streaming, and full Kanban UI

---

## License

MIT License
