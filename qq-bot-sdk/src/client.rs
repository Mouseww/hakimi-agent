use crate::auth::TokenManager;
use crate::error::{Error, Result};
use crate::gateway::{EventHandler, EventReceiver, Gateway, GatewayEvent};
use crate::media::{MediaClient, MediaMessageType};
use crate::message::*;
use crate::model::*;
use crate::throttle::ThrottledClient;
use std::path::Path;
use std::sync::Arc;
use tracing::error;

pub struct QQBotClient {
    token_manager: Arc<TokenManager>,
    api_client: ApiClient,
    media_client: MediaClient,
    #[allow(dead_code)]
    gateway: Option<Gateway>,
    event_rx: Option<EventReceiver>,
    intents: Intents,
}

impl QQBotClient {
    pub fn new(app_id: String, app_secret: String) -> Self {
        let token_manager = Arc::new(TokenManager::new(app_id, app_secret));
        let api_client = ApiClient::new(token_manager.clone());
        let media_client = MediaClient::new(token_manager.clone());

        Self {
            token_manager,
            api_client,
            media_client,
            gateway: None,
            event_rx: None,
            intents: Intents::default_messages(),
        }
    }

    pub fn with_sandbox(mut self) -> Self {
        self.token_manager = Arc::new(
            TokenManager::new(
                self.token_manager.credentials.app_id.clone(),
                self.token_manager.credentials.app_secret.clone(),
            )
            .with_sandbox(),
        );
        self.api_client = ApiClient::new(self.token_manager.clone()).with_sandbox();
        self.media_client = MediaClient::new(self.token_manager.clone()).with_sandbox();
        self
    }

    pub fn with_intents(mut self, intents: Intents) -> Self {
        self.intents = intents;
        self
    }

    pub async fn start<H: EventHandler + 'static>(mut self, handler: H) -> Result<()> {
        let (gateway, event_rx) = Gateway::new(self.token_manager.clone(), self.intents);
        self.event_rx = Some(event_rx);

        let gateway_handle = tokio::spawn({
            let gateway = gateway.clone();
            async move {
                if let Err(e) = gateway.connect().await {
                    error!("Gateway error: {}", e);
                }
            }
        });

        self.run_event_loop(handler).await?;
        gateway_handle.abort();

        Ok(())
    }

    async fn run_event_loop<H: EventHandler>(&mut self, handler: H) -> Result<()> {
        let mut event_rx = self
            .event_rx
            .take()
            .ok_or(Error::Other("Event receiver not initialized".to_string()))?;

        while let Some(event) = event_rx.recv().await {
            match event {
                GatewayEvent::Ready(ready) => {
                    handler.on_ready(&ready).await;
                }
                GatewayEvent::MessageCreate(msg) => {
                    handler.on_message(&msg).await;
                }
                GatewayEvent::AtMessageCreate(msg) => {
                    handler.on_at_message(&msg).await;
                }
                GatewayEvent::DirectMessageCreate(msg) => {
                    handler.on_direct_message(&msg).await;
                }
                GatewayEvent::C2CMessageCreate(msg) => {
                    handler.on_c2c_message(&msg).await;
                }
                GatewayEvent::GroupAtMessageCreate(msg) => {
                    handler.on_group_at_message(&msg).await;
                }
                GatewayEvent::Reconnect => {
                    // Gateway 自动处理重连
                }
                GatewayEvent::Disconnected => {
                    // Gateway 自动处理重连
                }
            }
        }

        Ok(())
    }

    pub fn api(&self) -> &ApiClient {
        &self.api_client
    }

    pub fn media(&self) -> &MediaClient {
        &self.media_client
    }
}

#[derive(Clone)]
pub struct ApiClient {
    token_manager: Arc<TokenManager>,
    client: reqwest::Client,
    base_url: String,
    throttler: Option<ThrottledClient>,
}

impl ApiClient {
    pub fn new(token_manager: Arc<TokenManager>) -> Self {
        Self {
            token_manager,
            client: reqwest::Client::new(),
            base_url: "https://api.sgroup.qq.com".to_string(),
            throttler: None,
        }
    }

