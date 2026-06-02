# 🐙 Hakimi Agent

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.234-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1743-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/lines-44K+-orange?style=for-the-badge" alt="Lines">
</p>

<p align="center">
  <strong>用 Rust 全栈重写的生产级 AI Agent 框架</strong><br>
  <sub>源自 <a href="https://github.com/NousResearch/hermes-agent">Nous Research Hermes Agent</a> 架构，从零用 Rust 实现</sub>
</p>

<p align="center">
  <a href="#安装">安装</a> ·
  <a href="#为什么选-hakimi">为什么选 Hakimi</a> ·
  <a href="#核心能力">核心能力</a> ·
  <a href="#架构设计">架构</a> ·
  <a href="#性能对比">性能对比</a> ·
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

**任意平台（已安装 Rust）：**
```bash
cargo install hakimi-agent
```

**快速上手：**
```bash
hakimi setup      # 交互式配置向导
hakimi doctor     # 诊断环境与连接
```

---

## 为什么选 Hakimi？

Python 写的 AI Agent 框架启动慢、吃内存、还动不动运行时报错。Hakimi 从零用 Rust 重写，把性能和安全做到极致。

| 指标 | Python Agent | Hakimi (Rust) |
|------|-------------|---------------|
| 启动时间 | ~2s | ~50ms |
| 空闲内存 | ~150MB | ~15MB |
| 异步模型 | asyncio + GIL | tokio 原生 async |
| 工具安全 | 运行时才报错 | 编译期类型保证 |
| 测试数量 | ~500 | 1743 |

**不是 wrapper，不是 demo，是真能上的生产系统：**
- 20+ 种 API 错误类型自动识别并恢复
- 多 Key 凭证池 + 熔断 + 自动轮换
- 三层上下文压缩，无需手动维护 context window
- 记录在 `onboarding.seen` 下的首次触达引导提示
- 决策树式对话记录，支持回溯分支
- 意图推理引擎——预判你下一步要用什么工具
- 角色自适应——自动切换程序员、研究员、写作者等模式

---

## 核心能力

### 🌟 特色功能

**智能上下文管理**
- 三层压缩：丢弃旧工具结果 → LLM 摘要中间轮次 → 滑动窗口
- 全自动，零手动配置
- 模型上下文窗口感知：`model.context_length` 覆盖静态元数据，并统一驱动压缩与工具披露阈值

**63+ 内置工具**
- **文件操作**：读写搜索补丁，安全沙箱保护
- **终端**：命令执行 + 后台进程管理
- **Web**：搜索、内容提取、浏览器自动化（Chromium + 截图视觉请求 + Playwright 缓存/headless-shell 发现 + 原始 CDP 派发 + CDP frame tree 检查 + 云浏览器 Provider 就绪状态 + Provider CDP endpoint 路由）
- **桌面**：Hermes 风格 `computer_use` 就绪面，支持安全等待、macOS 截图/应用发现与受保护的动作 schema
- **代码执行**：Python/JS/Bash 沙箱运行
- **媒体**：图片分析、视频分析、语音合成、带静音幻觉过滤和超大 WAV 分块的语音转文字
- **记忆**：持久化记忆 + FTS5 全文检索 + `hakimi knowledge` / TUI `/knowledge` / 网关 `/knowledge` 图谱操作
- **效率**：待办清单、支持 Profile 路由、工作日志、事件轨迹、诊断、通知订阅、swarm 图创建与 dashboard 读写管理的 Kanban 看板、定时任务
- **元能力**：子 Agent 委派、Mixture-of-Agents 多模型推理、技能系统、插件机制
- **评测**：Hermes 兼容的 ShareGPT JSONL 轨迹保存，覆盖完成与失败轮次

