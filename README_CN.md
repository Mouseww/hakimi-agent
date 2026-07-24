# Hakimi Agent

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.5.125-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1781-passing?style=for-the-badge&color=brightgreen" alt="Tests">
</p>

<p align="center">
  <strong>生产级 AI Agent 框架 — 用 Rust 从零重写，追求速度、安全与多端可控</strong><br>
  <sub>架构灵感来自 <a href="https://github.com/NousResearch/hermes-agent">Nous Research Hermes Agent</a>，目标对齐并超越</sub>
</p>

<p align="center">
  <a href="#一键安装">安装</a> ·
  <a href="#为什么选-hakimi">为什么选 Hakimi</a> ·
  <a href="#功能列表">功能</a> ·
  <a href="#独特设计">独特设计</a> ·
  <a href="#命令说明">命令</a> ·
  <a href="#hakimi-studio">Studio</a> ·
  <a href="#架构">架构</a> ·
  <a href="README.md">English</a>
</p>

---

## 一键安装

**Linux / macOS**

```bash
curl -sSL https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.sh | bash
```

**Windows（PowerShell）**

```powershell
irm https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.ps1 | iex
```

**源码安装（已装 Rust）**

```bash
cargo install --git https://github.com/Mouseww/hakimi-agent --locked
# 或克隆后：
cargo build --release -p hakimi-agent
```

**第一次使用**

```bash
hakimi setup      # 配置向导（模型、密钥、网关等）
hakimi doctor     # 诊断连通性与环境
hakimi            # 交互 CLI
hakimi --serve    # WebUI + API，默认 127.0.0.1:3005
hakimi --gateway  # 多平台网关（Telegram / Discord / …）
```

- CLI 发布包：GitHub Releases（tag `v*`）
- Studio 桌面包（deb / AppImage / MSI / DMG）：Actions **Desktop** 产物，或同 tag Release 附件

---

## 为什么选 Hakimi？

| | 常见 Python Agent | **Hakimi（Rust）** |
|--|-------------------|-------------------|
| 启动 | ~2s | **~50ms** |
| 空闲内存 | ~150MB | **~15MB** |
| 异步 | asyncio + GIL | **tokio 原生** |
| 工具安全 | 运行时炸 | **编译期 trait** |
| 错误恢复 | 简单重试 | **20+ 分类器 + 策略** |
| 上下文 | 手工/易碎 | **三级智能压缩** |
| 入口 | CLI 或聊天 | **CLI · TUI · WebUI · Gateway · Studio 桌面** |

不是薄封装：凭证池与熔断、密钥脱敏、SSRF 防护、路径 jail、多设备 Studio 协议，以及 CLI + 桌面双发布管线。

---

## 功能列表

### Agent 核心

- Agent 循环 **只在 Rust**（非 TypeScript），支持 SSE / 真流式输出
- **63+ 工具**：文件、Shell、Web、浏览器/CDP、computer-use、代码执行、视觉/TTS/STT、todo、cron、记忆、知识图谱、MCP、委派
- **子 Agent / 团队**：`delegate_task`、具名人格、多层委派与深度上限
- **智能上下文**：丢弃陈旧工具噪声 → LLM 摘要 → 滑动窗口；按模型上下文长度自适应
- **意图 + 角色**：意图分类，Coder / Researcher / Writer 等模式切换
- **记忆**：短/长/工作记忆、FTS5、会话检索（发现 / 滚动 / 浏览）
- **检查点**：共享 shadow-git 存储于 `~/.hakimi/checkpoints`（不污染项目 `.git`）

### 控制面

| 入口 | 说明 |
|------|------|
| **CLI** | REPL、setup、doctor、skills、plugins、profiles |
| **TUI** | ratatui、斜杠指令、语音 PTT、皮肤 |
| **WebUI** | React 控制台：聊天、会话、Office View、cron、配置 |
| **Gateway** | Telegram · Discord · Slack · Signal · WhatsApp · 飞书 · 企微 · Matrix · 邮件 · … |
| **Studio** | 本机优先工作台：工作区 IDE、多设备接管、Hub 中继、桌面壳 |

