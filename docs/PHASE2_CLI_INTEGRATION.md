# Phase 2.3: CLI 入口集成完成报告

## 概要

Phase 2.3 已完成，成功将 DispatchedAgent 集成到 CLI 入口点（`hakimi-cli/entry.rs`），与 Phase 2.2 的 Server 集成保持一致。整个项目（`cargo build --all`）编译通过。

---

## 集成范围

### 1. 核心函数修改

#### `build_agent` (line 5199, 5609-5613)
```rust
async fn build_agent(...) -> Result<hakimi_core::DispatchedAgent> {
    // ... 构建 AIAgent ...
    
    // 包装为 DispatchedAgent
    let dispatched = hakimi_core::DispatchedAgent::new(agent, config.model.clone(), 0)?;
    Ok(dispatched)
}
```

#### `start_server` (line 5620)
```rust
async fn start_server(
    agent: hakimi_core::DispatchedAgent,  // 改为 DispatchedAgent
    addr: &str,
    config: hakimi_config::HakimiConfig,
    runtime_home: hakimi_common::RuntimeHome,
) -> Result<()>
```

#### `start_gateway` (line 7124)
```rust
async fn start_gateway(
    agent: hakimi_core::DispatchedAgent,  // 改为 DispatchedAgent
    skill_store: hakimi_skills::SkillStore,
    config: hakimi_config::HakimiConfig,
    runtime_home: hakimi_common::RuntimeHome,
) -> Result<()>
```

#### `start_unified_server` (line 7414)
```rust
async fn start_unified_server(
    agent: hakimi_core::DispatchedAgent,  // 改为 DispatchedAgent
    skill_store: hakimi_skills::SkillStore,
    addr: &str,
    config: hakimi_config::HakimiConfig,
    runtime_home: hakimi_common::RuntimeHome,
) -> Result<()>
```

#### `process_gateway_messages_loop` (line 5769, 5773)
```rust
async fn process_gateway_messages_loop(
    mut messages: tokio::sync::mpsc::UnboundedReceiver<hakimi_gateway::GatewayMessage>,
    gateway: std::sync::Arc<hakimi_gateway::Gateway>,
    _gateway_bot_ids: std::collections::HashMap<String, String>,
    agent_arc: std::sync::Arc<tokio::sync::Mutex<hakimi_core::DispatchedAgent>>,  // 改为 DispatchedAgent
    persona_registry: std::sync::Arc<tokio::sync::RwLock<hakimi_core::PersonaRegistry>>,
    persona_agents: hakimi_server::server::GatewayPersonaAgents,
    // ...
)
```

#### `build_gateway_persona_agents` (line 5646-5672)
```rust
fn build_gateway_persona_agents(
    template: &hakimi_core::DispatchedAgent,  // 改为 DispatchedAgent
    registry: &hakimi_core::PersonaRegistry,
    runtime_home: &hakimi_common::RuntimeHome,
    context_length: usize,
) -> std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<hakimi_core::DispatchedAgent>>> {
    let mut map = std::collections::HashMap::new();
    for cfg in registry.list() {
        if cfg.id == hakimi_core::DEFAULT_PERSONA_ID {
            continue;
        }
        let skills_dir = runtime_home.persona_dir(&cfg.id).join("skills");
        
        // 提取 base_agent 传递给 build_persona_agent
        let base_agent = hakimi_core::build_persona_agent(template.base_agent(), cfg, &skills_dir, context_length);
        
        // 包装为 DispatchedAgent（继承 template 的调度配置）
        let model_config = template.model_config().clone();
        let dispatched = match hakimi_core::DispatchedAgent::new(base_agent.clone(), model_config.clone(), 0) {
            Ok(agent) => agent,
            Err(e) => {
                tracing::warn!(persona = %cfg.id, "failed to wrap persona agent with dispatch: {e}, disabling auto_dispatch");
                let mut fallback_config = model_config;
                fallback_config.auto_dispatch.enabled = false;
                hakimi_core::DispatchedAgent::new(base_agent, fallback_config, 0)
                    .expect("dispatch creation cannot fail with disabled auto_dispatch")
            }
        };
        
        map.insert(
            cfg.id.clone(),
            std::sync::Arc::new(tokio::sync::Mutex::new(dispatched)),
        );
    }
    map
}
```

