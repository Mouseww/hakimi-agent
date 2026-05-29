use std::path::{Path, PathBuf};
use std::sync::Arc;

use hakimi_common::{HakimiError, Result};
use hakimi_tools::Tool;
use tracing::{debug, info, warn};

use crate::Plugin;
use crate::http_tool::{HttpPluginConfig, HttpToolPlugin};

/// Default plugin directory: ~/.hakimi/plugins/
fn default_plugin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hakimi")
        .join("plugins")
}

/// Plugin loader that discovers and loads plugins from various sources.
pub struct PluginLoader {
    /// Directory to scan for plugin files.
    plugin_dir: PathBuf,
    /// All loaded plugins.
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginLoader {
    /// Create a new plugin loader using the default plugin directory.
    pub fn new() -> Self {
        Self {
            plugin_dir: default_plugin_dir(),
            plugins: Vec::new(),
        }
    }

    /// Create a plugin loader with a custom plugin directory.
    pub fn with_dir(plugin_dir: impl Into<PathBuf>) -> Self {
        Self {
            plugin_dir: plugin_dir.into(),
            plugins: Vec::new(),
        }
    }

    /// Return the plugin directory path.
    pub fn plugin_dir(&self) -> &Path {
        &self.plugin_dir
    }

    /// Load all plugins from the plugin directory.
    ///
    /// Scans for:
    /// - `.yaml` / `.yml` files → HTTP tool plugin configs
    /// - `.json` files → HTTP tool plugin configs
    /// - `.so` / `.dylib` files → native dynamic library plugins (stub)
    /// - `.wasm` files → WASM plugins (stub)
    pub fn load_all(&mut self) -> Result<()> {
        if !self.plugin_dir.exists() {
            info!(dir = %self.plugin_dir.display(), "plugin directory does not exist, skipping");
            return Ok(());
        }

        info!(dir = %self.plugin_dir.display(), "scanning for plugins");

        let dir = self.plugin_dir.clone();
        // Load YAML/JSON config files as HTTP tool plugins
        self.load_http_plugins(&dir)?;

        // Stub: native dynamic libraries
        self.load_native_plugins(&dir)?;

        // Stub: WASM plugins
        self.load_wasm_plugins(&dir)?;

        info!(count = self.plugins.len(), "plugins loaded");
        Ok(())
    }

    /// Load an explicit HTTP plugin config (YAML string).
    pub fn load_http_from_yaml(&mut self, yaml: &str) -> Result<()> {
        let config: HttpPluginConfig = serde_yaml::from_str(yaml)
            .map_err(|e| HakimiError::Config(format!("invalid plugin config: {e}")))?;
        let name = config.name.clone();
        let mut plugin = HttpToolPlugin::from_config(&config);
        plugin.init(&serde_json::Value::Null)?;
        info!(plugin = %name, "loaded HTTP plugin from config");
        self.plugins.push(Box::new(plugin));
        Ok(())
    }

    /// Get all tools from all loaded plugins.
    pub fn all_tools(&self) -> Vec<Arc<dyn Tool>> {
        self.plugins.iter().flat_map(|p| p.tools()).collect()
    }

    /// Get a reference to all loaded plugins.
    pub fn plugins(&self) -> &[Box<dyn Plugin>] {
        &self.plugins
    }

    /// Scan directory for .yaml/.yml/.json files and load as HTTP tool plugins.
    fn load_http_plugins(&mut self, dir: &Path) -> Result<()> {
        let extensions = ["yaml", "yml", "json"];
        let mut paths = Vec::new();
        for ext in &extensions {
            let pattern = dir.join(format!("*.{ext}"));
            let pattern_str = pattern.to_string_lossy();
            if let Ok(entries) = glob::glob(&pattern_str) {
                for p in entries.flatten() {
                    paths.push(p);
                }
            }
        }
        for path in paths {
            match self.load_http_plugin_file(&path) {
                Ok(()) => {}
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to load plugin file");
                }
            }
        }
        Ok(())
    }

    /// Load a single HTTP plugin config file.
    fn load_http_plugin_file(&mut self, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path).map_err(HakimiError::Io)?;