### Gateway 亮点

- 流式编辑、限流、UTF-8 安全分片
- 忙碌输入：排队或抢占（`gateways.busy_input_mode`）
- 斜杠指令：`/cron`、`/usage`、`/stop`、`/undo`、`/voice`、`/update` …
- `hide_tool_details`：保留 ⚙️ 进度，隐藏 STDOUT/JSON 明细
- Cron：间隔 + 五段表达式，投递到 origin / home / 全频道

### 安全

- 输出前密钥/JWT 脱敏
- Skills / cron / 上下文文件注入启发式检测
- SSRF 黑名单、危险 shell 模式、工具死循环护栏
- 写路径 sandbox + Studio **路径 deny 策略**（`.env`、`.git`、密钥等）
- Studio 多设备 **Controller / Viewer** 角色

### 扩展

- **MCP** 客户端（stdio / HTTP / SSE）+ 目录片段
- HTTP 插件（YAML）与 **WASM** 插件路径（演进中）
- Skills Hub 安装社区技能
- OpenAI 兼容发现：`/v1/models`、`/v1/chat/completions`、`/v1/runs` …
- 隔离 **profile**（`--profile`）隔离配置 / 记忆 / 会话 / cron

---

## 独特设计

1. **Office View** — 人格 = 工位：实时状态、显示器上的工具进度、协作交接动画，SSE 推送无轮询  
2. **Studio 协议** — 序号事件、gap 检测 + session reset、单 Active Runner、Controller/Viewer、Hub 纯中继（中继不落 provider 密钥）  
3. **本机优先执行** — 默认本机跑；需要时经 Hub `worker_dispatch` 切远程  
4. **队列 + 抢占** — 跟进消息可排队，也可打断当前 run  
5. **工作区 jail + 检查点** — 路径 jail、写前自动快照、会话 worktree 隔离  
6. **桌面 = 壳** — `hakimi-desktop` 内嵌后端 + WebUI；Tauri 窗口只是 UI，Agent 循环仍在 Rust  
7. **对齐 Hermes 再超越** — 网关斜杠、皮肤、语音、会话检索形态对齐，多设备 / Office / Studio 打包走得更远  

设计文档：[`docs/hakimi-studio/`](docs/hakimi-studio/) · [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)

---

## 命令说明

### 日常

```bash
hakimi                 # 交互 CLI
hakimi setup           # 配置向导
hakimi doctor          # 健康检查
hakimi --serve         # WebUI + REST/SSE，:3005
hakimi --gateway       # 消息平台网关
```

### Studio / 桌面

```bash
# 无头：内嵌 WebUI + /v1/studio WebSocket
cargo run -p hakimi-desktop -- --bind 127.0.0.1:3015

# 冒烟：监听、打印 URL、退出
cargo run -p hakimi-desktop -- --once

# 原生窗口（Tauri 2；Linux 需 webkit2gtk-4.1）
cargo run -p hakimi-desktop --features gui
```

文档：[`docs/hakimi-studio/DESKTOP.md`](docs/hakimi-studio/DESKTOP.md)

### 运维 / 产品

```bash
hakimi plugin list|install|info …
hakimi skills …
hakimi knowledge …
hakimi skin list|set …
hakimi cron …          # 或 WebUI / 网关 /cron
```

### 网关内指令（示例）

| 指令 | 作用 |
|------|------|
| `/stop` | 取消当前 run 并清空队列 |
| `/undo [N]` | 回退轮次 / 预填编辑 |
| `/usage` | Token、费用、限流 |
| `/cron …` | 定时任务 |
| `/voice …` | TTS / STT |
| `/update` | 自更新路径 |