**多平台网关**
- Telegram · Discord · Slack · Mattermost · Webhook · Microsoft Graph webhook · Signal · SMS/Twilio · Email/SMTP · WhatsApp Business Cloud · Home Assistant · Matrix · 钉钉 · 企业微信 · 飞书/Lark · BlueBubbles/iMessage · QQBot 出站 · WeChat · Weixin/iLink 别名
- 配置驱动多适配器接入：聊天平台和 Webhook 网关可同时运行
- 实时流式输出，支持 progressive edits、Telegram 原生草稿 preview、flood-control 退避、按平台配置 preview 策略，长回复会按平台限制做 UTF-8 安全分片
- 持久化生命周期诊断会把适配器、连接、路由、过滤和编辑事件写入 `~/.hakimi/logs/gateway-events.log`；`/logs`、`/logs events`、`/logs gateway` 可直接读取近期日志，不再依赖外部 `tail`
- 网关 `/undo [N]` 可回退近期内存会话轮次，并回显目标提示词，方便编辑后重发
- 网关 `/usage` 会显示上一轮 token/成本/限流数据，按需使用 OpenRouter-compatible `/v1/models` 实时价格，并展示 OpenRouter `/credits` 与 `/key` 额度/用量、Anthropic OAuth 账户窗口和 Codex 用量窗口，且不会暴露凭证
- 聊天里直接创建定时任务 `/cron add`
- 网关 `/voice on|off|tts|status|doctor` 可切换口语化回复并报告语音 I/O 就绪状态，不污染 prompt cache 和聊天历史
- TUI `/config [field]` 可查看脱敏运行配置摘要；`/gateway [cmd]` 可查看已配置适配器、缓存频道目标和生命周期事件；`/sessions [cmd]` 可浏览已保存 SQLite 会话；`/skills [cmd]` 可浏览/搜索本地 Skills Hub 元数据；`/cron [cmd]` 可在本地管理持久化 cron 数据库；`/undo [N]` 可把近期提示词放回输入框继续编辑；`/checkpoints [cmd]` 可在不进入模型循环的情况下查看和管理共享 shadow-git 检查点；`/voice status` 与可配置 Ctrl+B/Ctrl+字母按键录音共用 `voice.*` 配置、TTS/转写工具、音频环境检查、PCM16 WAV 录音产物校验、超大 WAV 分块 STT 派发、本地 TTS 播放启动、支持录音后端和自动 transcript 提交的 `voice_capture` 工具、连续重启录音、二次按键取消录音、三次无语音自动退出，以及 Hermes 风格开始/停止提示音

**可扩展**
- MCP 协议客户端 — stdio / HTTP / SSE 传输，支持 CLI/网关目录搜索与配置片段生成，并支持 stdio 服务端发起 sampling 时转发工具 schema 与返回 `tool_use` handoff
- HTTP 插件系统，YAML 模板
- HTTP API 发现端点 — OpenAI 兼容 `/v1/models`、`/v1/capabilities`、`/v1/skills`、`/v1/toolsets`、文本 `/v1/chat/completions` 在 `stream=true` 时返回 completed SSE snapshot、`/v1/responses` 支持 SQLite 持久化 `previous_response_id` 链式续写并返回 completed SSE snapshot，带实时生命周期 SSE 的可轮询且可取消 `/v1/runs`，以及会话生命周期/消息/搜索能力发现，方便外部 UI 探测能力
- WebUI 管理 API — `/api/status`、`/api/sessions` 创建/更新/删除/fork 以及消息/搜索检查、`/api/mcp/servers`、`/api/credentials/pool`、`/api/webhooks` 和 Kanban `/api/kanban` 看板/任务读写管理提供脱敏运行状态，并支持运行期作用域的管理写入
- Skills Hub — 社区技能市场
- 静态 i18n 基础设施 — 支持 `display.language`、`HAKIMI_LANGUAGE` / `HERMES_LANGUAGE`、Hermes 兼容语言别名、YAML catalog 目录加载、英文 fallback 和静态用户文案的命名占位符
- CLI Skin Engine — `hakimi skin list|inspect|set|path` 与网关 `/skin` 可发现内置和 `~/.hakimi/skins/*.yaml` 主题，缺失字段继承 `default`，持久化 `display.skin`，并把选中的 branding/colors/logo/hero 应用到 CLI 启动横幅，同时驱动 TUI 思考态 spinner faces/verbs/wings，以及状态栏、session、选择态、补全提示、帮助标题、输入区、响应框、工具前缀、工具 emoji 标签、运行中工具进度和工具面板配色
- 隔离 Profile — 管理命名工作区、克隆/导出 Profile 归档、安装/更新带 `distribution.yaml` 的可分享 Profile 分发包、创建 `~/.hakimi/bin/<profile>` 包装别名，通过网关 `/profile` 操作，并让 `--profile` / sticky `active_profile` 运行绑定到 Profile 作用域的配置、记忆、会话、技能、cron、轨迹、网关日志和 TUI 默认路径
- 10 个精选 MCP 目录项：GitHub、文件系统、Brave Search、PostgreSQL、Puppeteer、记忆、fetch、SQLite、思维链，以及 Hermes 审核过的 n8n bridge

