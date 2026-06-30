# QQ Bot SDK - 快速参考

## 安装
```toml
[dependencies]
qq-bot-sdk = { path = "." }
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

## 基础使用
```rust
use qq_bot_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = QQBotClient::new(app_id, app_secret);
    client.start(MyBot).await
}
```

## 发送消息

### 文本
```rust
api.reply_message(&msg, "Hello".to_string()).await?;
```

### 图片
```rust
api.send_image(&target, "image.jpg", Some(msg_id), client.media()).await?;
```

### Markdown
```rust
let md = MarkdownMessage::new("# Title\n**bold**");
api.send_markdown(&target, md, Some(msg_id)).await?;
```

### 按钮
```rust
let kb = Keyboard::new()
    .add_row(KeyboardRow::new()
        .add_button(Button::new("id", "Click", ActionType::Callback)));
api.send_with_keyboard(&target, "Choose:".into(), kb, None).await?;
```

### Embed
```rust
let embed = Embed::new()
    .title("Title")
    .add_field("Name", "Value");
api.send_embed(&target, embed, Some(msg_id)).await?;
```

## 限流
```rust
let api = client.api().clone().with_rate_limiting();
```

## 事件处理
```rust
#[async_trait::async_trait]
impl EventHandler for MyBot {
    async fn on_ready(&self, ready: &ReadyEvent) { }
    async fn on_at_message(&self, msg: &Message) { }
    async fn on_c2c_message(&self, msg: &Message) { }
    async fn on_group_at_message(&self, msg: &Message) { }
}
```

## Intents
```rust
Intents::default_messages()  // 频道 + 私信 + 群组
Intents::all_public()         // 所有公域事件
```

## 限流规则
- 频道：20 条/分钟
- 私信：5 条/分钟
- 群组：20 条/分钟

## 文档
- [完整指南](./USAGE.md)
- [架构设计](./DESIGN.md)
- [示例代码](./examples/advanced_bot.rs)
