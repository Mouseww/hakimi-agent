<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.3.0-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1035-passing?style=for-the-badge&color=brightgreen" alt="Tests">
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
hakimi --setup
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

**生产级特性：** 1035 个测试 · 20+ API 错误类型自动分类与恢复 · 多密钥凭证池与熔断 · 三层上下文压缩 · Anthropic Prompt 缓存

---

## 核心能力

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

### 🛠️ 30 个内置工具

- **文件**: read_file, write_file, search_files, patch
- **终端**: terminal, process (后台进程管理)
- **Web**: web_search, web_extract
- **记忆**: memory (持久化), session_search (FTS5 全文检索)
- **代码**: code_exec (Python/JS/Bash)
- **浏览器**: browser_navigate, browser_snapshot, browser_click, browser_type, browser_screenshot (Chromium 自动化)
- **媒体**: vision_analyze (图片分析), image_generate
- **效率**: todo, clarify, checkpoint (git 快照回滚)
- **安全**: file_safety (路径保护), secret_redaction (密钥脱敏), prompt_injection_detection
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

20+ 错误类型自动分类：认证失败 → 轮换密钥；限流 → 指数退避；上下文溢出 → 触发压缩；模型不存在 → 切换备选。

### 🔧 MCP (Model Context Protocol)

完整 MCP 客户端，支持 stdio / HTTP / SSE 三种传输。内置 9 个热门服务器目录（filesystem、GitHub、Brave Search、PostgreSQL、Puppeteer、memory、fetch、SQLite、sequential-thinking）。

### 📦 插件系统

```yaml
# ~/.hakimi/plugins/weather.yaml
name: weather
tools:
  - name: get_weather
    endpoint: "https://wttr.in/{city}?format=j1"
    method: GET
    description: "获取城市天气"
```

内置 4 个即用模板。`hakimi plugins list` 浏览，`hakimi plugins init <name>` 一键生成。

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
│   ├── hakimi-tools/       # 25 个内置工具 + 注册表
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
| 测试 | ~500 | 1035 |

---

## 开发

```bash
# 编译全部
cargo build --workspace

# 运行全部测试 (1035 tests)
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
- [x] 25 个内置工具
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
- [ ] 语音输入/输出

---

## 许可证

MIT License — 详见 [LICENSE](LICENSE)

---

<p align="center">
  <b>用 🦀 Rust 和 ❤️ 构建</b><br>
  <sub>源自 <a href="https://github.com/NousResearch/hermes-agent">Hermes Agent</a> by Nous Research</sub>
</p>
