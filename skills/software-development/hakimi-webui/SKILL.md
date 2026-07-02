---
name: hakimi-webui
description: "Build and maintain Hakimi Agent's WebUI: Rust/Axum backend + React/TypeScript frontend, intelligent model dispatch, multi-persona support, gateway integration."
version: 1.0.0
author: Hakimi Agent Team
license: MIT
platforms: [linux]
metadata:
  tags: [hakimi, rust, webui, agent, development, multi-agent, gateway]
  homepage: https://github.com/Mouseww/hakimi-agent
  related_skills: [hermes-agent, rust-workspace-development]
---

# Hakimi Agent WebUI

**Hakimi Agent** 是一个用 Rust 构建的智能 AI Agent 框架，具有完整的 WebUI、多 Agent 隔离、智能模型调度和跨平台 Gateway 集成。它的目标是在功能深度、稳定性和用户体验上**对齐并超越 Hermes Agent**。

本技能帮助你**理解 Hakimi 的架构、配置、开发和调试**，涵盖：
- WebUI 前后端架构（Axum + React）
- 智能模型调度系统（轻量/主力/高级思考模型三层架构）
- 多 Persona 系统（隔离配置、Channel Bindings）
- Gateway 集成（Telegram、Discord、Slack 等）
- 开发工作流（编译、测试、发布）

---

## 快速开始

```bash
# 克隆仓库
git clone https://github.com/Mouseww/hakimi-agent
cd hakimi-agent

# 构建（Release 模式）
cargo build --release

# 安装到系统
sudo cp target/release/hakimi /usr/local/bin/

# 初始化配置
hakimi init

# 启动 WebUI（统一模式：Gateway + WebUI）
hakimi --serve --addr 127.0.0.1:3005

# 或者仅启动 WebUI（无 Gateway）
hakimi --webui-only --addr 127.0.0.1:3005
```

---

## 架构概览

### 核心组件

```
hakimi-agent/
├── crates/
│   ├── hakimi-core/        # Agent 核心逻辑
│   │   ├── agent.rs          # AIAgent + AIAgentBuilder
│   │   ├── dispatched_agent.rs  # 智能模型调度包装器
│   │   ├── persona_registry.rs  # 多 Persona 管理
│   │   └── team.rs           # Team（多 Agent 协作）
│   ├── hakimi-server/      # Axum WebUI 后端
│   │   ├── api.rs            # REST API 路由
│   │   └── server.rs         # Server 启动逻辑
│   ├── hakimi-gateway/     # 跨平台消息 Gateway
│   │   ├── telegram.rs       # Telegram adapter
│   │   └── ...
│   ├── hakimi-transports/  # LLM Provider 适配器
│   │   ├── anthropic.rs      # Anthropic Claude
│   │   ├── chat_completions.rs  # OpenAI-compatible
│   │   └── ...
│   ├── hakimi-tools/       # 工具库（terminal、file、web 等）
│   ├── hakimi-config/      # 配置管理
│   ├── hakimi-session/     # SQLite 会话存储
│   ├── hakimi-context/     # 上下文压缩引擎
│   └── hakimi-skills/      # 技能系统
├── hakimi-webui/           # React + TypeScript 前端
│   ├── src/
│   │   ├── Chat.tsx          # 聊天界面
│   │   ├── SettingsPanel.tsx # 配置面板
│   │   └── api.ts            # API 客户端
│   └── package.json
├── Cargo.toml              # Workspace 配置
└── README.md
```

### 关键概念

#### 1. **智能模型调度 (Intelligent Model Dispatch)**

Hakimi 支持**三层模型架构**，根据任务复杂度自动选择合适的模型：

- **轻量模型 (Light)**: 文件检索、简单查询、快速响应场景（如 `gpt-4o-mini`）
- **主力模型 (Primary)**: 常规开发任务、代码生成、问题解决（如 `gpt-4o`）
- **高级思考模型 (Reasoning)**: 复杂架构设计、系统规划、难题诊断（如 `o1`）

**配置示例 (`~/.hakimi/config.toml`)**:

