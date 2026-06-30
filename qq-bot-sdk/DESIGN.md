# QQ Bot SDK 技术设计文档

## 1. 架构概览

### 1.1 Crate 结构
```
qq-bot-sdk/
├── src/
│   ├── lib.rs        - 库入口，导出公共 API
│   ├── error.rs      - 统一错误类型定义
│   ├── model.rs      - 协议数据结构（Gateway、REST API）
│   ├── auth.rs       - OAuth2 认证和 Token 管理
│   ├── gateway.rs    - WebSocket Gateway 连接
│   └── client.rs     - REST API 客户端和主客户端
└── examples/
    └── simple_bot.rs - 示例 Bot 实现
```

### 1.2 核心组件关系
```
QQBotClient
    ├── TokenManager (Arc) ─────┐
    ├── ApiClient               │
    │   └── TokenManager (Arc) ─┘ (共享)
    └── Gateway
        └── TokenManager (Arc) ─┘ (共享)
```

## 2. 核心模块设计

### 2.1 TokenManager (auth.rs)

**职责：**
- OAuth2 认证流程
- Access Token 获取和自动刷新
- 线程安全的 Token 缓存

**关键实现：**
```rust
pub struct TokenManager {
    credentials: Credentials,
    token: Arc<RwLock<Option<AccessToken>>>,  // 线程安全缓存
    client: reqwest::Client,
    api_base: String,
}
```

**特性：**
- 提前 60 秒刷新过期 Token（避免边界条件）
- 使用 `parking_lot::RwLock` 提高并发性能
- 支持沙箱环境切换

**API：**
- `get_token()` - 获取有效 Token，自动刷新
- `refresh_token()` - 强制刷新 Token

### 2.2 Gateway (gateway.rs)

**职责：**
- WebSocket 长连接管理
- 协议层处理（OpCode）
- 自动心跳和会话恢复
- 断线重连机制
- 事件分发

**关键实现：**
```rust
pub struct Gateway {
    token_manager: Arc<TokenManager>,
    intents: Intents,
    session_state: Arc<RwLock<SessionState>>,  // 会话状态
    event_tx: EventSender,
}

struct SessionState {
    session_id: Option<String>,  // 用于 Resume
    seq: u64,                    // 序列号
    should_resume: bool,         // 是否尝试恢复
}
```

**连接流程：**
1. 连接 WebSocket (`wss://api.sgroup.qq.com/websocket`)
2. 接收 `Hello` (Op 10) → 获取心跳间隔
3. 发送 `Identify` (Op 2) 或 `Resume` (Op 6)
4. 启动心跳任务（独立 Tokio task）
5. 监听 Dispatch 事件 (Op 0)

**重连策略：**
- 指数退避：1s → 2s → 4s → ... → 最大 60s
- 保留 `session_id` 和 `seq`，优先尝试 Resume
- `InvalidSession` (Op 9) 时清空状态，重新 Identify

**心跳机制：**
```rust
tokio::spawn(async move {
    let mut ticker = interval(heartbeat_interval);
    loop {
        ticker.tick().await;
        send_heartbeat(Op 1).await;
    }
});
```

**事件分发：**
```rust
pub enum GatewayEvent {
    Ready(ReadyEvent),
    MessageCreate(Message),
    AtMessageCreate(Message),
    DirectMessageCreate(Message),
    C2CMessageCreate(Message),
    GroupAtMessageCreate(Message),
    Reconnect,
    Disconnected,
}
```

### 2.3 ApiClient (client.rs)

**职责：**
- REST API 封装
- HTTP 请求构建和错误处理
- 消息发送（频道、私信、C2C、群组）

**关键实现：**
```rust
#[derive(Clone)]
pub struct ApiClient {
    token_manager: Arc<TokenManager>,
    client: reqwest::Client,
    base_url: String,
}
```

**API 端点：**
- `/channels/{channel_id}/messages` - 频道消息
- `/dms/{guild_id}/messages` - 私信
- `/v2/users/{openid}/messages` - C2C 消息
- `/v2/groups/{group_openid}/messages` - 群组消息