    pub fn with_sandbox(mut self) -> Self {
        self.base_url = "https://sandbox.api.sgroup.qq.com".to_string();
        self
    }

    /// 启用限流（推荐）
    pub fn with_rate_limiting(mut self) -> Self {
        self.throttler = Some(ThrottledClient::for_channel());
        self
    }

    /// 使用自定义限流器
    pub fn with_custom_throttler(mut self, throttler: ThrottledClient) -> Self {
        self.throttler = Some(throttler);
        self
    }

    async fn get_auth_header(&self) -> Result<String> {
        let token = self.token_manager.get_token().await?;
        Ok(format!("QQBot {}", token))
    }

    /// 发送频道消息
    pub async fn send_channel_message(
        &self,
        channel_id: &str,
        req: SendMessageRequest,
    ) -> Result<SendMessageResponse> {
        let url = format!("{}/channels/{}/messages", self.base_url, channel_id);
        self.post_json(&url, &req).await
    }

    /// 发送私信
    pub async fn send_dm(
        &self,
        guild_id: &str,
        req: SendMessageRequest,
    ) -> Result<SendMessageResponse> {
        let url = format!("{}/dms/{}/messages", self.base_url, guild_id);
        self.post_json(&url, &req).await
    }

    /// 发送 C2C 消息（用户私聊）
    pub async fn send_c2c_message(
        &self,
        openid: &str,
        req: SendMessageRequest,
    ) -> Result<SendMessageResponse> {
        let url = format!("{}/v2/users/{}/messages", self.base_url, openid);
        self.post_json(&url, &req).await
    }

    /// 发送群组消息
    pub async fn send_group_message(
        &self,
        group_openid: &str,
        req: SendMessageRequest,
    ) -> Result<SendMessageResponse> {
        let url = format!("{}/v2/groups/{}/messages", self.base_url, group_openid);
        self.post_json(&url, &req).await
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &impl serde::Serialize,
    ) -> Result<T> {
        let auth = self.get_auth_header().await?;

        let resp = self
            .client
            .post(url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            return Err(Error::Api(format!(
                "HTTP {}: {}",
                status.as_u16(),
                error_text
            )));
        }

        let result = resp.json().await?;
        Ok(result)
    }
}

// ===== 便捷方法 =====

impl ApiClient {
    /// 回复消息（自动选择正确的 API）
    pub async fn reply_message(
        &self,
        msg: &Message,
        content: String,
    ) -> Result<SendMessageResponse> {
        let req = SendMessageRequest {
            content,
            msg_id: Some(msg.id.clone()),
            event_id: None,
            msg_type: Some(0),
            markdown: None,
            keyboard: None,
            media: None,
            ark: None,
            embed: None,
            image: None,
        };

        if let Some(channel_id) = &msg.channel_id {
            self.send_channel_message(channel_id, req).await
        } else if let Some(group_openid) = &msg.group_openid {
            self.send_group_message(group_openid, req).await
        } else if let Some(author) = &msg.author {
            self.send_c2c_message(&author.id, req).await
        } else {
            Err(Error::Other(
                "Cannot determine message type for reply".to_string(),
            ))
        }
    }

    /// 发送 Markdown 消息
    pub async fn send_markdown(
        &self,
        target: &MessageTarget,
        markdown: MarkdownMessage,
        msg_id: Option<String>,
    ) -> Result<SendMessageResponse> {
        let req = SendMessageRequest {
            content: " ".to_string(), // Markdown 消息需要非空 content
            msg_id,
            event_id: None,
            msg_type: Some(0),
            markdown: Some(markdown.to_value()),
            keyboard: None,
            media: None,
            ark: None,
            embed: None,
            image: None,
        };

        self.send_to_target(target, req).await
    }

    /// 发送带按钮的消息
    pub async fn send_with_keyboard(
        &self,
        target: &MessageTarget,
        content: String,
        keyboard: Keyboard,
        msg_id: Option<String>,
    ) -> Result<SendMessageResponse> {
        let req = SendMessageRequest {
            content,
            msg_id,
            event_id: None,
            msg_type: Some(0),
            markdown: None,
            keyboard: Some(serde_json::to_value(keyboard)?),
            media: None,
            ark: None,
            embed: None,
            image: None,
        };

        self.send_to_target(target, req).await
    }

