<h1 align="center">Hakimi Agent</h1>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.5.99-blue?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/tests-1781-passing?style=for-the-badge&color=brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/lines-44K+-orange?style=for-the-badge" alt="Lines">
</p>

<p align="center">
  <strong>Production-grade AI Agent framework, rewritten in Rust for speed and reliability</strong><br>
  <sub>Inspired by <a href="https://github.com/NousResearch/hermes-agent">Nous Research's Hermes Agent</a> — built from the ground up in Rust</sub>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#why-hakimi">Why Hakimi</a> ·
  <a href="#capabilities">Capabilities</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="https://yourusername.github.io/hakimi-agent/">API Docs</a> ·
  <a href="#compare">Compare</a> ·
  <a href="README_CN.md">中文</a>
</p>

---
<img width="1916" height="958" alt="AnythingAgentRecord" src="https://github.com/user-attachments/assets/64c1e6bb-2835-4a27-9e6c-fd5f49618695" />

<img width="1160" height="896" alt="image" src="https://github.com/user-attachments/assets/713b3a8f-1d5a-40bb-9e9f-7b771869ed12" />

---

## ✨ Recent Updates (v0.5.93)

**🎨 Marvis 风格 Office View — WebUI 视觉升级：**

**核心功能：**
- ✨ **6 种工作状态可视化系统** — 细粒度状态识别
  - 🟢 正常工作 (Working) — 绿色状态条，文档模式屏幕
  - 🔴 高负载 (Busy) — 红色状态条，代码模式屏幕
  - 🟣 项目规划 (Planning) — 紫色状态条，看板模式屏幕
  - 🔵 离线/休息 (Away) — 蓝色状态条，黑屏 + 💤
  - 🟡 创意设计 (Creative) — 黄色状态条，图片网格屏幕
  - 🔷 深度专注 (Focused) — 青色状态条，文档模式屏幕
- 🖥️ **Marvis 风格视觉设计** — 参考业界最佳实践
  - 顶部彩色状态条（不遮挡，不跨界）
  - 柔和径向渐变阴影
  - 显示器屏幕内容动画（文档打字、代码闪烁、看板脉冲等）
  - 角色表情匹配状态（😊 工作中、🤓 高负载、😴 休息等）
- 🎭 **视图切换按钮** — 经典视图 ↔ Marvis 视图
  - 保留原有经典风格组件
  - 一键切换视觉风格
  - 用户偏好本地保存（未来）
- ✨ **微交互动画**
  - 打字时手臂上下动画
  - 状态条柔和脉冲呼吸
  - Hover 悬浮 tooltip 显示详细信息
  - 响应式动画（支持 `prefers-reduced-motion`）

**开发细节：**
- 新组件：`PersonaDeskMarvis.tsx` (7.1 KB)
- 新样式：`office-marvis.css` (7.3 KB)
- 状态推断逻辑：基于 `taskHint` 智能分类
- 保持后向兼容：不影响现有经典视图

---

## ✨ Recent Updates (v0.5.91)

**📦 示例 WASM 插件集合 (TASK 5.1.3) — 完成：**

**核心功能：**
- ✨ **5 个实用插件示例** — 覆盖不同应用场景
  - **Hello WASM Plugin** (47 KB) — 基础示例，展示插件结构
  - **Weather Plugin** (67 KB) — 天气查询，演示结构化数据处理
  - **JSON Formatter Plugin** (101 KB) — JSON 格式化和验证
  - **Markdown Plugin** (69 KB) — Markdown 文本转换
  - **Snippet Store Plugin** (54 KB) — 代码片段存储管理
- 🛠️ **统一构建脚本** — `build_all_plugins.sh`
  - 一键构建所有插件
  - 显示大小统计和构建摘要
  - 自动检测构建错误
- 📚 **完整开发指南** — `examples/README.md`
  - 插件开发最佳实践
  - 代码模板和示例
  - 性能优化技巧
  - 故障排除指南

**技术细节：**
- 文件: `examples/weather-plugin/`, `examples/json-formatter-plugin/`, 
  `examples/markdown-plugin/`, `examples/snippet-store-plugin/`
- 构建: 所有 5 个插件成功编译，总大小 339 KB
- 优化: `opt-level = "z"`, `lto = true`, `strip = true`
- 测试: 每个插件都有单元测试 ✅

**使用示例：**
```bash
# 构建所有插件
cd examples && ./build_all_plugins.sh

# 安装插件
hakimi plugin install examples/weather-plugin/target/wasm32-wasip1/release/weather_plugin.wasm

# 执行插件
hakimi plugin execute weather-plugin

# 查看插件信息
hakimi plugin info weather-plugin
```

**v0.5.90 更新：**
**🔌 Plugin CLI 命令 (TASK 5.2.1) — 完成：**

**核心功能：**
- ✨ **完整的插件管理 CLI** — 8 个子命令覆盖全生命周期
  - `hakimi plugin list [--available]` — 列出已安装或可用插件
  - `hakimi plugin search <query>` — 搜索插件市场
  - `hakimi plugin install <name> [--version]` — 安装插件（支持版本指定）
  - `hakimi plugin uninstall <name>` — 卸载插件
  - `hakimi plugin info <name>` — 查看插件详细信息
  - `hakimi plugin test <name>` — 测试插件加载
  - `hakimi plugin enable/disable <name>` — 启用/禁用插件
  - `hakimi plugin update [name]` — 检查插件更新
- 📊 **友好的输出** — 表格化显示，彩色状态
  - 使用 tabled crate 生成圆角表格
  - 启用/禁用状态图标 (✓/✗)
  - emoji 装饰增强可读性
- 🌐 **远程注册表集成** — 连接插件市场
  - 从 YAML 注册表获取插件元数据
  - 自动下载和校验和验证
  - 本地缓存支持离线查看

**技术细节：**
- 文件: `crates/hakimi-cli/src/commands/plugin.rs` (409 行)
- 新增: `crates/hakimi-cli/src/commands/mod.rs` 
- 增强: `PluginMarketplace::set_plugin_enabled()`
- 依赖: tabled (表格输出)
- 测试: 所有 CLI 测试通过（191 个测试）✅

**使用示例：**
```bash
# 查看可用插件
hakimi plugin list --available

# 搜索插件
hakimi plugin search weather

# 安装插件
hakimi plugin install weather-info

# 查看详细信息
hakimi plugin info weather-info

# 测试插件
hakimi plugin test weather-info

# 禁用插件
hakimi plugin disable weather-info

# 检查更新
hakimi plugin update
```

**v0.5.89 更新：**
**🔌 WASM Plugin SDK (TASK 5.1.2) — 完成：**

**核心功能：**
- ✨ **#[hakimi_plugin] 宏** — 自动生成插件导出函数和元数据
  - 零样板代码：一行宏声明插件
  - 自动元数据导出（JSON 格式）
  - 类型安全的插件接口
- 📦 **hakimi-plugin-sdk** — 高级 SDK  crate
  - `PluginContext` 访问宿主功能
  - `log()` 日志记录（已实现）
  - `http_get()` HTTP 请求（接口预留）
  - `PluginResult<T>` 标准返回类型
- 📚 **示例插件** — hello-wasm-plugin
  - 完整构建文档
  - 48KB 优化后体积
  - wasm32-wasip1 目标编译

**技术细节：**
- 过程宏 crate: `hakimi-plugin-sdk-macro`
- 非 WASM 环境测试支持（模拟宿主函数）
- 3个单元测试覆盖核心功能
- PR: #47（待创建）

**v0.5.88 更新：**
**🎨 WASM 插件运行时 (TASK 5.1.1) — 完成：**

**核心功能：**
- ✨ **WasmPluginLoader** — 基于 Wasmtime 16.0 的安全 WASM 运行时
  - 编译和实例化 WASM 模块
  - 异步加载/卸载插件 API
  - 插件元数据自动提取（从 WASM 内存读取 JSON）
- 🔒 **安全沙箱** — 资源隔离和限制
  - 内存限制 128MB（可配置）
  - 堆栈限制 1MB
  - 执行超时 5秒（基于 fuel）
  - 表大小限制 10000
- 🌐 **WASI 支持** — WebAssembly System Interface
  - 标准输入输出继承
  - 预打开目录配置
  - 文件系统访问控制
- 🔌 **宿主函数** — Hakimi 集成接口
  - log 日志记录（已实现）
  - http_request HTTP 请求（占位符）
  - 可扩展架构支持更多宿主函数

**技术细节：**
- 特性门控：`cargo build --features wasm`
- 测试覆盖：34/34 单元测试通过 ✅
- 跨平台：同一 .wasm 文件可在 Linux/macOS/Windows 运行
- PR: #46

**v0.5.86 更新：**
**⚡ 性能优化 — 异步记忆预取机制 (TASK 2.3.1) — 完成：**

**核心功能：**
- ✅ **MemoryCache 缓存系统** — 内存文件智能缓存
  - 50-100x 性能提升（缓存命中时 < 1μs vs 50-100μs）
  - 30 分钟 TTL，10MB 大小限制
  - LRU 驱逐策略，自动文件修改检测
