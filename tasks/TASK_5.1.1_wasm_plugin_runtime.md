# TASK 5.1.1: WASM 插件运行时

**状态**: 🟡 进行中 (60%)  
**优先级**: P1  
**预计工作量**: 8-10 小时  
**依赖**: TASK 4.1.1 (Plugin API), TASK 4.1.2 (Plugin Loader)  
**解锁**: TASK 5.1.2 (WASM Plugin SDK)  
**分支**: `feat/wasm-plugin-runtime`  
**PR**: #46  
**当前进度**: 核心加载器完成，待示例插件和集成测试

---

## 📋 任务目标

实现 WebAssembly 插件运行时，支持加载和执行 WASM 格式的插件，提供安全的沙箱环境，避免原生动态库的安全风险。

**当前问题**:
- 现有插件系统使用 libloading 加载原生动态库（.so/.dylib/.dll）
- 原生插件存在安全风险：可执行任意代码、访问文件系统、网络等
- 无法跨平台分发：不同平台需要编译不同的二进制文件
- 缺少沙箱隔离：插件可以访问宿主进程的所有资源

**目标**:
- 使用 Wasmtime 运行时加载和执行 WASM 插件
- 提供安全的沙箱环境，限制插件的资源访问
- 支持 WASI（WebAssembly System Interface）标准
- 实现宿主函数导入，允许插件调用 Hakimi 核心功能
- 插件间资源隔离（独立内存、文件系统、环境变量）
- 性能开销 < 20%（相比原生插件）

---

## 🎯 验收标准

- [ ] 实现 `WasmPluginLoader` 结构体，支持加载 .wasm 文件
- [ ] 集成 Wasmtime 运行时，配置沙箱权限
- [ ] 实现宿主函数导入（logger, http_request, storage 等）
- [ ] 支持 WASI 标准接口（文件系统、环境变量、命令行参数）
- [ ] 插件生命周期管理（initialize, execute, shutdown）
- [ ] 资源限制：内存 < 128MB，CPU 超时 5s
- [ ] 单元测试：加载测试插件，调用导出函数，验证沙箱隔离
- [ ] 集成测试：编写简单 WASM 插件（Rust + wasm32-wasi），端到端测试
- [ ] 性能基准测试：对比 WASM vs 原生插件
- [ ] 编译无错误：`cargo check --package hakimi-plugin --features wasm`
- [ ] 所有测试通过：`cargo test --package hakimi-plugin --features wasm`

---

## 📁 涉及文件

### 新增
- `crates/hakimi-plugin/src/wasm_loader.rs` (约 400 行)
  - `WasmPluginLoader` 结构体
  - `WasmPluginInstance` 结构体
  - `WasmHostFunctions` 宿主函数集合
  - `WasmSandbox` 配置
  
- `crates/hakimi-plugin/src/wasm_host.rs` (约 200 行)
  - 宿主函数实现（logger, http, storage）
  - WASI 配置和权限管理

- `examples/wasm_plugin/` (测试用 WASM 插件)
  - `Cargo.toml` - wasm32-wasi 目标配置
  - `src/lib.rs` - 简单的插件实现
  - `build.sh` - 构建脚本

### 修改
- `crates/hakimi-plugin/Cargo.toml`
  - 添加 wasmtime 依赖
  - 添加 wasm 特性门控
  
- `crates/hakimi-plugin/src/loader.rs`
  - 支持检测和分发 .wasm 文件到 WasmPluginLoader

- `crates/hakimi-plugin/src/lib.rs`
  - 导出 `WasmPluginLoader`

### 测试
- `crates/hakimi-plugin/tests/wasm_plugin_test.rs` (新增)
- `crates/hakimi-plugin/benches/wasm_bench.rs` (新增)

---

## 🛠️ 实施步骤

### 步骤 1: 添加 Wasmtime 依赖 (20 分钟)

**文件**: `crates/hakimi-plugin/Cargo.toml`

