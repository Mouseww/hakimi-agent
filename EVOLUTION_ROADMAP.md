# Hakimi Agent 进化路线图

> **目标**: 让 Hakimi 在功能深度、稳定性和多平台支持上全面超越 Hermes Agent

**当前版本**: v0.5.56  
**上次更新**: 2026-07-09  
**维护模式**: 自动化进化引擎（Always Approve 技术性修复）

---

## 🎯 北极星指标

| 维度 | Hermes 基准 | Hakimi 当前 | 目标 |
|------|-------------|------------|------|
| **记忆能力** | 插件化 + 8钩子 | 分级3层 + 动态检索 | ✅ **已对标** |
| **会话搜索** | 3模式 + lineage | 3模式 (无lineage) | ⚠️ **90%** |
| **上下文管理** | 70% 阈值 + 固定窗口 | 同左 + SmartEngine | ✅ **已对齐** |
| **测试覆盖率** | ~60% | ~75% | 🎯 **85%+** |
| **WebUI 体验** | N/A | Gateway配置界面 | 🎯 **超越** |
| **流式输出** | 假流式（缓冲） | chunk-by-chunk 真实时 | 🎯 **超越** |
| **跨平台支持** | 6平台 | 6平台 (musl静态) | ✅ **已对齐** |

---

## 📅 迭代计划（3个月窗口）

### Phase 1: 稳定性与可观测性 (Week 1-2)

**目标**: 生产级质量保障，防止静默失败

#### 里程碑 1.1: 日志与监控增强 (3天)
- [ ] **任务 1.1.1**: 为所有核心路径添加 tracing spans
  - 文件: `crates/hakimi-session/src/message_ops.rs`
  - 方法: `get_messages_around()`, `get_bookends()`, `search_messages()`
  - 指标: 查询耗时、结果数量、会话ID
  - 验收: 日志可通过 `RUST_LOG=hakimi_session=debug` 观察

- [ ] **任务 1.1.2**: 添加关键性能 metrics
  - 文件: 新建 `crates/hakimi-telemetry/`
  - 集成: Prometheus + OpenTelemetry (可选)
  - 指标: 
    - `session_search_duration_seconds{mode="discovery|scroll|browse"}`
    - `memory_load_bytes{target="user|memory|working"}`
    - `context_compression_ratio`
  - 验收: `/metrics` 端点返回 Prometheus 格式

- [ ] **任务 1.1.3**: 错误追踪与报警
  - 文件: `crates/hakimi-common/src/error.rs`
  - 定义: `SessionError`, `MemoryError`, `ContextError` 自定义类型
  - 上下文: 所有错误携带 `session_id`, `user_id`, `timestamp`
  - 验收: 错误日志包含完整调试信息

#### 里程碑 1.2: 工作记忆生命周期管理 (2天)
- [ ] **任务 1.2.1**: 实现会话结束时自动清理
  - 文件: `crates/hakimi-context/src/memory.rs`
  - 逻辑: 
    ```rust
    pub fn finalize_session(&self) -> Result<()> {
        // 1. 读取 working_memory.md
        // 2. 如果非空，追加到 memory.md（带时间戳）
        // 3. 清空 working_memory.md
        // 4. 记录日志
    }
    ```
  - 触发点: Gateway 收到 `/new` 命令或会话超时
  - 验收: 新会话开始时 working_memory.md 为空

- [ ] **任务 1.2.2**: 添加记忆容量监控
  - 文件: `crates/hakimi-context/src/memory.rs`
  - 逻辑: 
    - 每次加载记忆时检查文件大小
    - `> 60KB`: WARN 日志
    - `> 64KB`: 拒绝加载 + 返回错误提示用户清理
  - 验收: 测试用例验证限制生效

- [x] **任务 1.2.3**: 记忆归档机制
  - 文件: 新建 `~/.hakimi/memory/archive/`
  - 命令: `hakimi memory archive [--before 2026-06-01]`
  - 逻辑: 移动指定日期前的记忆到归档，保留引用索引
  - 验收: CLI 命令成功归档并显示统计