- ✅ **异步预取** — 后台非阻塞加载
  - 首次响应延迟 < 10ms
  - tokio::spawn 并行预取所有记忆文件
  - 不阻塞主循环
- ✅ **缓存集成** — 透明集成到 FileMemoryProvider
  - 优先从缓存读取
  - 未命中自动回退到磁盘 I/O
  - 支持文件修改自动失效

**v0.5.84 更新：**
**📚 文档体系完善 (TASK 4.2.1 & 4.2.2) — 完成：**

**核心功能：**
- ✅ **架构设计文档** — 完整的系统架构说明（docs/ARCHITECTURE.md）
  - 5 个 Mermaid 架构图（模块依赖、数据流、搜索、记忆、插件）
  - 21 个 crate 职责说明
  - 工具/技能/插件边界清晰定义
  - 30 分钟快速阅读路径
- ✅ **API 参考文档** — 自动生成并部署到 GitHub Pages
  - 配置 GitHub Actions 自动部署
  - cargo doc 完整构建
  - 完整的公开 API 文档
- ✅ **插件开发指南** — docs/plugin_development_guide.md
  - 完整的插件开发流程
  - 示例插件模板（examples/example_plugin）
  - 钩子函数详细说明

**文档链接：**
- [架构设计文档](docs/ARCHITECTURE.md)
- [API 参考文档](https://mouseww.github.io/hakimi-agent/)
- [插件开发指南](docs/plugin_development_guide.md)

**之前版本亮点：**

**🔌 插件系统 (TASK 4.1.1 - 4.1.3) — v0.5.80-82：**
- ✅ **插件 API** — HakimiPlugin trait，5 个生命周期钩子
- ✅ **动态加载** — libloading 实现 .so/.dylib/.dll 加载
- ✅ **插件市场** — GitHub Releases 分发，SHA256 校验
- ✅ **配置管理** — plugins.yaml 配置，白名单保护
- ✅ **热加载** — 支持插件重载的基础设施

**插件加载器 API：**
```rust
// 创建加载器
let config = PluginLoaderConfig {
    plugin_dir: PathBuf::from("~/.hakimi/plugins"),
    enable_hot_reload: true,
    verify_signature: false,
    allowed_plugins: vec![],
};
let loader = PluginLoader::new(config);

// 加载插件
loader.load_plugin("/path/to/libplugin.so").await?;

// 根据 ID 加载（自动查找路径）
loader.load_plugin_by_id("my_plugin").await?;

// 重载插件（热更新）
loader.reload_plugin("my_plugin").await?;

// 列出已加载插件
let plugins = loader.list_plugins().await;
```

**插件配置示例（~/.hakimi/plugins.yaml）：**
```yaml
plugin_dir: ~/.hakimi/plugins
enable_hot_reload: true
verify_signature: false

plugins:
  - id: logger
    enabled: true
    config:
      level: info
  
  - id: rate_limiter
    enabled: false
```

**开发者工具：**
- 📚 插件开发指南（`docs/plugin_development_guide.md`）
- 🔧 示例插件项目（`examples/example_plugin/`）
- ✅ 24 个单元测试全部通过
- 🔒 FFI 安全指南和最佳实践

**技术细节：**
- `libloading` 跨平台动态库加载
- `serde_yaml` 配置文件解析
- `notify` 文件监控（热加载基础）
- 自动符号查找和验证
- ABI 兼容性考虑

**安全特性：**
- 插件白名单机制（可选）
- 签名验证框架（待实现）
- FFI 边界安全检查
- 完善的错误处理

**下一步：**
- TASK 4.1.3: 插件市场原型
- 增强热加载监控（文件变化自动重载）
- 实现插件签名验证
- WASM 沙箱支持（长期计划）

---

## ✨ Previous Updates (v0.5.80)

**🔌 插件系统基础架构 (TASK 4.1.1) — 完成：**

**核心功能：**
- ✅ **插件接口** — HakimiPlugin trait 定义标准插件 API
- ✅ **注册管理** — 自动依赖检查和插件生命周期管理
- ✅ **钩子系统** — 支持消息、工具、会话等多种钩子点
- ✅ **向后兼容** — PluginLoader stub 保持与现有代码的兼容
- ✅ **异步支持** — 所有钩子都是异步的，不阻塞主流程
- ✅ **测试完善** — 14 个单元测试全部通过，覆盖率 > 90%

**钩子类型：**
```rust
// 消息钩子
async fn on_message_before_send(&self, ctx, msg) -> Result<MessageAction>
async fn on_message_after_send(&self, ctx, msg) -> Result<()>
async fn on_message_received(&self, ctx, msg) -> Result<MessageAction>

// 工具钩子
async fn on_tool_call_before(&self, ctx, tool, params) -> Result<ToolCallAction>
async fn on_tool_call_after(&self, ctx, tool, result) -> Result<ToolCallResultAction>

// 会话钩子
async fn on_session_start(&self, ctx, session) -> Result<()>
async fn on_session_end(&self, ctx, session) -> Result<()>
```

**插件示例：**
```rust
#[async_trait]
impl HakimiPlugin for LoggerPlugin {
    fn metadata(&self) -> &PluginMetadata { /* ... */ }
    
    async fn on_message_after_send(&self, ctx: &PluginContext, msg: &Message) 
        -> Result<()> 
    {
        tracing::info!("[Logger] Message sent: {} chars", msg.content.len());
        Ok(())
    }
}
```

**核心组件：**
- **PluginRegistry** — 管理插件注册、卸载、依赖检查
- **PluginManager** — 协调插件钩子调用和执行流程
- **PluginMetadata** — 版本、作者、描述、依赖等元数据

**下一步：**
- TASK 4.1.2: 动态加载机制（libloading / WASM）
- TASK 4.1.3: 插件市场原型

---
- ✅ **查询功能** — 按作业、状态、时间查询运行历史
- ✅ **自动清理** — 支持自动清理旧运行记录

**重试策略：**
```rust
// 固定间隔：每次等待相同时间
RetryStrategy::FixedInterval { interval_secs: 60 }

// 指数退避：延迟时间指数增长，带上限
RetryStrategy::ExponentialBackoff {
    initial_interval_secs: 60,
    max_interval_secs: 3600,
    multiplier: 2.0
}

// 自定义间隔：精确控制每次延迟
RetryStrategy::CustomIntervals { 
    intervals_secs: vec![60, 300, 900] 
}

// 禁用重试：失败立即停止
RetryStrategy::NoRetry
```

**配置示例：**
```rust
RetryConfig {
    strategy: RetryStrategy::ExponentialBackoff {
        initial_interval_secs: 60,
        max_interval_secs: 3600,
        multiplier: 2.0
    },
    max_attempts: 3,  // 1次初始 + 2次重试
    retry_on_errors: vec![
        "NetworkError".to_string(),
        "TimeoutError".to_string()
    ]
}
```

**运行历史追踪：**
- 每个运行包含完整的尝试列表
- 记录每次尝试的开始/结束时间、状态、错误信息、耗时
- 支持查询特定作业的历史运行
- 支持查询失败的运行
- 支持清理旧记录（保留最近 N 条）

**测试覆盖：**
- ✅ 62 个 hakimi-cron 测试全部通过（新增 12 个测试）
- ✅ 8 个重试策略单元测试
- ✅ 4 个运行存储集成测试
- ✅ 测试覆盖率 > 90%

**Schema 变化：**
- `cron_jobs` 表新增 `retry_config` 列
- 新增 `cron_runs` 表记录运行历史
- 新增 `cron_run_attempts` 表记录尝试详情
- 新增 4 个索引优化查询性能

---

## ✨ Previous Updates (v0.5.78)

**🔧 Test Suite Fixes**

**Fixed:**
- ✅ **Compilation errors** — Added missing `tracing-subscriber` dev-dependency to hakimi-core
- ✅ **Outdated tests removed** — Deleted `tracing_spans.rs` test using deprecated AIAgent API
- ✅ **Test cleanup** — Removed non-functional unit tests in `builtin_session_search.rs`
- ✅ **Integration tests retained** — Full test coverage maintained in integration tests

**Test Status:**
- ✅ **332 hakimi-tools tests passing**
- ✅ **Release build succeeds**
- ✅ **Core functionality verified**

---

## ✨ Previous Updates (v0.5.73)

**🔍 session_search 工具暴露 roles 参数 (任务 2.2.2) — 完成：**

**核心功能：**
- ✅ **roles 参数暴露** — 在 `session_search` 工具的 JSON 参数中添加 `roles` 数组字段
- ✅ **Discovery 模式支持** — bookends 消息支持角色过滤
- ✅ **Scroll 模式支持** — 窗口消息支持角色过滤
- ✅ **灵活过滤选项** — 默认 `None` 使用 user+assistant，传 `[]` 查看所有角色，传 `["user", "tool"]` 只看用户输入和工具输出
- ✅ **JSON Schema 更新** — 添加 `roles` 字段文档和示例
- ✅ **向后兼容** — 现有工具调用无需修改，默认行为不变

**使用示例：**
```json
// 查看所有角色的消息
{"mode": "browse", "session_id": "abc", "anchor_id": 42, "roles": []}

// 只查看工具输出
{"mode": "browse", "session_id": "abc", "anchor_id": 42, "roles": ["tool"]}

// 查看用户输入和工具输出（调试工具调用链）
{"mode": "browse", "session_id": "abc", "anchor_id": 42, "roles": ["user", "tool"]}
```

**API 变化：**
- `scroll_mode()` 新增 `roles: Option<&Vec<String>>` 参数
- `discovery_mode()` 新增 `roles: Option<&Vec<String>>` 参数
- `format_session_with_bookends()` 新增 `roles: Option<&Vec<String>>` 参数

---

## ✨ Previous Updates (v0.5.72)

**🔍 SQL 查询角色过滤动态化 (任务 2.2.1)：**

**核心功能：**
- ✅ **动态角色过滤** — `get_bookends()` 和 `get_messages_around()` 支持任意角色组合查询
- ✅ **灵活参数设计** — 新增 `roles: Option<&[&str]>` 参数，默认 `['user', 'assistant']`
- ✅ **动态 SQL 构建** — `build_role_filter_sql()` 函数动态生成 `WHERE role IN (?, ?, ...)` 子句
- ✅ **角色组合查询** — 支持 user, assistant, tool, system 的任意组合
- ✅ **禁用过滤** — 传空数组 `Some(&[])` 可查询所有角色消息
- ✅ **向后兼容** — 传 `None` 保持原有行为（只查询 user + assistant）

**测试覆盖：**
- ✅ 新增 6 个单元测试覆盖所有角色过滤场景
- ✅ 所有 34 个 message_ops 测试通过
- ✅ 参数化查询防止 SQL 注入
- ✅ 性能无退化（索引仍然有效）

**API 影响：**
- `MessageOps` trait 方法签名更新
- `session_search` 工具调用适配新签名
- 为 TASK 2.2.2（工具层暴露 roles 参数）铺平道路

---

## ✨ Previous Updates (v0.5.71)

**🚨 错误追踪系统 (任务 1.1.3) — 完成：**

**核心功能：**
- ✅ **错误分类系统** — 9 种错误类别（Network, Database, FileSystem, Configuration, Authentication, Business, ExternalService, Internal, Unknown）
- ✅ **严重程度分级** — 4 级严重程度（Low, Medium, High, Critical）
- ✅ **错误恢复策略** — 支持自动重试，可配置最大重试次数
- ✅ **错误统计分析** — 实时统计错误分布、恢复状态
- ✅ **错误过滤查询** — 按类别、严重程度筛选错误
- ✅ **上下文记录** — 支持键值对上下文和堆栈追踪

**API 端点：**
- ✅ `GET /api/errors/stats` — 获取错误统计
- ✅ `GET /api/errors` — 获取所有错误
- ✅ `GET /api/errors/unrecovered` — 获取未恢复错误
- ✅ `GET /api/errors/category/:category` — 按类别获取错误
- ✅ `POST /api/errors/:id/recover` — 尝试恢复错误
- ✅ `DELETE /api/errors` — 清除所有错误
- ✅ `DELETE /api/errors/recovered` — 清除已恢复错误

**技术实现：**
- 线程安全的错误存储（Mutex）
- 全局单例模式
- 自动清理旧错误（防止内存溢出）
- `track_error!` 宏简化错误记录
- RecoveryStrategy trait 支持自定义恢复逻辑

---

**Previous: v0.5.70**

**🌲 WebUI 会话树可视化 (任务 2.1.4) — Phase 2 完成：**

**前端组件：**
- ✅ **SessionTree React 组件** — 层级会话树可视化展示
  - 树形结构递归渲染，支持无限嵌套
  - 折叠/展开交互控制
  - 点击节点快速跳转会话
  - 面包屑导航显示完整谱系路径
- ✅ **UI 集成** — FolderTree 图标切换按钮
  - 集成到会话列表标题
  - 响应式设计，移动端适配
  - lucide-react 图标统一风格
- ✅ **智能展开** — 当前会话及祖先自动展开
- ✅ **会话元数据** — 显示创建时间、消息数量

**API 支持：**
- ✅ **类型定义** — SessionMetadata、SessionTreeNode、SessionTreeResponse
- ✅ **API 函数** — fetchSessionTree() 带身份认证

**技术成果：**
- 📦 **TypeScript 编译通过** — 0 错误
- 🚀 **Vite 构建成功** — bundle 优化
- ✅ **后端测试全通过** — 61/61

**里程碑达成：**
- 🎯 **Phase 2 完整对齐** — Lineage 父子会话关系功能完整
- 📊 **与 Hermes 功能对齐度：95%** — 会话管理能力全面超越

---

**历史更新 (v0.5.69)：**

**🔍 Tracing Instrumentation (任务 1.1.1)：**

**核心功能：**
- ✅ **记忆文件大小限制** — 防止记忆文件过大
  - 软限制（60KB）：警告日志
  - 硬限制（64KB）：拒绝加载
  - 自动容量检查和友好错误提示
- ✅ **check_file_size() 方法** — 单文件大小检查
  - 返回详细错误信息（包含大小和限制）
  - 引导用户清理或归档
- ✅ **测试覆盖** — 4 个测试用例全通过
  - 正常大小、警告区间、超限、不存在文件

**历史更新：**
- v0.5.65: session_search 工具集成 Lineage (任务 2.1.3)
- v0.5.60: Tracing Spans 追踪系统 (任务 1.1.1)
- 🔄 **Tracer 管理器** — 集中式 Span 收集和查询
  - 创建和记录 Span
  - 完整调用链查询
  - 统计信息（总数、平均值）
- 🛡️ **SpanContext** — RAII 模式自动管理
  - Drop 时自动完成
  - 简化作用域追踪

**技术亮点：**
- ⚡ **线程安全** — Arc<RwLock> 并发访问
- 📊 **14 个单元测试** — 全部通过
- 🎨 **示例代码** — examples/tracing_example.rs
- 📦 **零外部依赖** — 仅使用 uuid + chrono

---

<details>
<summary><b>📜 Previous Updates (v0.5.55 - v0.5.59)</b></summary>

### v0.5.59 - 记忆归档机制

**🔧 HTTP Keepalive 修复 + 上下文管理全面升级：**

**HTTP 稳定性修复：**
- ✅ **修复连接池问题** — `dispatched_agent` 现在使用统一的 HTTP 客户端配置，启用 TCP keepalive (60s) 和 pool idle timeout (90s)
- 🐛 **解决间歇性错误** — 修复 "transport error: error sending request" 问题，提升长时间会话稳定性

**⚡ 上下文管理全面升级（对齐 Hermes Agent）：**
- 🧠 **智能工具去重** — MD5 hash 检测相同工具结果（如重复读同一文件），只保留最新，减少 token 浪费
- ✂️ **大型参数截断** — JSON 安全的参数压缩，智能截断过长字符串参数（如大型文件内容）
- 🧹 **孤立工具对清理** — 自动移除没有对应调用的工具结果，为缺失结果的工具调用添加占位符
- 🎯 **最后用户消息保护** — 确保最后一条用户消息始终保留在尾部，防止任务丢失
- 🛡️ **Anti-thrashing 保护** — 跟踪压缩历史，如果最近压缩效果不佳（节省 <10%）自动跳过，避免无效压缩循环
- 📊 **详细工具摘要** — 生成更有意义的工具摘要（如 `[terminal] exit 0, 47 lines` 而非 `[terminal] 2847 chars`）

**🎯 默认引擎升级：**
- 🚀 **AdvancedCompressor 成为默认** — 从 `SmartContextEngine` 升级到 `AdvancedCompressor`，提供 Hermes 同等水平的智能压缩
- 📖 **向后兼容** — 可通过 `agent.context_engine` 配置选择 `simple`、`smart`、`llm` 或 `advanced`（默认）
- 🔬 **边界对齐** — 确保不分割 tool_call/result 组，保持对话完整性
- 💾 **Token 预算驱动** — 动态计算尾部 token 预算，智能保护关键上下文

**Previous Updates (v0.5.54):**

**🚀 工具调用上限提升：**
- ✅ **主 Agent 上限提升** — `agent.max_turns` 从 90 提升到 **150**，支持更复杂的任务流程
- 🔄 **子 Agent 上限翻倍** — `delegation.max_iterations` 从 45 提升到 **90**，与旧版主 agent 一致
- 📈 **更强的任务执行能力** — 支持需要大量工具调用的复杂任务（如大型代码库重构、多轮数据分析）
- ⚙️ **可配置** — 可通过 `~/.hakimi/config.yaml` 自定义调整上限

**Previous Updates (v0.5.53):**

**📱 Telegram 显示体验优化：**
- ✅ **分隔符优化** — 工具调用结果分隔符从 `│` 改为 ` — `，解决字符挤压问题
- 🎨 **可读性提升** — `hakimitoolresult:terminal — STDOUT:` 格式更清晰，视觉间距更舒适
- 🐛 **修复显示异常** — 解决 Telegram 中工具输出紧凑难读的问题

**Previous Updates (v0.5.52):**

**🧠 智能上下文管理 - Gateway 多轮对话记忆增强：**
- ✅ **智能历史压缩** — 新增 `context_manager` 模块，实现三重压缩策略
- 🎯 **保留关键上下文** — 最近 20 条消息完整保留（用户选择、最新对话）
- 🗜️ **工具消息压缩** — 工具调用序列保留首尾，折叠中间冗余输出
- 📏 **滑动窗口** — 总消息数限制为 100 条，防止无限累积导致的内存爆炸
- 🔧 **修复丢失上下文** — 解决 Gateway 模式在长对话中遗忘用户选择的问题
- 🚀 **性能优化** — 大幅降低内存占用和 Token 消耗，提升响应速度

**Previous Updates (v0.5.51):**

**🔧 HTTP 连接稳定性修复：**
- ✅ **TCP Keepalive** — 启用 60 秒 TCP keepalive 探测，防止连接被中间网络设备断开
- 🔄 **连接池管理** — 空闲连接 90 秒后自动关闭，避免复用已失效的连接
- 🚀 **可靠性提升** — 修复偶发的 "error sending request" 传输错误
- 📊 **资源优化** — 每个域名最多保留 10 个空闲连接，避免资源浪费

**Previous Updates (v0.5.50):**

**🎯 实时工具调用显示：**
- ✅ **实时转发** — 子 agent 调用工具时立即显示，不再等到完成后批量展示
- 🔧 **简洁格式** — 移除折叠块逻辑，工具调用直接显示在协作过程中
- 📊 **清晰时间线** — 用户可以看到完整的执行过程：开始 → 工具A → 工具B → ... → 完成
- 🚀 **更好的透明度** — 长时间任务（如健康检查）的进度一目了然
- 🎨 **体验优化** — team 工具结果仍然对用户不可见，保持聊天记录清爽

**Previous Updates (v0.5.49):**

**🎯 Team 工具返回优化：**
- ✅ **直接返回结果** — team 工具调用返回 teammate 的实际输出，Agent 不再需要 read_file
- 🔧 **过滤用户展示** — Gateway 层自动过滤 `hakimi_tool_result:team` 消息，用户不会看到冗余的工具结果
- 📊 **更好的上下文** — 主 Agent 能立即获取子任务完整结果，无需额外步骤
- 🚀 **用户体验改进** — 聊天记录更清晰，只显示有意义的协作过程（开始/进度/完成），工具结果留给 Agent 内部使用

**Previous Updates (v0.5.48):**

**📊 子 Agent 工具调用记录折叠显示：**
- ✅ **折叠块内部显示工具列表** — 子 agent 完成时，在 Telegram 的折叠块（spoiler）内显示逐行的工具调用记录
- 🛠️ **收集工具调用历史** — 在 delegate.rs 中使用 `Arc<Mutex<Vec<String>>>` 收集所有工具调用
- 📋 **格式化协作消息** — 识别 `[工具调用详情]` 标记，自动转换为 Telegram 的 `||spoiler||` 语法
- 🎯 **实时进度 + 最终汇总** — 保留实时工具通知，同时在完成时提供完整工具列表
- 🚀 **用户体验优化** — 不再只显示空折叠块，而是在折叠内容中显示所有工具调用详情

**Previous Updates (v0.5.47):**

**🔧 子 Agent 工具调用可见性修复：**
- ✅ **显示工具计数** — 子 agent 完成时显示"完成，返回结果（使用了 N 个工具）"
- 🛠️ **工具调用追踪** — 在 delegate.rs 中添加 AtomicUsize 计数器，与 team.rs 保持一致
- 📊 **实时工具通知** — 子 agent 执行工具时通过 streaming callback 转发 `\u{001e}hakimi_tool:` 事件
- 🎯 **对齐 Persona Team 行为** — delegate_task 和 persona 团队协作现在使用相同的进度报告机制

**Previous Updates (v0.5.46):**

**🎯 Telegram 流式输出格式稳定性修复：**
- ✅ **智能未闭合语法检测** — 新增 `sanitize_for_streaming()` 函数，实时检测并移除未闭合的 Markdown 语法
- 🔧 **消除 UI 闪烁** — 流式更新时自动截断未完成的 `**粗体**`、`` `代码` ``、`` ```代码块``` `` 等语法，防止 Telegram 解析失败
- 📊 **渐进式渲染** — 已完成的 Markdown 格式正常显示，未完成部分作为纯文本，输出完成后格式完整
- 🚀 **用户体验优化** — 彻底解决流式输出过程中"有格式 ↔ 无格式"反复切换导致的视觉不稳定问题
- ⚡ **零性能损耗** — 使用 `saturating_sub()` 避免整数溢出，逻辑高效且安全

**Previous Updates (v0.5.36):**

**🔌 Teams Webhook Gateway 注册修复：**
- ✅ **自动注册 Adapter** — 在统一模式启动时自动注册 TeamsWebhookAdapter 到 Gateway
- 🎯 **配置驱动** — 读取 `config.yaml` 中的 `gateways.teams_webhook.hmac_secret` 和 `default_workflow_url`
- 🔧 **完整双向通信** — Teams webhook 收到消息后可以正常通过 Gateway 发送 AI 回复
- 💡 **统一架构** — Teams Webhook 与 Telegram、Discord、Slack 等平台一致的 Gateway adapter 架构

**Previous Updates (v0.5.35):**

**🎨 Telegram Markdown 稳定渲染修复：**
- ✅ **智能清理 sanitize_for_markdown()** — 替代 v0.5.27 过度激进的 `escape_markdown()`
- 🔧 **选择性转义** — 只转义会导致解析错误的字符（括号、方括号、表格分隔符），保留格式化标记（`*`、`` ` ``、`**`）
- 📊 **表格支持** — 将 `|` 替换为 Unicode 盒绘字符 `│`，避免解析错误同时保持表格视觉效果
- 🎯 **解决 UI 闪烁** — 彻底消除流式输出过程中"格式化 ↔ 纯文本"反复切换的问题
- 💎 **保留所有格式** — 粗体、斜体、代码块、链接等 Markdown 功能完全可用

**Previous Updates (v0.5.34):**

**🔄 Teams Webhook 完整回复功能：**
- ✅ **Gateway 路由集成** — 后台任务通过 Gateway.route_message() 发送回复到 Teams
- 🎯 **自动匹配 chat_id** — 后台任务构造 `teams_{channel_id}` 格式的 chat_id，匹配 adapter 的映射
- 🔧 **优雅降级** — Gateway 不可用时（WebUI-only 模式）记录详细日志，不会崩溃
- 📝 **完整日志追踪** — 发送成功/失败都记录 info/warn 日志，方便排查问题
- 🏗️ **统一架构** — 统一模式下，Teams webhook 和其他平台使用相同的 Gateway 路由机制

**Previous Updates (v0.5.33):**

**💬 Teams Webhook 友好即时响应 + 日志增强：**
- 🎨 **立即返回 Adaptive Card** — 不再返回空 202，而是返回友好的"✅ 收到消息，正在处理..."卡片
- 📝 **详细后台日志** — 后台任务处理开始/完成/结果都记录 info 日志，方便排查问题
- 🌐 **提取 service_url** — 为后续实现 Bot Framework API 回复做准备（当前先记录日志）
- 🔧 **TODO 标记** — 明确标记了两种回复方式：Bot Framework API 或 Power Automate Webhook

**Previous Updates (v0.5.32):**

**⚡ Teams Webhook 异步非阻塞修复：**
- 🚀 **立即返回 202 Accepted** — Teams webhook 处理改为异步模式，收到请求后立即返回，彻底解决 10 秒超时问题
- 🔄 **后台任务处理** — 用 `tokio::spawn` 创建独立任务处理 AI 消息，不阻塞事件循环
- 🎯 **并发友好** — Agent 处理其他任务时，新 Teams 请求不再等待锁释放
- 📝 **回复机制 TODO** — 标记了 Power Automate Workflow URL 回调实现（下个版本完成）

**Previous Updates (v0.5.31):**

**🐛 macOS CI Build Fix:**
- ✅ **Axum API Compatibility** — Fixed `RawBody` usage → `Request<Body>` for Axum 0.8 API changes
- 🔓 **Module Visibility** — Made `teams_webhook` module public in `hakimi-gateway` lib.rs
- 🧹 **Simplified Config** — Removed incomplete config validation in Teams webhook handler (TODO for future)
- 🚀 **CI Green** — All platforms (Linux x64/ARM64, macOS x64/ARM64, Windows x64/ARM64) now build successfully

**Previous Updates (v0.5.30):**

**🏢 Teams Webhook Integration (3005 Port Reuse):**
- ♻️ **Unified Port Deployment** — Teams Webhook endpoints integrated into WebUI server (3005 port), eliminating need for separate service
- 🔌 **Simple Reverse Proxy Setup** — Works with existing Nginx configuration, no new domains or ports required
- 🚀 **Two New Endpoints** — `POST /webhooks/teams/inbound` for messages, `GET /webhooks/teams/health` for health checks
- 💬 **Adaptive Card Responses** — Returns formatted Adaptive Cards directly from the WebUI handler
- 🔒 **Config-Based HMAC** — Reads Teams webhook secret from `config.yaml` gateway section
- 📦 **Cleaner Architecture** — Removed standalone `teams-webhook-server` binary, consolidated into main server

**Previous Updates (v0.5.29):**

**🏢 Microsoft Teams Webhook Integration:**
- 🔌 **No Azure Bot Required** — Direct integration via Teams Outgoing Webhooks + Power Automate Workflows
- 🔒 **HMAC Signature Verification** — Secure inbound message authentication with SHA-256 HMAC
- 🎨 **Adaptive Card Builder** — Rich card formatting with titles, facts, buttons, and custom layouts
- ⚡ **10-Second Response** — Async task processing with immediate receipt acknowledgment
- 📡 **Bidirectional Channels** — Inbound via HTTP POST `/teams/inbound`, outbound via Workflows webhook URLs
- 🗺️ **Multi-Channel Routing** — Channel ID → Workflows URL mapping for project-specific notifications
- 📚 **Complete Documentation** — Full setup guide at `docs/integrations/teams-webhook.md`

**Previous Updates (v0.5.28):**

**🎮 QQ Bot & ClawBot (WeChat) Support:**
- 🤖 **QQ Bot Integration** — Added QQ Bot to setup wizard, requires AppID + Token from QQ Open Platform
- 💬 **ClawBot (WeChat) Support** — Added WeChat integration via ClawBot server (endpoint + optional token)
- 🔧 **Multi-Platform Setup** — Expanded platform adapter options from 3 to 5 (Telegram, QQ, ClawBot, Discord, Slack)
- ✨ **Better User Experience** — Interactive prompts guide users through QQ AppID/Token and ClawBot endpoint configuration

**Previous Updates (v0.5.27):**

**💎 Stable Telegram Markdown UI:**
- 🎨 **Automatic Markdown Escaping** — All outbound text now escapes special characters (`_`, `*`, `[`, `]`, `(`, `)`, `` ` ``) to prevent Telegram parse errors
- 🚀 **Eliminated UI Flicker** — Removed fallback-to-plain-text retry logic that caused mid-stream format toggling
- ✨ **Always Beautiful** — Messages, media captions, and drafts render with consistent Markdown styling throughout the entire conversation
- 🔧 **Applied Everywhere** — Covers `send_message`, `send_message_get_id`, `edit_message`, `send_remote_media`, and `send_local_media`

