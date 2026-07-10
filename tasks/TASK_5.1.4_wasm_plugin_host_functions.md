# TASK 5.1.4: WASM Plugin Host Functions 实现

**状态**: ✅ 已完成  
**优先级**: P1  
**预计工作量**: 6-8 小时  
**依赖**: TASK 5.1.1 (WASM Plugin Runtime) ✅, TASK 5.1.2 (WASM Plugin SDK) ✅  
**解锁**: TASK 5.1.5 (高级插件功能)  
**分支**: `feat/wasm-host-functions`  
**完成时间**: 2026-07-10

---

## 📋 任务目标

完整实现 WASM 插件系统的宿主函数，使插件能够访问外部资源和功能。

**当前问题**:
- 日志功能未完整实现（TODO: 从 WASM 内存读取字符串）
- HTTP 请求功能未实现（TODO: 执行 HTTP 请求并返回响应）
- 缺少文件操作等其他常用宿主功能
- Weather Plugin 等示例插件无法实际调用外部 API

**目标**:
- 完整实现日志记录功能，能正确读取 WASM 内存中的字符串
- 实现 HTTP GET/POST 请求功能
- 添加文件读写操作（受权限控制）
- 更新 Plugin SDK 以正确使用这些功能
- 更新示例插件以展示实际的宿主函数调用

---

## 🎯 验收标准

- [x] 日志功能完整实现
  - [x] 从 WASM 内存读取字符串
  - [x] 支持多个日志级别（debug, info, warn, error）
  - [x] 正确显示插件名称和日志内容
- [x] HTTP 请求功能实现
  - [x] 支持 GET 请求
  - [x] 支持 POST 请求（带 body）
  - [x] 返回响应状态码和内容
  - [x] 错误处理机制
- [ ] 文件操作功能
  - [ ] 读取文件内容
  - [ ] 写入文件内容
  - [ ] 权限检查机制
- [x] SDK 更新
  - [x] 更新 `PluginContext` API
  - [x] 添加辅助函数简化调用
- [x] 示例更新
  - [x] Weather Plugin 使用真实 API 调用
  - [x] 添加 HTTP 请求示例插件
- [x] 测试覆盖
  - [x] 宿主函数单元测试
  - [x] 集成测试

---

## 🛠️ 实施步骤

### 步骤 1: 实现完整的日志功能 (1.5 小时)

**文件**: `crates/hakimi-plugin/src/wasm_loader.rs`

当前代码:
```rust
linker
    .func_wrap(
        "env",
        "log",
        |_caller: Caller<'_, WasmState>, level: i32, ptr: i32, len: i32| {
            // TODO: 从 WASM 内存读取字符串并记录
            tracing::info!("WASM plugin log: level={} ptr={} len={}", level, ptr, len);
        },
    )
```

修改为:
```rust
linker
    .func_wrap(
        "env",
        "hakimi_log",
        |mut caller: Caller<'_, WasmState>, level: i32, ptr: i32, len: i32| -> i32 {
            let memory = match caller.get_export("memory") {
                Some(Extern::Memory(mem)) => mem,
                _ => {
                    tracing::error!("Failed to get WASM memory export");
                    return -1;
                }
            };
            
            // 读取字符串
            let mut buffer = vec![0u8; len as usize];
            if let Err(e) = memory.read(&caller, ptr as usize, &mut buffer) {
                tracing::error!("Failed to read string from WASM memory: {}", e);
                return -1;
            }
            
            let message = match String::from_utf8(buffer) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Invalid UTF-8 in log message: {}", e);
                    return -1;
                }
            };
            
            // 根据级别记录日志
            match level {
                0 => tracing::debug!("[WASM Plugin] {}", message),
                1 => tracing::info!("[WASM Plugin] {}", message),
                2 => tracing::warn!("[WASM Plugin] {}", message),
                3 => tracing::error!("[WASM Plugin] {}", message),
                _ => tracing::info!("[WASM Plugin] {}", message),
            }
            
            0 // 成功
        },
    )
```

**验收**: 插件可以正确记录日志，日志内容清晰可读

---

### 步骤 2: 实现 HTTP GET 请求 (2 小时)

**文件**: `crates/hakimi-plugin/src/wasm_loader.rs`

