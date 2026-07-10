//! Snippet Store Plugin
//!
//! 代码片段存储和检索示例插件
//! 演示状态管理和数据序列化

use hakimi_plugin_sdk::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
struct Snippet {
    name: String,
    code: String,
    language: String,
    description: String,
}

#[hakimi_plugin(
    name = "snippet-store",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Store and retrieve code snippets with metadata"
)]
pub struct SnippetStorePlugin;

impl SnippetStorePlugin {
    /// 插件执行函数 - 演示代码片段管理
    ///
    /// 展示预定义的代码片段库
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        ctx.log("info", "Snippet Store Plugin executing");
        
        // 预定义的代码片段库
        let snippets = vec![
            Snippet {
                name: "hello-world-rust".to_string(),
                code: "fn main() {\\n    println!(\\\"Hello, World!\\\");\\n}".to_string(),
                language: "rust".to_string(),
                description: "Basic Rust hello world".to_string(),
            },
            Snippet {
                name: "factorial-python".to_string(),
                code: "def factorial(n):\\n    return 1 if n <= 1 else n * factorial(n-1)".to_string(),
                language: "python".to_string(),
                description: "Recursive factorial function".to_string(),
            },
            Snippet {
                name: "quicksort-js".to_string(),
                code: "const quicksort = arr => arr.length <= 1 ? arr : [\\n  ...quicksort(arr.slice(1).filter(x => x < arr[0])),\\n  arr[0],\\n  ...quicksort(arr.slice(1).filter(x => x >= arr[0]))\\n];".to_string(),
                language: "javascript".to_string(),
                description: "Functional quicksort implementation".to_string(),
            },
        ];
        
        ctx.log("info", &format!("Found {} snippets in store", snippets.len()));
        
        let mut output = String::from("📚 Snippet Store Plugin\\n\\n");
        output.push_str(&format!("Total Snippets: {}\\n\\n", snippets.len()));
        
        for (idx, snippet) in snippets.iter().enumerate() {
            output.push_str(&format!(
                "{}. {} [{}]\\n   {}\\n   Code:\\n{}\\n\\n",
                idx + 1,
                snippet.name,
                snippet.language,
                snippet.description,
                self.indent_code(&snippet.code, 3)
            ));
        }
        
        Ok(output)
    }
    
    /// 辅助函数：为代码添加缩进
    fn indent_code(&self, code: &str, spaces: usize) -> String {
        let indent = " ".repeat(spaces);
        code.lines()
            .map(|line| format!("{}{}", indent, line))
            .collect::<Vec<_>>()
            .join("\\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet_store_plugin() {
        let plugin = SnippetStorePlugin;
        let ctx = PluginContext::default();
        
        let result = plugin.execute(&ctx);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output.contains("Snippet Store Plugin"));
        assert!(output.contains("Total Snippets: 3"));
        assert!(output.contains("hello-world-rust"));
    }
    
    #[test]
    fn test_indent_code() {
        let plugin = SnippetStorePlugin;
        let code = "line1\\nline2";
        let indented = plugin.indent_code(code, 2);
        
        assert!(indented.starts_with("  "));
        assert!(indented.contains("\\n  line2"));
    }
}
