# P1: SharedRuntime 抽取 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `AIAgent` 里 4 个共享重资源字段(`transport`、`tool_registry`、`knowledge_searcher`、`embedding_provider`)抽取到一个 `Arc<SharedRuntime>`,为后续 N 人格共享同一套运行时打地基,且不改变任何现有行为。

**Architecture:** 纯内部重构。新建 `SharedRuntime` 结构体持有 4 个字段;`AIAgent` 用 `pub shared: Arc<SharedRuntime>` 取代这 4 个字段。`AIAgentBuilder` 的 setter 与 `AIAgent` 的 getter / `with_*` 方法签名**全部保持不变**,只在 `build()` 末尾把 4 个字段打包进 `SharedRuntime`,并让两个构造后赋值方法经 `Arc::make_mut` 写入。因为外部 crate 和测试只通过 builder/getter/with_* 交互,所以波及面仅限 `hakimi-core` 内部的 `agent.rs` 和 `loop_impl.rs`。

**Tech Stack:** Rust 2024(nightly toolchain)、tokio、`Arc<dyn Trait>` 共享、cargo workspace。CI 用 `RUSTFLAGS="-Dwarnings"`,clippy/fmt 必须全绿。

---

## 范围与非目标

- **范围**:仅抽取上述 4 个字段。`context_engine`(每人格隔离)、`skill_store`(每人格隔离)、`messages`/`session_id`/`interrupt`/`tts_*` 等 per-agent 状态**保持留在 `AIAgent`**。
- **非目标**:本期不建 `PersonaRegistry`、不改路由、不动 API、不动 WebUI(那是 P2-P5)。凭证池 `CredentialPool`(`crates/hakimi-core/src/credential_pool.rs`)当前并不被 `AIAgent` 或 transport 持有,是独立模块,**不在本次抽取范围**。
- **验收**:`cargo test --workspace --all-features` 全绿(含原 990 行集成测试不改一行)、`cargo clippy --workspace --all-targets --all-features` 无警告、`cargo fmt --all -- --check` 通过、单人格行为零变化。

## File Structure

| 文件 | 动作 | 职责 |
|---|---|---|
| `crates/hakimi-core/src/shared.rs` | 新建 | 定义 `SharedRuntime`(全实例共享重资源,`#[derive(Clone)]`) |
| `crates/hakimi-core/src/lib.rs` | 修改(24 行) | 加 `pub mod shared;` + `pub use shared::SharedRuntime;` |
| `crates/hakimi-core/src/agent.rs` | 修改(748 行) | struct 字段、Clone、build()、两个 `with_*`、build_tool_context()、3 个 getter |
| `crates/hakimi-core/src/loop_impl.rs` | 修改(1064 行) | 4 处字段访问改为 `agent.shared.*` |
| `crates/hakimi-core/tests/integration.rs` | 修改(990 行) | 新增 1 个锚点测试,验证 `agent.shared` 可访问 |

---

## Task 1: 新建 SharedRuntime 结构体并导出

**Files:**
- Create: `crates/hakimi-core/src/shared.rs`
- Modify: `crates/hakimi-core/src/lib.rs`

- [ ] **Step 1: 创建 `shared.rs`**

新建文件 `crates/hakimi-core/src/shared.rs`,内容:

```rust
use std::sync::Arc;

use hakimi_tools::ToolRegistry;
use hakimi_transports::{EmbeddingProvider, ProviderTransport};

/// Resources shared across all personas (agents) in a single instance.
///
/// Constructed once and shared via [`Arc`] so that N personas can run
/// concurrently without duplicating heavy resources. Per-persona state
/// (model, system prompt, skills, context engine, messages) lives on
/// [`AIAgent`](crate::AIAgent) directly, not here.
///
/// Derives `Clone` (every field is `Arc`-backed or cheaply cloneable) so that
/// post-construction setters can mutate it in place via [`Arc::make_mut`].
#[derive(Clone)]
pub struct SharedRuntime {
    /// The LLM provider transport (connection + credential pool live inside).
    pub transport: Arc<dyn ProviderTransport>,
    /// The tool registry (its internals are already `Arc`-shared).
    pub tool_registry: ToolRegistry,
    /// Optional knowledge-graph searcher, shared across personas.
    pub knowledge_searcher: Option<Arc<dyn hakimi_common::KnowledgeSearcher>>,
    /// Optional embedding provider, shared across personas.
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
}
```