**智能回复：**
```rust
pub async fn reply_message(&self, msg: &Message, content: String) -> Result<SendMessageResponse> {
    // 根据 msg 中的字段自动选择 API
    if let Some(channel_id) = &msg.channel_id { ... }
    else if let Some(group_openid) = &msg.group_openid { ... }
    else if let Some(author) = &msg.author { ... }
}
```

### 2.4 Model (model.rs)

**数据结构分类：**

1. **Gateway Payload：**
   - `GatewayPayload` - 顶层消息结构
   - `OpCode` - 操作码枚举
   - `HelloPayload`, `IdentifyPayload`, `ResumePayload`

2. **事件模型：**
   - `ReadyEvent` - Bot 就绪
   - `Message` - 统一消息结构
   - `User`, `BotUser`

3. **Intents 权限：**
   ```rust
   pub struct Intents(pub u32);
   
   impl Intents {
       pub const GUILDS: u32 = 1 << 0;
       pub const GUILD_MESSAGES: u32 = 1 << 9;
       pub const GROUP_AND_C2C_EVENT: u32 = 1 << 25;
       // ...
   }
   ```

4. **API 请求/响应：**
   - `SendMessageRequest` - 消息发送请求
   - `SendMessageResponse` - 消息发送响应
   - `ApiError` - API 错误

### 2.5 QQBotClient (client.rs)

**职责：**
- 主入口，整合所有组件
- 事件循环管理
- 对外 API

**使用方式：**
```rust
let client = QQBotClient::new(app_id, app_secret)
    .with_intents(Intents::default_messages())
    .with_sandbox();

client.start(MyHandler).await?;
```

**事件循环：**
```rust
async fn run_event_loop<H: EventHandler>(&mut self, handler: H) -> Result<()> {
    while let Some(event) = event_rx.recv().await {
        match event {
            GatewayEvent::Ready(ready) => handler.on_ready(&ready).await,
            GatewayEvent::AtMessageCreate(msg) => handler.on_at_message(&msg).await,
            // ...
        }
    }
}
```

## 3. 依赖选择

| 依赖                | 用途                          | 理由                              |
|---------------------|-------------------------------|------------------------------------|
| `tokio`             | 异步运行时                    | 生态最成熟，功能完整               |
| `tokio-tungstenite` | WebSocket 客户端              | Tokio 原生支持，性能好             |
| `reqwest`           | HTTP 客户端                   | 易用性强，支持 JSON/multipart      |
| `serde` / `serde_json` | 序列化                      | Rust 标准方案                      |
| `parking_lot`       | 高性能锁                      | 比 std::sync 更快                  |
| `tracing`           | 结构化日志                    | 现代 Rust 日志标准                 |
| `anyhow` / `thiserror` | 错误处理                   | anyhow 便捷，thiserror 语义清晰    |
| `async-trait`       | 异步 trait                    | 稳定支持 trait 中的 async fn       |

## 4. 协议实现细节

### 4.1 WebSocket Payload 格式
```json
{
  "op": 0,           // OpCode
  "d": {...},        // Data payload
  "s": 123,          // Sequence (仅 Dispatch 有)
  "t": "EVENT_TYPE"  // Event type (仅 Dispatch 有)
}
```

### 4.2 认证流程
1. POST `/app/getAppAccessToken`
   ```json
   {
     "appId": "...",
     "clientSecret": "..."
   }
   ```
2. 响应：
   ```json
   {
     "access_token": "...",
     "expires_in": 7200
   }
   ```
3. 在 Gateway Identify 和 REST API 请求中使用：
   ```
   Authorization: QQBot {access_token}
   ```

### 4.3 消息事件类型
- `MESSAGE_CREATE` - 频道消息
- `AT_MESSAGE_CREATE` - 频道 @ 消息
- `DIRECT_MESSAGE_CREATE` - 私信
- `C2C_MESSAGE_CREATE` - 用户私聊（需单独权限）
- `GROUP_AT_MESSAGE_CREATE` - 群组 @ 消息（需单独权限）

### 4.4 Intents 配置
不同权限需要对应的 Intent：
- 频道消息：`GUILD_MESSAGES` (1 << 9)
- 私信：`DIRECT_MESSAGE` (1 << 12)
- C2C/群组：`GROUP_AND_C2C_EVENT` (1 << 25)