```toml
[features]
default = []
wasm = ["wasmtime", "wasmtime-wasi"]

[dependencies]
# ... 现有依赖 ...

# WASM 运行时（可选特性）
wasmtime = { version = "16.0", optional = true }
wasmtime-wasi = { version = "16.0", optional = true }
```

**验收**:
```bash
cargo check --package hakimi-plugin --features wasm
```

---

### 步骤 2: 实现 WasmPluginLoader (180 分钟)

**文件**: `crates/hakimi-plugin/src/wasm_loader.rs`

```rust
use wasmtime::*;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{PluginMetadata, PluginResult, PluginError};

/// WASM 插件沙箱配置
#[derive(Debug, Clone)]
pub struct WasmSandboxConfig {
    /// 最大内存（字节）
    pub max_memory_bytes: usize,
    
    /// 函数执行超时（秒）
    pub execution_timeout_secs: u64,
    
    /// 是否允许文件系统访问
    pub allow_filesystem: bool,
    
    /// 是否允许网络访问
    pub allow_network: bool,
    
    /// 预打开的目录（WASI）
    pub preopened_dirs: Vec<(String, String)>, // (guest_path, host_path)
}

impl Default for WasmSandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 128 * 1024 * 1024, // 128 MB
            execution_timeout_secs: 5,
            allow_filesystem: false,
            allow_network: false,
            preopened_dirs: vec![],
        }
    }
}

/// WASM 插件加载器
pub struct WasmPluginLoader {
    engine: Engine,
    instances: Arc<RwLock<std::collections::HashMap<String, WasmPluginInstance>>>,
    config: WasmSandboxConfig,
}

impl WasmPluginLoader {
    /// 创建新的 WASM 插件加载器
    pub fn new(config: WasmSandboxConfig) -> Result<Self> {
        let mut engine_config = Config::new();
        
        // 启用 WASI 支持
        engine_config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        engine_config.wasm_threads(false); // 禁用多线程
        
        // 配置内存限制
        engine_config.max_wasm_stack(1024 * 1024); // 1MB stack
        
        let engine = Engine::new(&engine_config)?;
        
        Ok(Self {
            engine,
            instances: Arc::new(RwLock::new(std::collections::HashMap::new())),
            config,
        })
    }

    /// 加载 WASM 插件
    pub async fn load_plugin<P: AsRef<Path>>(
        &self,
        wasm_path: P,
    ) -> PluginResult<String> {
        let path = wasm_path.as_ref();
        
        // 验证文件存在
        if !path.exists() {
            return Err(PluginError::LoadError(
                format!("WASM file not found: {}", path.display())
            ));
        }

        // 验证文件扩展名
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            return Err(PluginError::LoadError(
                format!("Invalid WASM file extension: {}", path.display())
            ));
        }

        // 读取 WASM 字节码
        let wasm_bytes = std::fs::read(path).map_err(|e| {
            PluginError::LoadError(format!("Failed to read WASM file: {}", e))
        })?;

        // 编译模块
        let module = Module::from_binary(&self.engine, &wasm_bytes)
            .map_err(|e| PluginError::LoadError(format!("Failed to compile WASM: {}", e)))?;

        // 创建 WASI 上下文
        let wasi = self.create_wasi_context()?;
        let mut store = Store::new(&self.engine, wasi);
        
        // 配置存储限制
        store.limiter(|state| state as &mut dyn ResourceLimiter);
        
        // 创建链接器并添加 WASI 导入
        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker(&mut linker, |s| s)?;
        
        // 添加自定义宿主函数
        self.add_host_functions(&mut linker)?;

        // 实例化模块
        let instance = linker.instantiate(&mut store, &module)
            .map_err(|e| PluginError::LoadError(format!("Failed to instantiate WASM: {}", e)))?;

        // 获取插件元数据
        let metadata = self.get_plugin_metadata(&instance, &mut store)?;
        let plugin_id = metadata.id.clone();

        // 存储插件实例
        let wasm_instance = WasmPluginInstance {
            module,
            store,
            instance,
            metadata: metadata.clone(),
        };

        self.instances.write().await.insert(plugin_id.clone(), wasm_instance);

        tracing::info!("Loaded WASM plugin: {} v{}", metadata.name, metadata.version);

        Ok(plugin_id)
    }

    /// 创建 WASI 上下文
    fn create_wasi_context(&self) -> PluginResult<WasiCtx> {
        let mut builder = WasiCtxBuilder::new();
        
        // 继承标准流
        builder.inherit_stdio();
        
        // 配置预打开目录
        for (guest_path, host_path) in &self.config.preopened_dirs {
            let dir = wasmtime_wasi::Dir::open_ambient_dir(
                host_path,
                wasmtime_wasi::ambient_authority()
            ).map_err(|e| {
                PluginError::LoadError(format!("Failed to open dir {}: {}", host_path, e))
            })?;
            
            builder.preopened_dir(dir, guest_path)?;
        }
        
        Ok(builder.build())
    }

    /// 添加宿主函数到链接器
    fn add_host_functions(&self, linker: &mut Linker<WasiCtx>) -> PluginResult<()> {
        // 日志函数
        linker.func_wrap(
            "env",
            "log",
            |_caller: Caller<'_, WasiCtx>, level: i32, ptr: i32, len: i32| {
                // TODO: 从 WASM 内存读取字符串并记录
                tracing::info!("WASM plugin log: level={} ptr={} len={}", level, ptr, len);
            },
        )?;

        // HTTP 请求函数
        linker.func_wrap(
            "env",
            "http_request",
            |_caller: Caller<'_, WasiCtx>, method_ptr: i32, url_ptr: i32| -> i32 {
                // TODO: 执行 HTTP 请求并返回响应
                tracing::info!("WASM plugin HTTP request: method_ptr={} url_ptr={}", method_ptr, url_ptr);
                0 // 成功
            },
        )?;

        Ok(())
    }

    /// 从 WASM 模块获取插件元数据
    fn get_plugin_metadata(
        &self,
        instance: &Instance,
        store: &mut Store<WasiCtx>,
    ) -> PluginResult<PluginMetadata> {
        // 尝试调用导出的 plugin_metadata 函数
        let func = instance.get_typed_func::<(), (i32, i32), _>(&mut *store, "plugin_metadata")
            .map_err(|e| {
                PluginError::LoadError(format!("Plugin missing 'plugin_metadata' export: {}", e))
            })?;

        let (ptr, len) = func.call(&mut *store, ())
            .map_err(|e| {
                PluginError::LoadError(format!("Failed to call plugin_metadata: {}", e))
            })?;

        // 从 WASM 内存读取 JSON 字符串
        let memory = instance.get_memory(&mut *store, "memory")
            .ok_or_else(|| PluginError::LoadError("Plugin missing 'memory' export".to_string()))?;

        let data = memory.data(&store);
        let json_bytes = &data[ptr as usize..(ptr + len) as usize];
        let json_str = std::str::from_utf8(json_bytes)
            .map_err(|e| PluginError::LoadError(format!("Invalid UTF-8 in metadata: {}", e)))?;

        let metadata: PluginMetadata = serde_json::from_str(json_str)
            .map_err(|e| PluginError::LoadError(format!("Invalid metadata JSON: {}", e)))?;

        Ok(metadata)
    }

    /// 卸载插件
    pub async fn unload_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        self.instances.write().await.remove(plugin_id);
        tracing::info!("Unloaded WASM plugin: {}", plugin_id);
        Ok(())
    }

    /// 列出已加载插件
    pub async fn list_plugins(&self) -> Vec<PluginMetadata> {
        self.instances.read().await.values()
            .map(|inst| inst.metadata.clone())
            .collect()
    }
}

/// WASM 插件实例
struct WasmPluginInstance {
    module: Module,
    store: Store<WasiCtx>,
    instance: Instance,
    metadata: PluginMetadata,
}

/// 资源限制器
impl ResourceLimiter for WasiCtx {
    fn memory_growing(&mut self, current: usize, desired: usize, _maximum: Option<usize>) -> bool {
        // 限制内存增长
        desired <= 128 * 1024 * 1024 // 128 MB
    }

    fn table_growing(&mut self, _current: u32, desired: u32, _maximum: Option<u32>) -> bool {
        // 限制表大小
        desired <= 10000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wasm_loader_creation() {
        let config = WasmSandboxConfig::default();
        let loader = WasmPluginLoader::new(config);
        assert!(loader.is_ok());
    }

    #[tokio::test]
    async fn test_load_wasm_file_not_found() {
        let config = WasmSandboxConfig::default();
        let loader = WasmPluginLoader::new(config).unwrap();
        
        let result = loader.load_plugin("/nonexistent/plugin.wasm").await;
        assert!(result.is_err());
        
        match result {
            Err(PluginError::LoadError(msg)) => {
                assert!(msg.contains("not found"));
            }
            _ => panic!("Expected LoadError"),
        }
    }
}
```