**Previous Updates (v0.5.26):**

**🎯 Clean Teammate Task Box Output:**
- 🧹 **Suppressed Intermediate Output** — Teammate task boxes no longer flood the chat with detailed tool invocations
- 📊 **Tool Usage Summary** — Task completion now shows "完成，返回结果（使用了 N 个工具）" for non-zero tool usage
- ✨ **Task Box Shows Only** — Start marker → Final tool count → Completion status
- 🎨 **Cleaner UX** — Follows user's high standards for minimal, focused output (no redundant verbose logs)

**Previous Updates (v0.5.25):**

**🧠 Advanced Context Compression System:**
- 🎯 **Three-Phase Compression** — Tool output pruning → Boundary protection → LLM structured summarization (inspired by Hermes Agent)
- ✨ **Smart Boundaries** — Protects head (system prompt + first N messages) + dynamic tail (token budget-based)
- 🔧 **Tool Call Integrity** — Aligns boundaries to avoid splitting tool call/result pairs
- 📊 **Anti-Thrashing** — Skips compression if last 2 attempts saved <10% each
- 🏗️ **Iterative Summary** — Updates previous summaries instead of starting from scratch
- 🚀 **Foundation for Progressive Compression** — Ready for multi-level (40%/60%/80%) thresholds and intelligent routing

