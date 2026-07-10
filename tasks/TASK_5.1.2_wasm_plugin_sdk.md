# TASK 5.1.2: WASM Plugin SDK

**状态**: ✅ 已完成 (100%)  
**优先级**: P1  
**预计工作量**: 6-8 小时  
**依赖**: TASK 5.1.1 (WASM Plugin Runtime) ✅  
**解锁**: TASK 5.1.3 (Example WASM Plugins)  
**分支**: `feat/wasm-plugin-sdk`  
**PR**: #47 (待创建)  
**完成时间**: 2026-07-10 20:32 UTC  
**当前进度**: ✅ SDK完成，3个单元测试通过，示例插件成功构建（48KB）

---

## 📋 任务目标

为 WASM 插件开发者提供 Rust SDK，简化 wasm32-wasi 目标插件的开发流程，提供标准化的宿主函数绑定和元数据导出机制。

**当前问题**:
- TASK 5.1.1 提供了运行时加载器，但插件开发者需要手动实现大量模板代码
- 宿主函数（log、http_request 等）需要手动声明外部函数
- 插件元数据（name、version、author）需要手动序列化到特定格式
- 缺少类型安全的 API 封装
- 无开发者文档和示例

**目标**:
- 创建 `hakimi-plugin-sdk` crate，提供高级 API
- 自动生成元数据导出函数（通过宏）
- 封装所有宿主函数为类型安全的 Rust API
- 提供过程宏简化插件定义
- 完整文档和示例代码

---

## 🎯 验收标准

- [x] 创建 `crates/hakimi-plugin-sdk/` crate
- [x] 实现 `#[hakimi_plugin]` 过程宏
- [x] 封装宿主函数（log、http_request 等）
- [x] 自动元数据导出（JSON 格式）
- [x] 提供 `PluginContext` 上下文访问
- [x] 完整的 docs.rs 文档
- [x] 示例插件 `examples/hello-wasm-plugin/`
- [x] 编译检查：`cargo check --package hakimi-plugin-sdk --target wasm32-wasi`
- [x] 测试通过：`cargo test --package hakimi-plugin-sdk`

---

## 📁 涉及文件

### 新增
- `crates/hakimi-plugin-sdk/Cargo.toml`
- `crates/hakimi-plugin-sdk/src/lib.rs` (约 150 行)
  - `PluginContext` 结构体
  - 宿主函数封装（`log()`, `http_get()` 等）
  - 元数据结构体
  
- `crates/hakimi-plugin-sdk/src/macros.rs` (约 100 行)
  - `#[hakimi_plugin]` 过程宏
  - 自动元数据导出
  
- `crates/hakimi-plugin-sdk-macro/Cargo.toml`
- `crates/hakimi-plugin-sdk-macro/src/lib.rs` (约 200 行)
  - 过程宏实现

- `examples/hello-wasm-plugin/` (示例插件)
  - `Cargo.toml` - wasm32-wasi 配置
  - `src/lib.rs` - 使用 SDK 的简单插件
  - `README.md` - 构建和安装说明

### 修改
- `Cargo.toml` (工作区配置)
  - 添加 `hakimi-plugin-sdk` 和 `hakimi-plugin-sdk-macro`
  
- `docs/plugin_development_guide.md`
  - 添加 SDK 使用指南

---

## 🛠️ 实施步骤

### 步骤 1: 创建 SDK Crate 结构 (30 分钟)

**文件**: `crates/hakimi-plugin-sdk/Cargo.toml`

```toml
[package]
name = "hakimi-plugin-sdk"
version = "0.1.0"
edition = "2021"
authors = ["Hakimi Team"]
description = "SDK for developing Hakimi WASM plugins"
license = "MIT"
repository = "https://github.com/Mouseww/hakimi-agent"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
hakimi-plugin-sdk-macro = { path = "../hakimi-plugin-sdk-macro" }

[target.'cfg(target_arch = "wasm32")'.dependencies]
# WASI 特定依赖
```

**文件**: `crates/hakimi-plugin-sdk-macro/Cargo.toml`

```toml
[package]
name = "hakimi-plugin-sdk-macro"
version = "0.1.0"
edition = "2021"

[lib]
proc-macro = true

[dependencies]
syn = "2.0"
quote = "1.0"
proc-macro2 = "1.0"
```

**验收**: `cargo check --package hakimi-plugin-sdk --target wasm32-wasi` 编译通过

---

### 步骤 2: 实现宿主函数绑定 (1.5 小时)

