<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.120-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1246-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/lines-44K+-orange?style=for-the-badge" alt="Lines">
</p>

<h1 align="center">🐙 Hakimi Agent</h1>

<p align="center">
  <b>用 Rust 重写的 AI Agent 框架 — 启动快 40 倍，内存省 90%</b><br>
  <sub>源自 <a href="https://github.com/NousResearch/hermes-agent">Nous Research Hermes Agent</a> 生产级架构，从零用 Rust 重写</sub>
</p>

<p align="center">
  <a href="#安装">安装</a> •
  <a href="#简介">简介</a> •
  <a href="#核心能力">核心能力</a> •
  <a href="#架构设计">架构</a> •
  <a href="#性能对比">性能对比</a> •
  <a href="#路线图">路线图</a> •
  <a href="README.md">English</a>
</p>

---

## 安装

**Linux / macOS：**
```bash
curl -sSL https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.sh | bash
```

**Windows (PowerShell)：**
```powershell
irm https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.ps1 | iex
```

**任意平台 (已安装 Rust)：**
```bash
cargo install hakimi-agent
```

安装后运行交互式配置向导：

```bash
hakimi setup
hakimi doctor
```

Linux 安装会把真实二进制放在 `~/.hakimi/bin/hakimi`，并把 `/usr/local/bin/hakimi` 作为 symlink/launcher 维护。托管 gateway 的 systemd unit 会优先使用 canonical 路径 `~/.hakimi/bin/hakimi --gateway start`，并设置稳定服务 PATH。Terminal/process 工具也会把 Hakimi 托管目录前置到当前 PATH：

```bash
~/.hakimi/bin:~/.cargo/bin:/usr/local/bin:/usr/bin:/bin
```

向导引导你完成 LLM 提供商、API Key、模型、平台适配器、MCP 服务器的配置，全部保存到 `~/.hakimi/config.yaml`。

---

## 简介