```toml
[model]
default = "gpt-4o"
provider = "openrouter"
base_url = "https://router.goldras.edu.kg/v1"
api_key = "sk-xxx"

[model.tiers]
enabled = true

[model.tiers.primary]
model = "gpt-4o"
provider = "openrouter"
base_url = "https://router.goldras.edu.kg/v1"
api_key = ""  # 留空则使用顶层 api_key

[model.tiers.light]
model = "gpt-4o-mini"
provider = "openrouter"
# base_url 留空则使用 provider 默认值

[model.tiers.reasoning]
model = "o1"
provider = "openrouter"
```

**调度决策通过 SSE 推送**（前端实时显示）：

```
🧠 Dispatching to reasoning tier (model: o1)
   Task complexity: architectural design
```

#### 2. **多 Persona 系统 (Multi-Persona Isolation)**

每个 Persona 拥有独立的：
- 系统提示词 (system_prompt)
- 技能集 (enabled_skills)
- 会话历史 (独立 SQLite 数据库)
- Channel Bindings (平台消息路由)

**Persona 配置文件结构**：

```
~/.hakimi/agents/
├── registry.yaml           # Persona 索引
├── default/
│   ├── persona.yaml        # 默认 Persona 配置
│   ├── MEMORY.md           # 持久化记忆
│   └── sessions.db         # 会话数据库
├── architect/
│   ├── persona.yaml
│   └── ...
└── backend-dev/
    ├── persona.yaml
    └── ...
```

**registry.yaml 示例**：

```yaml
default: default
personas:
  - default
  - architect
  - backend-dev
```

**persona.yaml 示例**：

```yaml
id: architect
name: "System Architect"
avatar: "🏗️"
description: "Specialized in system design and architecture"
is_default: false
addressable: true

model:
  default: "o1"
  provider: "openrouter"

system_prompt: |
  You are a senior system architect...

enabled_skills:
  - architectural-design
  - system-planning

bindings:
  - "telegram:bot123:chat456"
```

#### 3. **Gateway 集成**

Hakimi Gateway 支持多平台：
- Telegram
- Discord
- Slack
- Matrix
- WhatsApp (via Twilio)
- Signal

**启动模式**：

1. **统一模式** (推荐): Gateway + WebUI 在同一进程
   ```bash
   hakimi --serve --addr 127.0.0.1:3005
   ```

2. **分离模式**: Gateway 和 WebUI 独立运行
   ```bash
   # Terminal 1: Gateway
   hakimi --gateway-only
   
   # Terminal 2: WebUI
   hakimi --webui-only --addr 127.0.0.1:3005
   ```

**Systemd 服务**（统一模式）:

```ini
[Unit]
Description=Hakimi Agent (Unified: Gateway + WebUI)
After=network.target

[Service]
Type=simple
User=hakimi
ExecStart=/root/.hakimi/bin/hakimi --serve --addr 127.0.0.1:3005
Restart=always
RestartSec=10s

[Install]
WantedBy=multi-user.target
```

---

## CLI 命令参考

### 基础命令

```bash
# 初始化配置
hakimi init

# 聊天模式（CLI）
hakimi chat

# 配置向导
hakimi setup

# 健康检查
hakimi doctor

# 查看版本
hakimi --version
```

### WebUI 相关

```bash
# 启动 WebUI（统一模式）
hakimi --serve --addr 127.0.0.1:3005

# 仅 WebUI（无 Gateway）
hakimi --webui-only --addr 127.0.0.1:3005

# 自定义端口
hakimi --serve --addr 0.0.0.0:8080
```

### Gateway 相关

```bash
# 启动 Gateway
hakimi gateway run

# 安装为 Systemd 服务
hakimi gateway install

# 控制服务
systemctl --user start hakimi-gateway
systemctl --user stop hakimi-gateway
systemctl --user restart hakimi-gateway
systemctl --user status hakimi-gateway

# 查看日志
journalctl --user -u hakimi-gateway -f
```

### Persona 管理