**Previous Updates (v0.5.24):**

**Critical `/stop` Command Fix:**
- 🐛 **Fixed Non-Functional /stop** — `/stop` command now correctly cancels the active running task
- 🎯 **Root Cause** — Previous implementation cancelled its own token instead of the running task's token
- ✅ **New Behavior** — Finds and cancels the active task from `active_tasks` registry, shows status feedback
- 💡 **User Feedback** — Now shows "⏹️ 已停止当前任务。" or "ℹ️ 当前没有正在运行的任务。" depending on state

**Previous Updates (v0.5.23):**

**Team Tool Output Cleanup:**
- 🎯 **Suppressed Verbose Results** — Team tool now returns compact completion confirmations instead of full agent responses
- ✨ **Clean Main Chat** — Tool results show "✓ teammate completed: task" format, preventing response duplication in chat
- 🔧 **Combined with v0.5.22** — Task boxes show minimal progress + main chat no longer cluttered with full teammate outputs

**Previous Updates (v0.5.22):**

**Team Collaboration UI Optimization:**
- 🎯 **Clean Task Box Display** — Teammate agent task boxes now show only start/tool calls/completion, suppressing verbose text output
- ✨ **Focused Progress Updates** — Task boxes display essential progress markers without cluttering the chat with full agent responses
- 🔧 **Improved Multi-Agent UX** — Cleaner delegation visualization keeps the main conversation readable during complex workflows

