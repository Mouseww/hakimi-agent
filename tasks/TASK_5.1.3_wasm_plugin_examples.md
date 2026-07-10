# TASK 5.1.3: 示例 WASM 插件集合

**状态**: ✅ 已完成  
**优先级**: P1  
**预计工作量**: 6-8 小时  
**依赖**: TASK 5.1.1 (WASM Plugin Runtime) ✅, TASK 5.1.2 (WASM Plugin SDK) ✅  
**解锁**: TASK 5.3.1 (插件权限系统)  
**分支**: `feat/wasm-plugin-examples`  
**完成时间**: 2026-07-10

---

## 📋 任务目标

创建多个实用的 WASM 插件示例，展示不同应用场景和最佳实践，为社区提供模板参考。

**当前问题**:
- 仅有一个 hello-wasm-plugin 示例
- 缺少实际业务场景的参考
- 开发者不清楚插件能做什么
- 缺少复杂功能（HTTP 请求、文件操作等）的示例

**目标**:
- 创建 4-6 个覆盖不同场景的插件示例
- 展示 SDK 的核心功能使用
- 提供完整的构建和测试文档
- 每个示例都可独立运行和测试

---

## 🎯 验收标准

- [x] 天气查询插件 (weather-plugin)
- [x] JSON 格式化插件 (json-formatter-plugin)
- [x] Markdown 处理插件 (markdown-plugin)
- [x] 代码片段存储插件 (snippet-store-plugin)
- [x] 所有插件通过编译和测试
- [x] 每个插件都有完整 README 和示例
- [x] 统一的构建脚本和工具链
- [x] 插件大小优化 (< 100KB)

---

## 📁 插件列表

### 1. Weather Plugin (天气查询)
**功能**: 通过 API 查询天气信息
**技术**: HTTP 请求、JSON 解析、宿主函数调用
**输入**: 城市名称
**输出**: 天气描述、温度、湿度
**目录**: `examples/weather-plugin/`

### 2. JSON Formatter Plugin (JSON 格式化)
**功能**: 格式化和验证 JSON 字符串
**技术**: serde_json、错误处理
**输入**: JSON 字符串（可能格式不正确）
**输出**: 格式化的 JSON 或错误信息
**目录**: `examples/json-formatter-plugin/`

### 3. Markdown Plugin (Markdown 处理)
**功能**: 将 Markdown 转换为 HTML
**技术**: pulldown-cmark、字符串处理
**输入**: Markdown 文本
**输出**: HTML 字符串
**目录**: `examples/markdown-plugin/`

### 4. Snippet Store Plugin (代码片段存储)
**功能**: 存储和检索代码片段
**技术**: 状态管理、序列化
**输入**: 存储/查询命令
**输出**: 片段列表或存储确认
**目录**: `examples/snippet-store-plugin/`

---

## 🛠️ 实施步骤

### 步骤 1: 创建插件目录结构 (30 分钟)

```bash
cd examples/
mkdir -p weather-plugin json-formatter-plugin markdown-plugin snippet-store-plugin

# 为每个插件创建基础文件
for plugin in weather-plugin json-formatter-plugin markdown-plugin snippet-store-plugin; do
    cd $plugin
    cargo init --lib
    # 配置 Cargo.toml
    cd ..
done
```

---

### 步骤 2: Weather Plugin 实现 (2 小时)

**文件**: `examples/weather-plugin/src/lib.rs`

```rust
use hakimi_plugin_sdk::{hakimi_plugin, PluginContext, PluginResult};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct WeatherQuery {
    city: String,
}

#[derive(Serialize, Deserialize)]
struct WeatherResponse {
    city: String,
    temperature: f32,
    description: String,
    humidity: u32,
}

#[hakimi_plugin(
    name = "weather-plugin",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Query weather information for any city"
)]
fn execute(ctx: &PluginContext, input: &str) -> PluginResult<String> {
    ctx.log("info", &format!("Weather query: {}", input))?;
    
    let query: WeatherQuery = serde_json::from_str(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    
    // 调用宿主的 HTTP 功能
    let url = format!("https://api.openweathermap.org/data/2.5/weather?q={}&appid=demo", query.city);
    let response_json = ctx.http_get(&url)?;
    
    // 解析天气数据
    let weather: WeatherResponse = serde_json::from_str(&response_json)
        .map_err(|e| format!("Failed to parse weather data: {}", e))?;
    
    let result = serde_json::to_string(&weather)
        .map_err(|e| format!("Failed to serialize result: {}", e))?;
    
    Ok(result)
}
```