- [ ] **Step 2: 在 `lib.rs` 注册模块并 re-export**

修改 `crates/hakimi-core/src/lib.rs`。在模块声明区(现有 `pub mod agent;` 一行附近,按字母序在 `pub mod agent;` 之后)加入:

```rust
pub mod shared;
```

在 re-export 区(现有 `pub use agent::{AIAgent, AIAgentBuilder};` 之后)加入:

```rust
pub use shared::SharedRuntime;
```

- [ ] **Step 3: 编译验证**

Run: `cargo build -p hakimi-core`
Expected: 编译成功,无警告(`pub` 结构体经 `pub use` 导出,不会触发 dead_code;字段类型由 `transport`/`tool_registry` 等使用,不会触发 unused-import)。

- [ ] **Step 4: 提交**

```bash
git add crates/hakimi-core/src/shared.rs crates/hakimi-core/src/lib.rs
git commit -m "refactor(core): 新增 SharedRuntime 结构体(暂未接入)"
```

---

## Task 2: 将 AIAgent 改为持有 `Arc<SharedRuntime>`

这是一次原子重构:Rust 不允许"改一半"编译通过,所以先写红的锚点测试,再一次性改完所有点,最后整体转绿。

**Files:**
- Modify: `crates/hakimi-core/tests/integration.rs`
- Modify: `crates/hakimi-core/src/agent.rs` — import(`:14` 附近)、struct(`:23-24`、`:36-37`)、Clone(`:59-60`、`:72-73`)、两个 with_*(`:140-152`)、build()(`:404-434`)、build_tool_context()(`:629-645`)、getter(`:686`、`:691`、`:711`)
- Modify: `crates/hakimi-core/src/loop_impl.rs:118`、`:124`、`:180`、`:676`

- [ ] **Step 1: 写失败的锚点测试(RED)**

在 `crates/hakimi-core/tests/integration.rs` 文件末尾追加(沿用文件已有的 `MockTransport`、`make_context_engine()`、`AIAgent`、`Arc` 等导入):

```rust
#[tokio::test]
async fn test_agent_exposes_shared_runtime() {
    let transport = Arc::new(MockTransport::text_response("ok"));
    let agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-shared-runtime")
        .workdir("/tmp")
        .build()
        .unwrap();

    // 4 个共享资源现在统一挂在 agent.shared 上。
    assert_eq!(agent.shared.transport.provider_name(), "mock");
    assert!(agent.shared.knowledge_searcher.is_none());
    assert!(agent.shared.embedding_provider.is_none());
    // getter 兼容层仍然可用(外部 crate 依赖它)。
    assert_eq!(agent.provider_name(), "mock");
}
```

- [ ] **Step 2: 运行,确认编译失败(RED)**

Run: `cargo test -p hakimi-core --test integration test_agent_exposes_shared_runtime`
Expected: 编译失败,报错类似 `no field 'shared' on type '&AIAgent'`。这证明锚点有效。

- [ ] **Step 3: 在 `agent.rs` 顶部 import `SharedRuntime`**

修改 `crates/hakimi-core/src/agent.rs`,在 `use crate::trajectory::TrajectoryConfig;`(第 14 行附近)之后加一行:

```rust
use crate::shared::SharedRuntime;
```

- [ ] **Step 4: 改 struct 字段(agent.rs:23-24 与 36-37)**

把第 23-24 行:

```rust
    pub(crate) transport: Arc<dyn ProviderTransport>,
    pub(crate) tool_registry: ToolRegistry,
```

替换为:

```rust
    pub shared: Arc<SharedRuntime>,
```

再删除第 36-37 行(原 `knowledge_searcher` 与 `embedding_provider` 两行):

```rust
    pub(crate) knowledge_searcher: Option<Arc<dyn hakimi_common::KnowledgeSearcher>>,
    pub(crate) embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
```

- [ ] **Step 5: 改 Clone(agent.rs:59-60 与 72-73)**

把第 59-60 行:

```rust
            transport: self.transport.clone(),
            tool_registry: self.tool_registry.clone(),
```

替换为:

```rust
            shared: self.shared.clone(),
```

再删除第 72-73 行:

```rust
            knowledge_searcher: self.knowledge_searcher.clone(),
            embedding_provider: self.embedding_provider.clone(),
```

- [ ] **Step 6: 改两个构造后赋值方法(agent.rs:139-152)**

