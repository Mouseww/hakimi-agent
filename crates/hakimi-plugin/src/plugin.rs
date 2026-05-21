use std::sync::Arc;

use hakimi_common::Result;
use hakimi_tools::Tool;

/// Core trait for plugins that provide tools to the Hakimi Agent.
///
/// Each plugin can register one or more tools. Plugins are discovered and loaded
/// by the [`PluginLoader`](crate::PluginLoader).
pub trait Plugin: Send + Sync {
    /// Unique name identifying this plugin.
    fn name(&self) -> &str;

    /// Semver version string of the plugin.
    fn version(&self) -> &str;

    /// Human-readable description of what the plugin does.
    fn description(&self) -> &str;

    /// Return the tools provided by this plugin.
    fn tools(&self) -> Vec<Arc<dyn Tool>>;

    /// Initialize the plugin with the given configuration.
    ///
    /// Called once after loading. The config value is typically parsed from
    /// a YAML/JSON file or passed programmatically.
    fn init(&mut self, config: &serde_json::Value) -> Result<()>;
}