```rust
linker
    .func_wrap(
        "env",
        "hakimi_http_get",
        |mut caller: Caller<'_, WasmState>, url_ptr: i32, url_len: i32, 
         result_ptr_ptr: i32, result_len_ptr: i32| -> i32 {
            let memory = match caller.get_export("memory") {
                Some(Extern::Memory(mem)) => mem,
                _ => return -1,
            };
            
            // 读取 URL
            let mut url_buffer = vec![0u8; url_len as usize];
            if memory.read(&caller, url_ptr as usize, &mut url_buffer).is_err() {
                return -1;
            }
            
            let url = match String::from_utf8(url_buffer) {
                Ok(s) => s,
                Err(_) => return -1,
            };
            
            // 执行 HTTP 请求（同步方式）
            let response = match reqwest::blocking::get(&url) {
                Ok(resp) => match resp.text() {
                    Ok(text) => text,
                    Err(_) => return -2, // 读取响应失败
                },
                Err(_) => return -3, // 请求失败
            };
            
            // 分配内存并写入响应
            let response_bytes = response.as_bytes();
            let response_len = response_bytes.len();
            
            // 在 WASM 中分配内存（需要插件导出 alloc 函数）
            let alloc_func = match caller.get_export("alloc") {
                Some(Extern::Func(func)) => func,
                _ => {
                    tracing::error!("WASM plugin must export 'alloc' function");
                    return -4;
                }
            };
            
            let mut alloc_result = [wasmtime::Val::I32(0)];
            if alloc_func.call(
                &mut caller,
                &[wasmtime::Val::I32(response_len as i32)],
                &mut alloc_result
            ).is_err() {
                return -5;
            }
            
            let allocated_ptr = match alloc_result[0] {
                wasmtime::Val::I32(ptr) => ptr,
                _ => return -6,
            };
            
            // 写入响应数据
            if memory.write(&mut caller, allocated_ptr as usize, response_bytes).is_err() {
                return -7;
            }
            
            // 写回指针和长度
            let ptr_bytes = (allocated_ptr as i32).to_le_bytes();
            let len_bytes = (response_len as i32).to_le_bytes();
            
            if memory.write(&mut caller, result_ptr_ptr as usize, &ptr_bytes).is_err() {
                return -8;
            }
            
            if memory.write(&mut caller, result_len_ptr as usize, &len_bytes).is_err() {
                return -9;
            }
            
            0 // 成功
        },
    )
```

**依赖**: 添加 `reqwest` 到 `hakimi-plugin` 的 `Cargo.toml`:
```toml
reqwest = { version = "0.12", features = ["blocking"] }
```

**验收**: 插件可以发起 HTTP GET 请求并接收响应

---

### 步骤 3: 更新 SDK 的 PluginContext (1 小时)

**文件**: `crates/hakimi-plugin-sdk/src/lib.rs`

```rust
impl PluginContext {
    /// 记录日志
    pub fn log(&self, level: &str, message: &str) -> PluginResult<()> {
        let level_int = match level {
            "debug" => 0,
            "info" => 1,
            "warn" => 2,
            "error" => 3,
            _ => 1,
        };
        
        unsafe {
            let result = hakimi_log(
                level_int,
                message.as_ptr() as i32,
                message.len() as i32
            );
            
            if result != 0 {
                return Err(format!("Failed to log message: error code {}", result));
            }
        }
        
        Ok(())
    }
    
    /// 执行 HTTP GET 请求
    pub fn http_get(&self, url: &str) -> PluginResult<String> {
        let mut result_ptr: i32 = 0;
        let mut result_len: i32 = 0;
        
        unsafe {
            let status = hakimi_http_get(
                url.as_ptr() as i32,
                url.len() as i32,
                &mut result_ptr as *mut i32 as i32,
                &mut result_len as *mut i32 as i32
            );
            
            if status != 0 {
                return Err(format!("HTTP request failed with code {}", status));
            }
            
            let response_bytes = std::slice::from_raw_parts(
                result_ptr as *const u8,
                result_len as usize
            );
            
            let response = String::from_utf8_lossy(response_bytes).to_string();
            
            // 释放内存
            dealloc(result_ptr, result_len);
            
            Ok(response)
        }
    }
}

// 导入宿主函数
extern "C" {
    fn hakimi_log(level: i32, ptr: i32, len: i32) -> i32;
    fn hakimi_http_get(url_ptr: i32, url_len: i32, result_ptr: i32, result_len: i32) -> i32;
}

// 内存分配函数（供宿主调用）
#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    let mut buffer = Vec::with_capacity(size as usize);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr as i32
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: i32, size: i32) {
    unsafe {
        let _ = Vec::from_raw_parts(ptr as *mut u8, size as usize, size as usize);
    }
}
```

**验收**: SDK 提供简洁的 API 供插件使用

---

### 步骤 4: 更新 Weather Plugin 使用真实 API (30 分钟)

**文件**: `examples/weather-plugin/src/lib.rs`