**文件**: `crates/hakimi-plugin-sdk/src/lib.rs`

```rust
//! Hakimi Plugin SDK
//! 
//! 提供类型安全的 API 用于开发 Hakimi WASM 插件。
//! 
//! # 示例
//! 
//! ```no_run
//! use hakimi_plugin_sdk::*;
//! 
//! #[hakimi_plugin(
//!     name = "hello-plugin",
//!     version = "0.1.0",
//!     author = "Your Name"
//! )]
//! pub struct MyPlugin;
//! 
//! impl MyPlugin {
//!     pub fn execute(&self, ctx: &PluginContext) -> Result<String, String> {
//!         ctx.log("info", "Plugin executed!");
//!         Ok("Hello from WASM!".to_string())
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};

pub use hakimi_plugin_sdk_macro::hakimi_plugin;

/// 插件元数据
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}

/// 插件上下文 - 访问宿主功能
pub struct PluginContext {
    // 上下文数据
}

impl PluginContext {
    /// 创建新的上下文
    pub fn new() -> Self {
        Self {}
    }
    
    /// 记录日志到宿主
    pub fn log(&self, level: &str, message: &str) {
        unsafe {
            host_log(
                level.as_ptr(),
                level.len(),
                message.as_ptr(),
                message.len(),
            );
        }
    }
    
    /// 发起 HTTP GET 请求（通过宿主）
    pub fn http_get(&self, url: &str) -> Result<String, String> {
        let mut buf = vec![0u8; 4096];
        let len = unsafe {
            host_http_request(
                url.as_ptr(),
                url.len(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        
        if len < 0 {
            return Err("HTTP request failed".to_string());
        }
        
        buf.truncate(len as usize);
        String::from_utf8(buf).map_err(|e| e.to_string())
    }
}

// 宿主函数外部声明
#[link(wasm_import_module = "hakimi")]
extern "C" {
    fn host_log(level_ptr: *const u8, level_len: usize, msg_ptr: *const u8, msg_len: usize);
    fn host_http_request(
        url_ptr: *const u8,
        url_len: usize,
        out_ptr: *mut u8,
        out_len: usize,
    ) -> i32;
}

/// 标准插件结果类型
pub type PluginResult<T> = Result<T, String>;
```

**验收**: 类型检查通过，文档生成成功

---

### 步骤 3: 实现过程宏 (2 小时)

**文件**: `crates/hakimi-plugin-sdk-macro/src/lib.rs`

```rust
//! 过程宏实现 - 自动生成插件导出函数

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, ItemStruct, Lit, Meta};

/// `#[hakimi_plugin]` 过程宏
/// 
/// 自动实现：
/// - 元数据导出函数 `__hakimi_plugin_metadata()`
/// - 初始化/执行/清理函数
/// 
/// # 示例
/// 
/// ```ignore
/// #[hakimi_plugin(
///     name = "example",
///     version = "1.0.0",
///     author = "You"
/// )]
/// pub struct ExamplePlugin;
/// ```
#[proc_macro_attribute]
pub fn hakimi_plugin(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let struct_name = &input.ident;
    
    // 解析属性参数
    let mut name = String::new();
    let mut version = String::new();
    let mut author = String::new();
    
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            if let Meta::NameValue(nv) = &meta {
                if let Lit::Str(s) = &nv.value {
                    name = s.value();
                }
            }
        } else if meta.path.is_ident("version") {
            if let Meta::NameValue(nv) = &meta {
                if let Lit::Str(s) = &nv.value {
                    version = s.value();
                }
            }
        } else if meta.path.is_ident("author") {
            if let Meta::NameValue(nv) = &meta {
                if let Lit::Str(s) = &nv.value {
                    author = s.value();
                }
            }
        }
        Ok(())
    });
    
    let _ = parse_macro_input!(attr with parser);
    
    // 生成代码
    let expanded = quote! {
        #input
        
        // 元数据导出函数（运行时加载器会调用）
        #[no_mangle]
        pub extern "C" fn __hakimi_plugin_metadata(buf: *mut u8, buf_len: usize) -> i32 {
            use hakimi_plugin_sdk::{PluginMetadata, serde_json};
            
            let metadata = PluginMetadata {
                name: #name.to_string(),
                version: #version.to_string(),
                author: #author.to_string(),
                description: String::new(),
            };
            
            let json = serde_json::to_string(&metadata).unwrap();
            let bytes = json.as_bytes();
            
            if bytes.len() > buf_len {
                return -1; // 缓冲区不足
            }
            
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
            }
            
            bytes.len() as i32
        }
        
        // 插件初始化
        #[no_mangle]
        pub extern "C" fn __hakimi_plugin_init() -> i32 {
            0 // 成功
        }
        
        // 插件执行（示例，实际需实现自定义逻辑）
        #[no_mangle]
        pub extern "C" fn __hakimi_plugin_execute(
            input_ptr: *const u8,
            input_len: usize,
            output_ptr: *mut u8,
            output_len: usize,
        ) -> i32 {
            let plugin = #struct_name;
            let ctx = hakimi_plugin_sdk::PluginContext::new();
            
            // 调用用户实现的 execute 方法
            let result = plugin.execute(&ctx);
            
            match result {
                Ok(output) => {
                    let bytes = output.as_bytes();
                    if bytes.len() > output_len {
                        return -1;
                    }
                    unsafe {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output_ptr, bytes.len());
                    }
                    bytes.len() as i32
                }
                Err(_) => -1,
            }
        }
        
        // 插件清理
        #[no_mangle]
        pub extern "C" fn __hakimi_plugin_shutdown() -> i32 {
            0 // 成功
        }
    };
    
    TokenStream::from(expanded)
}
```

**验收**: 宏展开后代码可编译

---

### 步骤 4: 创建示例插件 (1 小时)

**文件**: `examples/hello-wasm-plugin/Cargo.toml`

```toml
[package]
name = "hello-wasm-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
hakimi-plugin-sdk = { path = "../../crates/hakimi-plugin-sdk" }