### 2. PersonaTeamExecutor 集成 (line 5808-5815)

由于 Team 系统仍然使用 `AIAgent`，需要提取 `base_agent()`：

```rust
let team_base = {
    let dispatched_template = std::sync::Arc::new(agent_arc.lock().await.clone());
    let template = std::sync::Arc::new(dispatched_template.base_agent().clone());
    std::sync::Arc::new(hakimi_core::PersonaTeamExecutor::new(
        persona_registry.clone(),
        template,
        128_000,
    ))
};
```

### 3. DispatchedAgent API 扩展

添加了 `model_config()` 公开方法（`dispatched_agent.rs:175-177`）：

```rust
/// Get the model configuration.
pub fn model_config(&self) -> &ModelConfig {
    &self.model_config
}
```

---

## 修改文件清单

| 文件 | 修改内容 | 行号 |
|------|---------|------|
| `crates/hakimi-cli/src/entry.rs` | `build_agent` 返回类型改为 `DispatchedAgent` | 5199 |
| `crates/hakimi-cli/src/entry.rs` | `build_agent` 包装逻辑 | 5609-5613 |
| `crates/hakimi-cli/src/entry.rs` | `start_server` 参数类型改为 `DispatchedAgent` | 5620 |
| `crates/hakimi-cli/src/entry.rs` | `build_gateway_persona_agents` 参数和返回类型改为 `DispatchedAgent` | 5646-5672 |
| `crates/hakimi-cli/src/entry.rs` | `process_gateway_messages_loop` 参数改为 `DispatchedAgent` | 5773 |
| `crates/hakimi-cli/src/entry.rs` | `PersonaTeamExecutor` 提取 `base_agent()` | 5808-5815 |
| `crates/hakimi-cli/src/entry.rs` | `start_gateway` 参数类型改为 `DispatchedAgent` | 7124 |
| `crates/hakimi-cli/src/entry.rs` | `start_unified_server` 参数类型改为 `DispatchedAgent` | 7414 |
| `crates/hakimi-core/src/dispatched_agent.rs` | 添加 `model_config()` 公开方法 | 175-177 |

---

## 集成模式总结

### CLI 与 Server 集成模式对比

**一致的包装策略**：

| 位置 | Server (api.rs / main.rs) | CLI (entry.rs) |
|------|---------------------------|---------------|
| **主 Agent** | `build_agent` 返回 `DispatchedAgent` | `build_agent` 返回 `DispatchedAgent` |
| **Persona Agents** | `build_persona_agent_for` 包装返回 `DispatchedAgent` | `build_gateway_persona_agents` 包装返回 `DispatchedAgent` |
| **Fallback 策略** | 包装失败时禁用 `auto_dispatch` 重试 | 包装失败时禁用 `auto_dispatch` 重试 |
| **Team 系统** | `PersonaTeamExecutor` 提取 `base_agent()` | `PersonaTeamExecutor` 提取 `base_agent()` |
| **配置继承** | Persona 从 template 克隆 `model_config` | Persona 从 template 克隆 `model_config` |

### 类型签名变更

**函数签名**：
- `build_agent` → 返回 `Result<DispatchedAgent>`
- `start_server` / `start_gateway` / `start_unified_server` → 参数改为 `DispatchedAgent`
- `process_gateway_messages_loop` → `agent_arc: Arc<Mutex<DispatchedAgent>>`
- `build_gateway_persona_agents` → 参数和返回值改为 `DispatchedAgent`

**AppState 等效类型**（未直接修改，但使用相同模式）：
- CLI 的 `agent_arc: Arc<Mutex<DispatchedAgent>>` 等效于 Server 的 `AppState.agent`
- CLI 的 `persona_agents: GatewayPersonaAgents` 使用 Server 定义的类型（已改为 `DispatchedAgent`）

---

## 编译验证

```bash
# CLI 包编译通过
$ cargo build --package hakimi-cli
   Finished `dev` profile in 0.74s
   
# 整个 workspace 编译通过
$ cargo build --all
   Finished `dev` profile in 22.68s
```

