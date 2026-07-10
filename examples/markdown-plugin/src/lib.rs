//! Markdown Plugin
//!
//! 简单的 Markdown 转 HTML 示例插件
//! 演示文本处理和格式转换

use hakimi_plugin_sdk::*;

#[hakimi_plugin(
    name = "markdown-plugin",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Convert Markdown to formatted text or HTML"
)]
pub struct MarkdownPlugin;

impl MarkdownPlugin {
    /// 插件执行函数 - 处理 Markdown 文本
    ///
    /// 当前版本进行简单的 Markdown 格式化演示
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        ctx.log("info", "Markdown Plugin executing");
        
        // 示例 Markdown 文本
        let markdown_text = r#"# Hakimi Agent

Welcome to **Hakimi**, a powerful AI agent framework!

## Features

- **WASM Plugin System**: Extend functionality with plugins
- **Memory Management**: Efficient context handling
- **Session Tracking**: Full conversation lineage
- **Tool Integration**: Built-in and custom tools

## Code Example

```rust
use hakimi_plugin_sdk::*;

#[hakimi_plugin(name = "my-plugin")]
pub struct MyPlugin;
```

## Links

- [GitHub](https://github.com/hakimi-agent)
- [Documentation](https://docs.hakimi-agent.com)

---

*Powered by Hakimi v0.5*
"#;
        
        ctx.log("info", &format!("Processing {} bytes of Markdown", markdown_text.len()));
        
        // 简化版转换（实际可以使用 pulldown-cmark 等库）
        let formatted = self.format_markdown(markdown_text);
        
        ctx.log("info", "Markdown processed successfully");
        
        let output = format!(
            "📝 Markdown Plugin\\n\\n\
            === Original Markdown ===\\n{}\\n\\n\
            === Processed Output ===\\n{}",
            markdown_text,
            formatted
        );
        
        Ok(output)
    }
    
    /// 简单的 Markdown 格式化（演示用）
    fn format_markdown(&self, text: &str) -> String {
        let mut result = String::new();
        
        for line in text.lines() {
            let formatted_line = if line.starts_with("# ") {
                format!("\\n=== {} ===\\n", &line[2..])
            } else if line.starts_with("## ") {
                format!("\\n--- {} ---\\n", &line[3..])
            } else if line.starts_with("- ") {
                format!("  • {}", &line[2..])
            } else if line.starts_with("**") && line.ends_with("**") {
                line.replace("**", "").to_uppercase()
            } else {
                line.to_string()
            };
            
            result.push_str(&formatted_line);
            result.push('\n');
        }
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_plugin() {
        let plugin = MarkdownPlugin;
        let ctx = PluginContext::default();
        
        let result = plugin.execute(&ctx);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output.contains("Markdown Plugin"));
        assert!(output.contains("Hakimi Agent"));
    }
    
    #[test]
    fn test_format_markdown() {
        let plugin = MarkdownPlugin;
        let markdown = "# Title\\n- Item 1\\n- Item 2";
        let formatted = plugin.format_markdown(markdown);
        
        assert!(formatted.contains("=== Title ==="));
        assert!(formatted.contains("• Item 1"));
    }
}