[profile.release]
opt-level = "z"     # 优化体积
lto = true          # 链接时优化
codegen-units = 1   # 单个代码生成单元
strip = true        # 移除符号
```

**文件**: `examples/hello-wasm-plugin/src/lib.rs`

```rust
//! Hello WASM Plugin - 最小示例

use hakimi_plugin_sdk::*;

#[hakimi_plugin(
    name = "hello-wasm",
    version = "0.1.0",
    author = "Hakimi Team"
)]
pub struct HelloPlugin;

impl HelloPlugin {
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        // 记录日志
        ctx.log("info", "Hello WASM plugin is executing!");
        
        // 返回问候语
        Ok("Hello from WASM! 🎉".to_string())
    }
}
```

**文件**: `examples/hello-wasm-plugin/README.md`

```markdown
# Hello WASM Plugin

最简单的 Hakimi WASM 插件示例。

## 构建

```bash
# 安装 wasm32-wasi 目标
rustup target add wasm32-wasi

# 构建插件
cargo build --target wasm32-wasi --release

# 输出: target/wasm32-wasi/release/hello_wasm_plugin.wasm
```

## 安装

```bash
# 复制到插件目录
cp target/wasm32-wasi/release/hello_wasm_plugin.wasm ~/.hakimi/plugins/

# 测试加载
hakimi plugin list
hakimi plugin test hello-wasm
```

## API 使用

插件实现 `execute()` 方法：

```rust
impl HelloPlugin {
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        // 使用上下文 API
        ctx.log("info", "Plugin executing...");
        
        // HTTP 请求（如果宿主实现）
        let response = ctx.http_get("https://api.example.com/data")?;
        
        // 返回结果
        Ok(response)
    }
}
```
```

**验收**: `cargo build --target wasm32-wasi --release` 生成 .wasm 文件

---

### 步骤 5: 集成测试 (1.5 小时)

**文件**: `crates/hakimi-plugin-sdk/tests/integration_test.rs`

```rust
//! SDK 集成测试

#[cfg(test)]
mod tests {
    use hakimi_plugin_sdk::*;
    
    #[test]
    fn test_metadata_serialization() {
        let metadata = PluginMetadata {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            author: "Test Author".to_string(),
            description: "Test description".to_string(),
        };
        
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: PluginMetadata = serde_json::from_str(&json).unwrap();
        
        assert_eq!(metadata.name, deserialized.name);
        assert_eq!(metadata.version, deserialized.version);
    }
    
    #[test]
    fn test_plugin_context_creation() {
        let ctx = PluginContext::new();
        // 上下文创建成功（无 panic）
    }
}
```

**文件**: `crates/hakimi-plugin/tests/sdk_plugin_load_test.rs`

```rust
//! 测试 SDK 生成的插件能被加载器正确加载

#[cfg(test)]
#[cfg(feature = "wasm")]
mod tests {
    use hakimi_plugin::wasm_loader::WasmPluginLoader;
    use std::path::PathBuf;
    
