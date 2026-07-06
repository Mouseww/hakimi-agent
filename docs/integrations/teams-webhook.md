# Microsoft Teams Webhook Integration for Hakimi Agent

无需 Azure Bot 注册的 Microsoft Teams 双向集成方案。

## 方案总览

通过两个互相独立的机制实现双向通道：

**入方向（员工 → 智能体）**:
- 员工在频道 @AgentBot 发消息
- Teams Outgoing Webhook 将消息 POST 到你的 HTTP 服务
- 服务验证 HMAC 签名，10 秒内返回"已收到"回执
- 服务异步拉起智能体处理任务

**出方向（智能体 → 员工）**:
- 智能体产出结果
- 服务 POST Adaptive Card 到 Power Automate Workflows 的 webhook URL
- Flow bot 将卡片发到指定频道

## 快速开始

### 1. 创建出方向通道（Workflows webhook）

在你希望智能体发消息的频道操作：

1. 频道名旁点 `...` → Workflows（工作流）
2. 选择模板 "Post to a channel when a webhook request is received"
3. 命名建议：`agent-notify-<频道名>`
4. 创建完成后复制生成的 HTTP URL

测试：
```bash
curl -X POST "YOUR_WORKFLOW_URL" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "message",
    "attachments": [{
      "contentType": "application/vnd.microsoft.card.adaptive",
      "content": {
        "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
        "type": "AdaptiveCard",
        "version": "1.4",
        "body": [
          {"type": "TextBlock", "text": "测试成功！", "wrap": true}
        ]
      }
    }]
  }'
```

### 2. 配置 Hakimi Agent

在 `config.yaml` 中添加 Teams Webhook 配置：

```yaml
gateway:
  teams_webhook:
    hmac_secret: "YOUR_BASE64_SECRET_FROM_TEAMS"  # 从 Outgoing Webhook 获取
    default_workflow_url: "https://prod-xx.westus.logic.azure.com/workflows/..."
    bot_id: "agent-bot"
    
    # （可选）多频道映射
    channel_workflows:
      "19:abc123@thread.tacv2": "https://prod-xx.westus.logic.azure.com/workflows/channel1"
      "19:def456@thread.tacv2": "https://prod-xx.westus.logic.azure.com/workflows/channel2"
```

### 3. 创建入方向通道（Outgoing Webhook）

由团队所有者操作：

1. 进入团队 → 团队名旁 `...` → Manage team（管理团队）→ Apps 标签页
2. 页面右下角点 "Create an outgoing webhook"
3. 填写：
   - **Name**: `AgentBot`（员工 @ 它时用的名字）
   - **Callback URL**: `https://your-domain.com/teams/inbound`
   - **Description**: 随意
4. 创建成功后会弹出**安全令牌（security token）**，只显示一次，立即保存

### 4. 启动 HTTP 服务器

```rust
use hakimi_gateway::teams_webhook::{TeamsWebhookAdapter, TeamsWebhookConfig, TeamsWebhookServer};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let config = TeamsWebhookConfig {
        hmac_secret: std::env::var("TEAMS_HMAC_SECRET").unwrap(),
        default_workflow_url: std::env::var("TEAMS_WORKFLOW_URL").unwrap(),
        ..Default::default()
    };
    
    let adapter = Arc::new(TeamsWebhookAdapter::new(config));
    
    // 连接 adapter
    adapter.clone().connect().await.unwrap();
    
    // 启动 HTTP 服务器（监听 /teams/inbound）
    let server = TeamsWebhookServer::new(adapter.clone(), "0.0.0.0:3000".parse().unwrap());
    tokio::spawn(async move {
        server.serve().await.expect("Server failed");
    });
    
    // 接收消息并处理
    let mut rx = adapter.take_receiver().unwrap();
    while let Some(msg) = rx.recv().await {
        println!("收到消息: {}", msg.text);
        
        // 处理后回复
        adapter.send_message(&msg.chat_id, "处理完成！").await.unwrap();
    }
}
```

## Adaptive Card 高级用法

### 带按钮的卡片

```rust
use hakimi_gateway::teams_webhook::AdaptiveCardBuilder;

let mut builder = AdaptiveCardBuilder::new("任务完成");
builder
    .add_text("已成功部署到生产环境")
    .add_fact("部署时间", "2026-07-06 15:30")
    .add_fact("版本号", "v1.2.3")
    .add_button("查看日志", "https://logs.example.com/deploy/12345")
    .add_button("回滚", "https://rollback.example.com/v1.2.2");

let card_json = builder.build();
// 发送到 Workflows webhook
```

## 能力边界

✅ **支持**:
- 标准频道内的 @机器人 消息
- HMAC 签名验证（安全）
- Adaptive Card 富文本展示
- 按钮、链接、表格等交互元素

❌ **不支持**:
- 私聊（DM）、群聊、私有频道
- 自定义机器人名字和头像（出方向显示为 "Workflows"）
- 文件上传（可用链接代替）

## 安全注意事项

1. **HMAC 校验绝不能省** — 回调 URL 是公网地址，必须验证签名
2. **Workflows URL 等同于凭证** — 只存在环境变量或 Key Vault，不进代码仓库
3. **对指令做白名单** — 早期建议只响应固定的几种指令前缀
4. **记录审计日志** — 谁（aadObjectId）、何时、发了什么指令

## 验证清单

- [ ] curl 直接 POST Workflows URL，频道收到卡片
- [ ] 浏览器访问 `/healthz` 返回 `{"ok": true}`
- [ ] 配置 HMAC secret 到环境变量，重启服务
- [ ] 频道里 @AgentBot 你好，几秒内看到回执
- [ ] 稍后看到 Flow bot 推送的结果卡片

## 故障排查

| 现象 | 排查方向 |
|------|---------|
| @不到机器人 | Outgoing Webhook 按团队创建，确认在同一团队的标准频道 |
| 回执超时 | 回调超过 10 秒或返回了非 200；检查同步阻塞操作 |
| 服务收到请求但 401 | HMAC 密钥贴错（注意是 base64 原文，不要二次编码） |
| Workflows 返回 400 | payload 不是 Adaptive Card 包裹格式 |
| Workflows 突然失效 | 创建者账号被停用导致 flow 变孤儿；配置 co-owner |

## 生产化建议

1. **队列模式** — 入站端点只把任务写入队列（Azure Storage Queue / Service Bus），由 worker 消费，避免 App Service 实例回收丢任务
2. **多频道路由** — 维护"项目 → 频道"的配置，让每个项目的通知进各自频道
3. **权限控制** — 用 `aadObjectId` 关联企业目录，实现基于角色的访问控制
4. **监控告警** — 对 `/teams/inbound` 限流，记录异常请求

## 参考资料

- [Microsoft Teams Outgoing Webhooks](https://docs.microsoft.com/en-us/microsoftteams/platform/webhooks-and-connectors/how-to/add-outgoing-webhook)
- [Power Automate Workflows](https://make.powerautomate.com)
- [Adaptive Cards Designer](https://adaptivecards.io/designer/)