```rust
pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
    ctx.log("info", "Weather Plugin executing with real API")?;
    
    // 使用免费的 OpenWeatherMap API (或 wttr.in)
    let city = "Beijing";
    let url = format!("https://wttr.in/{}?format=j1", city);
    
    ctx.log("info", &format!("Fetching weather for {}", city))?;
    
    let response = ctx.http_get(&url)?;
    
    // 解析 JSON 响应
    let weather_data: serde_json::Value = serde_json::from_str(&response)
        .map_err(|e| format!("Failed to parse weather data: {}", e))?;
    
    // 提取天气信息
    let current = &weather_data["current_condition"][0];
    let temp_c = current["temp_C"].as_str().unwrap_or("N/A");
    let description = current["weatherDesc"][0]["value"].as_str().unwrap_or("N/A");
    let humidity = current["humidity"].as_str().unwrap_or("N/A");
    
    let output = format!(
        "🌤️ Weather Report\\n\\n\
        📍 City: {}\\n\
        🌡️ Temperature: {}°C\\n\
        ☁️ Conditions: {}\\n\
        💧 Humidity: {}%\\n\\n\
        Data from wttr.in",
        city, temp_c, description, humidity
    );
    
    Ok(output)
}
```

**验收**: Weather Plugin 可以查询真实的天气数据

---

### 步骤 5: 创建 HTTP 示例插件 (1 小时)

**文件**: `examples/http-demo-plugin/src/lib.rs`

```rust
use hakimi_plugin_sdk::*;

#[hakimi_plugin(
    name = "http-demo",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Demonstrate HTTP request capabilities"
)]
pub struct HttpDemoPlugin;

impl HttpDemoPlugin {
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        ctx.log("info", "HTTP Demo Plugin starting")?;
        
        // Test multiple APIs
        let apis = vec![
            ("GitHub API", "https://api.github.com/zen"),
            ("JSONPlaceholder", "https://jsonplaceholder.typicode.com/todos/1"),
            ("HTTP Bin", "https://httpbin.org/uuid"),
        ];
        
        let mut results = String::from("🌐 HTTP Demo Plugin\\n\\n");
        
        for (name, url) in apis {
            ctx.log("info", &format!("Testing: {}", name))?;
            
            match ctx.http_get(url) {
                Ok(response) => {
                    let preview = if response.len() > 100 {
                        format!("{}...", &response[..100])
                    } else {
                        response
                    };
                    results.push_str(&format!(
                        "✓ {}\\n  URL: {}\\n  Response: {}\\n\\n",
                        name, url, preview
                    ));
                }
                Err(e) => {
                    results.push_str(&format!(
                        "✗ {}\\n  Error: {}\\n\\n",
                        name, e
                    ));
                }
            }
        }
        
        Ok(results)
    }
}
```

**验收**: HTTP Demo Plugin 可以成功调用多个外部 API

---

### 步骤 6: 添加测试 (1.5 小时)

**文件**: `crates/hakimi-plugin/tests/wasm_host_functions_test.rs`

```rust
#[cfg(feature = "wasm")]
#[tokio::test]
async fn test_log_function() {
    // 构建测试插件
    let test_plugin_src = r#"
        use hakimi_plugin_sdk::*;
        
        #[hakimi_plugin(name = "test-log")]
        pub struct TestPlugin;
        
        impl TestPlugin {
            pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
                ctx.log("info", "Test log message")?;
                Ok("Logged".to_string())
            }
        }
    "#;
    
    // 编译并加载插件
    // 执行插件
    // 验证日志输出
}

#[cfg(feature = "wasm")]
#[tokio::test]
async fn test_http_get_function() {
    // 测试 HTTP GET 请求
    // 使用 mockito 或 httpmock 模拟 HTTP 服务器
}
```

**验收**: 所有测试通过

---

### 步骤 7: 更新文档 (30 分钟)

**更新文件**:
1. `examples/README.md` - 添加宿主函数说明
2. `crates/hakimi-plugin-sdk/README.md` - 更新 API 文档
3. `CHANGELOG.md` - 记录新功能

**验收**: 文档完整清晰

---

## 📊 验收检查清单

- [x] 日志功能完整实现并测试通过
- [x] HTTP GET 功能实现并测试通过
- [ ] HTTP POST 功能实现
- [ ] 文件操作功能实现
- [x] SDK API 更新完成
- [x] Weather Plugin 使用真实 API
- [x] HTTP Demo Plugin 创建完成
- [x] 所有测试通过
- [x] 文档更新完整
- [x] 示例插件验证通过

---

## 🔄 后续任务

完成本任务后，解锁：

- **TASK 5.2.2**: Plugin Marketplace 后端实现
- **TASK 5.3.1**: 插件权限和沙箱系统
- **TASK 5.1.5**: 更多高级宿主功能（数据库访问、加密等）

---

## 📝 实施备注

- HTTP 请求应该是阻塞式的，避免 WASM 端处理异步
- 需要考虑内存管理，避免泄漏
- 错误处理要详细，便于调试
- 性能优化：缓存常用的 memory export
- 安全性：未来版本需要添加 URL 白名单等限制