---

### 步骤 3: 创建测试 WASM 插件 (90 分钟)

**目录结构**:
```
examples/wasm_plugin/
├── Cargo.toml
├── src/
│   └── lib.rs
└── build.sh
```

**文件**: `examples/wasm_plugin/Cargo.toml`

```toml
[package]
name = "example-wasm-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = "z"
lto = true
strip = true
```

**文件**: `examples/wasm_plugin/src/lib.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct PluginMetadata {
    id: String,
    name: String,
    version: String,
    author: String,
}

/// 导出插件元数据
#[no_mangle]
pub extern "C" fn plugin_metadata() -> *const u8 {
    let metadata = PluginMetadata {
        id: "example-wasm-plugin".to_string(),
        name: "Example WASM Plugin".to_string(),
        version: "0.1.0".to_string(),
        author: "Hakimi Team".to_string(),
    };
    
    let json = serde_json::to_string(&metadata).unwrap();
    let bytes = json.into_bytes();
    let len = bytes.len();
    
    // 分配内存并返回指针
    let ptr = bytes.as_ptr();
    std::mem::forget(bytes); // 防止释放
    
    // 返回 (ptr, len) 作为两个 i32
    // 注意：这是简化版本，实际需要更复杂的内存管理
    ptr as *const u8
}

/// 插件初始化
#[no_mangle]
pub extern "C" fn initialize() -> i32 {
    // 初始化逻辑
    0 // 成功
}

/// 插件执行
#[no_mangle]
pub extern "C" fn execute(input_ptr: *const u8, input_len: usize) -> i32 {
    // 从输入指针读取数据
    // 执行插件逻辑
    // 写入输出
    0 // 成功
}

/// 插件清理
#[no_mangle]
pub extern "C" fn shutdown() -> i32 {
    // 清理逻辑
    0 // 成功
}
```