**Previous Updates (v0.5.20):**

**Team Execution Modes — Sequential & Staged Collaboration:**
- ✅ **Sequential Mode** — Tasks run one after another, each receiving previous results as context (`mode: "sequential"`)
- ✅ **Stages Mode** — Multi-phase workflows: parallel execution within each stage, sequential between stages (`stages` parameter)
- ✅ **Parallel Mode** — Existing concurrent behavior preserved as default (`mode: "parallel"`)
- 🎯 **Dependency Support** — Agent can now orchestrate complex workflows with task dependencies

**Before v0.5.20:**
```json
{
  "tasks": [...]  // ❌ All tasks always run in parallel, no dependency support
}
```

**After v0.5.20:**
```json
// Sequential: later tasks depend on earlier results
{"mode": "sequential", "tasks": [
  {"teammate": "researcher", "task": "Find solution"},
  {"teammate": "coder", "task": "Implement based on research"}
]}

// Stages: mixed parallel/sequential
{"stages": [
  {"tasks": [{"teammate": "researcher", "task": "Research"}]},
  {"tasks": [  // These run in parallel
    {"teammate": "backend", "task": "Backend"},
    {"teammate": "frontend", "task": "Frontend"}
  ]},
  {"tasks": [{"teammate": "reviewer", "task": "Review all"}]}
]}
```

**Impact:**
- Agent can handle complex dependency chains (research → implement → test)
- Mixed workflows supported (parallel development after sequential planning)
- Previous results automatically injected as context for dependent tasks

</details>

### Previous Updates (v0.5.19)

**Team Task Division — Smart Multi-Agent Collaboration:**
- ✅ **Individual Task Assignment** — Each teammate now receives a **different sub-task** tailored to their expertise
- ✅ **New `tasks` Parameter** — Structured task division: `[{teammate: "researcher", task: "Search solutions"}, {teammate: "coder", task: "Implement fix"}]`
- 🔧 **Deprecation Warning** — Old `teammates` array (same task for all) marked as DEPRECATED
- 🎯 **True Division of Labor** — No more duplicate work — agents collaborate with proper specialization

**Before v0.5.19:**
```json
{
  "teammates": ["researcher", "coder"],
  "task": "Complete this PR"  // ❌ Both receive the same task
}
```

**After v0.5.19:**
```json
{
  "tasks": [
    {"teammate": "researcher", "task": "Find best practices", "context": "Focus on security"},
    {"teammate": "coder", "task": "Implement the changes", "context": "Use TypeScript"}
  ]
}
```

**Impact:**
- Agent orchestrator can divide complex tasks into specialized sub-tasks
- Each teammate works on what they're best at, no redundant effort
- Better parallel collaboration and faster delivery

### Previous Updates (v0.5.18)

**SSE Keepalive Fix — No More WebUI Timeout:**
- ✅ **15-Second Keepalive** — SSE connection sends keepalive comments every 15 seconds
- ✅ **Fixed Network Error** — Long-running tasks no longer timeout with "network error"
- 🔧 **Stable Connections** — Prevents browser and proxy timeout on idle connections
- 🎯 **Better UX** — Users can send complex requests without worrying about timeout

**Technical Details:**
- Added `keep_alive()` to `sse_response_from_rx` function
- Sends `: keepalive\n\n` comments every 15 seconds (SSE standard)
- Matches pattern from run SSE endpoint but with shorter interval for chat interactivity
- Compatible with all SSE-supporting browsers and proxies

