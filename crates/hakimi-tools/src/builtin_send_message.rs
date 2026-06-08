use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use tracing::debug;

use crate::Tool;

/// A queued outbound message waiting to be picked up by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    /// Target in `platform:chat_id` format (e.g., "telegram:123456789").
    pub target: String,
    /// The message content to send.
    pub message: String,
    /// ID of the session that generated this message.
    pub session_id: String,
    /// ISO 8601 timestamp when the message was queued.
    pub queued_at: String,
}

/// Global outbound message queue shared between tools and the gateway.
pub static MESSAGE_QUEUE: LazyLock<Mutex<VecDeque<QueuedMessage>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// Pop the next message from the outbound queue (non-blocking).
/// Returns `None` if the queue is empty.
pub fn pop_message() -> Option<QueuedMessage> {
    MESSAGE_QUEUE.lock().ok().and_then(|mut q| q.pop_front())
}

/// Get the current number of queued messages.
pub fn queue_len() -> usize {
    MESSAGE_QUEUE.lock().map(|q| q.len()).unwrap_or(0)
}

/// A cached outbound target discovered from gateway configuration or sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelDirectoryEntry {
    /// Platform identifier, for example `slack` or `discord`.
    pub platform: String,
    /// Platform chat/channel/user identifier used for delivery.
    pub id: String,
    /// Human-friendly name that can be used in `send_message.target`.
    pub name: String,
    /// Optional bot/role identifier that discovered this target.
    #[serde(default)]
    pub bot_id: String,
    /// Channel kind shown to the model, for example `home`, `channel`, or `dm`.
    #[serde(default, rename = "type")]
    pub channel_type: String,
    /// Whether this is the default target for a bare platform name.
    #[serde(default)]
    pub is_home: bool,
}

