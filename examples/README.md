# Hakimi WASM Plugin Examples

这个目录包含多个实用的 WASM 插件示例，展示不同应用场景和最佳实践。

## 🎯 可用插件

### 1. Hello WASM Plugin
**目录**: `hello-wasm-plugin/`  
**功能**: 基础示例插件，展示插件结构和基本功能  
**大小**: ~47 KB  
**技术**: 基础宏使用、日志记录、字符串处理

### 2. Weather Plugin  
**目录**: `weather-plugin/`  
**功能**: 查询城市天气信息  
**大小**: ~67 KB  
**技术**: 结构化数据处理、serde 序列化、模拟 API 响应

### 3. JSON Formatter Plugin
**目录**: `json-formatter-plugin/`  
**功能**: 格式化和验证 JSON 字符串  
**大小**: ~101 KB  
**技术**: JSON 解析、pretty-printing、错误处理

### 4. Markdown Plugin
**目录**: `markdown-plugin/`  
**功能**: 将 Markdown 转换为格式化文本  
**大小**: ~69 KB  
**技术**: 文本解析、格式转换、字符串处理

### 5. Snippet Store Plugin
**目录**: `snippet-store-plugin/`  
**功能**: 存储和检索代码片段  
**大小**: ~54 KB  
**技术**: 数据结构管理、多语言支持、格式化输出

## 🛠️ 构建所有插件

使用提供的脚本一次性构建所有插件：

```bash
./build_all_plugins.sh
```

这将编译所有插件并显示构建摘要：
- 成功/失败计数
- 每个插件的大小
- 总体大小统计

## 📦 单独构建

在任何插件目录中：

```bash
cargo build --target wasm32-wasip1 --release
```

编译后的 WASM 文件位于：
```
target/wasm32-wasip1/release/<plugin_name>.wasm
```

## 🚀 使用插件

### 1. 安装插件

```bash
hakimi plugin install examples/weather-plugin/target/wasm32-wasip1/release/weather_plugin.wasm
```

### 2. 列出已安装插件

```bash
hakimi plugin list
```

### 3. 执行插件

```bash
hakimi plugin execute weather-plugin
```

### 4. 查看插件信息

```bash
hakimi plugin info weather-plugin
```

## 📚 插件开发指南

每个插件都遵循相同的结构：

### 基本结构

```
my-plugin/
├── Cargo.toml          # 依赖配置
├── README.md           # 插件文档
└── src/
    └── lib.rs          # 插件实现
```

### Cargo.toml 模板

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
hakimi-plugin-sdk = { path = "../../crates/hakimi-plugin-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = "z"      # 大小优化
lto = true           # 链接时优化
strip = true         # 删除调试符号
```

### 插件代码模板

```rust
use hakimi_plugin_sdk::*;

#[hakimi_plugin(
    name = "my-plugin",
    version = "0.1.0",
    author = "Your Name",
    description = "Plugin description"
)]
pub struct MyPlugin;

impl MyPlugin {
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        ctx.log("info", "Plugin executing");
        
        // 你的插件逻辑
        
        Ok("Result".to_string())
    }
}
```

## 🎨 最佳实践

### 1. 错误处理
- 使用 `PluginResult<T>` 作为返回类型
- 提供清晰的错误信息
- 避免 panic，使用 `Result`

### 2. 日志记录
```rust
ctx.log("info", "Operation started");
ctx.log("error", &format!("Failed: {}", error));
```

### 3. 大小优化
- 使用 `opt-level = "z"` 进行大小优化
- 启用 `lto = true` 链接时优化
- 使用 `strip = true` 删除调试符号
- 避免不必要的依赖

### 4. 测试
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin() {
        let plugin = MyPlugin;
        let ctx = PluginContext::default();
        
        let result = plugin.execute(&ctx);
        assert!(result.is_ok());
    }
}
```

## 📊 性能指标

| 插件 | 大小 | 编译时间 | 加载时间 |
|------|------|----------|----------|
| hello-wasm-plugin | 47 KB | ~9s | <1ms |
| weather-plugin | 67 KB | ~9s | <1ms |
| json-formatter | 101 KB | ~9s | <1ms |
| markdown-plugin | 69 KB | ~9s | <1ms |
| snippet-store | 54 KB | ~9s | <1ms |

## 🔧 故障排除

### 构建失败

1. **检查 Rust 版本**
```bash
rustc --version  # 需要 1.95.0+
```

2. **检查 WASM 目标**
```bash
rustup target add wasm32-wasip1
```

3. **清理构建缓存**
```bash
cargo clean
```

### 加载失败

1. **验证 WASM 文件存在**
```bash
ls -lh target/wasm32-wasip1/release/*.wasm
```

2. **检查插件元数据**
```bash
hakimi plugin info <plugin-name>
```

## 🚀 下一步

- 查看 [Plugin SDK 文档](../../crates/hakimi-plugin-sdk/README.md)
- 了解 [插件 API 设计](../../docs/plugin-api.md)
- 探索 [插件市场](https://hakimi-plugins.com)

## 📝 贡献

欢迎提交新的插件示例！请确保：
- 包含完整的 README
- 添加单元测试
- 优化 WASM 大小
- 遵循代码规范

## 📄 许可证

所有示例插件遵循 MIT 许可证。