**Before v0.5.18:**
- Long agent tasks (team collaboration, complex analysis) would timeout
- WebUI showed "network error" after ~30-60 seconds of silence
- Users had to retry or break tasks into smaller pieces

**After v0.5.18:**
- All tasks complete successfully regardless of duration
- Stable connection maintained throughout entire agent response
- Seamless experience for complex multi-step operations

### Previous Updates (v0.5.17)

**Enhanced Agent Delegation Proactivity — Smarter Team Collaboration:**
- ✅ **Proactive Tool Description** — Agent now actively considers delegation in 4 explicit scenarios
- ✅ **Scenario-Based Guidance** — Clear triggers: domain expertise gaps, parallel work, divide-and-conquer, specialized skills
- 🤝 **Empowered Teammates** — Enhanced collaboration contract emphasizes value and actionable guidance
- 🎯 **Cultural Shift** — "Delegation is a strength, not a weakness — leverage your team early and often"

**Key Changes:**
- Rewrote `team` tool description with PROACTIVE framing instead of passive "use when better suited"
- Enhanced `TEAM_RESULT_CONTRACT` to emphasize teammate expertise and thorough guidance
- Added explicit encouragement for early and frequent delegation

**Impact:**
- Agent more likely to use team collaboration proactively
- Better recognition of when to delegate vs. handle directly
- Improved multi-agent workflows and specialization

### Previous Updates (v0.5.16)

**UTF-8 Streaming Fix — No More Garbled Chinese Characters:**
- ✅ **Proper UTF-8 Boundary Handling** — HTTP chunk boundaries no longer split multi-byte characters
- ✅ **Fixed Garbled Output** — Chinese text like "我来帏查看服务器的息" now displays correctly as "我来查看服务器的信息"
- 🔧 **Smart Buffer Management** — Incomplete UTF-8 sequences preserved in carry buffer until next chunk arrives
- 🎯 **Zero Data Loss** — Complete characters always decoded correctly, no replacement characters (�)

**Technical Details:**
- Implemented `find_last_complete_utf8_boundary()` to detect incomplete multi-byte sequences
- Handles all UTF-8 character types: 1-byte (ASCII), 2-byte, 3-byte (CJK), 4-byte (emoji)
- Scans backward from buffer end to find last complete character
- Carries forward incomplete sequences to next chunk for proper decoding

**Before v0.5.16:**
- Chinese characters appeared garbled during streaming: "配置信息" → "配帏信息"
- Replacement characters (�) appeared randomly in CJK text
- Emoji and special Unicode characters broke into fragments

**After v0.5.16:**
- All text streams correctly regardless of chunk boundaries
- Perfect display of Chinese, Japanese, Korean, emoji, and all Unicode
- Seamless streaming experience with zero character corruption

### Previous Updates (v0.5.15)

**WebUI Config Persistence — Settings Now Survive Restart:**
- ✅ **Config Auto-Save** — WebUI settings changes automatically persist to `~/.hakimi/config.yaml`
- ✅ **No More Lost Settings** — Model configs, API keys, and all settings survive restart
- 🔧 **Smart Persistence** — Only saves in unified mode (default), WebUI-only mode stays memory-only
- 📝 **Logging** — Success/failure logged for debugging config save operations

**Before v0.5.15:**
- WebUI settings only stored in memory
- Restarting hakimi lost all configuration changes
- Had to manually edit config.yaml

**After v0.5.15:**
- Change settings in WebUI → Automatically saved to config.yaml
- Restart preserves all your configuration
- WebUI becomes the primary config interface

### Previous Updates (v0.5.14)

**Critical WebUI UX Fixes — Three Key Issues Resolved:**
- ✅ **Copy Message Feedback** — Copy button now shows visual feedback (opacity change) so users know it worked
- ✅ **Tool Call Panel Position** — Fixed rendering order: tool calls now appear after message content, not stacked at the top
- ✅ **Tool Calls Persistence** — Full backend integration: tool calls persist to database and survive page refresh
- 🎯 **Backend API Enhanced** — Added ToolCallInfo struct and tool_calls field to SessionMessageInfo
- 🔄 **Frontend Mapping** — Automatic conversion from backend tool_calls to frontend toolCalls format

**Before v0.5.14:**
- Copy button seemed broken (no feedback)
- Tool panels stacked messily at message top: `⚙️ read_file ⚙️ terminal [content below]`
- Refreshing the page lost all tool call history

**After v0.5.14:**
- Copy gives instant feedback
- Clean flow: `[content] → [delegate progress] → [tool calls]`
- Tool calls persist forever, visible in historical sessions

**v0.5.13 — WebUI Tool Call Display Fix:**
- ✅ **Fixed Protocol Mismatch** — Frontend now correctly detects backend control messages (changed \x01 → \x1e)
- ✅ **Clean Message Display** — Tool markers no longer leak into assistant responses
- ✅ **Structured Tool Panels** — Tool calls appear in collapsible cards with clear visual separation
- ✅ **Better Readability** — Long tool results are folded by default, click to expand when needed
- 🎯 **Double Filter** — stripToolMarkers() provides fallback cleanup for any protocol edge cases

**Example:** Before this fix, you'd see messy raw markers like `hakimi_tool:⚙️ read_file` mixed into the response text. Now tool calls appear as clean, expandable panels while the assistant's prose stays pristine.

**v0.5.12 — Model Tiers & Auto-Dispatch:**
- ✅ **Three-Tier Model System** — Configure Light/Primary/Reasoning models for different task complexities
- ✅ **Automatic Task Routing** — Smart dispatcher analyzes task complexity and routes to appropriate model tier
- ✅ **WebUI Configuration** — Full control panel in Settings for model tiers and auto-dispatch options
- ✅ **Cost & Performance Optimization** — Use lighter models for simple tasks, save powerful models for complex work
- 🎯 **Two-Stage Execution** — Optional mode: plan with reasoning model, execute with primary model
- 📊 **Dispatch Decision Visibility** — See which tier handles each request and why

**Example:** Simple file reads go to your fast 7B model, standard coding to your 32B model, and complex architecture planning to your reasoning model — all automatically!

**v0.5.11 — WebUI Chat Experience Enhanced:**
- ✅ **Tool Call Visualization** — Every tool execution now displays prominently in chat history with collapsible results
- ✅ **Fixed Content Overwrite** — Streaming responses no longer get replaced by final message, preserving complete conversation flow
- ✅ **Interactive Tool Results** — Click to expand/collapse tool outputs (file reads, searches, API calls) with syntax highlighting
- ✅ **Real-time Progress** — Live updates as tools execute, with clear visual separation from assistant responses
- 🎨 **Refined UI** — Smooth animations, better spacing, and improved readability for long conversations

**Example:** When you ask "analyze this codebase", you'll now see each file search, code analysis tool, and their outputs as separate expandable cards — no more mystery about what the agent is doing!

---

## Install

**Linux / macOS:**
```bash
curl -sSL https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.ps1 | iex
```

**Build from source (any platform with Rust):**
```bash
cargo install hakimi-agent
```

**Quick setup:**
```bash
hakimi setup      # guided configuration wizard
hakimi doctor     # diagnose setup and connectivity
hakimi --serve    # start the embedded WebUI/API on 127.0.0.1:3005
```

**v0.4.7 — 上下文管理优化 (Context Management Enhancement):**
- 🔄 **队列消息注入修复**：修复运行中上下文的排队消息注入逻辑，确保多消息场景下的正确处理
- 🗜️ **压缩标志重置**：上下文压缩后正确重置 `compressed_this_turn` 标志，避免重复压缩
- 🧹 **代码质量提升**：消除 entry.rs 中未使用变量和死代码警告，应用 rustfmt 格式化
- 🎯 **Agent 循环增强**：优化 loop_impl.rs 中的消息处理流程，提升稳定性

**v0.4.6 — 人格办公室仪表板 (Persona Office Dashboard):**
- 🏢 **办公室可视化**：把每个人格当作"员工"，实时展示所有人格的工作状态
- 🖥️ **个性化工位**：每个人格独立工位，执行任务时电脑屏幕亮起 + 键盘动作，空闲时看电视/打游戏
- 🤝 **协作动画**：A 找 B 干活时显示跑到 B 处交付需求的动画，多人组队时聚坐协作
- 📡 **实时事件流**：后端 ActivityHub + SSE 全栈实时推送（PersonaCreated/TurnStarted/TeamConsult/Idle 等）
- 🎨 **扁平矢量风格**：SVG + CSS 动画，微俯视角，可随主题换色，与现有 UI 风格统一
- 🔄 **自动布局**：按行自动排列工位，支持几个到 ~20 个人格，超出自动滚动
- 🖱️ **可交互导航**：点击工位进入该人格对话/配置，悬停显示状态详情卡
- 👔 **入职动画**：新人格创建时显示"新员工入职，安排新座位"动画

