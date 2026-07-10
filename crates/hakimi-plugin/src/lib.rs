pub mod manager;
pub mod registry;
pub mod config;

// Legacy plugin loader stub for backward compatibility
pub mod loader;
pub use loader::PluginLoader;

use async_trait::async_trait;
use hakimi_common::error::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 插件特定错误类型
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Plugin load error: {0}")]
    LoadError(String),
    
    #[error("Plugin not found: {0}")]
    NotFound(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Initialization error: {0}")]
    InitError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

// 为方便使用，定义插件专用的 Result 类型
pub type PluginResult<T> = std::result::Result<T, PluginError>;

// 重新导出 config
pub use config::{PluginsConfig, PluginEntry};

/// 插件元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct PluginMetadata {
    /// 插件唯一标识（建议使用反向域名，如 com.example.my-plugin）
    pub id: String,
    
    /// 插件显示名称
    pub name: String,
    
    /// 插件版本（遵循 semver）
    pub version: String,
    
    /// 插件作者
    pub author: String,
    
    /// 插件描述
    pub description: String,
    
    /// 插件依赖（其他插件 ID）
    pub dependencies: Vec<String>,
    
    /// 最低 Hakimi 版本要求
    pub min_hakimi_version: Option<String>,
}

/// 插件上下文（传递给插件的环境信息）
#[derive(Debug, Clone)]
pub struct PluginContext {
    /// 当前会话 ID
    pub session_id: String,
    
    /// 用户 ID（如果可用）
    pub user_id: Option<String>,
    
    /// 插件配置（从 config.yaml 读取）
    pub config: serde_json::Value,
}

/// 简化的消息结构（避免直接依赖 hakimi-session）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

/// 简化的会话结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 插件生命周期钩子
#[async_trait]
pub trait HakimiPlugin: Send + Sync {
    /// 获取插件元数据
    fn metadata(&self) -> &PluginMetadata;
    
    /// 插件初始化（系统启动时调用一次）
    async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// 插件清理（系统关闭时调用）
    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
    
    // === 会话钩子 ===
    
    /// 会话开始时触发
    async fn on_session_start(&self, ctx: &PluginContext, session: &Session) -> Result<()> {
        let _ = (ctx, session);
        Ok(())
    }
    
    /// 会话结束时触发
    async fn on_session_end(&self, ctx: &PluginContext, session: &Session) -> Result<()> {
        let _ = (ctx, session);
        Ok(())
    }
    
    // === 消息钩子 ===
    
    /// 消息发送前触发（可修改消息或拒绝发送）
    async fn on_message_before_send(
        &self,
        ctx: &PluginContext,
        message: Message,
    ) -> Result<MessageAction> {
        let _ = ctx;
        Ok(MessageAction::Continue(message))
    }
    
    /// 消息发送后触发（只读，用于日志/分析）
    async fn on_message_after_send(
        &self,
        ctx: &PluginContext,
        message: &Message,
    ) -> Result<()> {
        let _ = (ctx, message);
        Ok(())
    }
    
    /// 消息接收时触发（可修改消息或过滤）
    async fn on_message_received(
        &self,
        ctx: &PluginContext,
        message: Message,
    ) -> Result<MessageAction> {
        let _ = ctx;
        Ok(MessageAction::Continue(message))
    }
    
    // === 工具调用钩子 ===
    
    /// 工具调用前触发（可修改参数或拒绝调用）
    async fn on_tool_call_before(
        &self,
        ctx: &PluginContext,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<ToolCallAction> {
        let _ = (ctx, tool_name);
        Ok(ToolCallAction::Continue(params))
    }
    
    /// 工具调用后触发（可修改结果）
    async fn on_tool_call_after(
        &self,
        ctx: &PluginContext,
        tool_name: &str,
        result: serde_json::Value,
    ) -> Result<ToolCallResultAction> {
        let _ = (ctx, tool_name);
        Ok(ToolCallResultAction::Continue(result))
    }
}

/// 消息处理动作
#[derive(Debug)]
pub enum MessageAction {
    /// 继续处理（可能已修改消息）
    Continue(Message),
    
    /// 拒绝消息（附带原因）
    Reject(String),
    
    /// 替换为自定义响应
    Replace(Message),
}

/// 工具调用动作
#[derive(Debug)]
pub enum ToolCallAction {
    /// 继续调用（可能已修改参数）
    Continue(serde_json::Value),
    
    /// 取消调用（附带原因）
    Cancel(String),
}

/// 工具调用结果动作
#[derive(Debug)]
pub enum ToolCallResultAction {
    /// 继续返回结果（可能已修改）
    Continue(serde_json::Value),
    
    /// 替换为自定义结果
    Replace(serde_json::Value),
    
    /// 标记为失败
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_metadata_serialization() {
        let metadata = PluginMetadata {
            id: "com.test.plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            author: "Test Author".to_string(),
            description: "A test plugin".to_string(),
            dependencies: vec![],
            min_hakimi_version: Some("0.5.0".to_string()),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: PluginMetadata = serde_json::from_str(&json).unwrap();
        
        assert_eq!(metadata.id, deserialized.id);
        assert_eq!(metadata.name, deserialized.name);
    }

    #[test]
    fn test_message_serialization() {
        let message = Message {
            id: "msg1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: 1234567890,
        };

        let json = serde_json::to_string(&message).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        
        assert_eq!(message.id, deserialized.id);
        assert_eq!(message.content, deserialized.content);
    }
}