impl ChannelDirectoryEntry {
    pub fn home(platform: &str, id: &str, name: &str, channel_type: &str, bot_id: &str) -> Self {
        Self {
            platform: platform.trim().to_ascii_lowercase(),
            id: id.trim().to_string(),
            name: name.trim().to_string(),
            bot_id: bot_id.trim().to_string(),
            channel_type: channel_type.trim().to_string(),
            is_home: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ChannelDirectory {
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    platforms: BTreeMap<String, Vec<ChannelDirectoryEntry>>,
}

#[cfg(test)]
static TEST_CHANNEL_DIRECTORY_PATH: LazyLock<Mutex<Option<PathBuf>>> =
    LazyLock::new(|| Mutex::new(None));

/// Location compatible with Hermes' cached channel directory.
pub fn channel_directory_path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_CHANNEL_DIRECTORY_PATH
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
    {
        return path;
    }

    std::env::var("HAKIMI_CHANNEL_DIRECTORY")
        .ok()
        .or_else(|| std::env::var("HERMES_CHANNEL_DIRECTORY").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| hakimi_common::effective_hakimi_home().join("channel_directory.json"))
}

/// Persist the currently known gateway targets for `send_message(action="list")`.
pub fn write_channel_directory(entries: &[ChannelDirectoryEntry]) -> std::io::Result<PathBuf> {
    let path = channel_directory_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut platforms: BTreeMap<String, Vec<ChannelDirectoryEntry>> = BTreeMap::new();
    for entry in entries {
        if entry.platform.trim().is_empty() || entry.id.trim().is_empty() {
            continue;
        }
        platforms
            .entry(entry.platform.trim().to_ascii_lowercase())
            .or_default()
            .push(entry.clone());
    }

    let directory = ChannelDirectory {
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
        platforms,
    };
    let json = serde_json::to_string_pretty(&directory).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

fn load_channel_directory() -> ChannelDirectory {
    let path = channel_directory_path();
    let Ok(contents) = std::fs::read_to_string(path) else {
        return ChannelDirectory::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

fn normalize_channel_query(value: &str) -> String {
    value.trim().trim_start_matches('#').to_ascii_lowercase()
}

fn entry_target_label(platform: &str, entry: &ChannelDirectoryEntry) -> String {
    if platform == "discord" && !entry.channel_type.trim().is_empty() {
        format!("#{}", entry.name.trim_start_matches('#'))
    } else {
        entry.name.clone()
    }
}

fn format_channel_directory_for_display() -> String {
    let directory = load_channel_directory();
    if !directory
        .platforms
        .values()
        .any(|entries| !entries.is_empty())
    {
        return "No messaging platforms connected or no channels discovered yet.".to_string();
    }

    let mut lines = Vec::new();
    lines.push("Available messaging targets:".to_string());
    if let Some(updated_at) = directory.updated_at.as_deref() {
        lines.push(format!("Updated: {updated_at}"));
    }
    lines.push(String::new());

    for (platform, entries) in directory.platforms {
        if entries.is_empty() {
            continue;
        }
        lines.push(format!("{}:", title_case(&platform)));
        for entry in entries {
            let label = entry_target_label(&platform, &entry);
            let kind = if entry.channel_type.trim().is_empty() {
                "channel"
            } else {
                entry.channel_type.trim()
            };
            let home = if entry.is_home { " home" } else { "" };
            lines.push(format!(
                "  {platform}:{label} -> {} ({kind}{home})",
                entry.id
            ));
        }
        lines.push(String::new());
    }

    lines.push(
        "Use these values as send_message target. Bare platform names use the home target when one is cached."
            .to_string(),
    );
    lines.join("\n")
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn resolve_channel_target(platform: &str, raw_target: Option<&str>) -> Option<String> {
    let directory = load_channel_directory();
    let platform = platform.trim().to_ascii_lowercase();
    let entries = directory.platforms.get(&platform)?;
    if entries.is_empty() {
        return None;
    }

    let Some(raw) = raw_target.map(str::trim).filter(|s| !s.is_empty()) else {
        return entries
            .iter()
            .find(|entry| entry.is_home)
            .or_else(|| entries.first())
            .map(|entry| entry.id.clone());
    };

    for entry in entries {
        if entry.id == raw {
            return Some(entry.id.clone());
        }
    }

    let query = normalize_channel_query(raw);
    for entry in entries {
        if normalize_channel_query(&entry.name) == query
            || normalize_channel_query(&entry_target_label(&platform, entry)) == query
        {
            return Some(entry.id.clone());
        }
    }

    let matches: Vec<&ChannelDirectoryEntry> = entries
        .iter()
        .filter(|entry| normalize_channel_query(&entry.name).starts_with(&query))
        .collect();
    if matches.len() == 1 {
        return Some(matches[0].id.clone());
    }
    None
}

fn channel_directory_has_platform(platform: &str) -> bool {
    let directory = load_channel_directory();
    directory
        .platforms
        .get(&platform.trim().to_ascii_lowercase())
        .is_some_and(|entries| !entries.is_empty())
}

/// Resolve a cached gateway target from the shared channel directory.
///
/// `raw_target=None` returns the platform home target, falling back to the
/// first cached entry for compatibility with `send_message`.
pub fn resolve_cached_channel_target(platform: &str, raw_target: Option<&str>) -> Option<String> {
    resolve_channel_target(platform, raw_target)
}

/// Return all cached home delivery targets as `platform:chat_id` strings.
pub fn cached_home_delivery_targets() -> Vec<String> {
    let directory = load_channel_directory();
    let mut targets = Vec::new();
    for (platform, entries) in directory.platforms {
        for entry in entries.into_iter().filter(|entry| entry.is_home) {
            if let Some(target) = gateway_target(&platform, &entry.id) {
                targets.push(target);
            }
        }
    }
    targets
}

fn gateway_target(platform: &str, chat_id: &str) -> Option<String> {
    let platform = platform.trim();
    let chat_id = chat_id.trim();
    if platform.is_empty() || chat_id.is_empty() {
        None
    } else {
        Some(format!("{}:{}", platform.to_ascii_lowercase(), chat_id))
    }
}

fn looks_like_explicit_target(platform: &str, target: &str) -> bool {
    let raw = target.trim();
    if raw.is_empty() || raw.starts_with('#') {
        return false;
    }
    if raw.contains(':') || raw.contains('@') || raw.starts_with('!') || raw.starts_with('+') {
        return true;
    }
    if raw.starts_with("oc_")
        || raw.starts_with("ou_")
        || raw.starts_with("on_")
        || raw.starts_with("chat_")
        || raw.starts_with("open_")
    {
        return true;
    }
    if matches!(platform, "telegram" | "discord")
        && raw.chars().all(|c| c == '-' || c.is_ascii_digit())
    {
        return true;
    }
    raw.len() >= 8
        && raw
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn normalize_send_target(target: &str) -> Result<String> {
    let target = target.trim();
    if target.eq_ignore_ascii_case("origin") {
        return Ok("origin".to_string());
    }

    if let Some((platform, raw_ref)) = target.split_once(':') {
        let platform = platform.trim().to_ascii_lowercase();
        let raw_ref = raw_ref.trim();
        if platform.is_empty() {
            return Err(HakimiError::Tool(
                "send_message target has an empty platform".into(),
            ));
        }
        if let Some(resolved) = resolve_channel_target(&platform, Some(raw_ref)) {
            return Ok(format!("{platform}:{resolved}"));
        }
        if raw_ref.is_empty() {
            return Err(HakimiError::Tool(format!(
                "no home target cached for '{platform}'. Use send_message(action='list') or pass an explicit platform:chat_id target."
            )));
        }
        if looks_like_explicit_target(&platform, raw_ref)
            || !channel_directory_has_platform(&platform)
        {
            return Ok(format!("{platform}:{raw_ref}"));
        }
        return Err(HakimiError::Tool(format!(
            "could not resolve '{raw_ref}' on {platform}. Use send_message(action='list') to see available targets or pass an explicit platform chat ID."
        )));
    }

    let platform = target.to_ascii_lowercase();
    if platform.is_empty() {
        return Err(HakimiError::Tool(
            "send_message target cannot be empty".into(),
        ));
    }
    resolve_channel_target(&platform, None)
        .map(|chat_id| format!("{platform}:{chat_id}"))
        .ok_or_else(|| {
            HakimiError::Tool(format!(
                "no home target cached for '{platform}'. Use send_message(action='list') or pass an explicit platform:chat_id target."
            ))
        })
}

/// Built-in tool for sending messages to external platforms via the gateway queue.
pub struct SendMessageTool;

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn toolset(&self) -> &str {
        "communication"
    }

    fn description(&self) -> &str {
        "Send a message to an external platform or list cached gateway targets. Use action='list' before sending to a named channel/person; bare platform names use the cached home target."
    }

    fn emoji(&self) -> &str {
        "\u{1f4e8}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["send", "list"],
                    "description": "Action to perform. 'send' queues a message. 'list' shows cached gateway targets."
                },
                "target": {
                    "type": "string",
                    "description": "Delivery target. Use 'platform', 'platform:home', 'platform:#channel-name', or explicit 'platform:chat_id'."
                },
                "message": {
                    "type": "string",
                    "description": "The message content to send."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("send");
        if action == "list" {
            return Ok(json!({
                "targets": format_channel_directory_for_display()
            })
            .to_string());
        }
        if action != "send" {
            return Err(HakimiError::Tool(format!(
                "unsupported send_message action '{action}'. Expected 'send' or 'list'."
            )));
        }

        let target = args
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: target".into()))?;

        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: message".into()))?;

        let target = normalize_send_target(target)?;

        let now = chrono::Utc::now().to_rfc3339();

        let queued = QueuedMessage {
            target: target.clone(),
            message: message.to_string(),
            session_id: ctx.session_id.clone(),
            queued_at: now,
        };

        debug!(
            target = %target,
            message_len = message.len(),
            session_id = %ctx.session_id,
            "queuing outbound message"
        );

        let mut queue = MESSAGE_QUEUE
            .lock()
            .map_err(|e| HakimiError::Tool(format!("failed to lock message queue: {e}")))?;

        let queue_size = queue.len();
        queue.push_back(queued);

        Ok(format!(
            "Message queued for delivery to '{target}'. Queue position: {}. Total queued: {}.",
            queue_size + 1,
            queue_size + 1
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;
    use std::sync::{Mutex, MutexGuard};

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn test_guard() -> MutexGuard<'static, ()> {
        TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        }
    }

    /// Drain the message queue to avoid cross-test pollution
    fn drain_queue() {
        while pop_message().is_some() {}
    }

    fn set_test_channel_directory(entries: &[ChannelDirectoryEntry]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("channel_directory.json");
        *TEST_CHANNEL_DIRECTORY_PATH.lock().unwrap() = Some(path);
        write_channel_directory(entries).unwrap();
        dir
    }

    fn clear_test_channel_directory() {
        *TEST_CHANNEL_DIRECTORY_PATH.lock().unwrap() = None;
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = SendMessageTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["properties"]["action"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.is_empty());
    }

    #[test]
    fn test_tool_properties() {
        let tool = SendMessageTool;
        assert_eq!(tool.name(), "send_message");
        assert_eq!(tool.toolset(), "communication");
        assert!(tool.check_available());
        assert_eq!(tool.emoji(), "📨");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_queue_message() {
        let _guard = test_guard();
        clear_test_channel_directory();
        drain_queue();

        let ctx = test_ctx();
        let args = json!({
            "target": "telegram:123456789",
            "message": "Hello from the agent!"
        });

        let result = SendMessageTool.execute(&args, &ctx).await.unwrap();

        assert!(result.contains("queued"));
        assert!(result.contains("telegram:123456789"));

        // Pop the message and verify
        let msg = pop_message().expect("expected a queued message");
        assert_eq!(msg.target, "telegram:123456789");
        assert_eq!(msg.message, "Hello from the agent!");
        assert_eq!(msg.session_id, "test");

        drain_queue();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_list_action_formats_cached_targets() {
        let _guard = test_guard();
        let _dir = set_test_channel_directory(&[
            ChannelDirectoryEntry::home("slack", "C123456789", "home", "home", "slack"),
            ChannelDirectoryEntry {
                platform: "slack".into(),
                id: "C987654321".into(),
                name: "engineering".into(),
                bot_id: "slack".into(),
                channel_type: "channel".into(),
                is_home: false,
            },
        ]);

        let ctx = test_ctx();
        let result = SendMessageTool
            .execute(&json!({"action": "list"}), &ctx)
            .await
            .unwrap();

        assert!(result.contains("Available messaging targets"));
        assert!(result.contains("slack:home"));
        assert!(result.contains("slack:engineering"));
        clear_test_channel_directory();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_bare_platform_uses_cached_home_target() {
        let _guard = test_guard();
        drain_queue();
        let _dir = set_test_channel_directory(&[ChannelDirectoryEntry::home(
            "slack",
            "C123456789",
            "home",
            "home",
            "slack",
        )]);

        let ctx = test_ctx();
        SendMessageTool
            .execute(
                &json!({
                    "target": "slack",
                    "message": "home message"
                }),
                &ctx,
            )
            .await
            .unwrap();

        let msg = pop_message().unwrap();
        assert_eq!(msg.target, "slack:C123456789");
        assert_eq!(msg.message, "home message");
        clear_test_channel_directory();
        drain_queue();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_named_target_resolves_from_directory() {
        let _guard = test_guard();
        drain_queue();
        let _dir = set_test_channel_directory(&[ChannelDirectoryEntry {
            platform: "discord".into(),
            id: "987654321".into(),
            name: "deploys".into(),
            bot_id: "discord".into(),
            channel_type: "channel".into(),
            is_home: false,
        }]);

        let ctx = test_ctx();
        SendMessageTool
            .execute(
                &json!({
                    "target": "discord:#deploys",
                    "message": "release ready"
                }),
                &ctx,
            )
            .await
            .unwrap();

        let msg = pop_message().unwrap();
        assert_eq!(msg.target, "discord:987654321");
        clear_test_channel_directory();
        drain_queue();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_unresolved_named_target_errors_with_list_hint() {
        let _guard = test_guard();
        drain_queue();
        let _dir = set_test_channel_directory(&[ChannelDirectoryEntry::home(
            "slack",
            "C123456789",
            "home",
            "home",
            "slack",
        )]);

        let ctx = test_ctx();
        let err = SendMessageTool
            .execute(
                &json!({
                    "target": "slack:#missing",
                    "message": "hello"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        assert!(format!("{err}").contains("send_message(action='list')"));
        assert!(pop_message().is_none());
        clear_test_channel_directory();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_explicit_thread_target_remains_compatible() {
        let _guard = test_guard();
        clear_test_channel_directory();
        drain_queue();

        let ctx = test_ctx();
        SendMessageTool
            .execute(
                &json!({
                    "target": "telegram:-1001234567890:17585",
                    "message": "topic message"
                }),
                &ctx,
            )
            .await
            .unwrap();

        let msg = pop_message().unwrap();
        assert_eq!(msg.target, "telegram:-1001234567890:17585");
        clear_test_channel_directory();
        drain_queue();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_pop_message() {
        let _guard = test_guard();
        clear_test_channel_directory();
        drain_queue();

        let ctx = test_ctx();
        SendMessageTool
            .execute(&json!({"target": "discord:abc", "message": "test"}), &ctx)
            .await
            .unwrap();

        let msg = pop_message().unwrap();
        assert_eq!(msg.target, "discord:abc");
        assert_eq!(msg.message, "test");

        // Queue should be empty now
        assert!(pop_message().is_none());

        drain_queue();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_queue_multiple_messages() {
        let _guard = test_guard();
        clear_test_channel_directory();
        drain_queue();

        let ctx = test_ctx();
        SendMessageTool
            .execute(&json!({"target": "telegram:1", "message": "first"}), &ctx)
            .await
            .unwrap();
        SendMessageTool
            .execute(&json!({"target": "telegram:2", "message": "second"}), &ctx)
            .await
            .unwrap();

        assert_eq!(queue_len(), 2);

        let msg1 = pop_message().unwrap();
        assert_eq!(msg1.message, "first");
        let msg2 = pop_message().unwrap();
        assert_eq!(msg2.message, "second");
        assert!(pop_message().is_none());

        drain_queue();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_invalid_target_format_error() {
        let _guard = test_guard();
        clear_test_channel_directory();
        let ctx = test_ctx();
        let args = json!({
            "target": "no-colon-here",
            "message": "hello"
        });
        let err = SendMessageTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("no home target cached"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_missing_target_error() {
        let _guard = test_guard();
        clear_test_channel_directory();
        let ctx = test_ctx();
        let args = json!({"message": "hello"});
        let err = SendMessageTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("target"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_missing_message_error() {
        let _guard = test_guard();
        clear_test_channel_directory();
        let ctx = test_ctx();
        let args = json!({"target": "telegram:123"});
        let err = SendMessageTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("message"));
    }
}
