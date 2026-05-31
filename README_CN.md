# 🐙 Hakimi Agent

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.154-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1418-passing?style=for-the-badge&color=brightgreen" alt="Tests">
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
| 测试数量 | ~500 | 1418 |

**不是 wrapper，不是 demo，是真能上的生产系统：**
- 20+ 种 API 错误类型自动识别并恢复
- 多 Key 凭证池 + 熔断 + 自动轮换
- 三层上下文压缩，无需手动维护 context window
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

**58+ 内置工具**
- **文件操作**：读写搜索补丁，安全沙箱保护
- **终端**：命令执行 + 后台进程管理
- **Web**：搜索、内容提取、浏览器自动化（Chromium）
- **代码执行**：Python/JS/Bash 沙箱运行
- **媒体**：图片分析、视频分析、语音合成、语音转文字
- **记忆**：持久化记忆 + FTS5 全文检索
- **效率**：待办清单、支持 Profile 路由、工作日志、事件轨迹、诊断与通知订阅的 Kanban 看板、定时任务
- **元能力**：子 Agent 委派、技能系统、插件机制

**多平台网关**
- Telegram · Discord · Slack · Mattermost · Webhook · Signal · Matrix · 钉钉 · 企业微信 · WeChat
- 配置驱动多适配器接入：聊天平台和 Webhook 网关可同时运行
- 实时流式输出
- 聊天里直接创建定时任务 `/cron add`
- 网关 `/voice on|off|tts|status` 可切换口语化回复，不污染 prompt cache 和聊天历史
- TUI `/voice status` 与可配置 Ctrl+B/Ctrl+字母诊断共用 `voice.*` 配置，和 TTS/转写工具保持一致

**可扩展**
- MCP 协议客户端 — stdio / HTTP / SSE 传输
- HTTP 插件系统，YAML 模板
- Skills Hub — 社区技能市场
- 隔离 Profile — 管理命名工作区、克隆/导出 Profile 归档，并通过网关 `/profile` 操作
- 内置 9 个 MCP 服务器：GitHub、文件系统、Brave Search、PostgreSQL、Puppeteer、记忆、fetch、SQLite、思维链

### 🛡️ 生产级安全

- **密钥脱敏** — API Key、JWT、Token 自动遮蔽
- **Prompt 注入检测** — 扫描技能、定时任务、上下文文件
- **SSRF 防护** — 阻断内网/元数据 URL 请求
- **命令安全** — 阻断危险 Shell 模式
- **工具循环防护** — 对重复无进展的只读工具调用给出提示，并拦截失控的完全重复调用
- **写入保护** — 限定可写入的目录
- **配置文件读取保护** — 保护 config.yaml 等敏感文件
- **工具输出上限** — 可用 `tools.output.max_bytes` 配置工具结果进入上下文前的统一截断边界

### Hakimi 独有创新

> 以下功能原版 Hermes Agent 没有，是 Hakimi 首创：

**知识图谱记忆** — 用 petgraph 有向图替代扁平记忆文件。10 种节点类型、12 种边类型，支持 BFS 搜索、最短路径、子图提取。

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
├── hakimi-transports/    # LLM 传输层 (OpenAI/Anthropic/Gemini)
├── hakimi-tools/         # 58+ 内置工具 + 插件注册
├── hakimi-session/       # SQLite WAL + FTS5 + 决策树
├── hakimi-context/       # 上下文引擎 + 压缩 + 意图推理 + 角色
├── hakimi-knowledge/    # 知识图谱 (petgraph)
├── hakimi-skills/        # 技能系统 + 元技能提炼
├── hakimi-cron/          # 持久化定时任务调度器
├── hakimi-gateway/       # 10 个运行时可启用的平台适配器
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
| 测试数量 | ~500 | 1418 |

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
- [x] OpenAI / Anthropic / Gemini 传输层 + SSE 流式
- [x] 58+ 内置工具
- [x] 10 个运行时可启用的平台适配器
- [x] MCP 客户端 + 服务器目录
- [x] 插件系统 + HTTP 模板
- [x] ratatui TUI 界面
- [x] 智能上下文压缩（3 层）
- [x] 错误分类器 + 凭证池
- [x] Prompt 缓存 (Anthropic)
- [x] 图片 + 视频分析
- [x] 知识图谱记忆
- [x] 意图推理引擎
- [x] 决策树回溯
- [x] 角色自适应
- [x] 元技能自动提取
- [x] 浏览器自动化 (Chromium)
- [x] Kanban 看板 + Profile 路由 + 工作日志 + 通知游标
- [x] 网关语音回复模式
- [x] TUI 语音就绪诊断与媒体工具配置对齐
- [ ] WASM 插件运行时
- [ ] Web 仪表盘
- [ ] CLI 按键录音语音输入

---

## 许可证

MIT License
