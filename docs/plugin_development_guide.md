# Hakimi 插件开发指南

本指南介绍如何为 Hakimi Agent 开发、编译和部署插件。

## 🚀 快速开始

### 1. 创建插件项目

```bash
cargo new --lib my-plugin
cd my-plugin
```

### 2. 配置 Cargo.toml

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

# 独立工作区（如果在 Hakimi 仓库外）
[workspace]

[lib]
crate-type = ["cdylib"]  # 生成动态库

[dependencies]
hakimi-plugin = "0.1"
```

### 3. 实现插件

在 `src/lib.rs` 中：

```rust
use hakimi_plugin::PluginMetadata;

/// 导出插件元数据（必需）
#[no_mangle]
pub extern "C" fn plugin_metadata() -> PluginMetadata {
    PluginMetadata {
        id: "my_plugin".to_string(),
        name: "My Plugin".to_string(),
        version: "0.1.0".to_string(),
        author: "Your Name".to_string(),
        description: "Plugin description".to_string(),
        dependencies: vec![],
        min_hakimi_version: Some("0.5.0".to_string()),
    }
}
```

### 4. 构建插件

```bash
cargo build --release

# 生成的文件位于：
# Linux:   target/release/libmy_plugin.so
# macOS:   target/release/libmy_plugin.dylib
# Windows: target/release/my_plugin.dll
```

### 5. 安装插件

```bash
# 复制到 Hakimi 插件目录
mkdir -p ~/.hakimi/plugins
cp target/release/libmy_plugin.so ~/.hakimi/plugins/
```

### 6. 配置加载

创建或编辑 `~/.hakimi/plugins.yaml`：

```yaml
plugin_dir: ~/.hakimi/plugins
enable_hot_reload: true
verify_signature: false

plugins:
  - id: my_plugin
    enabled: true
    config:
      # 插件特定配置
      some_option: value
```

## 📚 插件 API

### 元数据结构

```rust
pub struct PluginMetadata {
    /// 插件唯一标识（建议使用反向域名）
    pub id: String,
    
    /// 插件显示名称
    pub name: String,
    
    /// 插件版本（遵循 semver）
    pub version: String,
    
    /// 插件作者
    pub author: String,
    
    /// 插件描述
    pub description: String,
    
    /// 插件依赖（其他插件 ID）
    pub dependencies: Vec<String>,
    
    /// 最低 Hakimi 版本要求
    pub min_hakimi_version: Option<String>,
}
```

### 必需的导出函数

每个插件**必须**导出以下函数：

```rust
#[no_mangle]
pub extern "C" fn plugin_metadata() -> PluginMetadata {
    // 返回插件元数据
}
```

## 🔌 插件生命周期

插件加载和卸载遵循以下生命周期：

1. **加载阶段**
   - Hakimi 加载动态库（.so/.dylib/.dll）
   - 调用 `plugin_metadata()` 获取元数据
   - 验证依赖关系
   - 检查白名单（如果启用）

2. **运行阶段**
   - 插件在 Hakimi 运行期间保持加载
   - 可通过 API 查询插件信息

3. **卸载阶段**
   - 调用插件清理函数（如有）
   - 释放动态库

## 🛠️ 高级功能

### 插件依赖

如果你的插件依赖其他插件：

```rust
PluginMetadata {
    // ...
    dependencies: vec![
        "base_plugin".to_string(),
        "utils_plugin".to_string(),
    ],
    // ...
}
```

Hakimi 会自动检查依赖关系，确保依赖的插件先加载。

### 版本要求

指定插件所需的最低 Hakimi 版本：

```rust
PluginMetadata {
    // ...
    min_hakimi_version: Some("0.5.0".to_string()),
    // ...
}
```

## 🔒 安全注意事项

### 1. 代码执行风险

动态库可以执行任意代码。**仅从可信源加载插件**。

### 2. ABI 兼容性

- 插件应与 Hakimi 使用相同版本的 Rust 编译器构建
- String、Vec 等类型在 FFI 边界上不是 ABI 稳定的
- 当前实现使用简化的元数据传递

### 3. 内存安全

- 不要在插件中使用 `unsafe` 代码，除非绝对必要
- 确保所有分配的内存都被正确释放
- 避免静态变量和全局状态

### 4. 插件签名（未来）

未来版本将支持插件签名验证：

```yaml
verify_signature: true
```

签名验证可以防止加载被篡改的插件。

## 📦 示例插件

完整的示例插件位于 `examples/example_plugin/`：

```bash
cd examples/example_plugin
cargo build --release
cp target/release/libexample_plugin.so ~/.hakimi/plugins/
```

查看源代码以了解最佳实践。

## 🐛 调试插件

### 启用详细日志

```bash
RUST_LOG=hakimi_plugin=debug,my_plugin=debug hakimi
```

### 常见问题

**问题：插件无法加载**

- 检查文件扩展名（.so/.dylib/.dll）
- 确保文件路径正确
- 验证插件目录权限
- 查看 Hakimi 日志中的错误信息

**问题：找不到 `plugin_metadata` 符号**

- 确保函数使用了 `#[no_mangle]` 属性
- 确保函数签名完全匹配
- 检查是否正确导出了符号（`nm` 或 `dumpbin` 工具）

**问题：插件崩溃**

- 检查是否有 `unsafe` 代码
- 验证内存管理
- 使用 `valgrind` 或 `miri` 检测内存错误

## 📊 性能优化

### 1. 减小插件大小

```toml
[profile.release]
opt-level = "z"     # 优化大小
lto = true          # 链接时优化
codegen-units = 1   # 单个编译单元
strip = true        # 移除符号
```

### 2. 异步操作

插件应避免阻塞操作。使用 `tokio` 的异步 API。

### 3. 资源管理

- 及时释放不再使用的资源
- 使用连接池管理数据库连接
- 实现优雅的关闭逻辑

## 🌍 跨平台支持

### Linux

```bash
cargo build --release --target x86_64-unknown-linux-gnu
# 生成 libmy_plugin.so
```

### macOS

```bash
cargo build --release --target x86_64-apple-darwin
# 生成 libmy_plugin.dylib
```

### Windows

```bash
cargo build --release --target x86_64-pc-windows-msvc
# 生成 my_plugin.dll
```

## 🤝 贡献

欢迎提交插件示例和工具！

- 遵循 Rust 编码规范
- 添加完善的文档注释
- 包含单元测试
- 更新 CHANGELOG

## 📄 许可证

示例插件采用 MIT 许可证。您的插件可以使用任何兼容的许可证。

## 📞 支持

- GitHub Issues: https://github.com/Mouseww/hakimi-agent/issues
- Discussions: https://github.com/Mouseww/hakimi-agent/discussions
- 文档: https://hakimi-agent.readthedocs.io
