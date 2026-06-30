# QQ Bot SDK (Rust)

功能完整的 QQ 官方机器人 Rust SDK，支持 WebSocket 长连接、富媒体消息、Markdown、按钮交互等高级功能。

## ✨ 功能特性

### 核心功能
- ✅ WebSocket 长连接（心跳、重连、断线恢复）
- ✅ OAuth2 认证和 Token 自动刷新
- ✅ 多种消息类型（频道、私信、群组、C2C）
- ✅ 事件驱动架构
- ✅ 沙盒环境支持

### 富媒体支持 (P0)
- ✅ 图片上传和发送（自动选择上传策略）
- ✅ 文件上传和发送（支持大文件分片上传 >10MB）
- ✅ 语音和视频上传
- ✅ 接收和解析富媒体附件
- ✅ 附件下载（内存或文件）

### 高级消息类型 (P1)
- ✅ Markdown 消息（普通和模板）
- ✅ Embed 卡片消息
- ✅ ARK 特殊卡片
- ✅ 按钮交互（Keyboard）
- ✅ 多种按钮类型（回调、链接、@机器人）

### 工程优化 (P2)
- ✅ 限流处理（滑动窗口算法，符合 QQ 官方限制）
- ✅ 自动重试（指数退避 + 随机抖动）
- ✅ 单元测试覆盖
- ✅ 完整文档和示例

## 📦 安装

```toml
[dependencies]
qq-bot-sdk = { path = "path/to/qq-bot-sdk" }
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = "0.3"
async-trait = "0.1"
```

## 🚀 快速开始

```rust
use qq_bot_sdk::prelude::*;

struct MyBot;

#[async_trait::async_trait]
impl EventHandler for MyBot {
    async fn on_ready(&self, ready: &ReadyEvent) {
        println!("Bot ready: {}", ready.user.username);
    }

    async fn on_at_message(&self, msg: &Message) {
        println!("收到 @ 消息: {}", msg.content);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let client = QQBotClient::new(
        "your_app_id".to_string(),
        "your_app_secret".to_string(),
    );

    client.start(MyBot).await?;
    Ok(())
}
```

### 运行示例

```bash
# 设置环境变量
export QQ_APP_ID="your_app_id"
export QQ_APP_SECRET="your_app_secret"
export QQ_SANDBOX="true"  # 可选：使用沙盒环境

# 运行简单示例
cargo run --example simple_bot

# 运行完整功能示例
cargo run --example advanced_bot
```

## 📚 功能示例

### 发送图片

```rust
let target = MessageTarget::Channel("channel_id".to_string());
api.send_image(&target, "path/to/image.jpg", Some(msg_id), client.media()).await?;
```

### Markdown 消息

```rust
let markdown = MarkdownMessage::new(r#"
# 标题
**粗体** *斜体*
[链接](https://bot.q.qq.com)
"#);
api.send_markdown(&target, markdown, Some(msg_id)).await?;
```

### 按钮交互

```rust
let keyboard = Keyboard::new()
    .add_row(
        KeyboardRow::new()
            .add_button(Button::new("btn_1", "确认", ActionType::Callback))
            .add_button(Button::new("btn_2", "取消", ActionType::Callback))
    );
api.send_with_keyboard(&target, "请选择：".to_string(), keyboard, None).await?;
```

### Embed 卡片

```rust
let embed = Embed::new()
    .title("📊 状态报告")
    .description("系统运行正常")
    .add_field("CPU", "45%")
    .add_field("内存", "2.1GB / 8GB");
api.send_embed(&target, embed, Some(msg_id)).await?;
```

### 启用限流

```rust
let api = client.api().clone().with_rate_limiting();
// 自动符合 QQ 官方限流规则：频道 20条/分钟，私信 5条/分钟
```

## 📁 项目结构

