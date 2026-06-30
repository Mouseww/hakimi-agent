# QQ Bot SDK - 使用指南

## 目录
- [快速开始](#快速开始)
- [富媒体消息](#富媒体消息)
- [Markdown 消息](#markdown-消息)
- [按钮交互](#按钮交互)
- [Embed 卡片](#embed-卡片)
- [限流和重试](#限流和重试)
- [错误处理](#错误处理)
- [最佳实践](#最佳实践)

## 快速开始

```rust
use qq_bot_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = QQBotClient::new(
        "your_app_id".to_string(),
        "your_app_secret".to_string(),
    );
    
    let bot = MyBot;
    client.start(bot).await?;
    Ok(())
}

struct MyBot;

#[async_trait::async_trait]
impl EventHandler for MyBot {
    async fn on_at_message(&self, msg: &Message) {
        println!("收到消息: {}", msg.content);
    }
}
```

## 富媒体消息

### 发送图片

```rust
// 自动选择上传策略（小文件直接上传，大文件分片上传）
let target = MessageTarget::Channel("channel_id".to_string());

api.send_image(
    &target,
    "path/to/image.jpg",
    Some(msg_id),
    client.media(),
).await?;
```

### 发送文件

```rust
api.send_file(
    &target,
    "path/to/document.pdf",
    Some(msg_id),
    client.media(),
).await?;
```

### 手动上传控制

```rust
// 小文件上传（< 10MB）
let file_info = client.media()
    .upload_image("small.jpg", MediaMessageType::Channel)
    .await?;

// 大文件自动分片上传（> 10MB）
let file_info = client.media()
    .upload_image("large.jpg", MediaMessageType::Channel)
    .await?;
```

### 接收和处理附件

```rust
async fn on_message(&self, msg: &Message) {
    for att in &msg.attachments {
        let parsed = ParsedAttachment::from_attachment(att);
        
        match parsed.media_type {
            AttachmentMediaType::Image => {
                // 下载图片
                let data = parsed.download().await?;
                // 或保存到文件
                parsed.download_to_file("downloaded.jpg").await?;
            }
            AttachmentMediaType::File => {
                println!("收到文件: {:?}", parsed.filename);
            }
            _ => {}
        }
    }
}
```

## Markdown 消息

### 基础 Markdown

```rust
let markdown = MarkdownMessage::new(r#"
# 标题

## 子标题

**粗体** *斜体* `代码`

- 列表项 1
- 列表项 2

[链接文本](https://example.com)
"#);

api.send_markdown(&target, markdown, Some(msg_id)).await?;
```

### 模板 Markdown

```rust
let markdown = MarkdownMessage::new("")
    .with_template("template_id_123")
    .add_param("title", vec!["动态标题".to_string()])
    .add_param("content", vec!["动态内容".to_string()]);

api.send_markdown(&target, markdown, Some(msg_id)).await?;
```

## 按钮交互

### 创建按钮

```rust
let keyboard = Keyboard::new()
    .add_row(
        KeyboardRow::new()
            .add_button(
                Button::new("btn_1", "确认", ActionType::Callback)
                    .with_style(ButtonStyle::Blue)
                    .with_data("confirm:yes")
            )
            .add_button(
                Button::new("btn_2", "取消", ActionType::Callback)
                    .with_data("confirm:no")
            )
    )
    .add_row(
        KeyboardRow::new()
            .add_button(
                Button::new("btn_link", "查看文档", ActionType::Link)
                    .with_link("https://bot.q.qq.com/wiki/")
            )
    );

api.send_with_keyboard(&target, "请选择操作：".to_string(), keyboard, None).await?;
```

### 按钮类型

1. **回调按钮** (`ActionType::Callback`)
   - 点击后触发 `InteractionEvent`
   - 可携带自定义数据

2. **链接按钮** (`ActionType::Link`)
   - 点击后打开 URL

3. **@机器人按钮** (`ActionType::AtBot`)
   - 点击后在输入框填充 @机器人

### 处理按钮回调

```rust
// TODO: 在 Gateway 中添加 Interaction 事件支持
async fn on_interaction(&self, event: &InteractionEvent) {
    // 解析按钮数据
    let data = &event.data;
    
    // 响应交互
    // ...
}
```

## Embed 卡片

```rust
let embed = Embed::new()
    .title("📊 服务器状态")
    .description("实时监控数据")
    .prompt("有更新")
    .thumbnail("https://example.com/icon.png")
    .add_field("CPU", "45%")
    .add_field("内存", "2.1GB / 8GB")
    .add_field("在线用户", "1,234");

api.send_embed(&target, embed, Some(msg_id)).await?;
```

## ARK 消息

ARK 是 QQ 的特殊卡片消息格式，适用于特定模板：

```rust
let ark = ArkMessage::new(23) // 模板 ID
    .add_kv("title", "标题")
    .add_kv("desc", "描述")
    .add_kv("prompt", "提示");

api.send_ark(&target, ark, Some(msg_id)).await?;
```

## 限流和重试

### 启用自动限流

```rust
let api = client.api().clone().with_rate_limiting();
```

### 自定义限流策略

```rust
use std::time::Duration;

// 频道消息：每分钟 20 条
let channel_limiter = RateLimiter::new(20, Duration::from_secs(60));
let channel_throttler = ThrottledClient::new(
    channel_limiter,
    RetryPolicy::default()
);

let api = client.api().clone()
    .with_custom_throttler(channel_throttler);
```

### 配置重试策略

```rust
let retry_policy = RetryPolicy {
    max_retries: 5,                         // 最多重试 5 次
    initial_delay: Duration::from_millis(500), // 初始延迟 500ms
    max_delay: Duration::from_secs(30),     // 最大延迟 30s
    multiplier: 2.0,                        // 指数退避系数
    jitter: true,                           // 添加随机抖动
};
```

### QQ 官方限流规则

| 消息类型 | 限制 |
|---------|------|
| 频道消息 | 20 条/分钟 |
| 私信消息 | 5 条/分钟 |
| 群组消息 | 20 条/分钟 |

SDK 已内置这些规则，使用 `with_rate_limiting()` 即可。

## 错误处理

### 错误类型

```rust
pub enum Error {
    Http(reqwest::Error),     // HTTP 请求错误
    Io(std::io::Error),        // IO 错误
    Auth(String),              // 认证错误
    Ws(String),                // WebSocket 错误
    Json(serde_json::Error),   // JSON 解析错误
    Other(String),             // 其他错误
}
```

### 处理模式

```rust
match api.reply_message(msg, content).await {
    Ok(resp) => {
        println!("消息已发送: {}", resp.id);
    }
    Err(Error::Http(e)) if e.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => {
        println!("触发限流，请稍后重试");
    }
    Err(Error::Auth(msg)) => {
        println!("认证失败: {}", msg);
    }
    Err(e) => {
        println!("发送失败: {}", e);
    }
}
```

## 最佳实践

### 1. 使用限流避免封禁

```rust
// ✅ 推荐：启用自动限流
let api = client.api().clone().with_rate_limiting();

// ❌ 不推荐：频繁发送无限流
for _ in 0..100 {
    api.send_message(...).await?; // 可能触发限流
}
```

### 2. 合理处理附件

```rust
// ✅ 检查文件大小
let metadata = tokio::fs::metadata(&path).await?;
if metadata.len() > 20 * 1024 * 1024 {
    return Err("文件过大".into());
}

// ✅ 下载附件时设置超时
let data = tokio::time::timeout(
    Duration::from_secs(30),
    parsed.download()
).await??;
```

### 3. 优雅处理消息

```rust
async fn on_at_message(&self, msg: &Message) {
    // ✅ 异步处理，不阻塞事件循环
    tokio::spawn(async move {
        if let Err(e) = handle_message(msg).await {
            tracing::error!("处理消息失败: {}", e);
        }
    });
}
```

### 4. 使用结构化日志

```rust
use tracing::{info, warn, error};

info!(msg_id = %msg.id, user = %msg.author.id, "收到消息");
warn!(attempt = retry_count, "重试发送消息");
error!(err = %e, "发送失败");
```

### 5. 环境配置

```bash
# .env
QQ_APP_ID=your_app_id
QQ_APP_SECRET=your_app_secret
QQ_SANDBOX=false
RUST_LOG=info
```

```rust
use dotenv::dotenv;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt().init();
    
    let app_id = std::env::var("QQ_APP_ID")?;
    // ...
}
```

### 6. 优雅关闭

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    
    // 监听 Ctrl+C
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tx.send(()).ok();
    });
    
    // 运行 Bot
    tokio::select! {
        result = client.start(bot) => result?,
        _ = rx => {
            println!("收到关闭信号");
        }
    }
    
    Ok(())
}
```

## 完整示例

参考 `examples/advanced_bot.rs` 查看包含所有功能的完整示例。

## 常见问题

### Q: 如何判断是否触发限流？

A: 检查 HTTP 状态码 `429 Too Many Requests`。SDK 的重试机制会自动处理。

### Q: 大文件上传失败怎么办？

A: SDK 自动处理分片上传（>10MB）。如果仍然失败，检查文件是否超过 QQ 限制（通常 20MB）。

### Q: 如何测试 Bot？

A: 使用沙盒环境：

```rust
let client = QQBotClient::new(app_id, app_secret)
    .with_sandbox();
```

### Q: 按钮回调如何接收？

A: 需要在 Gateway 中添加 `INTERACTION` Intent 并实现 `on_interaction` 方法。

## 更多资源

- [QQ 机器人官方文档](https://bot.q.qq.com/wiki/)
- [API 参考](https://bot.q.qq.com/wiki/develop/api/)
- [SDK GitHub](https://github.com/your-repo/qq-bot-sdk)