**文件**: `examples/wasm_plugin/build.sh`

```bash
#!/bin/bash
set -e

echo "Building WASM plugin..."

# 确保 wasm32-wasi 目标已安装
rustup target add wasm32-wasi

# 构建 WASM
cargo build --target wasm32-wasi --release

# 复制到测试目录
mkdir -p ../../target/wasm-plugins
cp target/wasm32-wasi/release/example_wasm_plugin.wasm \
   ../../target/wasm-plugins/

echo "WASM plugin built successfully!"
echo "Output: ../../target/wasm-plugins/example_wasm_plugin.wasm"
```

---

### 步骤 4: 集成测试 (90 分钟)

**文件**: `crates/hakimi-plugin/tests/wasm_plugin_test.rs`

```rust
#[cfg(feature = "wasm")]
mod wasm_tests {
    use hakimi_plugin::{WasmPluginLoader, WasmSandboxConfig};
    use std::path::PathBuf;

    fn get_test_wasm_path() -> PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest_dir)
            .parent().unwrap()
            .parent().unwrap()
            .join("target/wasm-plugins/example_wasm_plugin.wasm")
    }

    #[tokio::test]
    async fn test_load_wasm_plugin() {
        let wasm_path = get_test_wasm_path();
        
        // 如果测试插件不存在，跳过测试
        if !wasm_path.exists() {
            eprintln!("Test WASM plugin not found, run: cd examples/wasm_plugin && ./build.sh");
            return;
        }

        let config = WasmSandboxConfig::default();
        let loader = WasmPluginLoader::new(config).unwrap();
        
        let plugin_id = loader.load_plugin(&wasm_path).await.unwrap();
        assert_eq!(plugin_id, "example-wasm-plugin");

        let plugins = loader.list_plugins().await;
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "Example WASM Plugin");
    }

    #[tokio::test]
    async fn test_unload_wasm_plugin() {
        let wasm_path = get_test_wasm_path();
        
        if !wasm_path.exists() {
            return;
        }

        let config = WasmSandboxConfig::default();
        let loader = WasmPluginLoader::new(config).unwrap();
        
        let plugin_id = loader.load_plugin(&wasm_path).await.unwrap();
        loader.unload_plugin(&plugin_id).await.unwrap();

        let plugins = loader.list_plugins().await;
        assert_eq!(plugins.len(), 0);
    }

    #[tokio::test]
    async fn test_wasm_sandbox_memory_limit() {
        // TODO: 创建一个超出内存限制的 WASM 插件
        // 验证加载或执行时被拒绝
    }

    #[tokio::test]
    async fn test_wasm_filesystem_isolation() {
        // TODO: 创建尝试访问未授权文件的 WASM 插件
        // 验证文件访问被拒绝
    }
}
```

