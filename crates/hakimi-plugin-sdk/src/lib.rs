//! Hakimi Plugin SDK
//!
//! 提供类型安全的 API 用于开发 Hakimi WASM 插件。
//!
//! # 示例
//!
//! ```ignore
//! use hakimi_plugin_sdk::*;
//!
//! #[hakimi_plugin(
//!     name = "hello-plugin",
//!     version = "0.1.0",
//!     author = "Your Name"
//! )]
//! pub struct MyPlugin;
//!
//! impl MyPlugin {
//!     pub fn execute(&self, ctx: &PluginContext) -> Result<String, String> {
//!         ctx.log("info", "Plugin executed!");
//!         Ok("Hello from WASM!".to_string())
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};

pub use hakimi_plugin_sdk_macro::hakimi_plugin;
pub use serde;
pub use serde_json;

/// 插件元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}

/// 插件上下文 - 访问宿主功能
pub struct PluginContext {
    // 上下文数据（未来可扩展）
}

impl PluginContext {
    /// 创建新的上下文
    pub fn new() -> Self {
        Self {}
    }

    /// 记录日志到宿主
    ///
    /// # 参数
    ///
    /// * `level` - 日志级别 ("trace", "debug", "info", "warn", "error")
    /// * `message` - 日志消息
    ///
    /// # 示例
    ///
    /// ```ignore
    /// ctx.log("info", "Plugin is executing...");
    /// ```
    pub fn log(&self, level: &str, message: &str) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            host_log(level.as_ptr(), level.len() as i32, message.as_ptr(), message.len() as i32);
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // 非 WASM 环境下的模拟实现（用于测试）
            eprintln!("[{}] {}", level.to_uppercase(), message);
        }
    }

    /// 发起 HTTP GET 请求（通过宿主）
    ///
    /// # 参数
    ///
    /// * `url` - 请求 URL
    ///
    /// # 返回
    ///
    /// * `Ok(String)` - 响应体
    /// * `Err(String)` - 错误消息
    ///
    /// # 示例
    ///
    /// ```ignore
    /// let response = ctx.http_get("https://api.example.com/data")?;
    /// ```
    pub fn http_get(&self, url: &str) -> Result<String, String> {
        #[cfg(target_arch = "wasm32")]
        {
            let mut buf = vec![0u8; 4096];
            let len =
                unsafe { host_http_request(url.as_ptr(), url.len() as i32, buf.as_mut_ptr(), buf.len() as i32) };

            if len < 0 {
                return Err(format!("HTTP request failed with code: {}", len));
            }

            buf.truncate(len as usize);
            String::from_utf8(buf).map_err(|e| format!("Invalid UTF-8 response: {}", e))
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // 非 WASM 环境下返回模拟数据
            let _ = url; // 防止未使用警告
            Err("HTTP requests only available in WASM runtime".to_string())
        }
    }
}

impl Default for PluginContext {
    fn default() -> Self {
        Self::new()
    }
}

// 宿主函数外部声明（仅在 WASM 目标下）
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "hakimi")]
extern "C" {
    fn host_log(level_ptr: *const u8, level_len: i32, msg_ptr: *const u8, msg_len: i32);
    fn host_http_request(
        url_ptr: *const u8,
        url_len: i32,
        out_ptr: *mut u8,
        out_len: i32,
    ) -> i32;
}

/// 标准插件结果类型
pub type PluginResult<T> = Result<T, String>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_context_creation() {
        let ctx = PluginContext::new();
        // 上下文创建成功（无 panic）
        assert!(std::mem::size_of_val(&ctx) >= 0);
    }

    #[test]
    fn test_metadata_serialization() {
        let metadata = PluginMetadata {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            author: "Test Author".to_string(),
            description: "Test description".to_string(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: PluginMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata.name, deserialized.name);
        assert_eq!(metadata.version, deserialized.version);
        assert_eq!(metadata.author, deserialized.author);
    }

    #[test]
    fn test_log_in_non_wasm() {
        let ctx = PluginContext::new();
        // 应该不会 panic（在非 WASM 环境下输出到 stderr）
        ctx.log("info", "Test message");
    }
}
