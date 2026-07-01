use serde::{Deserialize, Serialize};
use serde_json::Value;

// ===== Gateway Payload =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayPayload {
    pub op: OpCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[repr(u8)]
#[serde(try_from = "u8", into = "u8")]
pub enum OpCode {
    Dispatch = 0,
    Heartbeat = 1,
    Identify = 2,
    Resume = 6,
    Reconnect = 7,
    InvalidSession = 9,
    Hello = 10,
    HeartbeatAck = 11,
    HttpCallbackAck = 13,
}

impl TryFrom<u8> for OpCode {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(OpCode::Dispatch),
            1 => Ok(OpCode::Heartbeat),
            2 => Ok(OpCode::Identify),
            6 => Ok(OpCode::Resume),
            7 => Ok(OpCode::Reconnect),
            9 => Ok(OpCode::InvalidSession),
            10 => Ok(OpCode::Hello),
            11 => Ok(OpCode::HeartbeatAck),
            13 => Ok(OpCode::HttpCallbackAck),
            _ => Err(format!("Unknown OpCode: {}", value)),
        }
    }
}

impl From<OpCode> for u8 {
    fn from(op: OpCode) -> Self {
        op as u8
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloPayload {
    pub heartbeat_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentifyPayload {
    pub token: String,
    pub intents: u32,
    pub shard: Option<[u32; 2]>,
    pub properties: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumePayload {
    pub token: String,
    pub session_id: String,
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyEvent {
    pub version: u32,
    pub session_id: String,
    pub user: BotUser,
    pub shard: Option<[u32; 2]>,
}

// ===== Bot User =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotUser {
    pub id: String,
    pub username: String,
    pub bot: bool,
}

// ===== Message Events =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageEvent {
    #[serde(rename = "MESSAGE_CREATE")]
    MessageCreate(Message),
    #[serde(rename = "AT_MESSAGE_CREATE")]
    AtMessageCreate(Message),
    #[serde(rename = "DIRECT_MESSAGE_CREATE")]
    DirectMessageCreate(Message),
    #[serde(rename = "C2C_MESSAGE_CREATE")]
    C2CMessageCreate(Message),
    #[serde(rename = "GROUP_AT_MESSAGE_CREATE")]
    GroupAtMessageCreate(Message),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub channel_id: Option<String>,
    pub guild_id: Option<String>,
    pub group_id: Option<String>,
    pub group_openid: Option<String>,
    pub author: Option<User>,
    pub content: String,
    pub timestamp: String,
    #[serde(default)]
    pub mention_everyone: bool,
    #[serde(default)]
    pub mentions: Vec<User>,
    #[serde(default)]
    pub attachments: Vec<MessageAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub bot: Option<bool>,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttachment {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

// ===== Intents =====

#[derive(Debug, Clone, Copy)]
pub struct Intents(pub u32);

impl Intents {
    pub const GUILDS: u32 = 1 << 0;
    pub const GUILD_MEMBERS: u32 = 1 << 1;
    pub const GUILD_MESSAGES: u32 = 1 << 9;
    pub const GUILD_MESSAGE_REACTIONS: u32 = 1 << 10;
    pub const DIRECT_MESSAGE: u32 = 1 << 12;
    pub const GROUP_AND_C2C_EVENT: u32 = 1 << 25;
    pub const INTERACTION: u32 = 1 << 26;
    pub const MESSAGE_AUDIT: u32 = 1 << 27;
    pub const AUDIO_OR_LIVE_CHANNEL_MEMBER: u32 = 1 << 19;

    pub fn new() -> Self {
        Self(0)
    }

    pub fn all_public() -> Self {
        Self(
            Self::GUILDS
                | Self::GUILD_MEMBERS
                | Self::GUILD_MESSAGES
                | Self::GUILD_MESSAGE_REACTIONS
                | Self::DIRECT_MESSAGE
                | Self::INTERACTION
                | Self::AUDIO_OR_LIVE_CHANNEL_MEMBER,
        )
    }

    pub fn default_messages() -> Self {
        Self(Self::GUILD_MESSAGES | Self::DIRECT_MESSAGE | Self::GROUP_AND_C2C_EVENT)
    }

    pub fn add(&mut self, intent: u32) {
        self.0 |= intent;
    }

    pub fn remove(&mut self, intent: u32) {
        self.0 &= !intent;
    }

    pub fn has(&self, intent: u32) -> bool {
        (self.0 & intent) == intent
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl Default for Intents {
    fn default() -> Self {
        Self::new()
    }
}

// ===== API Request/Response Models =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyboard: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<MessageMedia>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ark: Option<crate::message::ArkMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<crate::message::Embed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMedia {
    pub file_info: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub id: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: i32,
    pub message: String,
}
