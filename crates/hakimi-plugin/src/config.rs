use crate::PluginResult;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 插件配置文件结构（plugins.yaml）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
    /// 插件目录
    #[serde(default = "default_plugin_dir")]
    pub plugin_dir: PathBuf,

    /// 是否启用热加载
    #[serde(default)]
    pub enable_hot_reload: bool,

    /// 是否验证签名
    #[serde(default)]
    pub verify_signature: bool,

    /// 要加载的插件列表
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    /// 插件 ID
    pub id: String,

    /// 是否启用
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// 自定义配置（传递给插件）
    #[serde(default)]
    pub config: serde_json::Value,
}

fn default_plugin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hakimi")
        .join("plugins")
}

fn default_true() -> bool {
    true
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            plugin_dir: default_plugin_dir(),
            enable_hot_reload: false,
            verify_signature: false,
            plugins: vec![],
        }
    }
}

impl PluginsConfig {
    /// 从 YAML 文件加载配置
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> PluginResult<Self> {
        let path = path.as_ref();

        // 展开 ~ 符号
        let expanded_path = if path.starts_with("~") {
            dirs::home_dir()
                .ok_or_else(|| {
                    crate::PluginError::ConfigError("Cannot determine home directory".to_string())
                })?
                .join(path.strip_prefix("~").unwrap())
        } else {
            path.to_path_buf()
        };

        let content = std::fs::read_to_string(&expanded_path).map_err(|e| {
            crate::PluginError::ConfigError(format!("Failed to read config file: {}", e))
        })?;

        let config: Self = serde_yaml::from_str(&content)
            .map_err(|e| crate::PluginError::ConfigError(format!("Failed to parse YAML: {}", e)))?;

        Ok(config)
    }

    /// 保存到 YAML 文件
    pub fn save<P: AsRef<std::path::Path>>(&self, path: P) -> PluginResult<()> {
        let content = serde_yaml::to_string(self).map_err(|e| {
            crate::PluginError::ConfigError(format!("Failed to serialize to YAML: {}", e))
        })?;

        std::fs::write(path, content).map_err(|e| {
            crate::PluginError::ConfigError(format!("Failed to write config file: {}", e))
        })?;

        Ok(())
    }

    /// 生成示例配置
    pub fn example() -> Self {
        Self {
            plugin_dir: default_plugin_dir(),
            enable_hot_reload: true,
            verify_signature: false,
            plugins: vec![
                PluginEntry {
                    id: "logger".to_string(),
                    enabled: true,
                    config: serde_json::json!({
                        "level": "info",
                        "output": "stdout"
                    }),
                },
                PluginEntry {
                    id: "rate_limiter".to_string(),
                    enabled: true,
                    config: serde_json::json!({
                        "max_requests": 100,
                        "window_secs": 60
                    }),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PluginsConfig::default();
        assert!(!config.enable_hot_reload);
        assert!(!config.verify_signature);
        assert_eq!(config.plugins.len(), 0);
    }

    #[test]
    fn test_example_config() {
        let config = PluginsConfig::example();
        assert_eq!(config.plugins.len(), 2);
        assert_eq!(config.plugins[0].id, "logger");
        assert!(config.plugins[0].enabled);
    }

    #[test]
    fn test_config_serialization() {
        let config = PluginsConfig::example();
        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(yaml.contains("plugin_dir:"));
        assert!(yaml.contains("enable_hot_reload:"));
        assert!(yaml.contains("plugins:"));
    }

    #[test]
    fn test_config_deserialization() {
        let yaml = r#"
plugin_dir: /tmp/plugins
enable_hot_reload: true
verify_signature: false
plugins:
  - id: test_plugin
    enabled: true
    config:
      key: value
"#;

        let config: PluginsConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.plugin_dir, PathBuf::from("/tmp/plugins"));
        assert!(config.enable_hot_reload);
        assert_eq!(config.plugins.len(), 1);
        assert_eq!(config.plugins[0].id, "test_plugin");
    }
}