Hakimi 是 [Hermes Agent](https://github.com/NousResearch/hermes-agent) 的 Rust 重写——Nous Research 生产环境使用的 AI Agent 框架，服务数千用户。不是 demo，不是 wrapper，是从零用 Rust 重写的完整实现。

**与 Python Agent 框架的性能差异：**

| 指标 | Python Agent | Hakimi (Rust) |
|------|-------------|---------------|
| 启动时间 | ~2s | ~50ms |
| 空闲内存 | ~150MB | ~15MB |
| 并发模型 | asyncio + 线程桥接 | tokio 原生 async (无 GIL) |
| 工具注册 | 运行时 AST 扫描 | 编译期 trait (零开销) |
| 类型安全 | 运行时崩溃 | 编译期捕获 |

**生产级特性：** 1246 个测试 · 20+ API 错误类型自动分类与恢复 · 多密钥凭证池、熔断与终态认证隔离 · 三层上下文压缩 · Anthropic Prompt 缓存 · MCP/插件工具渐进披露 · Gateway 入站访问策略 · MCP sampling/createMessage · Gateway stream pacing

---

## 核心能力

### 🌟 最新发布

- **v0.3.120 Credential pool terminal auth quarantine**
  - 对齐 Hermes credential pool：provider 返回 `token_revoked`、`token_invalidated`、`invalid_grant`、`refresh_token_reused` 等 401 OAuth 终态时，会将该凭证标记为 `dead`。
  - `dead` 凭证不会在 cooldown 结束后重新进入 round-robin、fill-first、random 或 least-used 轮换；只有显式重新认证或替换 token 才会恢复。
  - 凭证池统计现在区分临时 exhausted 与永久 dead，并保留最后一次 provider 状态码和 reason，便于诊断。

- **v0.3.119 Gateway stream pacing**
  - 对齐 Hermes stream consumer：gateway streaming 现在使用可配置编辑节奏和缓冲字符阈值，不再固定 450 ms 更新一次。
  - `gateways.streaming.edit_interval_ms` 默认 `800`，`buffer_threshold_chars` 默认 `24`；阈值设为 `0` 时只按时间间隔编辑。
  - 工具、媒体、委派进度和 updater 结束边界会先 flush 待显示 assistant 文本，再打开下一条消息气泡。

- **v0.3.118 Gateway fresh-final streaming**
  - 对齐 Hermes stream consumer：gateway streaming 会记录首个预览气泡的可见时长，长时间流式回答完成时可发送新的最终消息。
  - `gateways.streaming.fresh_final_after_seconds` 默认 `60` 秒；设为 `0` 可完全保留旧的原地编辑完成语义。
  - Telegram adapter 实现 `deleteMessage`，fresh final 发送成功后会尽力清理旧预览气泡。

- **v0.3.117 MCP sampling/createMessage**
  - 对齐 Hermes MCP sampling：stdio MCP 客户端可声明 sampling 支持，并响应服务器发起的 `sampling/createMessage` 请求。
  - sampling 请求复用 Hakimi 当前配置的 LLM transport 和 active model，不引入独立 provider 路径。
  - 不支持的客户端侧 server request 会返回 JSON-RPC 错误，不再被误当成日志或无关响应吞掉。

- **v0.3.116 Gateway MCP 服务器列表**
  - gateway `/mcp` 和 `/mcp list` 现在会展示真实配置的 MCP servers，不再返回固定占位文本。
  - `/mcp add|remove` 明确提示服务器增删由 `mcp_servers` 配置文件管理，并需要重启 gateway 生效。
  - 列表只展示服务器名称、启动命令、参数数量和环境变量数量，不打印环境变量值。

- **v0.3.115 Gateway 入站访问策略 · Gateway MCP 服务器列表**
  - 对齐 Hermes gateway 安全语义：入站 gateway 消息在任何 slash 命令或 agent turn 执行前先经过配置驱动 allowlist。
  - `gateways.allowed_users`、`gateways.telegram.allowed_users`、role Telegram allowlist 和 `gateways.clawbot.allowed_users` 会合并成一个统一 ingress policy。
  - 空 allowlist 保持既有 allow-all 行为；需要显式开放时可用 `gateways.allow_all` 覆盖。
- **v0.3.114 Terminal Shell Hooks**
  - 对齐 Hermes shell hook 的一个可验证子集：`terminal` 支持通过 `HAKIMI_PRE_TOOL_HOOK` 和 `HAKIMI_POST_TOOL_HOOK` 配置本机命令执行前后钩子。
  - hook 会从 stdin 接收 JSON payload，包含 `hook_event_name`、`tool_name`、`tool_input`、`session_id`、`cwd` 和 `extra.task_id`；post hook 还会收到渲染后的工具输出。
  - pre hook 可返回 `{"action":"block","message":"..."}` 或 `{"decision":"block","reason":"..."}`，在真正执行 terminal 命令前阻断高风险操作。
- **v0.3.113 工具渐进披露**
  - 对齐 Hermes tool_search 语义：大量 MCP/插件工具可折叠到 `tool_search`、`tool_describe`、`tool_call` 三个桥接工具之后，不再每轮都发送全部延迟工具 schema。
  - 核心 Hakimi 工具保持直接可见：terminal、file、memory、cron、browser、media、knowledge 等内置工具不会被误延迟。
  - `tools.tool_search.enabled`、`threshold_pct`、`search_default_limit`、`max_search_limit` 支持 Hermes 风格 `auto`/`on`/`off` 配置，并接入 CLI 与 server agent。
- **v0.3.112 插件列表可用性**
  - 对齐 Hermes 最新插件 CLI：`hakimi plugins list` 现在支持 `--plain` 和 `--json`，便于终端脚本与机器读取。
  - HTTP 插件配置可声明顶层 `version` 和 `description`，列表输出会展示真实插件元数据，不再固定为通用标签。
  - `--tools` 与 `--no-tools` 可在简洁清单和工具级检查之间切换。
- **v0.3.111 read_file 凭据读取防护**
  - 对齐 Hermes file_safety 语义：`read_file` 读取前会拒绝已知 Hakimi 凭据存储，包括 `config.yaml`、OAuth 缓存、MCP token 文件、项目 `.env*` 文件和 `cache/bws_cache.json`。
  - 对已存在路径先 canonicalize 再匹配，降低简单 symlink 绕过风险。
  - Windows 绝对路径检测改为 `Path::is_absolute()`，避免把 `C:\...` 误拼到当前工作目录下。
- **v0.3.110 子代理敏感工具屏蔽**
  - 对齐 Hermes 子代理安全语义：委派出的 child agent 不再获得 `delegate_task`、`clarify`、`memory`、`send_message` 或 `code_exec`。
  - 显式指定 child `toolsets` 仍然有效，但 denylist 会在筛选后继续生效，避免敏感工具被误重新启用。
  - 新增默认 delegation 和显式 toolset 两条离线 registry-filter 回归覆盖。
- **v0.3.109 上下文文件 Prompt Injection 防护**
  - 对齐 Hermes prompt-builder 安全语义：`AGENTS.md`、`CLAUDE.md`、`.cursorrules`、`SOUL.md` 和 `.cursor/rules/*.mdc` 在进入 system prompt 前会先扫描。
  - 可疑上下文文件会被替换为简短的 blocked placeholder，只报告稳定 finding id，不泄露原始内容。
  - prompt-builder 复用 Hakimi 现有 Rust 原生 prompt-injection detector，让上下文加载、文件安全和 cron 安全保持一致。
- **v0.3.108 可配置 LLM 上下文压缩**
  - `compression.engine: llm` 现在会选择 Hakimi 的 LLM-backed compressor，不再静默落回 smart 本地压缩引擎。
  - `compression.model` 可指定更便宜或更快的摘要模型；留空时使用当前对话模型。
  - CLI 与 server 现在通过同一个 context-engine factory 构建 `smart`、`simple` 和 `llm` 引擎。
- **v0.3.107 MCP 错误脱敏**
  - 对齐 Hermes MCP 安全语义：MCP 传输与 adapter 错误在暴露给 agent 前会移除类似凭据的文本。
  - StreamableHTTP 与 SSE 的错误响应体、JSON 解析片段会先经过共享脱敏器处理。
  - MCP `isError` 工具结果和 adapter 调用失败也会统一遮蔽 token、key、password 与 Bearer 值。
- **v0.3.106 浏览器 Dialog 处理**
  - 对齐 Hermes 浏览器 dialog 工具：可选 Chromium 自动化现在包含 `browser_dialog`，用于处理 alert、confirm、prompt 和 beforeunload。
  - 当原生 JavaScript dialog 阻塞页面时，`browser_snapshot` 会返回 `pending_dialogs` 供 agent 选择 accept 或 dismiss。
  - CLI、TUI 与 server 的 browser feature 构建会注册同一组 dialog responder。
- **v0.3.105 MCP Node 命令解析**
  - 对齐 Hermes MCP stdio 修复：以裸命令启动的 `node`、`npm`、`npx` 服务器，即使 server `PATH` 被用户收窄，也能从常见 Node 安装目录恢复。
  - 解析顺序先尊重继承 PATH，再检查 Hakimi 托管的 `~/.hakimi/node/bin`、`~/.local/bin`，以及 Unix 上的 `/usr/local/bin`。
  - fallback 启动会把命令所在目录前置到子进程 PATH，避免 `npx` shebang 二次执行时找不到 `node`。
- **v0.3.104 浏览器 Console + Eval**
  - 对齐 Hermes 浏览器调试面：可选 Chromium 自动化现在包含 `browser_console`。
  - 工具可读取捕获的 `console.log`/`warn`/`error`、未捕获 JS 错误，并可在当前页面上下文执行 JavaScript 表达式。
  - CLI 与 TUI 的 browser feature 构建会注册同一组共享浏览器 console/eval 工具。
- **v0.3.103 浏览器图片列表**
  - 对齐 Hermes 浏览器图片提取：可选 Chromium 自动化现在包含 `browser_get_images`。
  - 工具会返回非 data URI 图片 URL、alt 文本和原始尺寸，便于后续交给 vision 工具分析。
  - CLI 与 TUI 的 browser feature 构建会注册同一组共享浏览器图片列表工具。
- **v0.3.102 浏览器导航控制**
  - 对齐 Hermes 浏览器基础交互：可选 Chromium 自动化现在包含 `browser_scroll`、`browser_back` 和 `browser_press`，并保留 navigate/snapshot/click/type/screenshot。
  - CLI 与 TUI 的 browser feature 构建会注册同一组共享浏览器会话控制工具。
  - 新增离线 schema 与元数据回归覆盖，不在本地启动 Chromium。
- **v0.3.101 输出 token 预算恢复**
  - 对齐 Hermes 最新上下文恢复语义：当 provider 错误明确给出 `available_tokens` 时，只临时降低重试的 `max_tokens`，不把 prompt 误判为过长。
  - 长 Anthropic 对话可保留当前上下文，用安全输出上限重试，避免无谓压缩。
  - 新增 Anthropic 标准输出上限错误、自然语言变体、prompt overflow 非匹配的解析与重试参数回归覆盖。
- **v0.3.100 Web URL 脱敏对标**
  - 普通 `http`、`https`、`ws`、`wss` 和 `ftp` URL 现在会保持原样，包括 OAuth callback、magic link、预签名 URL，以及 agent 需要继续访问的 request-target query。
  - Provider key、JWT、数据库连接串密码、私钥、Bearer token 和纯 form-urlencoded 密钥字段仍会在工具输出展示前被遮蔽。
  - 新增 URL 透传、URL 内 provider token、URL 内 JWT 和 form body 脱敏的确定性回归覆盖，不调用真实供应商 API。
- **v0.3.99 TUI OSC 52 剪贴板回退**
  - `/copy [N]` 现在会在原生剪贴板写入工具不可用时，回退到 Hermes 风格的 OSC 52 终端剪贴板输出。
  - SSH、tmux 和终端模拟器场景即使缺少 `pbcopy`、`wl-copy` 或 PowerShell，也能复制 assistant 回复。
  - 新增 OSC 52 payload 包装的确定性回归覆盖，不调用真实剪贴板。
- **v0.3.98 插件 CLI + Gateway 管理**
  - `hakimi plugins list|templates|init|path` 现在直接复用现有 HTTP 插件加载器。
  - 内置 HTTP 插件模板会嵌入二进制，安全生成到 `~/.hakimi/plugins`，不会覆盖已有配置。
  - Gateway `/plugins list|templates|path` 与 CLI 共用同一套插件管理输出。
- **v0.3.97 会话标题自动生成**
  - 持久化 session 在没有手动标题时，会根据首条用户消息生成简洁标题。
  - 自动标题会保留用户手动设置的标题；若标题已被其他 session 占用，会追加短 session 后缀避免唯一索引冲突。
  - 标题截断改为按字符处理，避免中文等多字节文本被截断到非法 UTF-8 边界。
- **v0.3.96 TUI `/history` 会话历史回看**
  - Hakimi TUI 现在支持 `/history [N]` 和 `/hist [N]`，可在本地回看最近的 user/assistant 对话，不会把命令发送给模型。
  - 可选数字参数会展示最近 N 条可见对话消息，并跳过 tool/system 噪音。
  - `/history` 已进入共享 slash-command parser；gateway 会话会说明完整历史回看属于本地 TUI 或聊天客户端使用场景。
- **v0.3.95 工具输出密钥脱敏**
  - 新增共享 Rust 原生脱敏器，在工具输出展示前屏蔽 API key、Bearer token、私钥、JWT、数据库 URL、高置信 URL 内 token 和 form-urlencoded 密钥字段。
  - terminal、process、code_exec 和命令插件输出现在会对 stdout/stderr、已存命令、诊断信息和插件错误做统一脱敏。
  - 新增离线回归覆盖脱敏模式和工具输出边界，不依赖真实供应商 API。
- **v0.3.94 TUI `/copy` 剪贴板对标**
  - Hakimi TUI 现在支持 `/copy [N]`，可复制最近一条或倒数第 N 条 assistant 回复到本机系统剪贴板。
  - 命令会按平台尝试 Windows、macOS、WSL、Wayland、X11 的原生剪贴板写入工具，不新增运行时依赖。
  - `/copy` 已进入共享 slash-command parser；gateway 会话会提示本地剪贴板复制属于 TUI 使用场景。
- **v0.3.93 Telegram 命令菜单**
  - Telegram `setMyCommands` 现在会暴露 gateway 的质量与运维命令，包括 `/usage`、`/doctor`、`/logs`、`/providers`、`/platforms`、`/mcp`、`/browser`、`/backup` 和 `/dump`。
  - gateway `/help` 改为面向操作者的分组式命令说明，不再是过短的旧列表。
  - 新增 Telegram 菜单回归覆盖，确保关键质量与运维命令进入菜单。
- **v0.3.92 Terminal Workdir 回退**
  - terminal tool 现在会把 `workdir: ""` 和仅包含空白字符的 workdir 视为未提供，回退到 tool context workdir，不再把空路径传给 `current_dir`。
  - 新增针对空 workdir 解析与执行路径的回归覆盖。
- **v0.3.91 Linux 运行时路径**
  - 托管 systemd 安装现在优先使用 `~/.hakimi/bin/hakimi --gateway start`，并保持 `/usr/local/bin/hakimi` 为 symlink/launcher。
  - terminal 和 process 工具会把 Hakimi/Cargo 托管目录前置到系统 PATH 前，降低 systemd 环境与交互 shell 环境不一致导致的失败。
  - terminal 失败诊断现在区分命令路径不存在、PATH 找不到、binary 存在但不可执行。
- **v0.3.89 Cron Repeat 语义**
  - cron 任务现在会持久化 `repeat` 上限和已完成次数，gateway `/cron add` 与独立 `hakimi cron add` 都支持 `--repeat N`。
  - scheduler 和独立 `hakimi cron tick` 会在每次实际执行后累加完成次数，达到上限时自动移除任务。
  - 新增离线回归覆盖 repeat 持久化、claim 过滤、scheduler 清理，以及 CLI/gateway repeat 解析。
- **v0.3.88 Cron Gateway 投递**
  - gateway `/cron add` 现在会把创建任务的平台与聊天 ID 持久化为显式 `deliver` 目标，定时任务输出会回到创建它的会话。
  - scheduler 投递现在把 `local` 视为本地不投递，只向显式 `platform:chat_id` 目标排队，并支持逗号分隔的多目标去重。
  - 新增离线回归覆盖 gateway 创建任务的投递元数据、目标解析与 local-only 抑制。
- **v0.3.87 Cron Tick 执行**
  - 顶层 `hakimi cron tick` 现在会从共享 SQLite cron store 中 claim 当前到期任务，并通过 gateway scheduler 相同的委派 cron 执行路径运行一次。
  - gateway 和独立 tick 共享持久化 tick lock，并在执行前先推进 `next_run`，对齐 Hermes 防重入的调度语义。
  - 新增离线回归覆盖 due job claim、tick lock 竞争、tick 命令解析和 tick 输出预览截断。
- **v0.3.86 Cron 状态视图**
  - `/cron status` 与顶层 `hakimi cron status` 现在会读取共享 SQLite cron store，不启动 agent 会话即可查看调度状态。
  - 状态输出展示总任务数、启用任务、暂停任务、当前到期任务，以及下一条将到期任务和 gateway scheduler 提示。
  - 新增离线回归覆盖 gateway 与独立 CLI cron status 的持久化状态格式化。
- **v0.3.85 Cron Skill 装载执行**
  - 定时任务现在会在自动执行时读取已持久化的 `skills` 元数据，把匹配到的 Hakimi skill 内容装配进委派任务。
  - skill 装载后的组合 prompt 使用 Hermes 风格的宽松 assembled-skill 扫描，允许安全 runbook 内容，同时继续阻断明确 prompt injection 指令。
  - cron 任务返回空响应或精确 `[SILENT]` 时会抑制 gateway 自动投递，避免没有新内容时打扰远程聊天。
- **v0.3.84 独立 Cron CLI**
  - 顶层 `hakimi cron` 现在能管理与 gateway `/cron` 相同的持久化任务库，覆盖 list/add/edit/pause/resume/run/remove，不需要启动 agent 会话。
  - CLI 新增与编辑复用既有 schedule parser、prompt 注入扫描、SQLite 持久化和 `next_run` 重算路径，避免重复实现 cron 逻辑。
  - 新增顶层命令解析与持久化 store 委托回归，覆盖 `hakimi cron add` 和 `hakimi cron edit`。
- **v0.3.83 Gateway Cron 新增与编辑**
  - gateway 会话现在可以用 `/cron add <schedule> <prompt>` 或 `/cron add <cron expr> | <prompt>` 创建任务，并用 `/cron edit <job-id> schedule|prompt|name <value>` 调整既有任务。
  - 内置 `cronjob` 工具现在真正实现 `action="update"`，会在 prompt 更新时执行注入扫描，在 schedule 更新时重新计算 `next_run`。
  - `skills`、`enabled_toolsets`、`context_from` 和 `deliver` 字段现在能在 SQLite cron store 中往返保存，支撑 skill 装载与 gateway 定向投递。
- **v0.3.82 Usage Pricing 成本估算**
  - gateway `/usage` 现在会在 token 计数和 rate-limit 快照之外展示单轮 USD 估算成本。
  - `hakimi-common` 新增离线定价快照，覆盖常见 OpenAI、Anthropic、Gemini、DeepSeek 和 MiniMax 路由，并按供应商语义处理 cached token。
  - 新增定价回归测试，覆盖 OpenAI cached prompt、Anthropic cache read/write、模型前缀路由、订阅包含路由、未知模型和缺少 cache 定价等分支。
- **v0.3.81 Video Analysis 请求**
  - 新增 Rust 原生 `video_analyze` 媒体工具，支持 HTTP/HTTPS、`file://` 或本地视频路径，并生成可交给视频模型的结构化请求块。
  - 支持 mp4、webm、mov、avi、mkv、mpeg、mpg，执行 MIME 检测，并在模型调用前阻断过大的原始/base64 载荷。
  - 新增 schema、MIME 检测、file URL、结构化载荷和大小限制回归测试，不依赖真实供应商 API。
- **v0.3.80 自更新状态恢复修复**
  - `hakimi --update` 现在只备份用户状态路径：`memory`、`sessions`、`sessions.db*` 和 `profiles`，不再归档整个 `~/.hakimi` 目录。
  - 恢复更新前的 memory/session 状态时，不会再覆盖刚安装完成的 `~/.hakimi/bin/hakimi` canonical binary。
  - 新增状态恢复回归测试，验证 memory/session 能恢复，同时新 binary 保持不变。
- **v0.3.79 Gateway `/usage` 展示**
  - gateway 聊天里现在可以在一次对话后执行 `/usage`，查看当前模型、供应商、API 调用次数，以及 prompt/completion/total token 用量。
  - 如果当前 transport 捕获到了供应商 `x-ratelimit-*` 响应头，`/usage` 会一并展示最近的请求/Token rate-limit 快照，对齐 Hermes 给远程操作者的用量可见性。
  - 新增空状态、token 计数、cache/reasoning 桶和 rate-limit 快照渲染回归测试，不调用真实供应商 API。
- **v0.3.78 Rate Limit Tracking**
  - `hakimi-transports` 现在解析 OpenAI/Nous 风格的 `x-ratelimit-*` 请求/Token 分钟与小时窗口，并支持数字和时长形式的 reset 值。
  - Chat Completions、Responses、Anthropic、Gemini transport 会保留最近一次 rate-limit 快照，为后续 `/usage` 与 gateway 状态展示提供统一底座。
  - 新增解析、格式化、热点警告和 tracker 快照回归测试，不调用真实供应商 API。
- **v0.3.77 Think Scrubber 强化**
  - `ThinkScrubber` 现在按 Hermes 语义处理 `<think>`、`<thinking>`、`<reasoning>`、`<thought>`、`<REASONING_SCRATCHPAD>` 等标签，大小写不敏感，并支持 SSE delta 边界拆分标签。
  - streaming 与非 streaming Agent loop 都会把清理后的文本写入 `final_response` 和 assistant history，同时把隐藏 reasoning 单独保留。
  - 新增状态机与 Agent loop 回归覆盖：拆分标签、标签变体、行内闭合标签、非流式响应和 streaming accumulator。
- **v0.3.76 Doctor CLI / Gateway 诊断**
  - 新增 `hakimi doctor`，并保留兼容的 `hakimi --doctor`，无需启动 Agent loop 即可运行环境诊断。
  - gateway `/doctor` 现在会返回适合聊天窗口展示的纯文本诊断报告，不再落到占位响应。
  - 新增顶层 `doctor` / `setup` 命令解析与无 ANSI 诊断报告格式化回归覆盖。
- **v0.3.75 Home Assistant 工具组**
  - 新增 `ha_list_entities`、`ha_get_state`、`ha_list_services`、`ha_call_service`，用 Rust 原生 async REST 实现对齐 Hermes 的 Home Assistant 工具面。
  - 调用服务前校验 domain/service/entity_id，阻断 `shell_command`、`python_script`、`hassio`、`rest_command` 等高风险 HA 域。
  - 新增离线回归覆盖：输入校验、实体/服务摘要、payload 解析、schema、阻断域和服务响应解析，不依赖真实 Home Assistant 实例。
- **v0.3.74 Image Describe Vision Alias**
  - `image_describe` 现在复用 `vision_analyze` 管线，不再返回占位文本；旧媒体工作流会得到同样的 base64 data-url 视觉请求载荷。
  - `GAP_ANALYSIS` 不再把 Vision 同时列为缺失与完成。
  - 为 `image_describe` 补充元数据、schema、参数校验和本地文件载荷回归测试。
- **v0.3.73 Responses 流恢复**
  - OpenAI Responses 的 `response.incomplete` SSE 事件现在映射为 `length` 结束原因，Hakimi 会自动续写，不再把半截答案当作最终回复。
  - 流式供应商如果在 `Done` 或 `Finished` 终止事件前关闭连接，会被视为可重试传输失败，并复用现有 backoff 重试路径。
  - CLI、server 与 TUI 的 LLM transport 统一使用带 connect/read timeout 的 reqwest client，长 SSE 流保持可用，同时避免无限挂起。
- **v0.3.72 Cron Prompt Injection 防护**
  - 对用户创建的 cron prompt 做 Hermes 风格扫描，阻断 prompt injection、密钥外传、破坏性命令和不可见 Unicode 标记。
  - 到期自动执行前再次扫描；危险任务会被禁用并投递 gateway 通知，不会进入自动批准的 cron agent 上下文。
  - 基础 prompt injection 检测下沉到 `hakimi-common`，core 文件安全与 cron 安全共享同一套基线检测。
- **v0.3.71 Cron 手动触发**
  - gateway 会话里现在可以执行 `/cron run <job-id>`，把既有定时任务安排到下一次 scheduler tick 执行，对齐 Hermes 的即时触发语义。
  - 内置 `cronjob` 工具现在真正支持 `action="run"`，不再暴露“声明支持但执行时报 unsupported”的动作。
  - `hakimi-cron` 通过原地更新 `enabled` 与 `next_run` 触发任务，避免为了手动触发而重写整行任务记录。
- **v0.3.70 Gateway Cron 管理闭环**
  - gateway 会话里现在可以直接执行 `/cron list`、`/cron pause <job-id>`、`/cron resume <job-id>`、`/cron remove <job-id>`。
  - 这些命令直接操作共享的 `~/.hakimi/cron.db`，和 Rust 原生 cron 持久化状态保持一致。
  - 文档与差距分析也已同步修正：当前已完成基础运维动作，delivery、独立 CLI 管理与 skill 装载仍是后续 Hermes parity 工作。

### 🧠 Hakimi 原创特性

以下特性在原版 Hermes Agent 中不存在，是 Hakimi 独有的：

**知识图谱记忆** (`hakimi-knowledge`)
- 基于 petgraph 的有向图，10 种节点类型（实体、概念、事实、偏好、人物、地点、技能、工具、事件、笔记）和 12 种边类型
- BFS 邻居查询、最短路径、子图提取、模糊搜索
- 文件持久化 + 自动保存，接入 MemoryProvider 接口
- 用结构化、可查询的知识图谱替代扁平记忆文件

**意图推理** (`hakimi-context`)
- 将用户消息分类为 10 种意图（信息检索、任务执行、调试、规划、研究等）
- 基于关键词 + 模式的规则匹配，无 ML 依赖，零延迟
- 置信度评分、次级意图、预测下一步工具
- 上下文感知：结合近期工具调用历史修正预测

**决策树回溯** (`hakimi-session`)
- 对话存储为分支树，而非扁平列表
- 回溯到任意决策点，探索替代路径
- 跨分支对比结果
- JSON 序列化支持持久化和回放

**角色自适应** (`hakimi-context`)
- 8 种角色预设：程序员、研究员、写作者、分析师、导师、助手、运维、评审员
- 根据消息内容和工具上下文自动检测角色
- 按角色过滤和排序工具（程序员优先 terminal/patch，研究员优先 web_search）
- 角色切换历史记录

**元技能提炼** (`hakimi-skills`)
- 分析历史会话中的 6 种模式：工具序列、错误修复、搜索精炼、文件编辑、委派、配置
- 从提取的模式自动生成可复用的 YAML 技能文件
- 模式合并与置信度评分

### 🛠️ 41 个内置工具

- **文件**: read_file, write_file, search_files, patch
- **终端**: terminal, process (后台进程管理)
- **Web**: web_search, web_extract
- **Home Assistant**: ha_list_entities, ha_get_state, ha_list_services, ha_call_service
- **记忆**: memory (持久化), session_search (FTS5 全文检索)
- **代码**: code_exec (Python/JS/Bash)
- **浏览器**: browser_navigate, browser_snapshot, browser_click, browser_type, browser_scroll, browser_back, browser_press, browser_get_images, browser_console, browser_dialog, browser_screenshot (Chromium 自动化)
- **媒体**: vision_analyze (图片分析), video_analyze (视频分析请求), image_describe (旧工具兼容别名), image_generate, text_to_speech, transcribe_audio
- **效率**: todo, clarify, checkpoint (git 快照回滚)
- **安全**: file_safety (路径保护与 read_file 凭据读取防护), secret_redaction (密钥脱敏), prompt_injection_detection
- **元操作**: delegate_task (子 Agent 委派), skill_manage, send_message

### 🔌 传输层

| 传输 | API | 流式 | 状态 |
|------|-----|------|------|
| ChatCompletions | OpenAI 兼容 (`/v1/chat/completions`) | ✅ SSE | 生产就绪 |
| Anthropic | Messages API (`/v1/messages`) | ✅ SSE + Prompt 缓存 | 生产就绪 |
| Gemini | Google Gemini native API | ✅ SSE | 生产就绪 |
| Bedrock | AWS Converse API | ✅ | 计划中 |

### 🌐 8 个平台适配器

Telegram · Discord · Slack · DingTalk · WeCom · Signal · Matrix · Webhook

Telegram 现在会直接上传本地生成图片，并把 TTS 生成的本地音频作为原生音频消息发送，因此 `image_generate` / `text_to_speech` 的结果可以直接投递给 gateway 用户，而不是只返回文件路径。针对语音输入链路，Hakimi 现在还提供 `transcribe_audio`，可转写本地音频文件或远程音频 URL；CLI 的按键录音模式仍是后续事项。

在 gateway 会话里，`/cron` 现在已经支持 `list`、`status`、`add --repeat N`、`edit`、`pause <job-id>`、`resume <job-id>`、`run <job-id>`、`remove <job-id>`，会直接操作共享的 SQLite `cron.db`；gateway 聊天中创建的任务会保留当前 `platform:chat_id` 作为投递目标，宿主机运维侧也可以运行 `hakimi cron tick`，用 gateway scheduler 相同的 tick lock 执行一次到期任务。设置 repeat 上限的任务会追踪已完成次数，并在达到上限后自动移除。

### 🧠 智能上下文压缩

三层压缩策略，无需手动管理上下文窗口：
- **Tier 1**: 丢弃旧的工具调用结果
- **Tier 2**: 用辅助 LLM 摘要中间对话轮次
- **Tier 3**: 滑动窗口保留最近对话

### 🔐 凭证池与错误恢复

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

20+ 错误类型自动分类：认证失败 → 轮换密钥；OAuth 终态失败 → 隔离凭证；限流 → 指数退避；上下文溢出 → 触发压缩；模型不存在 → 切换备选。

### 🔧 MCP (Model Context Protocol)

完整 MCP 客户端，支持 stdio / HTTP / SSE 三种传输。stdio 下基于 Node 的 MCP 服务器还会在 PATH 被收窄时，从 Hakimi 托管目录、用户本地目录和 `/usr/local/bin` 解析 `node`、`npm`、`npx`。远程 MCP 传输和工具错误路径会在暴露给 agent 前脱敏类似凭据的文本。内置 9 个热门服务器目录（filesystem、GitHub、Brave Search、PostgreSQL、Puppeteer、memory、fetch、SQLite、sequential-thinking）。

### 📦 插件系统

```yaml
# ~/.hakimi/plugins/weather.yaml
name: weather
version: "1.0"
description: "Weather lookup plugin backed by wttr.in"
tools:
  - name: get_weather
    endpoint: "https://wttr.in/{city}?format=j1"
    method: GET
    description: "获取城市天气"
```

CLI 内嵌 HTTP 插件模板。使用 `hakimi plugins templates` 浏览模板，`hakimi plugins init weather [name]` 生成到 `~/.hakimi/plugins`，并用 `hakimi plugins list --plain` / `hakimi plugins list --json` 或 gateway `/plugins list` 查看已加载插件。

---

## 架构设计

**20 个 crate，每个单一职责**：

```
hakimi-agent/
├── crates/
│   ├── hakimi-common/      # 共享类型，20+ 错误分类
│   ├── hakimi-config/      # YAML 配置，凭证池，环境变量展开
│   ├── hakimi-session/     # SQLite WAL + FTS5，决策树回溯
│   ├── hakimi-context/     # 上下文引擎，压缩，意图推理，角色适配
│   ├── hakimi-core/        # Agent 循环，错误分类器，凭证池，护栏
│   ├── hakimi-transports/  # LLM 传输 (OpenAI/Anthropic/Gemini) + Prompt 缓存
│   ├── hakimi-tools/       # 41 个内置工具 + 注册表
│   ├── hakimi-knowledge/   # 知识图谱记忆 (petgraph)
│   ├── hakimi-skills/      # 技能系统 + 元技能提炼
│   ├── hakimi-cron/        # 定时任务调度器 (SQLite 持久化)
│   ├── hakimi-gateway/     # 8 个平台适配器
│   ├── hakimi-mcp/         # MCP 客户端 (stdio/HTTP/SSE) + 服务器目录
│   ├── hakimi-plugin/      # 插件加载器
│   ├── hakimi-i18n/        # 国际化
│   ├── hakimi-batch/       # 并行批处理
│   ├── hakimi-server/      # HTTP REST API (Axum)
│   ├── hakimi-cli/         # REPL CLI + 配置向导 + 诊断
│   └── hakimi-tui/         # ratatui 终端 UI
```

### 核心循环

```
用户消息
    │
    ▼
┌──────────────────────────────────────────────────┐
│  AIAgent.run_conversation()                      │
│                                                  │
│  1. 分类意图 → 预测所需工具                       │
│  2. 适配角色 → 过滤/排序工具                      │
│  3. 构建系统提示 + 知识图谱上下文                 │
│  4. 凭证池获取 API Key → 调用 LLM (SSE 流式)     │
│  5. 工具调用 → 分发执行 → 循环                   │
│  6. 文本响应 → 返回                              │
│  7. 错误分类 → 自动恢复                          │
│  8. 护栏检查 → 循环检测/熔断                      │
│  9. 记录决策树节点                                │
└──────────────────────────────────────────────────┘
    │
    ▼
响应 + Token 用量 + 知识更新
```

---

## 性能对比

| 特性 | Hermes (Python) | Hakimi (Rust) |
|------|-----------------|---------------|
| 语言 | Python 3.11+ | Rust 2024 |
| 异步模型 | asyncio + 线程桥接 | tokio 原生 async |
| 内存模型 | threading.RLock | `Arc<RwLock>` |
| 工具注册 | 运行时 AST 扫描 | 编译期 trait 实现 |
| 启动时间 | ~2s | ~50ms |
| 空闲内存 | ~150MB | ~15MB |
| 流式传输 | Generator | SSE + futures Stream |
| 错误恢复 | 基础重试 | 20+ 分类 + 自动策略 |
| 凭证管理 | 单密钥 | 多密钥池 + 轮换 + 熔断 |
| 知识模型 | 扁平记忆文件 | 图数据库 (petgraph) |
| 意图识别 | 无 | 10 类分类器 |
| 角色适配 | 无 | 8 角色自动检测 |
| 对话模型 | 扁平消息列表 | 决策树 + 回溯 |
| 技能提炼 | 手动 | 自动模式提取 |
| 测试 | ~500 | 1246 |

---

## 开发

```bash
# 编译全部
cargo build --workspace

# 运行全部测试 (1246 tests)
cargo test --workspace

# Debug 日志
RUST_LOG=debug cargo run -p hakimi-cli

# Clippy 检查
cargo clippy --workspace
```

---

## 路线图

- [x] 核心 Agent 循环 + 工具分发
- [x] OpenAI / Anthropic / Gemini 传输 + SSE 流式
- [x] 41 个内置工具
- [x] 8 个平台适配器
- [x] MCP 客户端 (stdio/HTTP/SSE) + 服务器目录
- [x] 插件系统 + 模板
- [x] ratatui TUI
- [x] SQLite 会话存储 + FTS5
- [x] 智能上下文压缩 (3 层)
- [x] 错误分类器 (20+ 类型) + 凭证池
- [x] Prompt 缓存 (Anthropic)
- [x] Vision 分析 + Checkpoint 回滚
- [x] Profiles + i18n + 批处理
- [x] 安装脚本 + cargo install + CI/CD
- [x] **浏览器自动化** (Chromium via chromiumoxide)
- [x] 配置向导 + 诊断工具
- [x] **知识图谱记忆** (petgraph)
- [x] **意图推理引擎**
- [x] **决策树回溯**
- [x] **角色自适应**
- [x] **元技能自动提炼**
- [ ] WASM 插件运行时
- [ ] Web 仪表盘
- [ ] CLI 语音模式（按键录音 + 播放）

---

## 许可证

MIT License — 详见 [LICENSE](LICENSE)

---

<p align="center">
  <b>用 🦀 Rust 和 ❤️ 构建</b><br>
  <sub>源自 <a href="https://github.com/NousResearch/hermes-agent">Hermes Agent</a> by Nous Research</sub>
</p>