### 🛡️ 生产级安全

- **密钥脱敏** — API Key、JWT、Token 自动遮蔽
- **Prompt 注入检测** — 扫描技能、定时任务、上下文文件
- **SSRF 防护** — 阻断内网/元数据 URL 请求
- **命令安全** — 阻断危险 Shell 模式
- **工具循环防护** — 对重复无进展的只读工具调用给出提示，并拦截失控的完全重复调用
- **一次性引导提示** — CLI/网关首次触达提示会持久化到 `onboarding.seen`
- **写入保护** — 限定可写入的目录
- **配置文件读取保护** — 保护 config.yaml 等敏感文件
- **共享 shadow-git 检查点** — `checkpoint` 工具与网关 `/checkpoints` 的快照写入 `~/.hakimi/checkpoints/store`，不会污染项目 `.git`
- **工具输出上限** — 可用 `tools.output.max_bytes` 配置工具结果进入上下文前的统一截断边界

### Hakimi 独有创新

> 以下功能原版 Hermes Agent 没有，是 Hakimi 首创：

**知识图谱记忆** — 用 petgraph 有向图替代扁平记忆文件。10 种节点类型、12 种边类型，持久化到 `~/.hakimi/knowledge.json`，支持运行时工具、`hakimi knowledge ...`、TUI `/knowledge ...` 和网关 `/knowledge ...` 操作。

**意图推理** — 将用户消息分类为 10 种意图，零延迟规则匹配，结合工具历史预测下一步操作。

**决策树回溯** — 对话存储为分支树，任意决策点可回溯探索其他路径，支持跨分支结果对比。

**角色自适应** — 8 种角色预设（程序员、研究员、写作者等），根据消息内容自动切换，按角色筛选工具优先级。

**元技能提炼** — 自动分析历史会话中的 6 种模式，自动生成可复用的 YAML 技能文件。

---

## 架构设计

**20 个 crate，各司其职：**

```
hakimi-agent/
├── hakimi-core/          # Agent 主循环 + 错误分类 + 凭证池
├── hakimi-transports/    # LLM 传输层 (OpenAI/Anthropic/Gemini/Bedrock)
├── hakimi-tools/         # 63+ 内置工具 + 插件注册
├── hakimi-session/       # SQLite WAL + FTS5 + 决策树
├── hakimi-context/       # 上下文引擎 + 压缩 + 意图推理 + 角色
├── hakimi-knowledge/    # 知识图谱 (petgraph)
├── hakimi-skills/        # 技能系统 + 元技能提炼
├── hakimi-cron/          # 持久化定时任务调度器
├── hakimi-gateway/       # 19 个运行时可启用的平台适配器
├── hakimi-mcp/           # MCP 客户端 (stdio/HTTP/SSE)
├── hakimi-cli/           # REPL CLI + 安装向导 + 诊断
└── hakimi-tui/           # ratatui 终端界面
```

### 核心流程

```
用户消息
    │
    ▼
┌─────────────────────────────────────┐
│  ① 意图分类 → 预测工具需求          │
├─────────────────────────────────────┤
│  ② 角色自适应 → 筛选/排序工具       │
├─────────────────────────────────────┤
│  ③ 构建上下文 → 系统提示 + 知识图谱 │
│     → 应用三层压缩                   │
├─────────────────────────────────────┤
│  ④ 凭证池获取 API Key → LLM 调用   │
│     → SSE 流式输出                   │
├─────────────────────────────────────┤
│  ⑤ 工具调度 → 执行 + 安全检查       │
│     → 错误分类 + 自动恢复            │
├─────────────────────────────────────┤
│  ⑥ 记录决策树节点                   │
└─────────────────────────────────────┘
    │
    ▼
响应 + 记忆更新 + 用量统计
```

