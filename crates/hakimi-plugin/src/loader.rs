use std::path::{Path, PathBuf};

/// Legacy plugin loader stub for backward compatibility
/// 
/// This is a placeholder implementation to maintain compatibility with existing code.
/// The new plugin system uses `PluginRegistry` and `PluginManager` instead.
pub struct PluginLoader {
    plugin_dir: PathBuf,
}

impl PluginLoader {
    pub fn new() -> Self {
        Self {
            plugin_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".hakimi")
                .join("plugins"),
        }
    }
    
    pub fn plugin_dir(&self) -> &Path {
        &self.plugin_dir
    }
    
    pub fn plugins(&self) -> Vec<LegacyPlugin> {
        // Return empty vec for now - this is just a stub
        Vec::new()
    }
    
    pub fn load_all(&mut self) -> Result<(), String> {
        // Stub implementation
        Ok(())
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Legacy plugin struct for backward compatibility
pub struct LegacyPlugin {
    name: String,
    version: String,
    description: String,
    tools: Vec<LegacyTool>,
}

impl LegacyPlugin {
    pub fn name(&self) -> &str {
        &self.name
    }
    
    pub fn version(&self) -> &str {
        &self.version
    }
    
    pub fn description(&self) -> &str {
        &self.description
    }
    
    pub fn tools(&self) -> &[LegacyTool] {
        &self.tools
    }
}

/// Legacy tool struct for backward compatibility
pub struct LegacyTool {
    name: String,
}

impl LegacyTool {
    pub fn name(&self) -> &str {
        &self.name
    }
}