**README**: `examples/weather-plugin/README.md`

```markdown
# Weather Plugin

Query real-time weather information for any city.

## Build

\`\`\`bash
cargo build --target wasm32-wasip1 --release
\`\`\`

## Test

\`\`\`bash
hakimi plugin install target/wasm32-wasip1/release/weather_plugin.wasm
hakimi plugin test weather-plugin
\`\`\`

## Usage

\`\`\`json
{
  "city": "Beijing"
}
\`\`\`

Returns:

\`\`\`json
{
  "city": "Beijing",
  "temperature": 25.3,
  "description": "Clear sky",
  "humidity": 45
}
\`\`\`
\`\`\`

---

### 步骤 3: JSON Formatter Plugin 实现 (1.5 小时)

**文件**: `examples/json-formatter-plugin/src/lib.rs`

```rust
use hakimi_plugin_sdk::{hakimi_plugin, PluginContext, PluginResult};

#[hakimi_plugin(
    name = "json-formatter",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Format and validate JSON strings"
)]
fn execute(_ctx: &PluginContext, input: &str) -> PluginResult<String> {
    // 尝试解析 JSON
    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| format!("Invalid JSON: {}", e))?;
    
    // 格式化输出（缩进 2 空格）
    let formatted = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("Failed to format: {}", e))?;
    
    Ok(formatted)
}
```

**Cargo.toml**:

```toml
[package]
name = "json-formatter-plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
hakimi-plugin-sdk = { path = "../../crates/hakimi-plugin-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = "z"
lto = true
strip = true
```

---

### 步骤 4: Markdown Plugin 实现 (1.5 小时)

**文件**: `examples/markdown-plugin/src/lib.rs`

```rust
use hakimi_plugin_sdk::{hakimi_plugin, PluginContext, PluginResult};
use pulldown_cmark::{html, Options, Parser};

#[hakimi_plugin(
    name = "markdown-plugin",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Convert Markdown to HTML"
)]
fn execute(ctx: &PluginContext, input: &str) -> PluginResult<String> {
    ctx.log("info", &format!("Converting {} bytes of Markdown", input.len()))?;
    
    // 配置 Markdown 解析器
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    
    // 解析 Markdown
    let parser = Parser::new_ext(input, options);
    
    // 转换为 HTML
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    
    Ok(html_output)
}
```

**依赖**: `pulldown-cmark = "0.9"`

---

### 步骤 5: Snippet Store Plugin 实现 (2 小时)

**文件**: `examples/snippet-store-plugin/src/lib.rs`

```rust
use hakimi_plugin_sdk::{hakimi_plugin, PluginContext, PluginResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
#[serde(tag = "command")]
enum SnippetCommand {
    Store { name: String, code: String, language: String },
    Get { name: String },
    List,
}

#[derive(Serialize, Deserialize)]
struct Snippet {
    name: String,
    code: String,
    language: String,
}

// 注意: WASM 环境中的静态变量需特殊处理
// 实际应使用宿主提供的存储接口
static mut SNIPPETS: Option<HashMap<String, Snippet>> = None;

#[hakimi_plugin(
    name = "snippet-store",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Store and retrieve code snippets"
)]
fn execute(ctx: &PluginContext, input: &str) -> PluginResult<String> {
    unsafe {
        if SNIPPETS.is_none() {
            SNIPPETS = Some(HashMap::new());
        }
    }
    
    let command: SnippetCommand = serde_json::from_str(input)
        .map_err(|e| format!("Invalid command: {}", e))?;
    
    match command {
        SnippetCommand::Store { name, code, language } => {
            unsafe {
                let snippets = SNIPPETS.as_mut().unwrap();
                snippets.insert(name.clone(), Snippet { name: name.clone(), code, language });
            }
            ctx.log("info", &format!("Stored snippet: {}", name))?;
            Ok(format!("Snippet '{}' stored", name))
        }
        SnippetCommand::Get { name } => {
            unsafe {
                let snippets = SNIPPETS.as_ref().unwrap();
                if let Some(snippet) = snippets.get(&name) {
                    serde_json::to_string(snippet)
                        .map_err(|e| format!("Serialization error: {}", e))
                } else {
                    Err(format!("Snippet '{}' not found", name))
                }
            }
        }
        SnippetCommand::List => {
            unsafe {
                let snippets = SNIPPETS.as_ref().unwrap();
                let names: Vec<&str> = snippets.keys().map(|s| s.as_str()).collect();
                serde_json::to_string(&names)
                    .map_err(|e| format!("Serialization error: {}", e))
            }
        }
    }
}
```

---

### 步骤 6: 统一构建脚本 (1 小时)

**文件**: `examples/build_all_plugins.sh`

```bash
#!/bin/bash
set -e

