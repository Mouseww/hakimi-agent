use qq_bot_sdk::prelude::*;

#[tokio::test]
async fn test_rate_limiter() {
    let limiter = RateLimiter::new(5, std::time::Duration::from_secs(1));
    
    // 应该可以快速获取 5 个许可
    for _ in 0..5 {
        assert!(limiter.try_acquire());
    }
    
    // 第 6 个应该失败
    assert!(!limiter.try_acquire());
}

#[test]
fn test_markdown_builder() {
    let md = MarkdownMessage::new("# Hello\n## World")
        .with_template("template_123")
        .add_param("key1", vec!["value1".to_string()]);
    
    assert_eq!(md.custom_template_id, Some("template_123".to_string()));
    assert_eq!(md.params.len(), 1);
}

#[test]
fn test_keyboard_builder() {
    let keyboard = Keyboard::new()
        .add_row(
            KeyboardRow::new()
                .add_button(Button::new("btn1", "Click Me", ActionType::Callback))
        );
    
    assert_eq!(keyboard.content.rows.len(), 1);
    assert_eq!(keyboard.content.rows[0].buttons.len(), 1);
}

#[test]
fn test_embed_builder() {
    let embed = Embed::new()
        .title("Test Title")
        .description("Test Description")
        .add_field("Field1", "Value1")
        .add_field("Field2", "Value2");
    
    assert_eq!(embed.title, Some("Test Title".to_string()));
    assert_eq!(embed.fields.as_ref().unwrap().len(), 2);
}

#[test]
fn test_ark_message() {
    let ark = ArkMessage::new(23)
        .add_kv("title", "Test Title")
        .add_kv("desc", "Test Description");
    
    assert_eq!(ark.template_id, 23);
    assert_eq!(ark.kv.as_ref().unwrap().len(), 2);
}

#[test]
fn test_intents() {
    let mut intents = Intents::new();
    intents.add(Intents::GUILDS);
    intents.add(Intents::GUILD_MESSAGES);
    
    assert!(intents.has(Intents::GUILDS));
    assert!(intents.has(Intents::GUILD_MESSAGES));
    assert!(!intents.has(Intents::DIRECT_MESSAGE));
    
    intents.remove(Intents::GUILDS);
    assert!(!intents.has(Intents::GUILDS));
}

#[test]
fn test_message_target() {
    let msg = Message {
        id: "123".to_string(),
        channel_id: Some("channel_456".to_string()),
        guild_id: None,
        group_id: None,
        group_openid: None,
        author: None,
        content: "test".to_string(),
        timestamp: "2024-01-01T00:00:00Z".to_string(),
        mention_everyone: false,
        mentions: vec![],
        attachments: vec![],
    };
    
    let target = MessageTarget::from_message(&msg);
    assert!(matches!(target, Some(MessageTarget::Channel(_))));
}
