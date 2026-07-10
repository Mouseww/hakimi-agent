use crate::models::{
    InstalledManifest, InstalledPlugin, PluginMetadata, PluginRegistry, UpdateInfo,
};
use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// 插件市场管理器
pub struct PluginMarketplace {
    /// 注册表 URL
    registry_url: String,
    /// 缓存目录
    cache_dir: PathBuf,
    /// 已安装插件清单路径
    installed_manifest_path: PathBuf,
    /// 插件安装目录
    plugins_dir: PathBuf,
}

impl PluginMarketplace {
    /// 创建新的市场管理器
    pub fn new(registry_url: String, cache_dir: PathBuf, plugins_dir: PathBuf) -> Result<Self> {
        // 确保目录存在
        fs::create_dir_all(&cache_dir)?;
        fs::create_dir_all(&plugins_dir)?;

        let installed_manifest_path = plugins_dir.join("installed.yaml");

        Ok(Self {
            registry_url,
            cache_dir,
            installed_manifest_path,
            plugins_dir,
        })
    }

    /// 获取远程插件注册表
    pub async fn fetch_registry(&self) -> Result<PluginRegistry> {
        debug!("Fetching plugin registry from {}", self.registry_url);

        let response = reqwest::get(&self.registry_url)
            .await
            .context("Failed to fetch plugin registry")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch registry: HTTP {}",
                response.status()
            ));
        }

        let content = response
            .text()
            .await
            .context("Failed to read registry content")?;

        let registry: PluginRegistry =
            serde_yaml::from_str(&content).context("Failed to parse registry YAML")?;

        info!("Fetched registry with {} plugins", registry.plugins.len());

        // 缓存到本地
        let cache_path = self.cache_dir.join("registry.yaml");
        fs::write(&cache_path, content).context("Failed to cache registry")?;

        Ok(registry)
    }

    /// 从缓存加载注册表（fallback）
    pub fn load_cached_registry(&self) -> Result<PluginRegistry> {
        let cache_path = self.cache_dir.join("registry.yaml");
        let content = fs::read_to_string(&cache_path).context("Failed to read cached registry")?;
        let registry: PluginRegistry =
            serde_yaml::from_str(&content).context("Failed to parse cached registry")?;
        Ok(registry)
    }

    /// 安装插件
    pub async fn install_plugin(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<InstalledPlugin> {
        info!("Installing plugin: {}", name);

        // 获取注册表
        let registry = self.fetch_registry().await?;

        // 查找插件
        let metadata = registry
            .plugins
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| anyhow!("Plugin '{}' not found in registry", name))?;

        // 版本检查（如果指定）
        if let Some(req_version) = version {
            if metadata.version != req_version {
                return Err(anyhow!(
                    "Plugin '{}' version mismatch: requested {}, available {}",
                    name,
                    req_version,
                    metadata.version
                ));
            }
        }

        // 获取平台二进制文件名
        let binary_name = metadata
            .get_platform_binary()
            .ok_or_else(|| anyhow!("Plugin '{}' not available for current platform", name))?;

        // 构建下载 URL
        let download_url = format!("{}/{}", metadata.release_url, binary_name);
        debug!("Downloading from: {}", download_url);

        // 下载文件
        let response = reqwest::get(&download_url)
            .await
            .context("Failed to download plugin")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to download plugin: HTTP {}",
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .context("Failed to read plugin binary")?;

        // 验证校验和
        let checksum = metadata
            .get_platform_checksum()
            .ok_or_else(|| anyhow!("No checksum available for current platform"))?;
        self.verify_checksum(&bytes, checksum)?;

        // 保存到插件目录
        let plugin_path = self.plugins_dir.join(binary_name);
        fs::write(&plugin_path, bytes).context("Failed to write plugin binary")?;

        // 在 Unix 系统上设置可执行权限
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&plugin_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&plugin_path, perms)?;
        }

        // 更新已安装清单
        let installed = InstalledPlugin {
            name: name.to_string(),
            version: metadata.version.clone(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled: true,
            path: plugin_path.to_string_lossy().to_string(),
        };

        let mut manifest = self.load_installed_manifest()?;
        manifest.add(installed.clone());
        self.save_installed_manifest(&manifest)?;

        info!(
            "Successfully installed plugin '{}' version {}",
            name, metadata.version
        );

        Ok(installed)
    }

    /// 卸载插件
    pub fn uninstall_plugin(&self, name: &str) -> Result<()> {
        info!("Uninstalling plugin: {}", name);

        let mut manifest = self.load_installed_manifest()?;

        // 查找插件
        let plugin = manifest
            .find(name)
            .ok_or_else(|| anyhow!("Plugin '{}' not installed", name))?
            .clone();

        // 删除文件
        let path = Path::new(&plugin.path);
        if path.exists() {
            fs::remove_file(path).context("Failed to delete plugin binary")?;
            debug!("Deleted plugin binary: {}", plugin.path);
        } else {
            warn!("Plugin binary not found: {}", plugin.path);
        }

        // 更新清单
        manifest.remove(name);
        self.save_installed_manifest(&manifest)?;

        info!("Successfully uninstalled plugin '{}'", name);

        Ok(())
    }

    /// 检查更新
    pub async fn check_updates(&self) -> Result<Vec<UpdateInfo>> {
        let registry = self.fetch_registry().await?;
        let manifest = self.load_installed_manifest()?;

        let mut updates = Vec::new();

        for installed in &manifest.installed {
            if let Some(latest) = registry.plugins.iter().find(|p| p.name == installed.name) {
                if latest.version != installed.version {
                    updates.push(UpdateInfo {
                        name: installed.name.clone(),
                        current_version: installed.version.clone(),
                        latest_version: latest.version.clone(),
                        update_url: latest.release_url.clone(),
                    });
                }
            }
        }

        Ok(updates)
    }

    /// 搜索插件
    pub async fn search(&self, query: &str) -> Result<Vec<PluginMetadata>> {
        let registry = self.fetch_registry().await?;
        let results: Vec<PluginMetadata> = registry
            .plugins
            .into_iter()
            .filter(|p| p.matches_query(query))
            .collect();

        Ok(results)
    }

    /// 列出已安装插件
    pub fn list_installed(&self) -> Result<Vec<InstalledPlugin>> {
        let manifest = self.load_installed_manifest()?;
        Ok(manifest.installed)
    }

    /// 验证校验和
    fn verify_checksum(&self, data: &[u8], expected: &str) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        let actual = format!("{:x}", result);

        // 支持 "sha256:xxx" 格式
        let expected_hash = expected.strip_prefix("sha256:").unwrap_or(expected);

        if actual != expected_hash {
            return Err(anyhow!(
                "Checksum verification failed: expected {}, got {}",
                expected_hash,
                actual
            ));
        }

        debug!("Checksum verified: {}", expected_hash);
        Ok(())
    }

    /// 加载已安装清单
    fn load_installed_manifest(&self) -> Result<InstalledManifest> {
        if !self.installed_manifest_path.exists() {
            return Ok(InstalledManifest::empty());
        }

        let content = fs::read_to_string(&self.installed_manifest_path)
            .context("Failed to read installed manifest")?;

        let manifest: InstalledManifest =
            serde_yaml::from_str(&content).context("Failed to parse installed manifest")?;

        Ok(manifest)
    }

    /// 保存已安装清单
    fn save_installed_manifest(&self, manifest: &InstalledManifest) -> Result<()> {
        let content = serde_yaml::to_string(manifest).context("Failed to serialize manifest")?;

        // 确保父目录存在
        if let Some(parent) = self.installed_manifest_path.parent() {
            fs::create_dir_all(parent).context("Failed to create manifest directory")?;
        }

        fs::write(&self.installed_manifest_path, content)
            .context("Failed to write installed manifest")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_marketplace() -> (PluginMarketplace, TempDir, TempDir) {
        let cache_dir = TempDir::new().unwrap();
        let plugins_dir = TempDir::new().unwrap();

        let marketplace = PluginMarketplace::new(
            "https://example.com/registry.yaml".to_string(),
            cache_dir.path().to_path_buf(),
            plugins_dir.path().to_path_buf(),
        )
        .unwrap();

        (marketplace, cache_dir, plugins_dir)
    }

    #[test]
    fn test_marketplace_creation() {
        let (marketplace, cache_dir, plugins_dir) = create_test_marketplace();

        assert!(cache_dir.path().exists());
        assert!(plugins_dir.path().exists());
        assert_eq!(
            marketplace.installed_manifest_path,
            plugins_dir.path().join("installed.yaml")
        );
    }

    #[test]
    fn test_load_empty_manifest() {
        let (marketplace, _, _) = create_test_marketplace();
        let manifest = marketplace.load_installed_manifest().unwrap();
        assert_eq!(manifest.installed.len(), 0);
    }

    #[test]
    fn test_save_and_load_manifest() {
        let (marketplace, _, _) = create_test_marketplace();

        let mut manifest = InstalledManifest::empty();
        manifest.add(InstalledPlugin {
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            installed_at: "2026-07-10T10:00:00Z".to_string(),
            enabled: true,
            path: "/path/to/test.so".to_string(),
        });

        marketplace.save_installed_manifest(&manifest).unwrap();

        let loaded = marketplace.load_installed_manifest().unwrap();
        assert_eq!(loaded.installed.len(), 1);
        assert_eq!(loaded.installed[0].name, "test");
    }

    #[test]
    fn test_verify_checksum() {
        let (marketplace, _, _) = create_test_marketplace();

        let data = b"test data";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());

        // 测试正确校验和
        assert!(marketplace.verify_checksum(data, &hash).is_ok());

        // 测试带前缀的校验和
        let hash_with_prefix = format!("sha256:{}", hash);
        assert!(marketplace.verify_checksum(data, &hash_with_prefix).is_ok());

        // 测试错误校验和
        assert!(marketplace.verify_checksum(data, "wrong_hash").is_err());
    }

    #[test]
    fn test_list_installed() {
        let (marketplace, _, _) = create_test_marketplace();

        let mut manifest = InstalledManifest::empty();
        manifest.add(InstalledPlugin {
            name: "plugin1".to_string(),
            version: "0.1.0".to_string(),
            installed_at: "2026-07-10T10:00:00Z".to_string(),
            enabled: true,
            path: "/path/to/plugin1.so".to_string(),
        });
        manifest.add(InstalledPlugin {
            name: "plugin2".to_string(),
            version: "0.2.0".to_string(),
            installed_at: "2026-07-10T11:00:00Z".to_string(),
            enabled: false,
            path: "/path/to/plugin2.so".to_string(),
        });

        marketplace.save_installed_manifest(&manifest).unwrap();

        let installed = marketplace.list_installed().unwrap();
        assert_eq!(installed.len(), 2);
    }
}
