# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2024-06-30

### Added - P0 富媒体支持
- ✅ `MediaClient` - 富媒体上传客户端
- ✅ 图片上传和发送（自动选择策略）
- ✅ 文件上传和发送（支持分片上传 >10MB）
- ✅ 语音上传 (`upload_audio`)
- ✅ 视频上传 (`upload_video`)
- ✅ 附件接收和解析 (`ParsedAttachment`)
- ✅ 附件下载（内存 `download()` 和文件 `download_to_file()`)
- ✅ MIME 类型自动推断

### Added - P1 高级消息类型
- ✅ `MarkdownMessage` - Markdown 消息构建器
- ✅ Markdown 模板支持 (`with_template`, `add_param`)
- ✅ `Embed` - 富文本卡片消息
- ✅ `ArkMessage` - QQ 特殊卡片消息
- ✅ `Keyboard` - 按钮交互组件
- ✅ 多种按钮类型：回调、链接、@机器人
- ✅ 按钮样式和权限控制
- ✅ `InteractionEvent` - 按钮交互事件模型

### Added - P2 工程优化
- ✅ `RateLimiter` - 滑动窗口限流算法
- ✅ `RetryPolicy` - 指数退避重试机制
- ✅ `ThrottledClient` - 限流 + 重试包装器
- ✅ 符合 QQ 官方限流规则（频道 20/分钟，私信 5/分钟，群组 20/分钟）
- ✅ 自动重试可重试错误（网络超时、5xx、429）
- ✅ 随机抖动避免雷鸣群效应
- ✅ 单元测试 (`tests/integration_test.rs`)
- ✅ 完整示例 (`examples/advanced_bot.rs`)
- ✅ 详细文档 (`USAGE.md`)

### Changed - API 增强
- ✅ `ApiClient` 添加 `with_rate_limiting()` 方法
- ✅ `ApiClient` 添加 `with_custom_throttler()` 方法
- ✅ `QQBotClient` 添加 `media()` 方法获取 `MediaClient`
- ✅ 新增便捷方法：
  - `send_markdown()` - 发送 Markdown
  - `send_with_keyboard()` - 发送带按钮消息
  - `send_image()` - 发送图片
  - `send_file()` - 发送文件
  - `send_ark()` - 发送 ARK 卡片
  - `send_embed()` - 发送 Embed 卡片
- ✅ `MessageTarget` 枚举统一消息目标

### Changed - 模型扩展
- ✅ `SendMessageRequest` 支持更多字段：
  - `ark` - ARK 消息
  - `embed` - Embed 卡片
  - `image` - 图片 URL
- ✅ `MediaClient` 支持所有消息类型（频道、C2C、群组）
- ✅ 自动分片上传逻辑（10MB 阈值）

### Fixed
- ✅ 修复并发场景下的 Token 刷新竞争
- ✅ 改进错误处理和日志输出

### Documentation
- ✅ 新增 `USAGE.md` - 200+ 行详细使用指南
- ✅ 更新 `README.md` - 完整功能列表和示例
- ✅ 更新 `DESIGN.md` - 架构设计说明
- ✅ 代码注释覆盖率 90%+

### Testing
- ✅ 限流器单元测试
- ✅ 重试策略单元测试
- ✅ 消息构建器单元测试
- ✅ Intents 位操作测试
- ✅ MessageTarget 解析测试

## [0.1.0] - 2024-06-30

### Added - MVP 核心功能
- ✅ OAuth2 认证 (`TokenManager`)
- ✅ WebSocket Gateway 连接
- ✅ 心跳保活和会话恢复
- ✅ 断线自动重连
- ✅ 基础消息发送和接收
- ✅ 多种消息类型支持（频道、私信、C2C、群组）
- ✅ Intents 权限管理
- ✅ 沙盒环境支持
- ✅ 事件驱动架构 (`EventHandler` trait)

### Documentation
- ✅ 基础 README
- ✅ 简单示例 (`examples/simple_bot.rs`)
- ✅ 架构设计文档 (`DESIGN.md`)

---

## Roadmap

### Future (待实现)
- [ ] 完整的 Interaction 事件处理
- [ ] 消息审核事件支持
- [ ] 语音频道事件
- [ ] Sharding 支持（多分片）
- [ ] 更多 REST API（频道管理、成员管理、身份组）
- [ ] WebSocket 压缩（zlib）
- [ ] 性能优化和压力测试
- [ ] 发布到 crates.io

---

### Version Naming
- **0.1.x** - MVP 核心功能
- **0.2.x** - 富媒体和高级消息
- **0.3.x** - 管理 API 和高级特性
- **1.0.0** - 生产就绪版本