---

## 性能对比

| 特性 | Hermes (Python) | Hakimi (Rust) |
|------|----------------|---------------|
| 语言 | Python 3.11+ | Rust 2024 |
| 启动时间 | ~2s | ~50ms |
| 空闲内存 | ~150MB | ~15MB |
| 异步模型 | asyncio + GIL | tokio 原生 |
| 工具注册 | 运行时 AST 扫描 | 编译期 trait |
| 错误恢复 | 基础重试 | 20+ 分类器 |
| 知识模型 | 扁平文件 | 图数据库 (petgraph) |
| 意图检测 | 无 | 10 分类规则引擎 |
| 角色自适应 | 无 | 8 角色自动切换 |
| 对话模型 | 扁平列表 | 决策树 |
| 测试数量 | ~500 | 1743 |

---

## 开发

```bash
# 编译全部
cargo build --workspace

# 运行测试
cargo test --workspace

# Lint
cargo clippy --workspace

# 调试日志
RUST_LOG=debug cargo run -p hakimi-cli
```

---

## 路线图

- [x] Agent 主循环 + 工具调度
- [x] OpenAI / Anthropic / Gemini 传输层 + SSE 流式，并支持非流式 AWS Bedrock Converse
- [x] 63+ 内置工具
- [x] 19 个运行时可启用的平台适配器
- [x] 网关目标目录 + send_message 频道解析
- [x] MCP 客户端 + CLI/网关服务器目录
- [x] HTTP API 模型/能力发现端点 + 文本 Chat Completions/Responses SSE 快照 + 可取消 Runs 实时生命周期事件
- [x] WebUI 管理 API 摘要 + 运行期写入 + Kanban 读写管理
- [x] 插件系统 + HTTP 模板
- [x] Profile 分发包安装/更新/info，并保护用户数据
- [x] CLI Skin Engine，支持内置/用户 YAML 主题、`display.skin` 持久化、启动横幅主题化和 TUI spinner、状态栏、补全、帮助、工具 emoji/进度与界面主题化
- [x] ratatui TUI 界面、本地 slash 命令、脱敏配置浏览与网关状态面
- [x] 智能上下文压缩（3 层）
- [x] 错误分类器 + 凭证池
- [x] Prompt 缓存 (Anthropic)
- [x] 图片 + 视频分析
- [x] 知识图谱记忆，支持 CLI/TUI/网关操作命令
- [x] 意图推理引擎
- [x] 决策树回溯
- [x] 角色自适应
- [x] 元技能自动提取
- [x] 浏览器自动化 (Chromium + Playwright 缓存发现 + CDP 就绪探针 + frame tree 检查)
- [x] Computer Use 就绪面
- [x] Kanban 看板 + Profile 路由 + 工作日志 + 通知游标 + swarm 图 + dashboard 读写管理
- [x] 网关语音回复模式
- [x] TUI 语音就绪诊断与媒体工具配置对齐
- [x] 语音环境诊断与 STT 静音幻觉过滤
- [x] 语音采集用 PCM16 WAV 录音产物校验
- [x] 语音 TTS 播放文本清洗、MP3 缓存规划与本地播放器启动
- [x] 语音采集工具支持系统录音后端与 STT 派发
- [x] 超大 WAV 分块的录音 STT 派发
- [x] TUI Ctrl+B 连续语音录音循环
- [x] TUI 检查点查看与管理 slash 命令
- [x] TUI 已保存会话浏览 slash 命令
- [x] TUI 技能浏览 slash 命令
- [x] TUI 定时任务管理 slash 命令
- [x] 语音采集二次按键中断
- [x] 语音采集开始/停止提示音
- [x] 语音采集连续重启模式
- [x] 基于 OpenRouter 的 Mixture-of-Agents 多模型推理
- [x] 网关 `/usage` 展示 OpenRouter、Anthropic 与 Codex 账户用量
- [ ] WASM 插件运行时
- [ ] Web 仪表盘

---

## 许可证

MIT License
