use hakimi_plugin::PluginMetadata;

/// 示例插件：简单的日志记录插件
pub struct ExamplePlugin;

impl ExamplePlugin {
    pub fn new() -> Self {
        Self
    }
}

/// 导出插件元数据（动态加载入口点）
#[no_mangle]
pub extern "C" fn plugin_metadata() -> PluginMetadata {
    PluginMetadata {
        id: "example_plugin".to_string(),
        name: "Example Logger Plugin".to_string(),
        version: "0.1.0".to_string(),
        author: "Hakimi Team".to_string(),
        description: "A simple logging plugin for demonstration".to_string(),
        dependencies: vec![],
        min_hakimi_version: Some("0.5.0".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_metadata() {
        let metadata = plugin_metadata();
        assert_eq!(metadata.id, "example_plugin");
        assert_eq!(metadata.name, "Example Logger Plugin");
        assert_eq!(metadata.version, "0.1.0");
    }
}