---

### 步骤 5: 性能基准测试 (60 分钟)

**文件**: `crates/hakimi-plugin/benches/wasm_bench.rs`

```rust
#[cfg(feature = "wasm")]
mod benches {
    use criterion::{black_box, criterion_group, criterion_main, Criterion};
    use hakimi_plugin::{WasmPluginLoader, WasmSandboxConfig};
    use tokio::runtime::Runtime;
    use std::path::PathBuf;

    fn get_test_wasm_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap()
            .join("target/wasm-plugins/example_wasm_plugin.wasm")
    }

    fn bench_load_wasm_plugin(c: &mut Criterion) {
        let rt = Runtime::new().unwrap();
        let wasm_path = get_test_wasm_path();
        
        if !wasm_path.exists() {
            eprintln!("Test WASM plugin not found");
            return;
        }

        c.bench_function("load_wasm_plugin", |b| {
            b.to_async(&rt).iter(|| async {
                let config = WasmSandboxConfig::default();
                let loader = WasmPluginLoader::new(config).unwrap();
                black_box(loader.load_plugin(&wasm_path).await.unwrap())
            })
        });
    }

    fn bench_wasm_function_call(c: &mut Criterion) {
        let rt = Runtime::new().unwrap();
        let wasm_path = get_test_wasm_path();
        
        if !wasm_path.exists() {
            return;
        }

        let config = WasmSandboxConfig::default();
        let loader = rt.block_on(async {
            let loader = WasmPluginLoader::new(config).unwrap();
            loader.load_plugin(&wasm_path).await.unwrap();
            loader
        });

        c.bench_function("wasm_function_call", |b| {
            b.to_async(&rt).iter(|| async {
                // TODO: 调用 WASM 插件的 execute 函数
                black_box(0)
            })
        });
    }

    criterion_group!(benches, bench_load_wasm_plugin, bench_wasm_function_call);
    criterion_main!(benches);
}
```

