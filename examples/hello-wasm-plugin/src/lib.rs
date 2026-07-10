//! Hello WASM Plugin - 最小示例
//!
//! 这是一个使用 Hakimi Plugin SDK 开发的最简单 WASM 插件。

use hakimi_plugin_sdk::*;

#[hakimi_plugin(
    name = "hello-wasm",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "A simple hello world WASM plugin"
)]
pub struct HelloPlugin;

impl HelloPlugin {
    /// 插件执行函数
    ///
    /// 这是插件的主要逻辑入口，会被宿主调用。
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        // 记录日志到宿主
        ctx.log("info", "Hello WASM plugin is executing!");
        ctx.log("debug", "This is a debug message from the plugin");

        // 返回问候语
        Ok("Hello from WASM! 🎉\n\nThis plugin was loaded and executed successfully!".to_string())
    }
}