### 开发

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
RUST_LOG=debug cargo run -p hakimi-cli
```

云端 CI（不在本地 EL9 打 GUI）：

- **CI** — fmt / clippy / tests  
- **Desktop** — Ubuntu 22.04 / Windows / macOS 无头 + GUI 包  
- **Release** — tag `v*` → 多目标 CLI（Desktop 成功时附加桌面产物）

---

## Hakimi Studio

本机优先的 **AI 开发工作台**：

| 组件 | 职责 |
|------|------|
| `hakimi-studio-api` | 协议、EventBus、runtime、AgentHost |
| `hakimi-workspace` | 路径 jail、worktree、检查点 |
| `hakimi-hub` | 纯中继或内嵌 runtime |
| `hakimi-server` | Runner + Studio WS + Hub worker |
| `hakimi-desktop` | 本机二进制 / 可选 Tauri GUI |
| WebUI Studio 面板 | 文件、聊天、设备、生态、cron、检查点 |

```
设备 A（Controller）──WS──┐
设备 B（Viewer）   ──WS──┼── Runtime ── Agent(Rust) ── 工具 / Workspace
Hub（仅中继）      ──WS──┘       ▲
                                └── 可选 remote worker_dispatch
```

权限：[`docs/hakimi-studio/PERMISSIONS.md`](docs/hakimi-studio/PERMISSIONS.md)  
检查点：[`docs/hakimi-studio/CHECKPOINT.md`](docs/hakimi-studio/CHECKPOINT.md)  
协议：[`docs/hakimi-studio/protocol.md`](docs/hakimi-studio/protocol.md)

---

## 架构

```
hakimi-agent/
├── hakimi-core/           # 循环、错误、凭证、委派
├── hakimi-transports/     # OpenAI / Anthropic / Gemini / Bedrock …
├── hakimi-tools/          # 内置工具 + 注册表
├── hakimi-session/        # SQLite WAL + FTS5
├── hakimi-context/        # 压缩、意图、角色
├── hakimi-knowledge/      # 图谱记忆
├── hakimi-skills/         # Skills
├── hakimi-cron/           # 持久化调度
├── hakimi-gateway/        # 平台适配器
├── hakimi-mcp/            # MCP 客户端
├── hakimi-cli/ · hakimi-tui/
├── hakimi-server/         # 统一 serve + Studio + hub worker
├── hakimi-studio-api/ · hakimi-workspace/ · hakimi-hub/
└── hakimi-desktop/        # Studio 桌面壳
```

**单轮流程（简化）**

```
消息 → 意图/角色 → 上下文（压缩）→ 凭证池
     → LLM 流式 → 工具分发 + 护栏 → 会话/记忆
```

---

## 对比

| | Hermes（Python） | **Hakimi** |
|--|------------------|------------|
| 语言 | Python 3.11+ | **Rust 2024** |
| 启动 / 内存 | ~2s / ~150MB | **~50ms / ~15MB** |
| 工具模型 | 运行时 | **编译期 trait** |
| 多设备 Studio | — | **序号事件、交接、Hub 中继** |
| Office / 人格工位 | — | **一等公民 WebUI** |
| 桌面打包 | — | **Tauri 2 CI 矩阵** |
| Gateway 广度 | 强 | **对齐 + 更多适配器** |

---

## 配置速查

| 项 | 说明 |
|----|------|
| 配置目录 | `~/.hakimi/`（或 profile 作用域） |
| WebUI 密码 | `HAKIMI_WEBUI_PASSWORD` → Bearer |
| 语言 | `display.language` / `HAKIMI_LANGUAGE` |
| 隐藏工具明细 | `gateways.hide_tool_details`（保留 ⚙️ 进度） |
| 忙碌输入 | `gateways.busy_input_mode`：`queue` \| `interrupt` |

完整配置请用 `hakimi setup`。

---

## 许可证

MIT License

---

**English** → [README.md](README.md) · **Studio 设计** → [docs/hakimi-studio/DESIGN.md](docs/hakimi-studio/DESIGN.md)
