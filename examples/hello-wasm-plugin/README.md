# Hello WASM Plugin

最简单的 Hakimi WASM 插件示例，展示 SDK 的基本用法。

## 构建

### 前置要求

```bash
# 安装 wasm32-wasi 目标
rustup target add wasm32-wasi
```

### 编译插件

```bash
# 从项目根目录
cargo build --manifest-path examples/hello-wasm-plugin/Cargo.toml --target wasm32-wasi --release

# 输出位置
# target/wasm32-wasi/release/hello_wasm_plugin.wasm
```

### 优化后体积

```bash
# 使用 wasm-opt 进一步优化（可选）
wasm-opt -Oz target/wasm32-wasi/release/hello_wasm_plugin.wasm \
  -o hello_wasm_plugin_optimized.wasm
```

## 安装

```bash
# 复制到插件目录
mkdir -p ~/.hakimi/plugins
cp target/wasm32-wasi/release/hello_wasm_plugin.wasm ~/.hakimi/plugins/

# 使用 hakimi CLI 安装（推荐）
hakimi plugin install target/wasm32-wasi/release/hello_wasm_plugin.wasm
```

## 测试

```bash
# 列出已安装插件
hakimi plugin list

# 查看插件信息
hakimi plugin info hello-wasm

# 测试加载
hakimi plugin test hello-wasm
```

## 代码说明

### 最小实现

```rust
use hakimi_plugin_sdk::*;

#[hakimi_plugin(
    name = "hello-wasm",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "A simple hello world WASM plugin"
)]
pub struct HelloPlugin;

impl HelloPlugin {
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        // 使用上下文记录日志
        ctx.log("info", "Plugin executing...");
        
        // 返回结果
        Ok("Hello from WASM! 🎉".to_string())
    }
}
```

### 关键组件

- `#[hakimi_plugin]` 宏：自动生成所需的导出函数和元数据
- `PluginContext`：访问宿主功能（日志、HTTP等）
- `PluginResult<T>`：标准返回类型 `Result<T, String>`

### 元数据

宏会自动生成 `__hakimi_plugin_metadata()` 函数，返回 JSON 格式的插件信息：

```json
{
  "name": "hello-wasm",
  "version": "0.1.0",
  "author": "Hakimi Team",
  "description": "A simple hello world WASM plugin"
}
```

## 下一步

- 查看 SDK 文档：`cargo doc --package hakimi-plugin-sdk --open`
- 查看更多示例插件（天气查询、代码格式化等）
- 开发你自己的插件！

## 故障排除

### 编译错误：`target may not be installed`

```bash
rustup target add wasm32-wasi
```

### 加载错误：`Failed to instantiate module`

检查 WASM 文件完整性：

```bash
wasm-validate target/wasm32-wasi/release/hello_wasm_plugin.wasm
```

### 插件未找到

确保插件已安装并启用：

```bash
hakimi plugin list
hakimi plugin enable hello-wasm
```