---

### 步骤 6: 修改插件加载器分发逻辑 (30 分钟)

**文件**: `crates/hakimi-plugin/src/loader.rs`

在 `load_plugin` 方法中添加 WASM 检测：

```rust
pub async fn load_plugin<P: AsRef<Path>>(
    &self,
    library_path: P,
) -> PluginResult<String> {
    let path = library_path.as_ref();
    
    // 验证文件存在
    if !path.exists() {
        return Err(PluginError::LoadError(
            format!("Plugin library not found: {}", path.display())
        ));
    }

    // 检测文件类型
    let ext = path.extension().and_then(|e| e.to_str());
    
    match ext {
        Some("wasm") => {
            // 分发到 WASM 加载器
            #[cfg(feature = "wasm")]
            {
                let wasm_loader = crate::wasm_loader::WasmPluginLoader::new(
                    crate::wasm_loader::WasmSandboxConfig::default()
                )?;
                return wasm_loader.load_plugin(path).await;
            }
            
            #[cfg(not(feature = "wasm"))]
            {
                return Err(PluginError::LoadError(
                    "WASM support not enabled, rebuild with --features wasm".to_string()
                ));
            }
        }
        Some("so") | Some("dylib") | Some("dll") => {
            // 使用现有的 libloading 逻辑
            // ... (保持原有代码)
        }
        _ => {
            return Err(PluginError::LoadError(
                format!("Invalid plugin extension: {:?}", ext)
            ));
        }
    }
    
    // ... 原有 libloading 逻辑 ...
}
```

---

## 📊 完成检查清单

- [ ] Wasmtime 依赖添加完成
- [ ] `WasmPluginLoader` 实现完成
- [ ] WASI 上下文配置完成
- [ ] 宿主函数导入实现完成
- [ ] 测试 WASM 插件创建并编译成功
- [ ] 集成测试全部通过（4+ 测试用例）
- [ ] 性能基准测试完成，开销 < 20%
- [ ] 插件加载器分发逻辑完成
- [ ] 编译无错误：`cargo check --features wasm`
- [ ] 所有测试通过：`cargo test --features wasm`
- [ ] 文档更新（README, ARCHITECTURE.md）
- [ ] CHANGELOG 更新
- [ ] 版本号递增至 0.5.87
- [ ] PR 创建并合并

---

## 🔗 参考资料

- [Wasmtime 文档](https://docs.wasmtime.dev/)
- [WASI 规范](https://github.com/WebAssembly/WASI)
- [Rust WASM Book](https://rustwasm.github.io/docs/book/)
- [wasm32-wasi 目标](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasi.html)

---

## 📝 设计决策

### 为什么选择 Wasmtime？
- Rust 原生实现，无需 C/C++ 依赖
- 生产级性能和安全性
- WASI 支持完善
- 活跃的社区和维护

### 为什么不使用 wasmer？
- Wasmtime 在 Rust 生态中更广泛使用
- 更好的 async 支持
- 更小的二进制体积

### 宿主函数 vs WASI
- 核心系统接口使用 WASI 标准（文件、环境变量）
- Hakimi 特定功能使用自定义宿主函数（日志、HTTP、记忆）

### 内存限制策略
- 默认 128MB 限制，满足大多数插件需求
- 可配置，允许特定插件申请更多内存
- 使用 Wasmtime 的 ResourceLimiter 强制限制

---

## 🚧 后续任务

完成此任务后，下一步是：
- **TASK 5.1.2**: WASM Plugin SDK - 提供 Rust crate 简化插件开发
- **TASK 5.1.3**: WASM Plugin Examples - 创建示例插件集合
- **TASK 5.1.4**: WASM Plugin Marketplace - 支持 WASM 插件分发

---

**创建时间**: 2026-07-10 19:00 UTC  
**预计完成**: 2026-07-11 05:00 UTC（10 小时内）
