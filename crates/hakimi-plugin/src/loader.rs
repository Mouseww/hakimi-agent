use crate::{PluginError, PluginMetadata, PluginResult};
use libloading::{Library, Symbol};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 插件加载器配置
#[derive(Debug, Clone)]
pub struct PluginLoaderConfig {
    /// 插件目录路径
    pub plugin_dir: PathBuf,

    /// 是否启用热加载
    pub enable_hot_reload: bool,

    /// 是否验证插件签名
    pub verify_signature: bool,

    /// 允许的插件白名单（为空则允许所有）
    pub allowed_plugins: Vec<String>,
}

impl Default for PluginLoaderConfig {
    fn default() -> Self {
        let plugin_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".hakimi")
            .join("plugins");

        Self {
            plugin_dir,
            enable_hot_reload: false,
            verify_signature: false,
            allowed_plugins: vec![],
        }
    }
}

/// 插件加载器，管理动态库生命周期
pub struct PluginLoader {
    /// 已加载的动态库
    libraries: Arc<RwLock<HashMap<String, Library>>>,

    /// 已加载的插件实例（使用 Box 存储 trait object）
    plugins: Arc<RwLock<HashMap<String, Arc<PluginHandle>>>>,

    /// 插件配置
    config: PluginLoaderConfig,
}

/// 插件句柄，封装插件实例和元数据
struct PluginHandle {
    metadata: PluginMetadata,
    // 实际的插件指针（暂时存储为元数据，实际动态加载需要更复杂的处理）
}

impl PluginLoader {
    /// 创建新的插件加载器
    pub fn new(config: PluginLoaderConfig) -> Self {
        Self {
            libraries: Arc::new(RwLock::new(HashMap::new())),
            plugins: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// 获取插件目录路径
    pub fn plugin_dir(&self) -> &Path {
        &self.config.plugin_dir
    }

    /// 同步获取已加载插件列表（返回空 vec 作为占位）
    pub fn plugins(&self) -> Vec<PluginMetadata> {
        // 简化版本：直接返回空列表
        // 实际应该从 plugins RwLock 中读取，但需要 async
        vec![]
    }

    /// 加载插件目录中的所有插件（同步包装）
    pub fn load_all(&self) -> PluginResult<()> {
        // 简化版本：暂不实现自动加载
        // 实际应该扫描 plugin_dir 并加载所有插件
        Ok(())
    }

    /// 从共享库加载插件
    ///
    /// # 安全性
    ///
    /// 此函数使用 `libloading` 加载动态库，存在以下风险：
    /// - 加载恶意代码
    /// - 符号不匹配导致未定义行为
    /// - 内存安全问题
    ///
    /// 建议：
    /// 1. 仅从可信源加载插件
    /// 2. 启用签名验证
    /// 3. 使用沙箱环境（WASM 或容器）
    pub async fn load_plugin<P: AsRef<Path>>(&self, library_path: P) -> PluginResult<String> {
        let path = library_path.as_ref();

        // 验证文件存在
        if !path.exists() {
            return Err(PluginError::LoadError(format!(
                "Plugin library not found: {}",
                path.display()
            )));
        }

        // 验证文件扩展名
        let ext = path.extension().and_then(|e| e.to_str());
        match ext {
            Some("so") | Some("dylib") | Some("dll") => {}
            _ => {
                return Err(PluginError::LoadError(format!(
                    "Invalid library extension: {:?}",
                    ext
                )));
            }
        }

        // 验证签名（可选）
        if self.config.verify_signature {
            self.verify_plugin_signature(path)?;
        }

        // 加载动态库
        let library = unsafe {
            Library::new(path)
                .map_err(|e| PluginError::LoadError(format!("Failed to load library: {}", e)))?
        };

        // 查找插件元数据函数
        // 插件必须导出 `plugin_metadata` 函数：
        // #[no_mangle]
        // pub extern "C" fn plugin_metadata() -> PluginMetadata
        let get_metadata: Symbol<unsafe extern "C" fn() -> PluginMetadata> = unsafe {
            library.get(b"plugin_metadata\0").map_err(|e| {
                PluginError::LoadError(format!("Plugin missing 'plugin_metadata' symbol: {}", e))
            })?
        };

        let metadata = unsafe { get_metadata() };
        let plugin_id = metadata.id.clone();

        // 检查白名单
        if !self.config.allowed_plugins.is_empty()
            && !self.config.allowed_plugins.contains(&plugin_id)
        {
            return Err(PluginError::PermissionDenied(format!(
                "Plugin '{}' not in allowed list",
                plugin_id
            )));
        }

        // 创建插件句柄
        let handle = Arc::new(PluginHandle {
            metadata: metadata.clone(),
        });

        // 存储库和插件
        self.libraries
            .write()
            .await
            .insert(plugin_id.clone(), library);
        self.plugins.write().await.insert(plugin_id.clone(), handle);

        tracing::info!("Loaded plugin: {} v{}", metadata.name, metadata.version);

        Ok(plugin_id)
    }

    /// 根据插件 ID 加载插件（自动查找路径）
    pub async fn load_plugin_by_id(&self, plugin_id: &str) -> PluginResult<String> {
        let path = self.find_plugin_path(plugin_id)?;
        self.load_plugin(&path).await
    }

    /// 卸载插件
    pub async fn unload_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        // 移除插件实例
        self.plugins.write().await.remove(plugin_id);

        // 卸载动态库
        self.libraries.write().await.remove(plugin_id);

        tracing::info!("Unloaded plugin: {}", plugin_id);
        Ok(())
    }

    /// 重载插件（热更新）
    pub async fn reload_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        // 查找插件路径
        let path = self.find_plugin_path(plugin_id)?;

        // 卸载旧版本
        self.unload_plugin(plugin_id).await?;

        // 加载新版本
        self.load_plugin(&path).await?;

        tracing::info!("Reloaded plugin: {}", plugin_id);
        Ok(())
    }