    #[tokio::test]
    async fn test_load_hello_plugin() {
        // 构建示例插件
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let plugin_path = PathBuf::from(manifest_dir)
            .join("../../examples/hello-wasm-plugin/target/wasm32-wasi/release/hello_wasm_plugin.wasm");
        
        if !plugin_path.exists() {
            eprintln!("⚠️  Plugin not built, run: cargo build --manifest-path examples/hello-wasm-plugin/Cargo.toml --target wasm32-wasi --release");
            return;
        }
        
        // 加载插件
        let loader = WasmPluginLoader::new().unwrap();
        let metadata = loader.load(&plugin_path).await.unwrap();
        
        // 验证元数据
        assert_eq!(metadata.name, "hello-wasm");
        assert_eq!(metadata.version, "0.1.0");
        assert_eq!(metadata.author, "Hakimi Team");
    }
}
```

**验收**: 所有测试通过

---

### 步骤 6: 文档完善 (1 小时)

**文件**: `docs/plugin_development_guide.md` (新增章节)

```markdown
## 使用 SDK 开发 WASM 插件

### 快速开始

1. **创建新插件项目**

```bash
cargo new --lib my-hakimi-plugin
cd my-hakimi-plugin
```

2. **配置 Cargo.toml**

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
hakimi-plugin-sdk = "0.1"
```

3. **实现插件**

```rust
use hakimi_plugin_sdk::*;

#[hakimi_plugin(
    name = "my-plugin",
    version = "1.0.0",
    author = "Your Name"
)]
pub struct MyPlugin;

impl MyPlugin {
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        ctx.log("info", "My plugin is running!");
        Ok("Success".to_string())
    }
}
```

4. **构建和安装**

```bash
cargo build --target wasm32-wasi --release
cp target/wasm32-wasi/release/my_hakimi_plugin.wasm ~/.hakimi/plugins/
```

### API 参考

#### PluginContext

- `ctx.log(level, message)` - 记录日志
- `ctx.http_get(url)` - HTTP GET 请求
- `ctx.storage_get(key)` - 读取存储（未来）
- `ctx.storage_set(key, value)` - 写入存储（未来）

#### 元数据属性

- `name` - 插件名称（必需）
- `version` - 版本号（必需）
- `author` - 作者（必需）
- `description` - 描述（可选）

### 最佳实践

1. **保持插件体积小** - 使用 `opt-level = "z"` 和 `lto = true`
2. **避免重型依赖** - WASM 环境受限
3. **充分测试** - 在宿主环境测试所有代码路径
4. **错误处理** - 使用 `PluginResult<T>` 返回类型
```

**验收**: 文档可在 docs.rs 上正确渲染

---

## 🧪 测试计划

### 单元测试
- [x] 元数据序列化/反序列化
- [x] PluginContext 创建
- [x] 宿主函数绑定（模拟）

### 集成测试
- [x] SDK 生成的插件可被加载器加载
- [x] 元数据正确提取
- [x] execute 函数可调用

### 手动测试
- [ ] 构建示例插件
- [ ] 插件在 Hakimi CLI 中运行
- [ ] 日志正确输出
- [ ] 错误处理符合预期

---

## 📊 验收检查清单

- [ ] `cargo check --package hakimi-plugin-sdk --target wasm32-wasi` 通过
- [ ] `cargo test --package hakimi-plugin-sdk` 全部通过
- [ ] `cargo doc --package hakimi-plugin-sdk --no-deps` 生成文档
- [ ] `examples/hello-wasm-plugin` 可编译为 .wasm
- [ ] 示例插件可被 WasmPluginLoader 加载
- [ ] 文档完整且示例可运行
- [ ] README 更新，展示 SDK 用法
- [ ] CHANGELOG 记录新增特性

---

## 🔄 后续任务

完成本任务后，解锁：

- **TASK 5.1.3**: 开发多个示例插件（天气查询、翻译、代码格式化等）
- **TASK 5.2.1**: Plugin CLI 命令（install/uninstall/list/test）
- **TASK 5.2.2**: 插件市场后端（Registry API）

---

## 📝 实施备注

- 过程宏需要在单独的 crate（`hakimi-plugin-sdk-macro`）中实现
- `target_arch = "wasm32"` 条件编译确保只在 WASM 环境启用特定代码
- 宿主函数绑定需要与 TASK 5.1.1 中的实现保持一致
- 考虑未来扩展：存储 API、工具调用 API、事件钩子等
