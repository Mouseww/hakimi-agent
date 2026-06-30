use qq_bot_sdk::prelude::*;
use tracing::info;
use tracing_subscriber;

struct MyBot {
    api: ApiClient,
}

#[async_trait::async_trait]
impl EventHandler for MyBot {
    async fn on_ready(&self, ready: &ReadyEvent) {
        info!(
            "✅ Bot {} is ready! Session: {}",
            ready.user.username, ready.session_id
        );
    }

    async fn on_at_message(&self, msg: &Message) {
        info!("📨 Received @ message: {}", msg.content);

        if msg.content.contains("你好") || msg.content.contains("hello") {
            if let Err(e) = self
                .api
                .reply_message(msg, "你好！我是 Rust QQ Bot！".to_string())
                .await
            {
                eprintln!("Failed to send reply: {}", e);
            }
        } else if msg.content.contains("ping") {
            if let Err(e) = self.api.reply_message(msg, "pong! 🏓".to_string()).await {
                eprintln!("Failed to send reply: {}", e);
            }
        }
    }

    async fn on_c2c_message(&self, msg: &Message) {
        info!("💬 Received C2C message: {}", msg.content);

        if let Err(e) = self
            .api
            .reply_message(msg, format!("Echo: {}", msg.content))
            .await
        {
            eprintln!("Failed to send reply: {}", e);
        }
    }

    async fn on_group_at_message(&self, msg: &Message) {
        info!("👥 Received group @ message: {}", msg.content);

        if msg.content.contains("help") {
            let help_text = r#"
🤖 QQ Bot 帮助
-----------------
• @我 + 你好/hello - 打招呼
• @我 + ping - 测试响应
• 私聊我任何消息 - 自动回显
            "#;
            if let Err(e) = self.api.reply_message(msg, help_text.to_string()).await {
                eprintln!("Failed to send reply: {}", e);
            }
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
    let app_id = std::env::var("QQ_APP_ID").expect("Missing QQ_APP_ID environment variable");
    let app_secret =
        std::env::var("QQ_APP_SECRET").expect("Missing QQ_APP_SECRET environment variable");
    let use_sandbox = std::env::var("QQ_SANDBOX").unwrap_or_default() == "true";

    info!("🚀 Starting QQ Bot (sandbox: {})", use_sandbox);

    // 创建 Bot 客户端
    let mut client = QQBotClient::new(app_id, app_secret).with_intents(
        Intents::default_messages(), // 接收消息事件
    );

    if use_sandbox {
        client = client.with_sandbox();
    }

    // 创建事件处理器
    let handler = MyBot {
        api: client.api().clone(),
    };

    // 启动 Bot
    info!("🔌 Connecting to gateway...");
    client.start(handler).await?;

    Ok(())
}