#### 里程碑 1.3: 测试覆盖率提升到 80% (2天)
- [ ] **任务 1.3.1**: 补充 session_search 工具集成测试
  - 文件: `crates/hakimi-tools/src/builtin_session_search.rs`
  - 测试: 
    - Discovery 模式 + bookends 完整性
    - Scroll 边界检测（首尾）
    - Browse 排序正确性
    - FTS5 中文分词（如适用）
  - 验收: `cargo test --package hakimi-tools session_search` 全通过

- [x] **任务 1.3.2**: 添加 memory 工具错误路径测试
  - 文件: `crates/hakimi-tools/src/builtin_memory.rs`
  - 测试:
    - 记忆文件不存在
    - 权限拒绝
    - 内容超大（>64KB）
    - 并发写入冲突
  - 验收: 错误处理优雅（不 panic）

- [x] **任务 1.3.3**: 压力测试与边界测试
  - 文件: `crates/hakimi-session/tests/stress_test.rs`
  - 场景:
    - 10K 消息会话的搜索性能
    - 100 并发会话创建
    - 单次查询返回 1K+ 结果
  - 验收: 无 panic，响应时间 < 500ms (P95)
  - **完成**: v0.5.63, PR #18

---

### Phase 2: 功能完整性对齐 (Week 3-4)

**目标**: 补齐与 Hermes 的功能差距

#### 里程碑 2.1: Lineage 父子会话关系 (4天)
- [ ] **任务 2.1.1**: 数据库 schema 扩展
  - 文件: `crates/hakimi-session/src/schema.rs`
  - 字段: 
    ```sql
    ALTER TABLE sessions ADD COLUMN parent_id TEXT;
    ALTER TABLE sessions ADD COLUMN root_id TEXT;
    CREATE INDEX idx_lineage ON sessions(parent_id, root_id);
    ```
  - 迁移: 添加 migration 脚本（版本 v2）
  - 验收: 旧数据库自动升级

- [ ] **任务 2.1.2**: Lineage 查询 API
  - 文件: `crates/hakimi-session/src/session_ops.rs`
  - 方法:
    ```rust
    fn get_session_lineage(&self, session_id: &str) -> Result<Vec<SessionMetadata>>;
    fn get_root_session(&self, session_id: &str) -> Result<SessionMetadata>;
    fn get_child_sessions(&self, session_id: &str) -> Result<Vec<SessionMetadata>>;
    ```
  - 验收: 单元测试覆盖 3 代会话树

- [ ] **任务 2.1.3**: session_search 集成 lineage
  - 文件: `crates/hakimi-tools/src/builtin_session_search.rs`
  - 功能: Discovery 模式去重时优先保留 root 会话
  - 参数: 新增 `include_lineage: bool`（默认 true）
  - 验收: 搜索结果显示会话层级关系

- [ ] **任务 2.1.4**: WebUI 可视化会话树
  - 文件: `crates/hakimi-webui/src/components/SessionTree.tsx`
  - 展示: 树形结构 + 折叠/展开
  - 交互: 点击跳转到对应会话
  - 验收: 浏览器正确渲染 3 层嵌套

#### 里程碑 2.2: 角色过滤动态化 (2天)
- [ ] **任务 2.2.1**: SQL 查询参数化
  - 文件: `crates/hakimi-session/src/message_ops.rs`
  - 重构: `get_bookends()` 接受 `roles: &[&str]` 参数
  - 逻辑: 动态构建 `WHERE role IN (?, ?, ...)`
  - 验收: 支持任意角色组合

- [ ] **任务 2.2.2**: session_search 工具暴露参数
  - 文件: `crates/hakimi-tools/src/builtin_session_search.rs`
  - 参数: `role_filter: Option<String>` (逗号分隔)
  - 默认: Discovery 默认 `user,assistant`，Scroll 默认全部
  - 验收: 可搜索纯工具输出

#### 里程碑 2.3: 异步 prefetch 记忆 (3天)
- [ ] **任务 2.3.1**: 实现后台预取任务
  - 文件: `crates/hakimi-context/src/memory.rs`
  - 方法:
    ```rust
    pub async fn prefetch(&self, session_id: &str) -> Result<()> {
        // 1. 异步读取 user.md + memory.md
        // 2. 缓存到内存（带过期时间）
        // 3. 返回 Future，不阻塞主循环
    }
    ```
  - 触发: 会话创建后立即调用
  - 验收: 主循环延迟 < 50ms

