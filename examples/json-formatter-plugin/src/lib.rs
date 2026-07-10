//! JSON Formatter Plugin
//!
//! 格式化和验证 JSON 字符串的示例插件

use hakimi_plugin_sdk::*;

#[hakimi_plugin(
    name = "json-formatter",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Format and validate JSON strings with pretty-printing"
)]
pub struct JsonFormatterPlugin;

impl JsonFormatterPlugin {
    /// 插件执行函数 - 演示 JSON 格式化
    ///
    /// 当前版本格式化一个示例 JSON 以演示功能
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        ctx.log("info", "JSON Formatter Plugin executing");
        
        // 示例：格式化一个 JSON 对象
        let example_json = r#"{"name":"Alice","age":30,"emails":["alice@example.com","alice@work.com"],"address":{"city":"Beijing","country":"China"},"active":true}"#;
        
        ctx.log("info", &format!("Formatting {} bytes of JSON", example_json.len()));
        
        // 解析 JSON
        let value: serde_json::Value = serde_json::from_str(example_json)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
        
        // 格式化输出
        let formatted = serde_json::to_string_pretty(&value)
            .map_err(|e| format!("Failed to format JSON: {}", e))?;
        
        ctx.log("info", "JSON formatted successfully");
        
        let output = format!(
            "✅ JSON Formatter Plugin\n\n\
            Original ({} bytes):\n{}\n\n\
            Formatted ({} bytes):\n{}",
            example_json.len(),
            example_json,
            formatted.len(),
            formatted
        );
        
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_execute() {
        let plugin = JsonFormatterPlugin;
        let ctx = PluginContext::default();
        
        let result = plugin.execute(&ctx);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output.contains("JSON Formatter Plugin"));
        assert!(output.contains("\"name\": \"Alice\""));
        assert!(output.contains("\"age\": 30"));
    }
}
