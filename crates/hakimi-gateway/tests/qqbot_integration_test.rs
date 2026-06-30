// QQBot WebSocket Integration Test
//
// 这个测试验证 QQBot 适配器的基本功能

use hakimi_gateway::{PlatformAdapter, QQBotAdapter, QQBotAdapterConfig};

// 测试适配器初始化
#[test]
fn test_qqbot_adapter_initialization() {
    let config = QQBotAdapterConfig {
        bot_id: "test-bot".to_string(),
        app_id: "test_app_id".to_string(),
        client_secret: "test_secret".to_string(),
        home_channel: "".to_string(),
        default_chat_type: "c2c".to_string(),
        markdown_support: true,
        base_url: None,
        token_url: None,
    };

    let adapter = QQBotAdapter::new(config);
    assert_eq!(adapter.name(), "qqbot");
    assert_eq!(adapter.bot_id(), "test-bot");
}

// 测试 receiver 可以被取出
#[test]
fn test_receiver_can_be_taken() {
    let config = QQBotAdapterConfig {
        bot_id: "test-bot".to_string(),
        app_id: "test_app_id".to_string(),
        client_secret: "test_secret".to_string(),
        home_channel: "".to_string(),
        default_chat_type: "c2c".to_string(),
        markdown_support: true,
        base_url: None,
        token_url: None,
    };

    let mut adapter = QQBotAdapter::new(config);

    // 第一次取出应该成功
    let receiver = adapter.take_receiver();
    assert!(receiver.is_some());

    // 第二次取出应该返回 None
    let receiver2 = adapter.take_receiver();
    assert!(receiver2.is_none());
}

// 测试消息字符限制
#[test]
fn test_max_message_chars() {
    let config = QQBotAdapterConfig {
        bot_id: "test-bot".to_string(),
        app_id: "test_app_id".to_string(),
        client_secret: "test_secret".to_string(),
        home_channel: "".to_string(),
        default_chat_type: "c2c".to_string(),
        markdown_support: true,
        base_url: None,
        token_url: None,
    };

    let adapter = QQBotAdapter::new(config);
    assert_eq!(adapter.max_message_chars(), Some(4000));
}