## 5. 错误处理策略

### 5.1 错误类型
```rust
pub enum Error {
    WebSocket(String),      // WS 连接错误
    Http(reqwest::Error),   // HTTP 错误
    Json(serde_json::Error),// 序列化错误
    Auth(String),           // 认证错误
    InvalidPayload(String), // 协议错误
    ConnectionClosed,       // 连接关闭
    ReconnectRequired,      // 需要重连
}
```

### 5.2 重试策略
- **Token 刷新失败：** 返回错误，由上层决定
- **Gateway 连接失败：** 自动重连（指数退避）
- **API 请求失败：** 返回错误，由业务层重试

## 6. 性能优化

### 6.1 并发安全
- `TokenManager` 使用 `RwLock`，读多写少场景高效
- `SessionState` 也使用 `RwLock`，心跳任务不会阻塞主线程

### 6.2 内存管理
- 大对象使用 `Arc` 共享（`TokenManager`）
- 避免不必要的 clone（`Message` 等）

### 6.3 网络优化
- HTTP 连接复用（`reqwest::Client`）
- WebSocket 长连接

## 7. MVP 功能清单

### ✅ 已实现
- [x] OAuth2 认证和 Token 管理
- [x] WebSocket Gateway 连接
- [x] 心跳机制
- [x] 自动重连和会话恢复
- [x] 接收消息事件（频道、私信、C2C、群组）
- [x] 发送文本消息
- [x] 智能消息回复
- [x] Intents 权限配置
- [x] 沙箱环境支持
- [x] 结构化日志

### 🚧 待实现
- [ ] 富媒体消息（图片、文件上传）
- [ ] Markdown 和 Keyboard 支持
- [ ] 消息审核事件
- [ ] 语音频道事件
- [ ] Sharding 支持（大型 Bot）
- [ ] 更多 REST API（频道管理、成员管理）
- [ ] 单元测试和集成测试
- [ ] 文档和示例完善

## 8. 使用示例

### 8.1 最简单的 Echo Bot
```rust
struct EchoBot;

#[async_trait::async_trait]
impl EventHandler for EchoBot {
    async fn on_c2c_message(&self, msg: &Message) {
        // 自动回显
    }
}
```

### 8.2 多功能 Bot
```rust
struct MyBot {
    api: ApiClient,
    db: Database,
}

#[async_trait::async_trait]
impl EventHandler for MyBot {
    async fn on_ready(&self, ready: &ReadyEvent) {
        info!("Bot {} online", ready.user.username);
    }

    async fn on_at_message(&self, msg: &Message) {
        if msg.content.contains("查询") {
            let result = self.db.query(...).await;
            self.api.reply_message(msg, result).await.ok();
        }
    }
}
```

## 9. 安全考虑

1. **Token 安全：**
   - Token 仅存储在内存中（`RwLock`）
   - 不记录到日志
   - 支持环境变量配置

2. **输入验证：**
   - 所有 API 响应都经过 serde 验证
   - 错误处理避免 panic

3. **依赖安全：**
   - 使用主流、维护活跃的库
   - 定期更新依赖

## 10. 测试策略

### 10.1 单元测试
- `TokenManager::is_expired()` 逻辑
- `Intents` 位操作
- `Message` 序列化/反序列化

### 10.2 集成测试
- 沙箱环境完整流程
- Gateway 连接和事件接收
- API 消息发送

### 10.3 压力测试
- 高并发消息处理
- 长时间运行稳定性
- 重连机制可靠性

## 11. 部署建议

### 11.1 环境变量
```bash
export QQ_APP_ID="..."
export QQ_APP_SECRET="..."
export QQ_SANDBOX="true"  # 开发阶段
export RUST_LOG="info"
```

### 11.2 Docker 部署
```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/my-bot /usr/local/bin/
CMD ["my-bot"]
```

### 11.3 监控
- 使用 `tracing-subscriber` 输出结构化日志
- 接入 Prometheus/Grafana
- 监控关键指标：
  - Gateway 连接状态
  - 消息处理延迟
  - Token 刷新频率
  - 错误率

---

**版本：** 0.1.0 (MVP)  
**最后更新：** 2024-01
