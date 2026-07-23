//! WASM Plugin Loader - 安全的 WebAssembly 插件运行时
//!
//! 提供沙箱环境，支持 WASI 标准接口和自定义宿主函数。

use crate::{PluginError, PluginMetadata, PluginResult};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::*;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};

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

/// WASM 插件实例存储状态
struct WasmState {
    wasi: WasiCtx,
    max_memory: usize,
}

/// WASM 插件加载器
pub struct WasmPluginLoader {
    engine: Engine,
    instances: Arc<RwLock<HashMap<String, WasmPluginInstance>>>,
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

        // 启用资源限制器
        engine_config.consume_fuel(true);

        let engine = Engine::new(&engine_config)?;

        Ok(Self {
            engine,
            instances: Arc::new(RwLock::new(HashMap::new())),
            config,
        })
    }

    /// 加载 WASM 插件
    pub async fn load_plugin<P: AsRef<Path>>(&self, wasm_path: P) -> PluginResult<String> {
        let path = wasm_path.as_ref();

        // 验证文件存在
        if !path.exists() {
            return Err(PluginError::LoadError(format!(
                "WASM file not found: {}",
                path.display()
            )));
        }

        // 验证文件扩展名
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            return Err(PluginError::LoadError(format!(
                "Invalid WASM file extension: {}",
                path.display()
            )));
        }

        // 读取 WASM 字节码
        let wasm_bytes = std::fs::read(path)
            .map_err(|e| PluginError::LoadError(format!("Failed to read WASM file: {}", e)))?;

        // 编译模块
        let module = Module::from_binary(&self.engine, &wasm_bytes)
            .map_err(|e| PluginError::LoadError(format!("Failed to compile WASM: {}", e)))?;

        // 创建 WASI 上下文
        let wasi = self.create_wasi_context()?;
        let state = WasmState {
            wasi,
            max_memory: self.config.max_memory_bytes,
        };
        let mut store = Store::new(&self.engine, state);

        // 配置资源限制
        store.limiter(|state| state as &mut dyn ResourceLimiter);

        // 设置燃料限制（防止无限循环）
        store
            .set_fuel(1_000_000)
            .map_err(|e| PluginError::LoadError(format!("Failed to set fuel: {}", e)))?;

        // 创建链接器并添加 WASI 导入
        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker(&mut linker, |s: &mut WasmState| &mut s.wasi)
            .map_err(|e| PluginError::LoadError(format!("Failed to add WASI linker: {}", e)))?;

        // 添加自定义宿主函数
        self.add_host_functions(&mut linker)?;

        // 实例化模块
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| PluginError::LoadError(format!("Failed to instantiate WASM: {}", e)))?;

        // 获取插件元数据
        let metadata = self.get_plugin_metadata(&instance, &mut store)?;
        let plugin_id = metadata.id.clone();

        // 存储插件实例
        let wasm_instance = WasmPluginInstance {
            metadata: metadata.clone(),
        };

        self.instances
            .write()
            .await
            .insert(plugin_id.clone(), wasm_instance);

        tracing::info!(
            "Loaded WASM plugin: {} v{}",
            metadata.name,
            metadata.version
        );

        Ok(plugin_id)
    }

    /// 创建 WASI 上下文
    fn create_wasi_context(&self) -> PluginResult<WasiCtx> {
        let mut builder = WasiCtxBuilder::new();

        // 继承标准流
        builder.inherit_stdio();

        // 配置预打开目录
        for (guest_path, host_path) in &self.config.preopened_dirs {
            // 使用 wasmtime_wasi::Dir 来打开目录
            let dir =
                wasmtime_wasi::Dir::open_ambient_dir(host_path, wasmtime_wasi::ambient_authority())
                    .map_err(|e| {
                        PluginError::LoadError(format!("Failed to open dir {}: {}", host_path, e))
                    })?;

            builder
                .preopened_dir(dir, guest_path)
                .map_err(|e| PluginError::LoadError(format!("Failed to preopen dir: {}", e)))?;
        }

        Ok(builder.build())
    }

    /// 添加宿主函数到链接器
    fn add_host_functions(&self, linker: &mut Linker<WasmState>) -> PluginResult<()> {
        // 日志函数 - 从 WASM 内存读取字符串并记录
        linker
            .func_wrap(
                "hakimi",
                "host_log",
                |mut caller: Caller<'_, WasmState>,
                 level_ptr: i32,
                 level_len: i32,
                 msg_ptr: i32,
                 msg_len: i32| {
                    // 获取 WASM 内存
                    let memory = match caller.get_export("memory") {
                        Some(Extern::Memory(mem)) => mem,
                        _ => {
                            tracing::error!("Failed to get WASM memory export");
                            return;
                        }
                    };

                    // 读取日志级别字符串
                    let mut level_buffer = vec![0u8; level_len as usize];
                    if let Err(e) = memory.read(&caller, level_ptr as usize, &mut level_buffer) {
                        tracing::error!("Failed to read log level from WASM memory: {}", e);
                        return;
                    }

                    let level = match String::from_utf8(level_buffer) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!("Invalid UTF-8 in log level: {}", e);
                            return;
                        }
                    };

                    // 读取日志消息字符串
                    let mut msg_buffer = vec![0u8; msg_len as usize];
                    if let Err(e) = memory.read(&caller, msg_ptr as usize, &mut msg_buffer) {
                        tracing::error!("Failed to read log message from WASM memory: {}", e);
                        return;
                    }

                    let message = match String::from_utf8(msg_buffer) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!("Invalid UTF-8 in log message: {}", e);
                            return;
                        }
                    };

                    // 根据级别记录日志
                    match level.to_lowercase().as_str() {
                        "trace" => tracing::trace!("[WASM Plugin] {}", message),
                        "debug" => tracing::debug!("[WASM Plugin] {}", message),
                        "info" => tracing::info!("[WASM Plugin] {}", message),
                        "warn" => tracing::warn!("[WASM Plugin] {}", message),
                        "error" => tracing::error!("[WASM Plugin] {}", message),
                        _ => tracing::info!("[WASM Plugin] {}", message),
                    }
                },
            )
            .map_err(|e| {
                PluginError::LoadError(format!("Failed to wrap host_log function: {}", e))
            })?;

        // HTTP GET 请求函数
        linker
            .func_wrap(
                "hakimi",
                "host_http_request",
                |mut caller: Caller<'_, WasmState>,
                 url_ptr: i32,
                 url_len: i32,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    // 获取 WASM 内存
                    let memory = match caller.get_export("memory") {
                        Some(Extern::Memory(mem)) => mem,
                        _ => {
                            tracing::error!("Failed to get WASM memory export for HTTP request");
                            return -1;
                        }
                    };

                    // 读取 URL 字符串
                    let mut url_buffer = vec![0u8; url_len as usize];
                    if let Err(e) = memory.read(&caller, url_ptr as usize, &mut url_buffer) {
                        tracing::error!("Failed to read URL from WASM memory: {}", e);
                        return -2;
                    }

                    let url = match String::from_utf8(url_buffer) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!("Invalid UTF-8 in URL: {}", e);
                            return -3;
                        }
                    };

                    tracing::debug!("[WASM Plugin] HTTP GET request to: {}", url);

                    // 执行 HTTP GET 请求（阻塞式）
                    let response_text = match reqwest::blocking::get(&url) {
                        Ok(resp) => match resp.text() {
                            Ok(text) => text,
                            Err(e) => {
                                tracing::error!("Failed to read HTTP response body: {}", e);
                                return -4;
                            }
                        },
                        Err(e) => {
                            tracing::error!("HTTP request failed: {}", e);
                            return -5;
                        }
                    };

                    let response_bytes = response_text.as_bytes();
                    let bytes_to_write = response_bytes.len().min(out_len as usize);

                    // 写入响应到 WASM 内存
                    if let Err(e) = memory.write(
                        &mut caller,
                        out_ptr as usize,
                        &response_bytes[..bytes_to_write],
                    ) {
                        tracing::error!("Failed to write HTTP response to WASM memory: {}", e);
                        return -6;
                    }

                    tracing::debug!(
                        "[WASM Plugin] HTTP request successful, wrote {} bytes",
                        bytes_to_write
                    );

                    bytes_to_write as i32
                },
            )
            .map_err(|e| {
                PluginError::LoadError(format!("Failed to wrap host_http_request function: {}", e))
            })?;

        Ok(())
    }

    /// 从 WASM 模块获取插件元数据
    fn get_plugin_metadata(
        &self,
        instance: &Instance,
        store: &mut Store<WasmState>,
    ) -> PluginResult<PluginMetadata> {
        // 尝试调用导出的 plugin_metadata 函数
        let func = instance
            .get_typed_func::<(), (i32, i32)>(&mut *store, "plugin_metadata")
            .map_err(|e| {
                PluginError::LoadError(format!("Plugin missing 'plugin_metadata' export: {}", e))
            })?;

        let (ptr, len) = func.call(&mut *store, ()).map_err(|e| {
            PluginError::LoadError(format!("Failed to call plugin_metadata: {}", e))
        })?;

        // 从 WASM 内存读取 JSON 字符串
        let memory = instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| PluginError::LoadError("Plugin missing 'memory' export".to_string()))?;

        let data = memory.data(&*store);

        // 边界检查
        if ptr < 0 || len < 0 || (ptr + len) as usize > data.len() {
            return Err(PluginError::LoadError(format!(
                "Invalid metadata pointer: ptr={} len={} memory_size={}",
                ptr,
                len,
                data.len()
            )));
        }

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
        self.instances
            .read()
            .await
            .values()
            .map(|inst| inst.metadata.clone())
            .collect()
    }

    /// 获取插件元数据（通过插件 ID）
    pub async fn get_plugin(&self, plugin_id: &str) -> Option<PluginMetadata> {
        self.instances
            .read()
            .await
            .get(plugin_id)
            .map(|inst| inst.metadata.clone())
    }
}

/// WASM 插件实例
struct WasmPluginInstance {
    metadata: PluginMetadata,
}

/// 资源限制器实现
impl ResourceLimiter for WasmState {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        // 限制内存增长
        if desired <= self.max_memory {
            Ok(true)
        } else {
            tracing::warn!(
                "WASM plugin memory limit exceeded: current={} desired={} max={}",
                current,
                desired,
                self.max_memory
            );
            Ok(false)
        }
    }

    fn table_growing(
        &mut self,
        _current: u32,
        desired: u32,
        _maximum: Option<u32>,
    ) -> anyhow::Result<bool> {
        // 限制表大小
        Ok(desired <= 10000)
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

    #[tokio::test]
    async fn test_invalid_extension() {
        let config = WasmSandboxConfig::default();
        let loader = WasmPluginLoader::new(config).unwrap();

        use std::io::Write;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.as_file().write_all(b"not wasm").unwrap();

        let result = loader.load_plugin(tmp.path()).await;
        assert!(result.is_err());

        match result {
            Err(PluginError::LoadError(msg)) => {
                assert!(msg.contains("Invalid WASM file extension"));
            }
            _ => panic!("Expected LoadError for invalid extension"),
        }
    }
}