    /// 列出已加载插件
    pub async fn list_plugins(&self) -> Vec<PluginMetadata> {
        let plugins = self.plugins.read().await;
        plugins
            .values()
            .map(|handle| handle.metadata.clone())
            .collect()
    }

    /// 获取插件元数据
    pub async fn get_plugin_metadata(&self, plugin_id: &str) -> Option<PluginMetadata> {
        self.plugins
            .read()
            .await
            .get(plugin_id)
            .map(|handle| handle.metadata.clone())
    }

    /// 检查插件是否已加载
    pub async fn is_loaded(&self, plugin_id: &str) -> bool {
        self.plugins.read().await.contains_key(plugin_id)
    }

    /// 验证插件签名
    fn verify_plugin_signature(&self, _path: &Path) -> PluginResult<()> {
        // TODO: 实现签名验证逻辑
        // 1. 读取 .sig 文件
        // 2. 计算库文件 SHA256
        // 3. 验证签名

        tracing::warn!("Plugin signature verification not implemented yet");
        Ok(())
    }

    /// 查找插件动态库路径
    fn find_plugin_path(&self, plugin_id: &str) -> PluginResult<PathBuf> {
        let plugin_dir = &self.config.plugin_dir;

        // 尝试多种扩展名
        let extensions = if cfg!(target_os = "linux") {
            vec!["so"]
        } else if cfg!(target_os = "macos") {
            vec!["dylib"]
        } else if cfg!(target_os = "windows") {
            vec!["dll"]
        } else {
            vec!["so", "dylib", "dll"]
        };

        for ext in extensions {
            // 尝试带 lib 前缀的名称（Unix 风格）
            let path = plugin_dir.join(format!("lib{}.{}", plugin_id, ext));
            if path.exists() {
                return Ok(path);
            }

            // 尝试不带 lib 前缀的名称（Windows 风格）
            let path = plugin_dir.join(format!("{}.{}", plugin_id, ext));
            if path.exists() {
                return Ok(path);
            }
        }

        Err(PluginError::NotFound(format!(
            "Plugin library not found: {}",
            plugin_id
        )))
    }

    /// 启动文件监控（热加载）
    pub async fn start_hot_reload(&self) -> PluginResult<()> {
        if !self.config.enable_hot_reload {
            return Ok(());
        }

        // TODO: 使用 notify crate 监控插件目录
        // 当检测到 .so/.dylib/.dll 文件变化时，自动 reload_plugin()

        tracing::info!("Plugin hot-reload enabled (not implemented yet)");
        Ok(())
    }
}

impl Drop for PluginLoader {
    fn drop(&mut self) {
        tracing::debug!("PluginLoader dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plugin_loader_creation() {
        let config = PluginLoaderConfig::default();
        let loader = PluginLoader::new(config);

        assert_eq!(loader.list_plugins().await.len(), 0);
    }

    #[tokio::test]
    async fn test_find_plugin_path_not_found() {
        let config = PluginLoaderConfig {
            plugin_dir: PathBuf::from("/nonexistent"),
            ..Default::default()
        };
        let loader = PluginLoader::new(config);

        let result = loader.find_plugin_path("nonexistent_plugin");
        assert!(result.is_err());

        match result {
            Err(PluginError::NotFound(msg)) => {
                assert!(msg.contains("nonexistent_plugin"));
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_load_plugin_file_not_found() {
        let config = PluginLoaderConfig::default();
        let loader = PluginLoader::new(config);

        let result = loader.load_plugin("/nonexistent/plugin.so").await;
        assert!(result.is_err());

        match result {
            Err(PluginError::LoadError(msg)) => {
                assert!(msg.contains("not found"));
            }
            _ => panic!("Expected LoadError"),
        }
    }

    #[tokio::test]
    async fn test_load_plugin_invalid_extension() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let invalid_file = temp_dir.path().join("plugin.txt");
        fs::write(&invalid_file, b"not a library").unwrap();

        let config = PluginLoaderConfig::default();
        let loader = PluginLoader::new(config);

        let result = loader.load_plugin(&invalid_file).await;
        assert!(result.is_err());

        match result {
            Err(PluginError::LoadError(msg)) => {
                assert!(msg.contains("Invalid library extension"));
            }
            _ => panic!("Expected LoadError for invalid extension"),
        }
    }

    #[tokio::test]
    async fn test_unload_nonexistent_plugin() {
        let config = PluginLoaderConfig::default();
        let loader = PluginLoader::new(config);

        // 卸载不存在的插件应该成功（幂等操作）
        let result = loader.unload_plugin("nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_plugin_whitelist() {
        let config = PluginLoaderConfig {
            plugin_dir: PathBuf::from("/tmp"),
            allowed_plugins: vec!["allowed_plugin".to_string()],
            ..Default::default()
        };
        let loader = PluginLoader::new(config);

        // 测试白名单逻辑（虽然会在加载时失败，但可以验证结构）
        assert_eq!(loader.config.allowed_plugins.len(), 1);
    }
}