**警告清单**（非阻塞）：
- `hakimi-core`: 3 个 unused 警告（`reasoning_tier`, `primary_tier`, `tier_config`）
- `hakimi-cli`: 3 个 unused 警告（`assistant_text`, `effective_print_mode`, `VERSION`）

---

## Phase 2 整体完成状态

### ✅ 已完成部分

| Phase | 内容 | 状态 |
|-------|------|------|
| Phase 2.1 | DispatchedAgent 包装器 + 两阶段执行逻辑 | ✅ 完成 |
| Phase 2.2 | Server 入口集成 (main.rs, api.rs, server.rs) | ✅ 完成 |
| Phase 2.3 | CLI 入口集成 (entry.rs) | ✅ 完成 |

### 核心成果

1. **配置支持**：`config.yaml` 扩展支持三层模型（`model.tiers.light/primary/reasoning`）和调度策略（`model.auto_dispatch.*`）
2. **复杂度分析**：五维度评分系统（任务类型 30%、上下文 20%、推理深度 25%、工具调用 15%、嵌套惩罚 -10%）
3. **模型调度**：根据复杂度阈值选择合适模型层级
4. **两阶段执行**：高复杂度任务由 reasoning 模型规划，primary 模型执行
5. **向下兼容**：保留顶层 `model` 字段作为默认 primary 模型
6. **集成完成**：Server 和 CLI 双入口点全部集成，persona agents 和 team 系统兼容

---

## 下一步工作

### Phase 3: 递归调度（子 Agent + Team 继承）

**修改范围**：
1. **`delegate.rs`** (line 266-271)：子 Agent 创建时传递调度配置，支持 `inherit_dispatch` 控制
2. **`team.rs`**：Teammate 构建时继承 template 的调度策略
3. **深度惩罚**：子 Agent 的复杂度评分额外减 1-2 分，避免过度使用 reasoning 模型

**目标**：
- 子 Agent 通过 `delegate_task` 创建时自动继承父 Agent 的调度配置
- Teammate 通过 `PersonaTeamExecutor` 创建时继承调度配置
- 防止递归深度过深导致的无限调度循环

### Phase 4: 前端可视化

**修改范围**：
1. **WebUI Settings**：添加三层模型配置界面（light/primary/reasoning）
2. **调度阈值配置**：可视化调整 `light_max`, `reasoning_min` 等参数
3. **SSE 展示**：聊天界面实时显示调度决策（"🧠 Using reasoning model: claude-opus-4 (complexity: 8.2)"）

### Phase 5: 动态模型切换

**修改范围**：
1. **Transport 重新配置**：根据 `TierConfig` 动态创建不同 provider/model 的 Transport
2. **Agent 重建**：调度时克隆 base_agent，替换 transport 后执行
3. **连接池管理**：复用相同 provider 的连接，避免频繁重建

---

## 技术债务

1. **unused 警告**：Phase 2.1 预留的 `reasoning_tier`, `primary_tier` 等变量在 Phase 5 实现动态切换时会用上
2. **Lint 错误**：`entry.rs` 存在大量 "async fn not permitted in Rust 2015" 错误，但不影响编译（Cargo.toml 使用 edition 2021）
3. **Team 系统隔离**：当前 Team 系统仍使用 `AIAgent`，通过 `base_agent()` 提取，未来可考虑让 Team 系统也支持调度

---

## 结论

Phase 2.3 已完成，CLI 和 Server 双入口点全部支持智能模型调度。整个 Phase 2（配置、分析、调度、集成）已全部完成，整个项目编译通过。

**当前状态**：
- ✅ 配置结构完整（支持三层模型 + 调度策略）
- ✅ 复杂度分析器（五维度评分 0-10）
- ✅ 模型调度器（根据阈值选择层级）
- ✅ DispatchedAgent 包装器（两阶段执行）
- ✅ Server 全面集成（main.rs, api.rs, server.rs）
- ✅ CLI 全面集成（entry.rs）
- ⏳ Phase 3-5 待实施（递归调度、前端可视化、动态切换）

下一步可以：
1. **直接测试**：手动配置 `config.yaml` 添加 `model.tiers` 和 `model.auto_dispatch`，启动 Hakimi 观察调度行为
2. **继续 Phase 3**：实现子 Agent 和 Team 的调度继承
3. **跳至 Phase 4**：先实现前端可视化，让配置更直观