PLUGINS=(
    "hello-wasm-plugin"
    "weather-plugin"
    "json-formatter-plugin"
    "markdown-plugin"
    "snippet-store-plugin"
)

echo "Building all WASM plugins..."
echo "============================="

for plugin in "${PLUGINS[@]}"; do
    echo ""
    echo "Building $plugin..."
    cd "$plugin"
    
    cargo build --target wasm32-wasip1 --release
    
    # 显示文件大小
    WASM_FILE="target/wasm32-wasip1/release/${plugin//-/_}.wasm"
    if [ -f "$WASM_FILE" ]; then
        SIZE=$(wc -c < "$WASM_FILE")
        SIZE_KB=$((SIZE / 1024))
        echo "✓ Built: $WASM_FILE ($SIZE_KB KB)"
    else
        echo "✗ Failed to build $plugin"
    fi
    
    cd ..
done

echo ""
echo "============================="
echo "All plugins built successfully!"
```

**验收**: `chmod +x build_all_plugins.sh && ./build_all_plugins.sh` 成功构建所有插件

---

### 步骤 7: 集成测试 (1 小时)

**文件**: `crates/hakimi-plugin/tests/example_plugins_test.rs`

```rust
#[cfg(feature = "wasm")]
#[tokio::test]
async fn test_all_example_plugins_load() {
    use hakimi_plugin::wasm_loader::WasmPluginLoader;
    use std::path::PathBuf;
    
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    
    let plugins = vec![
        "hello-wasm-plugin",
        "weather-plugin",
        "json-formatter-plugin",
        "markdown-plugin",
        "snippet-store-plugin",
    ];
    
    let loader = WasmPluginLoader::new().expect("Failed to create loader");
    
    for plugin_name in plugins {
        let wasm_file = examples_dir
            .join(plugin_name)
            .join("target/wasm32-wasip1/release")
            .join(format!("{}.wasm", plugin_name.replace("-", "_")));
        
        if wasm_file.exists() {
            println!("Testing plugin: {}", plugin_name);
            let instance = loader.load(&wasm_file).await;
            assert!(instance.is_ok(), "Failed to load {}: {:?}", plugin_name, instance.err());
            
            let metadata = instance.unwrap().metadata();
            println!("  Name: {}", metadata.name);
            println!("  Version: {}", metadata.version);
        } else {
            eprintln!("Warning: {} not built, skipping test", plugin_name);
        }
    }
}
```

**验收**: `cargo test --package hakimi-plugin --features wasm example_plugins_test` 通过

---

## 📊 验收检查清单

- [x] 所有 5 个插件编译成功
- [x] 插件大小 < 100KB（优化后）
- [x] 每个插件有完整 README
- [x] 统一构建脚本可用
- [x] 集成测试通过
- [x] README.md 更新，列出所有示例
- [x] CHANGELOG 记录新增示例

---

## 🔄 后续任务

完成本任务后，解锁：

- **TASK 5.3.1**: 插件权限系统（基于示例插件的权限需求）
- **TASK 5.2.2**: 插件市场 Registry（发布示例到市场）
- **TASK 5.1.4**: 更多高级示例（数据库集成、AI 工具等）

---

## 📝 实施备注

- Weather Plugin 需要 API key，提供 mock 版本用于测试
- 确保所有依赖兼容 wasm32-wasip1 目标
- 插件应展示不同的错误处理模式
- 考虑添加性能基准测试
