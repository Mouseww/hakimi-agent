<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.1.0-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-420-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/lines-29K+-orange?style=for-the-badge" alt="Lines">
</p>

<h1 align="center">🐙 Hakimi Agent</h1>

<p align="center">
  <b>用 Rust 重写的 AI Agent 框架 — 比 Python 快 40 倍，内存占用降低 90%</b><br>
  <sub>源自 <a href="https://github.com/NousResearch/hermes-agent">Nous Research Hermes Agent</a> 生产级架构，从零用 Rust 重写</sub>
</p>

<p align="center">
  <a href="#简介">简介</a> •
  <a href="#核心能力">核心能力</a> •
  <a href="#快速开始">快速开始</a> •
  <a href="#架构设计">架构</a> •
  <a href="#与-hermes-python-对比">性能对比</a> •
  <a href="#路线图">路线图</a> •
  <a href="README.md">English</a>
</p>

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

**生产级特性：** 420 个测试 · 20+ API 错误类型自动分类与恢复 · 多密钥凭证池与熔断 · 三层上下文压缩 · Anthropic Prompt 缓存

---

## 核心能力

### 🛠️ 25 个内置工具

- **文件**: read_file, write_file, search_files, patch
- **终端**: terminal, process (后台进程管理)
- **Web**: web_search, web_extract
- **记忆**: memory (持久化), session_search (FTS5 全文检索)
- **代码**: code_exec (Python/JS/Bash)
- **媒体**: vision_analyze (图片分析), image_generate
- **生产**: todo, clarify, checkpoint (git快照回滚)
- **安全**: file_safety (路径保护), secret_redaction (密钥脱敏), prompt_injection_detection
- **元操作**: delegate_task (子Agent委派), skill_manage (技能系统), send_message

### 🔌 传输层

| 传输 | API | 流式 | 状态 |
|------|-----|------|------|
| ChatCompletions | OpenAI 兼容 (`/v1/chat/completions`) | ✅ SSE | 生产就绪 |
| Anthropic | Messages API (`/v1/messages`) | ✅ SSE + Prompt缓存 | 生产就绪 |
| Gemini | Google Gemini native API | ✅ SSE | 生产就绪 |
| Bedrock | AWS Converse API | ✅ | 计划中 |

### 🌐 8 个平台适配器

Telegram · Discord · Slack · DingTalk · WeCom · Signal · Matrix · Webhook

### 🧠 智能上下文压缩

三层压缩策略，无需手动管理上下文窗口：
- **Tier 1**: 丢弃旧的工具调用结果
- **Tier 2**: 用辅助 LLM 摘要中间对话轮次
- **Tier 3**: 滑动窗口保留最近对话

### 🔐 凭证池 & 错误恢复

```yaml
credential_pools:
  openrouter:
    strategy: round_robin  # round_robin / fill_first / random / least_used
    credentials:
      - api_key: "sk-key-1"
        priority: 10
      - api_key: "sk-key-2"
        priority: 5
```

20+ 错误类型自动分类：认证失败 → 轮换密钥；限流 → 指数退避+重试；上下文溢出 → 触发压缩；模型不存在 → 切换备选模型。

### 🔧 MCP (Model Context Protocol)

完整 MCP 客户端，支持 stdio / HTTP / SSE 三种传输：

```rust
let mut client = McpClient::connect_stdio("npx", &["@modelcontextprotocol/server-filesystem"]).await?;
client.initialize().await?;
let tools = client.list_tools().await?;
let result = client.call_tool("read_file", json!({"path": "/tmp/test.txt"})).await?;
```

### 📦 插件系统

```yaml
# ~/.hakimi/plugins/weather.yaml
name: my_api
tools:
  - name: get_weather
    endpoint: "https://api.weather.com/v1/current?city={city}"
    method: GET
    description: "获取城市天气"
```

---

## 快速开始

