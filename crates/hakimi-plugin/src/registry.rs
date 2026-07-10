use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::HakimiPlugin;
use crate::PluginMetadata;
use hakimi_common::error::{HakimiError, Result};

/// 插件注册表（管理所有已加载插件）
pub struct PluginRegistry {
    plugins: Arc<RwLock<HashMap<String, Arc<dyn HakimiPlugin>>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册插件
    pub async fn register(&self, plugin: Arc<dyn HakimiPlugin>) -> Result<()> {
        let metadata = plugin.metadata();
        let plugin_id = metadata.id.clone();

        // 检查是否已注册
        {
            let plugins = self.plugins.read().await;
            if plugins.contains_key(&plugin_id) {
                return Err(HakimiError::Other(format!(
                    "Plugin '{}' is already registered",
                    plugin_id
                )));
            }
        }

        // 检查依赖
        for dep in &metadata.dependencies {
            let plugins = self.plugins.read().await;
            if !plugins.contains_key(dep) {
                return Err(HakimiError::Other(format!(
                    "Plugin '{}' depends on '{}', which is not loaded",
                    plugin_id, dep
                )));
            }
        }

        // 注册插件
        {
            let mut plugins = self.plugins.write().await;
            plugins.insert(plugin_id.clone(), plugin);
        }

        tracing::info!("Plugin '{}' registered successfully", plugin_id);
        Ok(())
    }

    /// 卸载插件
    pub async fn unregister(&self, plugin_id: &str) -> Result<()> {
        let mut plugins = self.plugins.write().await;

        // 检查是否有其他插件依赖此插件
        for (id, plugin) in plugins.iter() {
            if plugin
                .metadata()
                .dependencies
                .contains(&plugin_id.to_string())
            {
                return Err(HakimiError::Other(format!(
                    "Cannot unregister '{}': plugin '{}' depends on it",
                    plugin_id, id
                )));
            }
        }

        plugins
            .remove(plugin_id)
            .ok_or_else(|| HakimiError::Other(format!("Plugin '{}' not found", plugin_id)))?;

        tracing::info!("Plugin '{}' unregistered", plugin_id);
        Ok(())
    }

    /// 获取插件
    pub async fn get(&self, plugin_id: &str) -> Option<Arc<dyn HakimiPlugin>> {
        let plugins = self.plugins.read().await;
        plugins.get(plugin_id).cloned()
    }

    /// 列出所有插件
    pub async fn list(&self) -> Vec<PluginMetadata> {
        let plugins = self.plugins.read().await;
        plugins.values().map(|p| p.metadata().clone()).collect()
    }

    /// 获取所有插件（用于批量钩子调用）
    pub async fn all(&self) -> Vec<Arc<dyn HakimiPlugin>> {
        let plugins = self.plugins.read().await;
        plugins.values().cloned().collect()
    }

    /// 获取插件数量
    pub async fn count(&self) -> usize {
        let plugins = self.plugins.read().await;
        plugins.len()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HakimiPlugin, PluginMetadata};
    use async_trait::async_trait;

    struct TestPlugin {
        metadata: PluginMetadata,
    }

    impl TestPlugin {
        fn new(id: &str, name: &str) -> Self {
            Self {
                metadata: PluginMetadata {
                    id: id.to_string(),
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    author: "Test".to_string(),
                    description: "Test plugin".to_string(),
                    dependencies: vec![],
                    min_hakimi_version: None,
                },
            }
        }

        fn with_dependencies(mut self, deps: Vec<String>) -> Self {
            self.metadata.dependencies = deps;
            self
        }
    }

    #[async_trait]
    impl HakimiPlugin for TestPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.metadata
        }
    }

    #[tokio::test]
    async fn test_register_plugin() {
        let registry = PluginRegistry::new();
        let plugin = Arc::new(TestPlugin::new("test.plugin", "Test Plugin"));

        let result = registry.register(plugin).await;
        assert!(result.is_ok());

        assert_eq!(registry.count().await, 1);
    }

    #[tokio::test]
    async fn test_duplicate_registration() {
        let registry = PluginRegistry::new();
        let plugin1 = Arc::new(TestPlugin::new("test.plugin", "Test Plugin"));
        let plugin2 = Arc::new(TestPlugin::new("test.plugin", "Test Plugin"));

        assert!(registry.register(plugin1).await.is_ok());
        let result = registry.register(plugin2).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already registered"));
    }

    #[tokio::test]
    async fn test_dependency_check() {
        let registry = PluginRegistry::new();

        // 先注册被依赖的插件
        let base_plugin = Arc::new(TestPlugin::new("base.plugin", "Base Plugin"));
        registry.register(base_plugin).await.unwrap();

        // 注册依赖 base.plugin 的插件
        let dependent_plugin = Arc::new(
            TestPlugin::new("dependent.plugin", "Dependent Plugin")
                .with_dependencies(vec!["base.plugin".to_string()]),
        );

        let result = registry.register(dependent_plugin).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_missing_dependency() {
        let registry = PluginRegistry::new();

        let plugin = Arc::new(
            TestPlugin::new("dependent.plugin", "Dependent Plugin")
                .with_dependencies(vec!["missing.plugin".to_string()]),
        );

        let result = registry.register(plugin).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not loaded"));
    }

    #[tokio::test]
    async fn test_list_plugins() {
        let registry = PluginRegistry::new();
        let plugin1 = Arc::new(TestPlugin::new("plugin1", "Plugin 1"));
        let plugin2 = Arc::new(TestPlugin::new("plugin2", "Plugin 2"));

        registry.register(plugin1).await.unwrap();
        registry.register(plugin2).await.unwrap();

        let list = registry.list().await;
        assert_eq!(list.len(), 2);

        let ids: Vec<String> = list.iter().map(|m| m.id.clone()).collect();
        assert!(ids.contains(&"plugin1".to_string()));
        assert!(ids.contains(&"plugin2".to_string()));
    }

    #[tokio::test]
    async fn test_get_plugin() {
        let registry = PluginRegistry::new();
        let plugin = Arc::new(TestPlugin::new("test.plugin", "Test Plugin"));

        registry.register(plugin).await.unwrap();

        let retrieved = registry.get("test.plugin").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().metadata().id, "test.plugin");

        let missing = registry.get("missing.plugin").await;
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_unregister_plugin() {
        let registry = PluginRegistry::new();
        let plugin = Arc::new(TestPlugin::new("test.plugin", "Test Plugin"));

        registry.register(plugin).await.unwrap();
        assert_eq!(registry.count().await, 1);

        let result = registry.unregister("test.plugin").await;
        assert!(result.is_ok());
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_unregister_with_dependents() {
        let registry = PluginRegistry::new();

        let base_plugin = Arc::new(TestPlugin::new("base.plugin", "Base Plugin"));
        registry.register(base_plugin).await.unwrap();

        let dependent_plugin = Arc::new(
            TestPlugin::new("dependent.plugin", "Dependent Plugin")
                .with_dependencies(vec!["base.plugin".to_string()]),
        );
        registry.register(dependent_plugin).await.unwrap();

        // 尝试卸载被依赖的插件
        let result = registry.unregister("base.plugin").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("depends on it"));
    }
}