        let config: HttpPluginConfig = if path.extension().is_some_and(|e| e == "json") {
            serde_json::from_str(&content)
                .map_err(|e| HakimiError::Config(format!("invalid JSON plugin config: {e}")))?
        } else {
            serde_yaml::from_str(&content)
                .map_err(|e| HakimiError::Config(format!("invalid YAML plugin config: {e}")))?
        };

        let name = config.name.clone();
        let mut plugin = HttpToolPlugin::from_config(&config);
        plugin.init(&serde_json::Value::Null)?;

        info!(plugin = %name, path = %path.display(), "loaded HTTP plugin");
        self.plugins.push(Box::new(plugin));
        Ok(())
    }

    /// Stub: scan for .so/.dylib native plugins.
    fn load_native_plugins(&self, dir: &Path) -> Result<()> {
        let patterns = ["*.so", "*.dylib"];
        for pattern in &patterns {
            let full = dir.join(pattern);
            let full_str = full.to_string_lossy();
            if let Ok(entries) = glob::glob(&full_str) {
                for entry in entries.flatten() {
                    let path = entry;
                    debug!(path = %path.display(), "native plugin found (loading not yet implemented)");
                    // TODO: Use `libloading` to dlopen the shared library and
                    // call a known entry-point symbol to obtain a Plugin impl.
                }
            }
        }
        Ok(())
    }

    /// Stub: scan for .wasm plugin files.
    fn load_wasm_plugins(&self, dir: &Path) -> Result<()> {
        let pattern = dir.join("*.wasm");
        let pattern_str = pattern.to_string_lossy();
        if let Ok(entries) = glob::glob(&pattern_str) {
            for entry in entries.flatten() {
                let path = entry;
                debug!(path = %path.display(), "WASM plugin found (loading not yet implemented)");
                // TODO: Use a WASM runtime (e.g. wasmtime) to load the module
                // and call a known export to obtain a Plugin impl.
            }
        }
        Ok(())
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_http_plugin_from_yaml() {
        let mut loader = PluginLoader::new();
        let yaml = r#"
name: test_api
tools:
  - name: get_user
    endpoint: https://api.example.com/users/{user_id}
    method: GET
    description: Get user by ID
    parameters:
      type: object
      properties:
        user_id:
          type: string
"#;
        loader.load_http_from_yaml(yaml).unwrap();
        assert_eq!(loader.plugins().len(), 1);
        assert_eq!(loader.plugins()[0].name(), "test_api");

        let tools = loader.all_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "get_user");
        assert_eq!(tools[0].toolset(), "http");
    }

    #[test]
    fn test_load_http_plugin_uses_declared_metadata() {
        let mut loader = PluginLoader::new();
        let yaml = r#"
name: weather_api
version: "1.2.3"
description: Weather API wrapper
tools:
  - name: get_weather
    endpoint: https://wttr.in/{city}
    method: GET
    description: Get weather
"#;
        loader.load_http_from_yaml(yaml).unwrap();

        assert_eq!(loader.plugins()[0].name(), "weather_api");
        assert_eq!(loader.plugins()[0].version(), "1.2.3");
        assert_eq!(loader.plugins()[0].description(), "Weather API wrapper");
    }

    #[test]
    fn test_load_http_plugins_from_dir() {
        let tmp = TempDir::new().unwrap();
        let yaml = r#"
name: dir_plugin
tools:
  - name: ping
    endpoint: https://example.com/ping
    method: GET
    description: Health check
"#;
        fs::write(tmp.path().join("ping.yaml"), yaml).unwrap();

        let mut loader = PluginLoader::with_dir(tmp.path());
        loader.load_all().unwrap();
        assert_eq!(loader.plugins().len(), 1);
        assert_eq!(loader.all_tools().len(), 1);
    }

    #[test]
    fn test_empty_plugin_dir() {
        let tmp = TempDir::new().unwrap();
        let mut loader = PluginLoader::with_dir(tmp.path());
        loader.load_all().unwrap();
        assert_eq!(loader.plugins().len(), 0);
    }

    #[test]
    fn test_missing_plugin_dir() {
        let mut loader = PluginLoader::with_dir("/nonexistent/path/plugins");
        loader.load_all().unwrap();
        assert_eq!(loader.plugins().len(), 0);
    }
}