- [ ] **任务 2.3.2**: 缓存失效策略
  - 逻辑:
    - 文件修改时间（mtime）检测
    - 30分钟 TTL
    - 内存上限 10MB
  - 验收: 压力测试无内存泄漏

---

### Phase 3: 性能优化与用户体验 (Week 5-8)

**目标**: 超越 Hermes 的交互体验

#### 里程碑 3.1: 真实时流式输出优化 (5天)
- [ ] **任务 3.1.1**: chunk-by-chunk 延迟分析
  - 工具: `tokio-console`, `flame graph`
  - 指标: 首字节时间（TTFB）、平均 chunk 间隔
  - 目标: TTFB < 100ms，chunk 间隔 < 50ms
  - 验收: 与 Hermes 对比测试

- [ ] **任务 3.1.2**: SSE 连接池优化
  - 文件: `crates/hakimi-webui/src/sse.rs`
  - 优化:
    - 复用 HTTP/2 连接
    - 心跳包防止超时（30s）
    - 自动重连机制（exponential backoff）
  - 验收: 24h 稳定性测试无断连

- [ ] **任务 3.1.3**: 前端渲染防抖
  - 文件: `crates/hakimi-webui/frontend/src/Chat.tsx`
  - 优化:
    - 累积 50ms 内的 chunks 再渲染
    - 虚拟滚动（长对话）
    - Markdown 增量解析
  - 验收: 无可见卡顿，FPS > 30

#### 里程碑 3.2: SmartContextEngine 动态策略 (4天)
- [ ] **任务 3.2.1**: 模型感知压缩
  - 文件: `crates/hakimi-context/src/smart_engine.rs`
  - 逻辑:
    ```rust
    fn get_compression_params(&self, model: &str) -> CompressionParams {
        match model {
            "gpt-4" => (threshold: 0.75, window: 150),
            "claude-3-opus" => (threshold: 0.70, window: 200),
            "llama-3-70b" => (threshold: 0.65, window: 100),
            _ => default_params(),
        }
    }
    ```
  - 配置: 从 `config.yaml` 读取模型 context limits
  - 验收: 不同模型压缩率不同

- [ ] **任务 3.2.2**: 压缩质量评估
  - 指标:
    - 信息保留率（关键词覆盖度）
    - Token 节省比例
    - 压缩耗时
  - 日志: 每次压缩记录指标
  - 验收: 可视化面板展示趋势

#### 里程碑 3.3: 向量检索集成（可选，按需） (7天)
- [ ] **任务 3.3.1**: Qdrant 集成
  - 依赖: `qdrant-client = "1.8"`
  - 文件: `crates/hakimi-vector/src/lib.rs`
  - 功能:
    - 消息自动向量化（embedding API）
    - 存储到 Qdrant collection
    - 语义搜索（top-k）
  - 验收: 语义搜索结果优于 FTS5

- [ ] **任务 3.3.2**: 混合检索策略
  - 逻辑:
    - FTS5 关键词召回（精确匹配）
    - 向量检索语义召回（模糊匹配）
    - 加权融合排序（BM25 + cosine similarity）
  - 验收: Recall@10 提升 20%+

- [ ] **任务 3.3.3**: 性能基准测试
  - 数据集: 1M 消息 + 1K 查询
  - 对比: SQLite FTS5 vs Qdrant vs 混合
  - 指标: QPS, Latency P50/P95/P99, Recall@K
  - 验收: 报告发布到文档

#### 里程碑 3.4: WebUI Gateway 配置界面 (5天)
- [ ] **任务 3.4.1**: 实时状态监控
  - 文件: `crates/hakimi-webui/frontend/src/pages/Gateway.tsx`
  - 展示:
    - Gateway 状态（运行/停止）
    - 活跃会话数
    - 总消息数
    - 最后活动时间
  - 验收: 5秒自动刷新

- [ ] **任务 3.4.2**: 配置编辑器
  - 功能:
    - YAML 语法高亮
    - 实时校验（schema validation）
    - 一键重启 Gateway
  - 验收: 修改配置后自动生效

- [ ] **任务 3.4.3**: 日志查看器
  - 展示: 最近 500 条日志（分页）
  - 过滤: 按级别（ERROR/WARN/INFO/DEBUG）
  - 搜索: 按关键词
  - 验收: 无需 SSH 查看日志