```bash
# 克隆并编译
git clone https://github.com/Mouseww/hakimi-agent.git
cd hakimi-agent
cargo build --release

# 设置 API Key
export OPENAI_API_KEY="sk-..."

# 启动 CLI 交互模式
./target/release/hakimi

# 单次查询模式
./target/release/hakimi --query "解释 Rust 的所有权机制"

# TUI 模式
./target/release/hakimi --tui

# HTTP API 服务
./target/release/hakimi --serve --port 3000
```

首次运行自动创建 `~/.hakimi/config.yaml`，编辑即可自定义模型、提供商、Agent 行为。

---

## 架构设计

**19 个 crate，每个单一职责**：

```
hakimi-agent/
├── crates/
│   ├── hakimi-common/      # 共享类型: Message, ToolCall, Usage, Error, 20+ 错误分类
│   ├── hakimi-config/      # YAML 配置, 凭证池配置, 环境变量展开
│   ├── hakimi-session/     # SQLite WAL + FTS5 全文检索, 决策树
│   ├── hakimi-context/     # 上下文引擎, 三层压缩, Prompt构建, 角色适配
│   ├── hakimi-core/        # AIAgent builder, 对话循环, 重试, 错误分类器, 凭证池, 护栏
│   ├── hakimi-transports/  # LLM 传输层 (OpenAI, Anthropic, Gemini) + SSE 流式 + Prompt缓存
│   ├── hakimi-tools/       # 25 个内置工具 + 注册表
│   ├── hakimi-cron/        # 定时任务调度器 (SQLite持久化)
│   ├── hakimi-gateway/     # 8 个平台适配器
│   ├── hakimi-mcp/         # MCP 客户端 (stdio/HTTP/SSE)
│   ├── hakimi-plugin/      # 插件加载器
│   ├── hakimi-skills/      # 技能系统 (YAML frontmatter .md)
│   ├── hakimi-i18n/        # 国际化 (YAML语言包)
│   ├── hakimi-batch/       # 并行批处理 + 检查点
│   ├── hakimi-server/      # HTTP REST API (Axum)
│   ├── hakimi-cli/         # REPL CLI + 配置向导 + Doctor诊断
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
│  1. 构建系统提示 + 上下文 (SmartContextEngine)    │
│  2. 凭证池获取 API Key → 调用 LLM (SSE 流式)     │
│  3. 工具调用 → 分发执行 → 循环                   │
│  4. 文本响应 → 返回                              │
│  5. 错误分类 → 自动恢复 (重试/轮换/压缩/降级)    │
│  6. 护栏检查 → 循环检测/熔断                      │
└──────────────────────────────────────────────────┘
    │
    ▼
响应 + Token 用量统计
```

---

## 与 Hermes (Python) 对比

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
| 平台支持 | 20+ | 8 (持续增加) |
| 测试 | ~500 | 420 |

---

## 开发

```bash
# 编译全部
cargo build --workspace

# 运行全部测试 (420 tests)
cargo test --workspace

# Debug 日志
RUST_LOG=debug cargo run -p hakimi-cli

# Clippy 检查
cargo clippy --workspace
```

---

## 路线图

- [x] 核心 Agent 循环 + 工具分发
- [x] OpenAI / Anthropic / Gemini 传输
- [x] SSE 流式传输
- [x] 25 个内置工具
- [x] 8 个平台适配器
- [x] MCP 客户端 (stdio/HTTP/SSE)
- [x] 插件系统
- [x] ratatui TUI
- [x] SQLite 会话存储 + FTS5
- [x] 智能上下文压缩 (3层)
- [x] 错误分类器 (20+ 类型)
- [x] 凭证池 (多密钥轮换)
- [x] Prompt 缓存 (Anthropic)
- [x] Vision 分析
- [x] Checkpoint 回滚
- [x] Profiles 多配置
- [x] i18n 国际化
- [x] 批处理 + 检查点
- [ ] 知识图谱记忆 (petgraph)
- [ ] 元技能自动提炼
- [ ] 意图推理引擎
- [ ] 决策树回溯
- [ ] 角色自适应
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