    /// 发送图片消息
    pub async fn send_image<P: AsRef<Path>>(
        &self,
        target: &MessageTarget,
        image_path: P,
        msg_id: Option<String>,
        media_client: &MediaClient,
    ) -> Result<SendMessageResponse> {
        // 上传图片
        let file_info = media_client
            .upload_image(image_path, target.to_media_type())
            .await?;

        let req = SendMessageRequest {
            content: " ".to_string(),
            msg_id,
            event_id: None,
            msg_type: Some(7), // 7 = 富媒体消息
            markdown: None,
            keyboard: None,
            media: Some(MessageMedia {
                file_info: file_info.file_info,
            }),
            ark: None,
            embed: None,
            image: None,
        };

        self.send_to_target(target, req).await
    }

    /// 发送文件消息
    pub async fn send_file<P: AsRef<Path>>(
        &self,
        target: &MessageTarget,
        file_path: P,
        msg_id: Option<String>,
        media_client: &MediaClient,
    ) -> Result<SendMessageResponse> {
        let file_info = media_client
            .upload_file(file_path, target.to_media_type())
            .await?;

        let req = SendMessageRequest {
            content: " ".to_string(),
            msg_id,
            event_id: None,
            msg_type: Some(7),
            markdown: None,
            keyboard: None,
            media: Some(MessageMedia {
                file_info: file_info.file_info,
            }),
            ark: None,
            embed: None,
            image: None,
        };

        self.send_to_target(target, req).await
    }

    /// 发送 Ark 消息
    pub async fn send_ark(
        &self,
        target: &MessageTarget,
        ark: ArkMessage,
        msg_id: Option<String>,
    ) -> Result<SendMessageResponse> {
        let req = SendMessageRequest {
            content: " ".to_string(),
            msg_id,
            event_id: None,
            msg_type: Some(0),
            markdown: None,
            keyboard: None,
            media: None,
            ark: Some(ark),
            embed: None,
            image: None,
        };

        self.send_to_target(target, req).await
    }

    /// 发送 Embed 消息
    pub async fn send_embed(
        &self,
        target: &MessageTarget,
        embed: Embed,
        msg_id: Option<String>,
    ) -> Result<SendMessageResponse> {
        let req = SendMessageRequest {
            content: " ".to_string(),
            msg_id,
            event_id: None,
            msg_type: Some(0),
            markdown: None,
            keyboard: None,
            media: None,
            ark: None,
            embed: Some(embed),
            image: None,
        };

        self.send_to_target(target, req).await
    }

    async fn send_to_target(
        &self,
        target: &MessageTarget,
        req: SendMessageRequest,
    ) -> Result<SendMessageResponse> {
        match target {
            MessageTarget::Channel(channel_id) => self.send_channel_message(channel_id, req).await,
            MessageTarget::C2C(openid) => self.send_c2c_message(openid, req).await,
            MessageTarget::Group(group_openid) => self.send_group_message(group_openid, req).await,
        }
    }
}

/// 消息目标
#[derive(Debug, Clone)]
pub enum MessageTarget {
    Channel(String),
    C2C(String),
    Group(String),
}

impl MessageTarget {
    pub fn from_message(msg: &Message) -> Option<Self> {
        if let Some(channel_id) = &msg.channel_id {
            Some(Self::Channel(channel_id.clone()))
        } else if let Some(group_openid) = &msg.group_openid {
            Some(Self::Group(group_openid.clone()))
        } else {
            msg.author
                .as_ref()
                .map(|author| Self::C2C(author.id.clone()))
        }
    }

    fn to_media_type(&self) -> MediaMessageType {
        match self {
            Self::Channel(_) => MediaMessageType::Channel,
            Self::C2C(_) => MediaMessageType::C2C,
            Self::Group(_) => MediaMessageType::Group,
        }
    }
}
