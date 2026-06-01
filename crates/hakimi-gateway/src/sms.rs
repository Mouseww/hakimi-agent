//! SMS / Twilio platform adapter.
//!
//! Sends outbound SMS through Twilio's REST API. Inbound Twilio webhook handling
//! remains intentionally outside this first Rust-native slice so the adapter
//! stays dependency-light and matches Hakimi's existing outbound gateway model.

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

const TWILIO_API_BASE: &str = "https://api.twilio.com/2010-04-01/Accounts";
const MAX_SMS_CHARS: usize = 1600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_sms_bot_id")]
    pub bot_id: String,
    /// Twilio account SID, usually `AC...`.
    pub account_sid: String,
    /// Twilio auth token.
    pub auth_token: String,
    /// E.164 sender phone number.
    pub from_number: String,
    /// Optional default recipient for bare `sms` sends and cron delivery.
    #[serde(default)]
    pub home_channel: String,
    /// Optional API base URL override for tests.
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_sms_bot_id() -> String {
    "sms".to_string()
}

pub struct SmsAdapter {
    config: SmsAdapterConfig,
    bot_id: String,
    client: Client,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl SmsAdapter {
    pub fn new(config: SmsAdapterConfig) -> Self {
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            client: Client::new(),
            receiver: Some(receiver),
        }
    }

    fn api_base(&self) -> String {
        self.config
            .base_url
            .as_deref()
            .unwrap_or(TWILIO_API_BASE)
            .trim_end_matches('/')
            .to_string()
    }

    fn messages_url(&self) -> String {
        format!(
            "{}/{}/Messages.json",
            self.api_base(),
            self.config.account_sid
        )
    }

    fn auth_value(&self) -> String {
        let credentials = format!("{}:{}", self.config.account_sid, self.config.auth_token);
        format!("Basic {}", STANDARD.encode(credentials))
    }

    fn recipient<'a>(&'a self, chat_id: &'a str) -> &'a str {
        let chat_id = chat_id.trim();
        if chat_id.is_empty() {
            self.config.home_channel.trim()
        } else {
            chat_id
        }
    }
}

#[async_trait]
impl PlatformAdapter for SmsAdapter {
    fn name(&self) -> &str {
        "sms"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.config.account_sid.trim().is_empty()
            || self.config.auth_token.trim().is_empty()
            || self.config.from_number.trim().is_empty()
        {
            anyhow::bail!("SMS gateway requires account_sid, auth_token, and from_number");
        }
        info!("SMS adapter connected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let to_number = self.recipient(chat_id);
        if to_number.is_empty() {
            anyhow::bail!("SMS send_message requires a recipient phone number");
        }

        let url = self.messages_url();
        let auth = self.auth_value();
        let body = strip_sms_markdown(text);
        for chunk in sms_chunks(&body) {
            let form = [
                ("From", self.config.from_number.trim()),
                ("To", to_number),
                ("Body", chunk.as_str()),
            ];
            let resp = self
                .client
                .post(&url)
                .header("Authorization", auth.clone())
                .form(&form)
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                anyhow::bail!(
                    "SMS send_message failed: status={}, body={}",
                    status,
                    body_text
                );
            }
        }

        info!(
            to = %redact_phone(to_number),
            text_len = text.len(),
            "SMS: message sent"
        );
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("SMS adapter disconnected");
        Ok(())
    }
}

fn sms_chunks(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() >= MAX_SMS_CHARS {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn strip_sms_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    let mut in_code_block = false;

    while let Some(ch) = chars.next() {
        if ch == '`' && chars.peek() == Some(&'`') {
            chars.next();
            if chars.peek() == Some(&'`') {
                chars.next();
                in_code_block = !in_code_block;
                continue;
            }
            out.push(ch);
            out.push('`');
            continue;
        }
        if matches!(ch, '*' | '_' | '`' | '~' | '#') && !in_code_block {
            continue;
        }
        out.push(ch);
    }

    collapse_blank_lines(out.trim())
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::new();
    let mut blank_count = 0usize;

    for line in text.lines() {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                out.push('\n');
            }
            continue;
        }
        blank_count = 0;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(line.trim());
    }

    out.trim().to_string()
}

fn redact_phone(phone: &str) -> String {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() <= 4 {
        return "***".to_string();
    }
    let tail: String = digits
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("***{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> SmsAdapterConfig {
        SmsAdapterConfig {
            bot_id: "sms".into(),
            account_sid: "ACtest123".into(),
            auth_token: "token_secret".into(),
            from_number: "+15550001111".into(),
            home_channel: "+15552223333".into(),
            base_url: None,
        }
    }

    #[test]
    fn test_construction() {
        let adapter = SmsAdapter::new(make_config());
        assert_eq!(adapter.name(), "sms");
        assert_eq!(adapter.bot_id(), "sms");
    }

    #[test]
    fn test_messages_url_uses_account_sid() {
        let adapter = SmsAdapter::new(make_config());
        assert_eq!(
            adapter.messages_url(),
            "https://api.twilio.com/2010-04-01/Accounts/ACtest123/Messages.json"
        );
    }

    #[test]
    fn test_base_url_override() {
        let mut config = make_config();
        config.base_url = Some("https://twilio.test/api/".into());
        let adapter = SmsAdapter::new(config);
        assert_eq!(
            adapter.messages_url(),
            "https://twilio.test/api/ACtest123/Messages.json"
        );
    }

    #[test]
    fn test_auth_value_is_basic() {
        let adapter = SmsAdapter::new(make_config());
        assert_eq!(
            adapter.auth_value(),
            "Basic QUN0ZXN0MTIzOnRva2VuX3NlY3JldA=="
        );
    }

    #[test]
    fn test_home_channel_fallback() {
        let adapter = SmsAdapter::new(make_config());
        assert_eq!(adapter.recipient(""), "+15552223333");
        assert_eq!(adapter.recipient("+15554445555"), "+15554445555");
    }

    #[test]
    fn test_strip_sms_markdown() {
        assert_eq!(strip_sms_markdown("## **Hello** `world`"), "Hello world");
        assert_eq!(strip_sms_markdown("a\n\n\nb"), "a\nb");
    }

    #[test]
    fn test_sms_chunks_are_char_safe() {
        let input = "好".repeat(MAX_SMS_CHARS + 1);
        let chunks = sms_chunks(&input);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), MAX_SMS_CHARS);
        assert_eq!(chunks[1], "好");
    }

    #[test]
    fn test_take_receiver_once() {
        let mut adapter = SmsAdapter::new(make_config());
        assert!(adapter.take_receiver().is_some());
        assert!(adapter.take_receiver().is_none());
    }
}