```bash
# 列出所有 Persona
curl http://127.0.0.1:3005/api/agents

# 创建 Persona
curl -X POST http://127.0.0.1:3005/api/agents   -H "Content-Type: application/json"   -d '{
    "id": "backend-dev",
    "name": "Backend Developer",
    "avatar": "💻",
    "system_prompt": "You are a backend developer...",
    "enabled_skills": ["rust-development"]
  }'

# 更新 Persona
curl -X PATCH http://127.0.0.1:3005/api/agents/backend-dev   -H "Content-Type: application/json"   -d '{"name": "Senior Backend Developer"}'

# 删除 Persona
curl -X DELETE http://127.0.0.1:3005/api/agents/backend-dev
```

---

## API 参考

### WebUI API 端点

#### 配置管理

- **GET `/api/config`** — 获取当前配置
- **POST `/api/config`** — 更新配置

#### 聊天

- **GET `/api/chat/stream`** (SSE) — 流式聊天
  - Query params: `message`, `session_id`, `persona_id` (optional)

#### Persona 管理

- **GET `/api/agents`** — 列出所有 Persona
- **POST `/api/agents`** — 创建 Persona
- **GET `/api/agents/{id}`** — 获取 Persona 详情
- **PATCH `/api/agents/{id}`** — 更新 Persona
- **DELETE `/api/agents/{id}`** — 删除 Persona

#### 会话

- **GET `/api/agents/{id}/sessions`** — 获取 Persona 的会话列表

#### 技能

- **GET `/api/agents/{id}/skills`** — 获取 Persona 的可用技能

---

## 开发工作流

### 编译 & 测试

```bash
# 开发模式编译
cargo build

# Release 模式（优化）
cargo build --release

# 运行测试
cargo test

# Clippy 检查
cargo clippy --all-targets --all-features -- -D warnings

# 格式化代码
cargo fmt --all
```

### 前端开发

```bash
cd hakimi-webui

# 安装依赖
npm install

# 开发服务器（热重载）
npm run dev

# 构建生产版本
npm run build

# TypeScript 类型检查
npx tsc --noEmit
```

### 发布流程

**自动化发布**（GitHub Actions）：

1. 更新版本号：`Cargo.toml` → `version = "0.3.x"`
2. 创建 Git tag：`git tag v0.3.x`
3. 推送：`git push --tags`
4. GitHub Actions 自动触发：
   - 编译 Linux musl 静态二进制
   - 运行 Clippy + 测试
   - 创建 GitHub Release
   - 上传 `hakimi-linux-x86_64.tar.gz`

**手动发布**：

```bash
# 1. 更新版本号
# 编辑 Cargo.toml

# 2. 提交并打 tag
git add -A
git commit -m "chore: bump version to v0.3.x"
git tag v0.3.x
git push origin main
git push origin v0.3.x

# 3. 监控 CI
# https://github.com/Mouseww/hakimi-agent/actions
```

---

## 常见问题排查

### 1. 编译错误

#### GLIBC 版本不兼容

**症状**: 部署到 EL9 服务器时提示 "version `GLIBC_2.XX' not found"

**原因**: CI 使用 ubuntu-latest (GLIBC 2.35) 编译，但目标服务器是 EL9 (GLIBC 2.34)

**解决方案**:

- **方案 A**: CI 使用 `ubuntu-20.04` (GLIBC 2.31，向下兼容)
  ```yaml
  # .github/workflows/release.yml
  runs-on: ubuntu-20.04
  ```

- **方案 B**: 使用 musl 静态链接（推荐）
  ```bash
  rustup target add x86_64-unknown-linux-musl
  cargo build --release --target x86_64-unknown-linux-musl
  ```

#### Clippy 警告阻塞 CI

**症状**: `cargo clippy` 返回非零退出码

**解决方案**:

```bash
# 本地修复所有 warnings
cargo clippy --fix --all-targets --all-features --allow-dirty

# 自动格式化
cargo fmt --all

# 验证
cargo clippy --all-targets --all-features -- -D warnings
```

### 2. Gateway 问题

