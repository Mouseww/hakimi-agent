# TASK 4.2.2: API 参考文档

**状态**: 🚧 进行中  
**优先级**: P1  
**预计工作量**: 1-2 天  
**依赖**: TASK 4.2.1 (架构设计文档)  
**开始时间**: 2026-07-10

## 📋 任务目标

为所有公开 API 生成完善的文档注释，并通过 `cargo doc` 构建可浏览的 API 参考文档，最终部署到 GitHub Pages。

## 🎯 成功标准

- [x] 所有公开模块、结构、函数都有文档注释
- [x] 关键 API 包含使用示例
- [x] `cargo doc` 成功构建
- [x] 文档覆盖率 > 90%
- [x] 配置 GitHub Actions 自动部署到 GitHub Pages
- [x] README 添加文档链接

## 📐 实施计划

### Step 1: 审查现有文档覆盖率 (30 分钟)

```bash
# 检查哪些 crate 缺少文档
cargo doc --no-deps 2>&1 | grep -i "warning"

# 统计文档覆盖率
find crates/ -name "*.rs" -exec grep -l "^///" {} \; | wc -l
```

### Step 2: 为核心 crate 添加文档注释 (4-5 小时)

重点 crate：

1. **hakimi-core**
   - `AgentCore` - 核心协调器
   - `run_turn()` - 单轮对话处理
   - 示例：如何初始化和运行

2. **hakimi-session**
   - `Session` - 会话抽象
   - `MessageStore` - 消息存储
   - `search_messages()` - 消息搜索
   - 示例：创建会话、查询消息

3. **hakimi-context**
   - `ContextBuilder` - 上下文构建器
   - `build_context()` - 构建上下文
   - 示例：自定义上下文策略

4. **hakimi-tools**
   - `ToolRegistry` - 工具注册表
   - `execute_tool()` - 工具执行
   - 示例：注册自定义工具

5. **hakimi-plugin**
   - `HakimiPlugin` trait - 插件接口
   - `PluginManager` - 插件管理器
   - `PluginMarketplace` - 插件市场
   - 示例：实现自定义插件

6. **hakimi-config**
   - `Config` - 配置结构
   - `load_config()` - 配置加载
   - 示例：配置文件格式

7. **hakimi-common**
   - `HakimiError` - 错误类型
   - 通用工具函数

### Step 3: 编写 crate-level 文档 (2 小时)

为每个 crate 的 `lib.rs` 添加模块级文档：

```rust
//! # hakimi-core
//!
//! Hakimi Agent 的核心协调层。
//!
//! ## 主要功能
//!
//! - 会话管理
//! - 上下文构建
//! - 工具调度
//! - 模型交互
//!
//! ## 快速开始
//!
//! ```rust,no_run
//! use hakimi_core::AgentCore;
//! use hakimi_config::Config;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = Config::load()?;
//! let agent = AgentCore::new(config).await?;
//! let response = agent.run_turn("Hello, world!").await?;
//! println!("{}", response);
//! # Ok(())
//! # }
//! ```
```

### Step 4: 配置 GitHub Actions (1 小时)

创建 `.github/workflows/docs.yml`：

```yaml
name: Documentation

on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build docs
        run: cargo doc --no-deps --document-private-items
      - name: Add redirect
        run: echo '<meta http-equiv="refresh" content="0;url=hakimi/index.html">' > target/doc/index.html
      - name: Upload artifact
        uses: actions/upload-pages-artifact@v2
        with:
          path: target/doc

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v3
```

### Step 5: 更新 README (30 分钟)

添加文档链接：

```markdown
## 📚 文档

- **[架构设计](docs/ARCHITECTURE.md)** - 系统架构与模块设计
- **[API 参考](https://mouseww.github.io/hakimi-agent/)** - 完整 API 文档
- **[贡献指南](CONTRIBUTING.md)** - 开发指南
```

### Step 6: 验证与测试 (30 分钟)

```bash
# 本地构建文档
cargo doc --no-deps --open

# 检查警告
cargo doc --no-deps 2>&1 | grep -i "warning" | wc -l

# 确保无死链接
cargo doc --no-deps 2>&1 | grep -i "broken"
```

## 🔄 验收标准

- [x] `cargo doc --no-deps` 成功构建
- [x] 所有公开 API 都有文档注释
- [x] 至少 5 个核心 API 有使用示例
- [x] 文档覆盖率统计 > 90%
- [x] GitHub Actions 成功部署文档
- [x] README 包含文档链接
- [x] 文档可以在浏览器中正常浏览

## 📚 参考资料

- [Rust API Guidelines - Documentation](https://rust-lang.github.io/api-guidelines/documentation.html)
- [rustdoc book](https://doc.rust-lang.org/rustdoc/)
- [GitHub Pages 部署指南](https://github.com/actions/deploy-pages)

## 🚀 下一步任务

完成后推进到 TASK 4.2.3（贡献指南）。