---

### Phase 4: 生态与开发者体验 (Week 9-12)

**目标**: 降低贡献门槛，建立社区

#### 里程碑 4.1: 插件系统设计 (7天)
- [ ] **任务 4.1.1**: 插件 API 定义
  - 文件: `crates/hakimi-plugin/src/lib.rs`
  - Trait:
    ```rust
    pub trait HakimiPlugin: Send + Sync {
        fn name(&self) -> &str;
        fn on_session_start(&self, ctx: &SessionContext) -> Result<()>;
        fn on_message(&self, msg: &Message) -> Result<Option<Message>>;
        fn on_session_end(&self, ctx: &SessionContext) -> Result<()>;
    }
    ```
  - 验收: 示例插件编译通过

- [ ] **任务 4.1.2**: 动态加载机制
  - 技术: `libloading` (动态链接) 或 WASM (`wasmtime`)
  - 配置: `plugins.yaml` 声明插件路径
  - 验收: 热加载插件无需重启

- [ ] **任务 4.1.3**: 插件市场原型
  - 功能:
    - GitHub Releases 自动发现
    - 一键安装（`hakimi plugin install <name>`）
    - 版本管理
  - 验收: 安装 3 个官方插件成功

#### 里程碑 4.2: 开发者文档完善 (5天)
- [ ] **任务 4.2.1**: 架构设计文档
  - 文件: `docs/ARCHITECTURE.md`
  - 内容:
    - 模块依赖图（mermaid）
    - 数据流图
    - 关键抽象说明
  - 验收: 新贡献者 30 分钟内理解

- [ ] **任务 4.2.2**: API 参考文档
  - 工具: `cargo doc --no-deps --open`
  - 覆盖: 所有公开 API + 示例
  - 部署: GitHub Pages 自动发布
  - 验收: 文档覆盖率 > 90%

- [ ] **任务 4.2.3**: 贡献指南
  - 文件: `CONTRIBUTING.md`
  - 内容:
    - 开发环境搭建
    - 代码风格（rustfmt + clippy）
    - PR 流程
    - 测试要求
  - 验收: 首次贡献者成功提交 PR

#### 里程碑 4.3: CI/CD 管道增强 (3天)
- [ ] **任务 4.3.1**: 多平台测试矩阵
  - 平台: Ubuntu 20.04/22.04, macOS 12/13, Windows Server 2022
  - Rust 版本: stable, beta, nightly
  - 验收: 所有组合通过测试

- [ ] **任务 4.3.2**: 自动化发布流程
  - 触发: Tag 推送 (`v*.*.*`)
  - 流程:
    1. 运行完整测试
    2. 构建 6 平台二进制
    3. 生成 changelog
    4. 创建 GitHub Release
    5. 推送 Docker 镜像
  - 验收: 0 人工干预

- [ ] **任务 4.3.3**: 持续性能基准
  - 工具: `criterion` + GitHub Actions
  - 对比: 每次 PR 与 main 分支性能差异
  - 阈值: 回归 > 10% 自动 block PR
  - 验收: 性能曲线可视化

---

## 🔄 自动化流程

### 每日自动任务

```yaml
# .github/workflows/daily-maintenance.yml
name: Daily Maintenance
on:
  schedule:
    - cron: '0 2 * * *'  # 每天 UTC 02:00

jobs:
  health-check:
    - 运行完整测试套件
    - 检查依赖更新（Dependabot）
    - 扫描安全漏洞（cargo audit）
    - 生成测试覆盖率报告

  auto-optimization:
    - 运行 clippy --fix
    - 运行 rustfmt
    - 检查死代码（cargo-udeps）
    - 自动提交 PR（如有改动）
```

### 每周自动任务

```yaml
# .github/workflows/weekly-review.yml
name: Weekly Review
on:
  schedule:
    - cron: '0 10 * * 1'  # 每周一 UTC 10:00

jobs:
  performance-benchmark:
    - 运行完整性能基准测试
    - 对比上周数据
    - 生成趋势图表
    - 如回归 > 15%，创建 Issue

  documentation-audit:
    - 检查 API 文档覆盖率
    - 扫描失效链接
    - 验证代码示例可编译
    - 生成文档质量报告

  roadmap-progress:
    - 统计已完成任务
    - 更新里程碑进度
    - 生成本周总结（Markdown）
    - 自动发布到 Discussions
```