#### 双重回复（真假猴王）

**症状**: Telegram 消息收到两条相同回复

**原因**: 后台 systemd 服务和本地测试进程同时运行

**解决方案**:

```bash
# 停止所有 hakimi 进程
systemctl --user stop hakimi-gateway
pkill -f hakimi

# 确认没有残留进程
ps aux | grep hakimi
```

#### Gateway 无响应

**症状**: 发送消息后没有任何反应

**排查步骤**:

1. 检查进程状态
   ```bash
   systemctl --user status hakimi-gateway
   ```

2. 查看日志
   ```bash
   journalctl --user -u hakimi-gateway -f
   ```

3. 检查配置
   ```bash
   cat ~/.hakimi/config.toml
   ```

4. 验证 Bot Token
   ```bash
   curl "https://api.telegram.org/bot<TOKEN>/getMe"
   ```

### 3. WebUI 问题

#### 端口冲突

**症状**: `Address already in use (os error 98)`

**原因**: 两个服务同时绑定 3005 端口

**解决方案**:

```bash
# 查看占用端口的进程
lsof -i :3005

# 停止冲突的服务
systemctl --user stop hakimi-webui  # 如果使用分离模式
systemctl --user stop hakimi        # 如果使用统一模式
```

#### 前端连接 502

**症状**: 浏览器提示 "Bad Gateway"

**排查**:

1. 确认后端运行
   ```bash
   curl http://127.0.0.1:3005/api/config
   ```

2. 检查 Nginx/Caddy 配置（如果使用反向代理）

3. 查看后端日志
   ```bash
   journalctl --user -u hakimi -f
   ```

### 4. 模型调度问题

#### 调度决策不显示

**症状**: WebUI 没有显示 "Dispatching to X tier" 消息

**原因**: SSE 流式输出被缓冲

**解决方案**:

检查 `dispatched_agent.rs` 中的 `emit_tier_decision()` 是否正确调用了 `streaming_callback`。

#### API key 泄露

**症状**: 前端 Settings 面板显示完整 API key

**预期行为**: 非空 API key 应该显示为 `••••••••`

**修复**:

检查 `api.rs` 中的 `get_config` handler 是否正确掩码处理：

```rust
api_key: if tier_config.api_key.is_empty() {
    String::new()
} else {
    "••••••••".to_string()
}
```

---

## 技术细节

### 智能模型调度实现

**核心文件**: `crates/hakimi-core/src/dispatched_agent.rs`

**关键类型**:

```rust
pub struct DispatchedAgent {
    base_agent: AIAgent,
    config: Arc<Config>,
    dispatch_strategy: DispatchStrategy,
}

pub enum DispatchStrategy {
    Single,      // 单一模型（向下兼容）
    TwoStage {   // 双阶段：reasoning → primary
        reasoning_threshold: f32,
    },
}
```

**调度流程**:

1. 用户消息进入 `run_conversation()`
2. 分析任务复杂度（关键词匹配 + 消息长度）
3. 选择 tier：
   - 轻量任务 → `Light` tier
   - 复杂任务 → `Reasoning` tier → `Primary` tier
   - 普通任务 → `Primary` tier
4. 动态创建对应 tier 的 `AIAgent`（通过 `create_agent_for_tier()`）
5. 执行对话循环
6. 返回结果

**SSE 推送**:

```rust
self.emit_tier_decision("reasoning", "architectural design");
// 前端收到 SSE event:
// event: tier_decision
// data: {"tier":"reasoning","reason":"architectural design"}
```

### 多 Persona 架构

**核心文件**: `crates/hakimi-core/src/persona_registry.rs`

**关键方法**:

```rust
impl PersonaRegistry {
    pub fn load(agents_dir: PathBuf) -> Result<Self> { ... }
    pub fn create(&mut self, cfg: PersonaConfig) -> Result<()> { ... }
    pub fn update(&mut self, cfg: PersonaConfig) -> Result<()> { ... }
    pub fn delete(&mut self, id: &str) -> Result<()> { ... }
    pub fn resolve_binding(&self, platform: &str, bot_id: &str) -> &str { ... }
    pub fn persist(&self) -> Result<()> { ... }
}
```

