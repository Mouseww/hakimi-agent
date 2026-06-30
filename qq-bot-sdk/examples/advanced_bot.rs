use qq_bot_sdk::prelude::*;
use std::env;
use tracing_subscriber;

struct MyBot {
    api: ApiClient,
    media: MediaClient,
}

#[async_trait::async_trait]
impl EventHandler for MyBot {
    async fn on_ready(&self, ready: &ReadyEvent) {
        tracing::info!("Bot ready: {} ({})", ready.user.username, ready.user.id);
    }

    async fn on_at_message(&self, msg: &Message) {
        tracing::info!("Received @message: {}", msg.content);

        // 处理不同的命令
        if msg.content.contains("图片") {
            self.handle_image_command(msg).await;
        } else if msg.content.contains("markdown") {
            self.handle_markdown_command(msg).await;
        } else if msg.content.contains("按钮") {
            self.handle_button_command(msg).await;
        } else if msg.content.contains("卡片") {
            self.handle_embed_command(msg).await;
        } else {
            // 简单回复
            if let Err(e) = self
                .api
                .reply_message(msg, "你好！我收到了你的消息。".to_string())
                .await
            {
                tracing::error!("Failed to reply: {}", e);
            }
        }
    }

    async fn on_c2c_message(&self, msg: &Message) {
        tracing::info!("Received C2C message: {}", msg.content);

        if let Err(e) = self
            .api
            .reply_message(msg, "你好！这是私聊回复。".to_string())
            .await
        {
            tracing::error!("Failed to reply C2C: {}", e);
        }
    }

    async fn on_group_at_message(&self, msg: &Message) {
        tracing::info!("Received group @message: {}", msg.content);

        if let Err(e) = self
            .api
            .reply_message(msg, "收到群消息！".to_string())
            .await
        {
            tracing::error!("Failed to reply group: {}", e);
        }
    }
}

impl MyBot {
    async fn handle_image_command(&self, msg: &Message) {
        // 示例：发送图片（需要有实际的图片文件）
        let target = match MessageTarget::from_message(msg) {
            Some(t) => t,
            None => return,
        };

        // 假设有一个图片文件
        if let Err(e) = self
            .api
            .send_image(&target, "example.jpg", Some(msg.id.clone()), &self.media)
            .await
        {
            tracing::error!("Failed to send image: {}", e);
        }
    }

    async fn handle_markdown_command(&self, msg: &Message) {
        let target = match MessageTarget::from_message(msg) {
            Some(t) => t,
            None => return,
        };

        let markdown = MarkdownMessage::new(
            r#"
# Markdown 消息示例

## 功能列表
- **粗体文本**
- *斜体文本*
- `代码块`

[点击访问](https://bot.q.qq.com)
"#,
        );

        if let Err(e) = self
            .api
            .send_markdown(&target, markdown, Some(msg.id.clone()))
            .await
        {
            tracing::error!("Failed to send markdown: {}", e);
        }
    }

    async fn handle_button_command(&self, msg: &Message) {
        let target = match MessageTarget::from_message(msg) {
            Some(t) => t,
            None => return,
        };

        let keyboard = Keyboard::new()
            .add_row(
                KeyboardRow::new()
                    .add_button(
                        Button::new("btn_1", "选项 A", ActionType::Callback)
                            .with_style(ButtonStyle::Blue)
                            .with_data("action:select:a"),
                    )
                    .add_button(
                        Button::new("btn_2", "选项 B", ActionType::Callback)
                            .with_data("action:select:b"),
                    ),
            )
            .add_row(
                KeyboardRow::new().add_button(
                    Button::new("btn_link", "访问文档", ActionType::Link)
                        .with_link("https://bot.q.qq.com/wiki/"),
                ),
            );

        if let Err(e) = self
            .api
            .send_with_keyboard(
                &target,
                "请选择一个选项：".to_string(),
                keyboard,
                Some(msg.id.clone()),
            )
            .await
        {
            tracing::error!("Failed to send keyboard: {}", e);
        }
    }

    async fn handle_embed_command(&self, msg: &Message) {
        let target = match MessageTarget::from_message(msg) {
            Some(t) => t,
            None => return,
        };

        let embed = Embed::new()
            .title("📊 状态报告")
            .description("这是一个 Embed 卡片消息示例")
            .prompt("有新消息")
            .add_field("服务器状态", "✅ 正常")
            .add_field("在线用户", "1,234 人")
            .add_field("消息数", "56,789 条");

        if let Err(e) = self
            .api
            .send_embed(&target, embed, Some(msg.id.clone()))
            .await
        {
            tracing::error!("Failed to send embed: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // 从环境变量读取配置
    let app_id = env::var("QQ_APP_ID").expect("Missing QQ_APP_ID");
    let app_secret = env::var("QQ_APP_SECRET").expect("Missing QQ_APP_SECRET");
    let use_sandbox = env::var("QQ_SANDBOX").unwrap_or_default() == "true";

    // 创建客户端
    let mut client = QQBotClient::new(app_id, app_secret);

    if use_sandbox {
        client = client.with_sandbox();
    }

    // 设置 intents
    let intents = Intents::default_messages();
    client = client.with_intents(intents);

    // 创建处理器
    let bot = MyBot {
        api: client.api().clone(),
        media: client.media().clone(),
    };

    tracing::info!("Starting QQ Bot...");

    // 启动机器人
    client.start(bot).await?;

    Ok(())
}