### 自动 PR 审查规则

```yaml
# .github/workflows/pr-checks.yml
name: PR Quality Gate
on: [pull_request]

jobs:
  must-pass:
    - ✅ 所有单元测试通过
    - ✅ 代码覆盖率 ≥ 当前水平
    - ✅ Clippy 无 warnings（技术债例外）
    - ✅ rustfmt 格式正确
    - ✅ 无安全漏洞（cargo audit）
    - ✅ 编译时间增量 < 10%

  auto-merge-conditions:
    - 🤖 依赖更新（patch 版本）
    - 🤖 文档修复
    - 🤖 格式化 / Clippy 自动修复
    - 🤖 测试用例新增（无行为变更）

  require-human-review:
    - 👤 新增公开 API
    - 👤 Breaking changes
    - 👤 性能回归 > 5%
    - 👤 修改核心逻辑（记忆/会话/上下文）
```

---

## 📊 进度追踪

### 当前状态（v0.5.56）

```
功能完整度: ████████░░ 80%
测试覆盖率: ███████░░░ 70%
文档完善度: ██████░░░░ 60%
性能优化: ██████░░░░ 60%
生态建设: ███░░░░░░░ 30%
```

### 3 个月后目标（v0.8.0）

```
功能完整度: █████████░ 95%
测试覆盖率: ████████░░ 85%
文档完善度: ████████░░ 85%
性能优化: ████████░░ 80%
生态建设: ██████░░░░ 65%
```

---

## 🚀 快速启动下一任务

### 立即可执行（高优先级）

```bash
# 任务 1.1.1: 添加 tracing spans
cd /root/hakimi-agent
git checkout -b feat/observability-phase1
# 开始编码...

# 任务 1.2.1: 工作记忆自动清理
cd /root/hakimi-agent
git checkout -b feat/working-memory-lifecycle
# 开始编码...

# 任务 1.3.1: session_search 集成测试
cd /root/hakimi-agent
git checkout -b test/session-search-coverage
# 开始编码...
```

### 推荐执行顺序

1. **Week 1**: 1.1.1 → 1.2.1 → 1.3.1（稳定性优先）
2. **Week 2**: 1.1.2 → 1.2.2 → 1.3.2（可观测性）
3. **Week 3**: 2.1.1 → 2.1.2 → 2.2.1（功能对齐）
4. **Week 4**: 3.1.1 → 3.1.2（用户体验）

---

## 📝 决策记录

### ADR-001: 不引入复杂后端依赖
**日期**: 2026-07-09  
**决策**: Phase 1-2 保持 SQLite + 文件系统，Phase 3 再考虑 Qdrant  
**理由**: 
- 当前架构简单可靠（单二进制部署）
- 用户规模未达向量检索必要阈值（<1M 消息）
- 避免过度工程化

### ADR-002: 异步优先但不强制
**日期**: 2026-07-09  
**决策**: 关键路径（会话搜索）保持同步，prefetch 等边缘功能异步  
**理由**:
- 同步代码更易调试和推理
- 当前性能瓶颈不在 I/O（SQLite 足够快）
- 异步带来的复杂度需谨慎评估

### ADR-003: 测试覆盖率目标 85%
**日期**: 2026-07-09  
**决策**: 单元测试 + 集成测试 + 压力测试组合覆盖  
**理由**:
- 80% 是工业标准，85% 是高质量项目标准
- 关键路径（记忆/会话/上下文）100% 覆盖
- UI 代码可适当降低要求（手动测试补充）

---

## 🤝 贡献与反馈

- **Issue 模板**: [Bug Report](.github/ISSUE_TEMPLATE/bug_report.md) | [Feature Request](.github/ISSUE_TEMPLATE/feature_request.md)
- **讨论区**: [GitHub Discussions](https://github.com/Mouseww/hakimi-agent/discussions)
- **技术债看板**: [Projects → Technical Debt](https://github.com/Mouseww/hakimi-agent/projects/1)

---

**最后更新**: 2026-07-09  
**下次审查**: 2026-08-09（每月复盘）