**启动流程**:

1. `PersonaRegistry::load()` 从 `~/.hakimi/agents/registry.yaml` 加载
2. 对每个 persona id，加载 `~/.hakimi/agents/{id}/persona.yaml`
3. 构建 `binding_index`（`platform:bot_id` → `persona_id`）
4. Gateway 收到消息时，通过 `resolve_binding()` 路由到对应 Persona

**持久化**:

- 所有 CRUD 操作（`create`, `update`, `delete`）都调用 `persist()`
- `persist()` 写入所有 `persona.yaml` 和 `registry.yaml`

---

## 最佳实践

### 1. 开发环境设置

```bash
# 推荐的开发工具链
rustup default stable
rustup component add clippy rustfmt

# 安装 cargo-watch（自动重新编译）
cargo install cargo-watch

# 监听文件变化并重新运行
cargo watch -x 'run -- --serve --addr 127.0.0.1:3005'
```

### 2. 调试技巧

```bash
# 启用详细日志
RUST_LOG=debug hakimi --serve

# 只看特定模块的日志
RUST_LOG=hakimi_core=trace,hakimi_server=debug hakimi --serve

# 查看 Backtrace
RUST_BACKTRACE=1 hakimi --serve
```

### 3. 生产部署

```bash
# 1. 使用 systemd 管理进程
sudo cp hakimi.service /etc/systemd/system/
sudo systemctl enable hakimi
sudo systemctl start hakimi

# 2. 配置反向代理（Nginx）
server {
    listen 80;
    server_name hakimi.example.com;
    
    location / {
        proxy_pass http://127.0.0.1:3005;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}

# 3. 配置 HTTPS（Let's Encrypt）
certbot --nginx -d hakimi.example.com
```

### 4. 性能优化

```bash
# 启用 LTO（Link-Time Optimization）
# Cargo.toml:
[profile.release]
lto = true
codegen-units = 1

# 使用 jemalloc（更好的内存分配器）
# Cargo.toml:
[dependencies]
jemallocator = "0.5"

# main.rs:
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;
```

---

## 扩展开发

### 添加新的 Tool

1. 在 `crates/hakimi-tools/src/` 创建新文件
2. 实现 `Tool` trait
3. 在 `registry.rs` 注册

**示例**:

```rust
// crates/hakimi-tools/src/example_tool.rs
use hakimi_common::HakimiError;

pub async fn example_tool(
    param: String,
) -> Result<String, HakimiError> {
    Ok(format!("Result: {}", param))
}

// 在 registry.rs 注册
registry.register(
    "example_tool",
    ToolSchema {
        name: "example_tool".to_string(),
        description: "An example tool".to_string(),
        parameters: ...
    },
    |args| Box::pin(example_tool(args.get("param").unwrap().to_string())),
);
```

### 添加新的 Gateway 平台

1. 在 `crates/hakimi-gateway/src/platforms/` 创建适配器
2. 实现 `PlatformAdapter` trait
3. 在 `gateway/mod.rs` 注册

---

## 参考资料

- **GitHub 仓库**: https://github.com/Mouseww/hakimi-agent
- **Hermes Agent**: https://github.com/NousResearch/hermes-agent
- **Rust 异步编程**: https://rust-lang.github.io/async-book/
- **Axum 文档**: https://docs.rs/axum/
- **React 文档**: https://react.dev/

---

## 贡献指南

1. Fork 仓库
2. 创建 feature 分支：`git checkout -b feature/amazing-feature`
3. 提交更改：`git commit -m 'Add amazing feature'`
4. 推送到分支：`git push origin feature/amazing-feature`
5. 创建 Pull Request

**代码风格**:

- Rust: 遵循 `rustfmt` 和 `clippy` 规范
- TypeScript: 遵循 ESLint 规则
- 所有 PR 必须通过 CI 检查

---

## License

MIT License - 详见 LICENSE 文件