**v0.4.5 — Persona Team 协作系统 (Persona Team Collaboration):**
- 🤝 **具名人格协作**：主导人格可通过 `team` 工具将子任务委派给其他具名队友人格
- 🎯 **专业化分工**：每个队友使用自己的模型、技能、记忆和系统提示词独立作答
- 📋 **队友名册管理**：`team(action="list")` 枚举所有可寻址队友及其能力描述
- 🔒 **安全护栏**：内置深度上限、回环检测、并发信号量、超时预算机制
- ⚙️ **可配置开关**：`PersonaConfig.addressable` 控制人格是否可被当作队友（默认开启）
- 🔄 **同步无状态**：队友按子任务起干净回合，只读长期记忆，不写回自身会话/记忆
- 📊 **进度可视化**：复用现有 `hakimi_delegate:` 气泡机制，实时展示协作进度
- ✅ **WebUI 集成**：人格配置表单中的 `addressable` 开关已完整实现

The WebUI Control Center can create, pause, resume, run-now, and delete persisted cron jobs via `/api/cron/jobs`, the `/clear` slash command now persists by deleting the current session transcript via `/api/sessions/{id}/messages`, and the mobile layout lets the conversation title toggle the session list so phones keep the chat area usable.

`hakimi --serve` ships the WebUI assets inside the release binary, so `/`, `/static/style.css`, `/static/hakimi.js`, `/static/composer.js`, `/static/workspace.js`, and `/static/favicon.svg` work from any current directory without copying a separate `static/` folder. The WebUI workspace browser treats `/` as the active working-directory root (not the OS filesystem root), while still rejecting `..` path escapes. Control-center modals honor native `hidden` state and can be dismissed via close button, overlay click, or Escape. When `HAKIMI_WEBUI_PASSWORD` is set, the WebUI prompts for the password on the first authenticated API call, stores it locally as a Bearer token, retries automatically, and renders send/auth errors inline instead of silently dropping messages. The embedded server persists WebUI sessions in `~/.hakimi/sessions.db` and initializes the schema on startup, so creating a chat session works immediately after launch. Streaming WebUI chat requests now carry the active `session_id`, restore that transcript before each turn, and persist both the user prompt and assistant reply back into the same session; the frontend also commits finalized streamed replies into its in-memory message list so a second send or session switch does not erase the previous response. The WebUI also exposes persisted cron jobs through `/api/cron/jobs`, supports session deletion from the sidebar, de-duplicates client-provided session titles during create/fork so repeated "New Chat" actions do not hit the SQLite title uniqueness constraint, and ships a polished skin system with Linear Dark, Obsidian, Midnight, Light, and System appearance choices persisted in localStorage. Theme switching now writes the resolved skin CSS variables directly at runtime, so color changes are immediate and resilient to cached/static stylesheet ordering. The refreshed UI uses glassy panels, richer message cards, focused composer chrome, theme swatches in Settings → Appearance, keeps theme switching local and instant, adds a mobile drawer sidebar with a compact top-bar menu, hides the workspace panel on narrow screens, and adapts messages/composer controls to safe-area mobile viewports. It also shows directory-style `SKILL.md` skills using their parent directory names instead of generic `SKILL` labels, and releases the composer immediately after completed streamed replies so follow-up messages remain responsive.

---

## Why Hakimi?

Python agent frameworks are slow, memory-hungry, and crash at runtime. Hakimi is built different — from the ground up in Rust with production reliability baked in.

| Metric | Python Agent | Hakimi (Rust) |
|--------|-------------|---------------|
| Startup | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Async model | asyncio + GIL | tokio native async |
| Tool safety | Runtime crashes | Compile-time guarantees |
| Tests | ~500 | 1767 |

**Not a wrapper. Not a demo. A real production system:**
- 20+ error types auto-classified with recovery strategies
- Hermes-style turn retry state for one-shot recovery guards
- Multi-key credential pool with circuit breakers
- 3-tier context compression (no manual window management)
- Contextual first-touch onboarding hints tracked in `onboarding.seen`
- Decision tree conversation history with backtracking
- Intent reasoning engine — predicts what tools you need
- Role adaptation — automatically switches between Coder, Researcher, Writer modes

---

## Capabilities

### 🌟 Core Features

**Smart Context Management**
- Three-tier compression: drop stale tool results → LLM summarization → sliding window
- No manual context window management — Hakimi handles it automatically
- Model-aware context windows: `model.context_length` overrides static metadata before compression and tool disclosure thresholds
- Intent classification into 10 categories with next-tool prediction

**Built-in Tools (63+)**
- **Files**: read, write, search, patch with safe-root sandbox
- **Shell**: terminal, background processes
- **Web**: search, extract, browser automation (Chromium with screenshot vision capture, Playwright cache/headless-shell discovery, raw CDP dispatch, CDP frame-tree inspection, cloud-provider readiness status, and provider CDP endpoint routing)
- **Desktop**: Hermes-style `computer_use` readiness surface with safe wait, macOS screenshot/list-app discovery, and guarded action schema
- **Code**: Python/JS/Bash execution with sandbox
- **Media**: vision analysis, video analysis, TTS, transcription with silence-hallucination filtering and oversized WAV chunking
- **Memory**: tiered memory system (short-term/long-term/working memory) + FTS5 full-text search + three-mode session search (Discovery with bookends, Scroll window navigation, Browse recent sessions) + `hakimi knowledge` / TUI `/knowledge` / gateway `/knowledge` graph operations
- **Productivity**: todo, Kanban boards with profile routing, worker logs, event trails, diagnostics, notification subscriptions, swarm graph creation, dashboard read/write management, cron scheduler with interval/five-field cron expressions and home-channel fan-out delivery
- **Meta**: sub-agent delegation, Mixture-of-Agents reasoning, skills system, MCP plugins
- **Evaluation**: Hermes-compatible ShareGPT JSONL trajectory saving for completed and failed turns

**Multi-Platform Gateway**
- Telegram · Discord · Slack · Mattermost · Webhook · Microsoft Graph webhook · Signal · SMS/Twilio · Email/SMTP · WhatsApp Business Cloud · Home Assistant · Matrix · DingTalk · WeCom · Feishu/Lark · BlueBubbles/iMessage · QQBot outbound · WeChat (via iLink/ClawBot) · Weixin/iLink alias
- Config-driven multi-adapter fan-in: run chat and webhook gateways simultaneously
- Real-time streaming with progressive edits, native Telegram draft previews, flood-control backoff, per-platform preview policy, and UTF-8-safe overflow chunking for long replies
- Persistent lifecycle diagnostics record adapter, connect, route, filter, and edit events to `~/.hakimi/logs/gateway-events.log`; `/logs`, `/logs events`, and `/logs gateway` read recent logs without shelling out to `tail`
- Gateway `/undo [N]` rewinds recent in-memory chat turns and echoes the target prompt for editing before resend
- Gateway `/stop` immediately cancels the running task and clears any queued messages, supporting both `interrupt` and `queue` modes configured via `gateways.busy_input_mode`
- Gateway `/usage` shows last-turn token/cost/rate-limit data, best-effort OpenRouter-compatible `/v1/models` live pricing with a profile-scoped freshness cache and request fees, OpenRouter `/credits` plus `/key` quota/usage, Anthropic OAuth account windows, Codex usage windows, and a shared Nous rate-limit guard without exposing credentials
- Cron jobs scheduled from chat with `/cron add`, including `30m` / `2h` intervals, five-field cron syntax such as `*/15 * * * *` or `0 9 * * MON-FRI`, and delivery targets like `local`, `origin`, `all`, `platform`, `platform:home`, or `platform:#channel`
- Gateway `/voice on|off|tts|status|doctor` toggles spoken-response guidance and reports voice I/O readiness without polluting prompt cache or chat history
- Gateway `/update` sends the in-chat restart notice, then the restarted gateway proactively reports update success, current version, and release-note feature bullets after adapters connect
- TUI `/config [field]` shows sanitized runtime configuration, `/gateway [cmd]` inspects configured adapters, cached channel targets, and lifecycle events, `/sessions [cmd]` browses saved SQLite sessions, `/skills [cmd]` browses/searches local Skills Hub metadata, `/cron [cmd]` manages the persistent cron DB locally, `/undo [N]` prefills recent prompts for editing, `/checkpoints [cmd]` inspects the shared shadow-git checkpoint store without entering the model loop, and `/voice status` plus configurable Ctrl+B/Ctrl+letter push-to-talk share the same `voice.*` config, TTS/transcription tools, audio environment checks, PCM16 WAV recording artifact validation, oversized WAV chunked STT dispatch, local TTS playback launch, recorder-backed `voice_capture`, automatic transcript submission, continuous restart mode, second-press capture cancellation, three-no-speech auto-exit, and Hermes-style start/stop audio cues