`with_embedding_provider` 与 `with_knowledge_searcher` 原本直接写 `self.embedding_provider`/`self.knowledge_searcher`,这两个字段已移入 `Arc<SharedRuntime>`,改为经 `Arc::make_mut` 写入(`SharedRuntime` 已 `#[derive(Clone)]`;这些方法在构造链上被调用时 Arc 引用计数为 1,`make_mut` 原地修改、不发生克隆)。把第 139-152 行:

```rust
    /// Set or replace the embedding provider.
    pub fn with_embedding_provider(mut self, provider: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        self.embedding_provider = provider;
        self
    }

    /// Set or replace the knowledge searcher.
    pub fn with_knowledge_searcher(
        mut self,
        searcher: Option<Arc<dyn hakimi_common::KnowledgeSearcher>>,
    ) -> Self {
        self.knowledge_searcher = searcher;
        self
    }
```

替换为:

```rust
    /// Set or replace the embedding provider (stored in the shared runtime).
    pub fn with_embedding_provider(mut self, provider: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        Arc::make_mut(&mut self.shared).embedding_provider = provider;
        self
    }

    /// Set or replace the knowledge searcher (stored in the shared runtime).
    pub fn with_knowledge_searcher(
        mut self,
        searcher: Option<Arc<dyn hakimi_common::KnowledgeSearcher>>,
    ) -> Self {
        Arc::make_mut(&mut self.shared).knowledge_searcher = searcher;
        self
    }
```

(注:`with_context_engine`(:134)、`with_skill_store`(:129)、`with_voice_settings`(:156)操作的都是仍留在 `AIAgent` 上的 per-agent 字段,**不需要改**。)

- [ ] **Step 7: 改 build() 组装 SharedRuntime(agent.rs:404-434)**

在 `build()` 里,`let workdir = self.workdir.unwrap_or_else(|| ".".to_string());`(第 404 行)之后、`info!(`(第 406 行)之前,插入:

```rust
        let shared = Arc::new(SharedRuntime {
            transport,
            tool_registry,
            knowledge_searcher: self.knowledge_searcher,
            embedding_provider: self.embedding_provider,
        });
```

然后修改 `Ok(AIAgent { ... })` 字面量:把第 416-417 行:

```rust
            transport,
            tool_registry,
```

替换为:

```rust
            shared,
```

并删除第 429-430 行:

```rust
            knowledge_searcher: self.knowledge_searcher,
            embedding_provider: self.embedding_provider,
```

(注:`transport`、`tool_registry` 是 build() 内的本地绑定;`self.knowledge_searcher`、`self.embedding_provider` 由 self 移入。组装放在 `info!` 前,不影响日志。)

- [ ] **Step 8: 改 build_tool_context(agent.rs:629、632、645)**

把第 629 行 `self.transport.clone(),` 改为 `self.shared.transport.clone(),`;
把第 632 行 `self.tool_registry.clone(),` 改为 `self.shared.tool_registry.clone(),`;
把第 645 行 `knowledge_searcher: self.knowledge_searcher.clone(),` 改为 `knowledge_searcher: self.shared.knowledge_searcher.clone(),`。

改完该函数对应片段为:

```rust
        let delegate_executor: Option<Arc<dyn hakimi_common::DelegateExecutor>> =
            Some(Arc::new(crate::CoreDelegateExecutor::new(
                self.shared.transport.clone(),
                self.context_engine.clone(),
                self.model.clone(),
                self.shared.tool_registry.clone(),
                self.workdir.clone(),
                self.skill_store.clone(),
                self.streaming_callback.clone(),
            )));
```

以及 `ToolContext { ... }` 内的:

```rust
            knowledge_searcher: self.shared.knowledge_searcher.clone(),
```

- [ ] **Step 9: 改 3 个 getter(agent.rs:686、691、711)**

把第 686 行 `self.transport.provider_name()` 改为 `self.shared.transport.provider_name()`;
把第 691 行 `self.transport.rate_limits()` 改为 `self.shared.transport.rate_limits()`;
把第 711 行 `&self.tool_registry` 改为 `&self.shared.tool_registry`。

改完三处为:

```rust
    pub fn provider_name(&self) -> &str {
        self.shared.transport.provider_name()
    }

    pub fn rate_limits(&self) -> Option<hakimi_transports::RateLimitState> {
        self.shared.transport.rate_limits()
    }

    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.shared.tool_registry
    }
```

