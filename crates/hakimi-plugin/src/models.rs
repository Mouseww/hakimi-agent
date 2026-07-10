use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 插件注册表 - 市场上所有可用插件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRegistry {
    pub version: String,
    pub plugins: Vec<PluginMetadata>,
}

/// 插件元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub repository: String,
    pub release_url: String,
    pub platforms: HashMap<String, String>, // platform -> filename
    pub checksum: HashMap<String, String>,  // platform -> sha256 hash
}

/// 已安装插件清单
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledManifest {
    pub version: String,
    pub installed: Vec<InstalledPlugin>,
}

/// 已安装插件信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub name: String,
    pub version: String,
    pub installed_at: String, // ISO 8601 format
    pub enabled: bool,
    pub path: String,
}

/// 更新信息
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub name: String,
    pub current_version: String,
    pub latest_version: String,
    pub update_url: String,
}

impl PluginMetadata {
    /// 获取当前平台的二进制文件名
    pub fn get_platform_binary(&self) -> Option<&str> {
        let platform = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            return None;
        };

        self.platforms.get(platform).map(|s| s.as_str())
    }

    /// 获取当前平台的校验和
    pub fn get_platform_checksum(&self) -> Option<&str> {
        let platform = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            return None;
        };

        self.checksum.get(platform).map(|s| s.as_str())
    }

    /// 匹配搜索查询
    pub fn matches_query(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.name.to_lowercase().contains(&query_lower)
            || self.display_name.to_lowercase().contains(&query_lower)
            || self.description.to_lowercase().contains(&query_lower)
    }
}

impl InstalledManifest {
    /// 创建空清单
    pub fn empty() -> Self {
        Self {
            version: "1.0".to_string(),
            installed: Vec::new(),
        }
    }

    /// 查找已安装插件
    pub fn find(&self, name: &str) -> Option<&InstalledPlugin> {
        self.installed.iter().find(|p| p.name == name)
    }

    /// 添加插件
    pub fn add(&mut self, plugin: InstalledPlugin) {
        // 移除旧版本
        self.installed.retain(|p| p.name != plugin.name);
        self.installed.push(plugin);
    }

    /// 移除插件
    pub fn remove(&mut self, name: &str) -> bool {
        let original_len = self.installed.len();
        self.installed.retain(|p| p.name != name);
        self.installed.len() < original_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_metadata_matches_query() {
        let plugin = PluginMetadata {
            name: "logger".to_string(),
            display_name: "Session Logger".to_string(),
            description: "Records all session events".to_string(),
            author: "Hakimi Team".to_string(),
            version: "0.1.0".to_string(),
            repository: "https://github.com/hakimi/logger".to_string(),
            release_url: "https://github.com/hakimi/logger/releases".to_string(),
            platforms: HashMap::new(),
            checksum: HashMap::new(),
        };

        assert!(plugin.matches_query("logger"));
        assert!(plugin.matches_query("session"));
        assert!(plugin.matches_query("RECORDS"));
        assert!(!plugin.matches_query("analytics"));
    }

    #[test]
    fn test_installed_manifest_operations() {
        let mut manifest = InstalledManifest::empty();

        let plugin1 = InstalledPlugin {
            name: "logger".to_string(),
            version: "0.1.0".to_string(),
            installed_at: "2026-07-10T10:00:00Z".to_string(),
            enabled: true,
            path: "/path/to/logger.so".to_string(),
        };

        // 添加
        manifest.add(plugin1.clone());
        assert_eq!(manifest.installed.len(), 1);
        assert!(manifest.find("logger").is_some());

        // 更新（相同名称）
        let plugin1_v2 = InstalledPlugin {
            name: "logger".to_string(),
            version: "0.2.0".to_string(),
            installed_at: "2026-07-10T11:00:00Z".to_string(),
            enabled: true,
            path: "/path/to/logger.so".to_string(),
        };
        manifest.add(plugin1_v2);
        assert_eq!(manifest.installed.len(), 1);
        assert_eq!(manifest.find("logger").unwrap().version, "0.2.0");

        // 移除
        assert!(manifest.remove("logger"));
        assert_eq!(manifest.installed.len(), 0);
        assert!(!manifest.remove("nonexistent"));
    }
}