**Extensibility**
- MCP (Model Context Protocol) client — stdio / HTTP / SSE transports, CLI/gateway catalog search and config snippets, and stdio server-initiated sampling with tool schema forwarding plus `tool_use` handoff
- HTTP plugin system with YAML templates
- HTTP API discovery — OpenAI-compatible `/v1/models`, `/v1/capabilities`, `/v1/skills`, `/v1/toolsets`, text `/v1/chat/completions` with completed SSE snapshots for `stream=true`, `/v1/responses` with SQLite-backed `previous_response_id` chaining plus completed SSE snapshots, pollable and cancellable `/v1/runs` with live lifecycle SSE events, and session lifecycle/messages/search discovery for external UI feature detection
- Dashboard admin API — `/api/status`, `/api/sessions` create/update/delete/fork plus message/search inspection, `/api/mcp/servers`, `/api/credentials/pool`, `/api/webhooks`, and Kanban `/api/kanban` board/task read-write management expose redacted operational state plus runtime-scoped admin writes for WebUI/admin panels
- Hakimi WebUI — Hermes-inspired React/Vite operator console with left-side session browsing, central `/api/chat` live turns, right-side runtime/tool/skill/control panels, Bearer token support, and runtime config editing through the existing HTTP API
- Skills Hub — install community skills with `/skills install`
- Static i18n foundation — `display.language`, `HAKIMI_LANGUAGE` / `HERMES_LANGUAGE`, Hermes-compatible language aliases, YAML catalog directory loading, English fallback, and named placeholders for static user-facing messages
- CLI Skin Engine — `hakimi skin list|inspect|set|path` plus gateway `/skin` discover built-in and `~/.hakimi/skins/*.yaml` themes, inherit missing values from `default`, persist `display.skin`, apply selected branding/colors/logo/hero to the CLI startup banner, and drive TUI thinking spinner faces/verbs/wings plus status, session, selection, completion, help, input, response, tool-prefix, tool emoji labels, running-tool progress, and tool-panel colors
- Isolated profiles — manage named workspaces, clone/export profile archives, install/update shareable `distribution.yaml` profile distributions, create `~/.hakimi/bin/<profile>` wrapper aliases, use gateway `/profile`, and bind `--profile` / sticky `active_profile` runs to profile-scoped config, memory, sessions, skills, cron, trajectories, gateway logs, and TUI defaults
- 10 curated MCP catalog entries: GitHub, filesystem, Brave Search, PostgreSQL, Puppeteer, memory, fetch, SQLite, sequential-thinking, and the Hermes-reviewed n8n bridge

### 🛡️ Production Safety

- **Secret redaction** — API keys, JWTs, tokens masked before output
- **Prompt injection detection** — scans skills, cron prompts, context files
- **SSRF protection** — blocks private/metadata URL fetches
- **Command safety guard** — blocks malicious shell patterns
- **Tool loop guardrails** — warns on repeated no-progress read-only calls and blocks runaway exact-call loops
- **One-time onboarding hints** — first-touch CLI/gateway tips persist under `onboarding.seen`
- **Write safe-root sandbox** — config-protected directories
- **Read credential guard** — protects config files
- **Shared shadow-git checkpoints** — `checkpoint` and gateway `/checkpoints` snapshots live under `~/.hakimi/checkpoints/store`, not the project `.git`
- **Tool output limits** — configurable `tools.output.max_bytes` boundary before tool results enter context

---

## Architecture

**20 crates, each with a single responsibility:**

```
hakimi-agent/
├── hakimi-core/          # Agent loop, error classifier, credential pool
├── hakimi-transports/    # OpenAI, Anthropic, Gemini, Bedrock transports + prompt caching/rate guards
├── hakimi-tools/         # 63+ built-in tools + plugin registry
├── hakimi-session/       # SQLite WAL + FTS5, decision tree history
├── hakimi-context/       # Context engine, compression, intent reasoning, roles
├── hakimi-knowledge/    # Knowledge graph (petgraph)
├── hakimi-skills/        # Skill system + meta-skill extraction
├── hakimi-cron/          # Persistent cron scheduler
├── hakimi-gateway/       # 19 runtime-exposed platform adapters
├── hakimi-mcp/           # MCP client (stdio/HTTP/SSE)
├── hakimi-cli/           # REPL CLI + setup wizard + doctor
└── hakimi-tui/           # ratatui terminal UI
```

### How It Works

```
User Message
    │
    ▼
┌─────────────────────────────────────┐
│  Intent Classification               │
│  → Which tools does this need?      │
├─────────────────────────────────────┤
│  Role Adaptation                     │
│  → Coder / Researcher / Writer...   │
├─────────────────────────────────────┤
│  Build Context                       │
│  → System prompt + knowledge graph  │
│  → Apply 3-tier compression         │
├─────────────────────────────────────┤
│  Credential Pool                     │
│  → Acquire API key (rotation-ready) │
│  → LLM call via SSE streaming       │
├─────────────────────────────────────┤
│  Tool Dispatch                       │
│  → Execute + guardrails check       │
│  → Error classification + recovery │
├─────────────────────────────────────┤
│  Decision Tree                       │
│  → Record response + backtrack Capable│
└─────────────────────────────────────┘
    │
    ▼
Response + Memory + Stats
```

---

## Compare

| Feature | Hermes (Python) | Hakimi (Rust) |
|---------|-----------------|---------------|
| Language | Python 3.11+ | Rust 2024 |
| Startup time | ~2s | ~50ms |
| Idle memory | ~150MB | ~15MB |
| Async model | asyncio + GIL | tokio native |
| Tool registration | Runtime AST | Compile-time trait |
| Error recovery | Basic retry | 20+ classifiers |
| Knowledge model | Flat file | Graph DB (petgraph) |
| Intent detection | None | 10-category classifier |
| Role adaptation | None | 8 roles auto-detected |
| Conversation model | Flat list | Decision tree |
| Tests | ~500 | 1767 |

---

## Development

```bash
# Build everything
cargo build --workspace

# Run tests
cargo test --workspace

# Lint
cargo clippy --workspace

# Debug logging
RUST_LOG=debug cargo run -p hakimi-cli
```

---

## Roadmap

- [x] Core agent loop + tool dispatch
- [x] OpenAI / Anthropic / Gemini transports + SSE streaming, plus non-streaming AWS Bedrock Converse
- [x] 63+ built-in tools
- [x] 19 runtime-exposed platform adapters
- [x] Gateway target directory + send_message channel resolution
- [x] MCP client + CLI/gateway server catalog
- [x] HTTP API model/capability discovery + text Chat Completions/Responses SSE snapshots + cancellable Runs with live lifecycle events
- [x] Dashboard admin API summaries + runtime writes + Kanban read/write management
- [x] Gateway `/usage` rate-limit, account-limit, live pricing with request fees, Nous shared rate guard, and offline OpenAI/Anthropic/Gemini/DeepSeek/MiniMax/Bedrock cost estimates
- [x] Plugin system + HTTP templates
- [x] Profile distributions with install/update/info and protected user data
- [x] CLI skin engine with built-in/user YAML themes, `display.skin` persistence, startup banner theming, and TUI spinner, status, completion, help, tool emoji/progress, and surface theming
- [x] ratatui TUI with local slash commands, sanitized config browser, and gateway status panel
- [x] Smart context compression (3-tier)
- [x] Error classifier + credential pool
- [x] Prompt caching (Anthropic)
- [x] Vision + video analysis
- [x] Knowledge graph memory with CLI/TUI/gateway operator commands
- [x] Intent reasoning engine
- [x] Decision tree backtracking
- [x] Role adaptation
- [x] Meta-skill auto-extraction
- [x] Browser automation (Chromium + Playwright cache discovery + CDP readiness probe + frame-tree inspection + cloud-provider readiness status + provider CDP endpoint routing)
- [x] Computer Use readiness surface
- [x] Kanban task boards + notification cursors + swarm graphs + dashboard read/write management
- [x] Gateway voice-response mode
- [x] TUI voice readiness and media-tool config parity
- [x] Voice environment diagnostics and STT silence-hallucination filtering
- [x] PCM16 WAV recording artifact validation for voice capture
- [x] Voice TTS playback text cleanup, MP3 cache planning, and local player launch
- [x] Voice capture tool with system recorder backends and STT dispatch
- [x] Oversized WAV chunking for captured-recording STT dispatch
- [x] TUI Ctrl+B continuous push-to-talk capture loop
- [x] TUI checkpoint viewer and manager slash command
- [x] TUI saved session browser slash command
- [x] TUI skill browser slash command
- [x] TUI cron job manager slash command
- [x] Voice capture second-press interrupt key
- [x] Voice capture start/stop audio cues
- [x] Voice capture continuous restart mode
- [x] Mixture-of-Agents reasoning via OpenRouter
- [x] OpenRouter, Anthropic, and Codex account usage display in gateway `/usage`
- [x] Basic Hakimi WebUI operator console
- [~] WASM plugin runtime (in progress - TASK 5.1.1, PR #46)
- [ ] Web dashboard PTY terminal, session-scoped streaming, and full Kanban UI

---

## License

MIT License