- [ ] **Step 10: 改 loop_impl.rs 的 4 处访问(loop_impl.rs:118、124、180、676)**

把第 117-124 行:

```rust
    agent
        .tool_registry
        .configure_tool_search(
            agent.tool_search_config.clone(),
            agent.tool_search_context_length,
        )
        .await;
    let tool_assembly = agent.tool_registry.get_model_definitions().await;
```

替换为:

```rust
    agent
        .shared
        .tool_registry
        .configure_tool_search(
            agent.tool_search_config.clone(),
            agent.tool_search_context_length,
        )
        .await;
    let tool_assembly = agent.shared.tool_registry.get_model_definitions().await;
```

把第 180 行 `agent.transport.as_ref(),` 改为 `agent.shared.transport.as_ref(),`;
把第 676 行 `let registry = agent.tool_registry.clone();` 改为 `let registry = agent.shared.tool_registry.clone();`。

(注:`configure_tool_search`/`get_model_definitions`/`register` 均为 `&self`(`ToolRegistry` 内部 `Arc<RwLock>` 做内部可变),所以经 `Arc<SharedRuntime>` 的不可变引用调用合法。)

- [ ] **Step 11: 编译并跑锚点测试(GREEN)**

Run: `cargo test -p hakimi-core --test integration test_agent_exposes_shared_runtime`
Expected: PASS。

- [ ] **Step 12: 跑 hakimi-core 全量测试(回归)**

Run: `cargo test -p hakimi-core --all-features`
Expected: 全绿,原有 990 行集成测试与各内联单测无一改动、无一失败。

- [ ] **Step 13: clippy 与 fmt**

Run: `cargo clippy -p hakimi-core --all-targets --all-features`
Expected: 无警告(CI 用 `-Dwarnings`,有警告即失败)。

Run: `cargo fmt -p hakimi-core`
然后 `cargo fmt --all -- --check` 应通过。

- [ ] **Step 14: 提交**

```bash
git add crates/hakimi-core/src/agent.rs crates/hakimi-core/src/loop_impl.rs crates/hakimi-core/tests/integration.rs
git commit -m "refactor(core): AIAgent 改为持有 Arc<SharedRuntime>"
```

---

## Task 3: 全 workspace 验收闸门

无新增代码改动,确认外部 crate(server/cli/tui/mcp 等)因 builder/getter/with_* 签名未变而零改动即可通过。

**Files:** 无(仅验证)

- [ ] **Step 1: 全量编译**

Run: `cargo build --workspace`
Expected: 全部 17 个 crate 编译成功。若 server(`main.rs:420-428` builder 构造)或 cli(`entry.rs:5517` `AIAgent::new(...).with_embedding_provider(...).with_knowledge_searcher(...)` 构造)报错,说明 builder/`new`/getter/`with_*` 签名被意外改动,需回到 Task 2 修正(它们的签名本计划要求保持不变)。

- [ ] **Step 2: 全量测试**

Run: `cargo test --workspace --all-features`
Expected: 全绿。

- [ ] **Step 3: 全量 clippy(对齐 CI)**

Run: `cargo clippy --workspace --all-targets --all-features`
Expected: 无警告。

- [ ] **Step 4: fmt 检查(对齐 CI)**

Run: `cargo fmt --all -- --check`
Expected: 无 diff。如有,先 `cargo fmt --all` 再提交。

- [ ] **Step 5: 提交(若 fmt 产生改动)**

```bash
git add -A
git commit -m "style: fmt 对齐(P1 SharedRuntime 抽取收尾)"
```

若 Step 4 无 diff 则跳过本步;P1 完成。

---

## 自检对照(spec 覆盖)

- spec §3.1 组件模型(SharedRuntime)→ Task 1 + Task 2 ✓
- spec §3.7 改造点 `agent.rs`/`loop_impl.rs`/`server.rs`/`entry.rs` → Task 2(core 内部)+ Task 3(验证外部零改动)✓
- spec §8 实现分期 P1「抽取 SharedRuntime,保持单人格行为,测试转绿」→ 本计划整体 ✓
- `context_engine`/`skill_store` 保持隔离(不进 SharedRuntime)→ Task 2 struct 设计 ✓
- 凭证池不在范围 → 范围与非目标节已声明 ✓
- 构造后赋值方法 `with_embedding_provider`/`with_knowledge_searcher` 经 `Arc::make_mut` 兼容 → Task 2 Step 6 ✓
```