```
qq-bot-sdk/
├── src/
│   ├── lib.rs           # 库入口
│   ├── auth.rs          # OAuth2 认证和 Token 管理
│   ├── gateway.rs       # WebSocket Gateway
│   ├── client.rs        # REST API 客户端
│   ├── media.rs         # 富媒体上传和处理
│   ├── message.rs       # 高级消息类型
│   ├── throttle.rs      # 限流和重试
│   ├── model.rs         # 数据模型
│   └── error.rs         # 错误类型
├── examples/
│   ├── simple_bot.rs    # 基础示例
│   └── advanced_bot.rs  # 完整功能示例
├── tests/
│   └── integration_test.rs
├── DESIGN.md            # 架构设计文档
├── USAGE.md             # 详细使用指南
└── CHANGELOG.md         # 变更日志
```

## 🔧 核心组件

### TokenManager
- OAuth2 认证流程
- Access Token 自动刷新
- 线程安全的 Token 缓存

### Gateway
- WebSocket 长连接管理
- 心跳保活和会话恢复
- 断线自动重连
- 事件分发

### MediaClient
- 图片/文件/语音/视频上传
- 小文件直接上传
- 大文件自动分片上传（>10MB）
- 附件下载和解析

### ThrottledClient
- 滑动窗口限流算法
- 指数退避重试
- 符合 QQ 官方限流规则

## 📖 文档

- [完整使用指南](./USAGE.md) - 包含所有功能的详细说明和示例
- [架构设计文档](./DESIGN.md) - 技术实现细节
- [变更日志](./CHANGELOG.md) - 版本更新记录

## 🧪 测试

```bash
# 运行单元测试
cargo test

# 运行集成测试
cargo test --test integration_test

# 查看测试覆盖率
cargo tarpaulin
```

## ⚙️ Intents 配置

```rust
// 默认消息事件（频道、私信、群组）
let intents = Intents::default_messages();

// 自定义
let mut intents = Intents::new();
intents.add(Intents::GUILD_MESSAGES);
intents.add(Intents::GROUP_AND_C2C_EVENT);
intents.add(Intents::INTERACTION);

// 所有公域事件
let intents = Intents::all_public();

let client = QQBotClient::new(app_id, app_secret)
    .with_intents(intents);
```

## ❗ 错误处理

```rust
match api.send_message(...).await {
    Ok(resp) => println!("消息已发送: {}", resp.id),
    Err(Error::Http(e)) if e.status() == Some(StatusCode::TOO_MANY_REQUESTS) => {
        println!("触发限流");
    }
    Err(Error::Auth(msg)) => println!("认证失败: {}", msg),
    Err(e) => println!("发送失败: {}", e),
}
```

## 🔐 限流规则

| 消息类型 | QQ 官方限制 | SDK 处理 |
|---------|------------|---------|
| 频道消息 | 20 条/分钟 | ✅ 自动限流 |
| 私信消息 | 5 条/分钟  | ✅ 自动限流 |
| 群组消息 | 20 条/分钟 | ✅ 自动限流 |

使用 `.with_rate_limiting()` 自动启用。

## 🛠️ 依赖库

- `tokio` - 异步运行时
- `tokio-tungstenite` - WebSocket 客户端
- `reqwest` - HTTP 客户端（支持 multipart）
- `serde` / `serde_json` - 序列化
- `tracing` - 结构化日志
- `parking_lot` - 高性能锁
- `async-trait` - 异步 trait
- `anyhow` / `thiserror` - 错误处理

## 🎯 最佳实践

1. **启用限流** - 避免触发 QQ 官方限制
   ```rust
   let api = client.api().clone().with_rate_limiting();
   ```

2. **异步处理消息** - 不阻塞事件循环
   ```rust
   tokio::spawn(async move {
       handle_message(msg).await;
   });
   ```

3. **结构化日志** - 便于调试
   ```rust
   tracing::info!(msg_id = %msg.id, "收到消息");
   ```

4. **错误恢复** - Gateway 自动重连，无需手动处理

## 📝 License

MIT

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

## 📮 联系方式

- QQ 机器人官方文档: https://bot.q.qq.com/wiki/
- SDK Issues: [GitHub Issues](https://github.com/your-repo/qq-bot-sdk/issues)
