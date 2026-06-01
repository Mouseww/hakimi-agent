//! Email / SMTP platform adapter.
//!
//! Hermes supports both IMAP inbound polling and SMTP outbound replies. This
//! Rust-native slice covers outbound SMTP delivery for gateway routing,
//! cron, and `send_message` without introducing an inbound mailbox worker.

use async_trait::async_trait;
use lettre::message::{Mailbox, Message, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{GatewayMessage, PlatformAdapter};

const DEFAULT_SMTP_PORT: u16 = 587;
const MAX_EMAIL_CHARS: usize = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_email_bot_id")]
    pub bot_id: String,
    /// SMTP server hostname.
    pub smtp_host: String,
    /// SMTP submission port. Defaults to 587 / STARTTLS.
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    /// Email address used in the From header and default SMTP username.
    pub address: String,
    /// SMTP password or app-specific password.
    pub password: String,
    /// Optional SMTP username when it differs from `address`.
    #[serde(default)]
    pub username: String,
    /// Optional default recipient for bare `email` sends and cron delivery.
    #[serde(default)]
    pub home_channel: String,
    /// Optional default subject for new outbound threads.
    #[serde(default = "default_subject")]
    pub subject: String,
}

fn default_email_bot_id() -> String {
    "email".to_string()
}

fn default_smtp_port() -> u16 {
    DEFAULT_SMTP_PORT
}

fn default_subject() -> String {
    "Hakimi Agent".to_string()
}

pub struct EmailAdapter {
    config: EmailAdapterConfig,
    bot_id: String,
    mailer: AsyncSmtpTransport<Tokio1Executor>,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
}

impl EmailAdapter {
    pub fn new(config: EmailAdapterConfig) -> anyhow::Result<Self> {
        let credentials = Credentials::new(smtp_username(&config), config.password.clone());
        let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(config.smtp_host.trim())?
            .port(config.smtp_port)
            .credentials(credentials)
            .build();
        let (_, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Ok(Self {
            config,
            bot_id,
            mailer,
            receiver: Some(receiver),
        })
    }

    fn recipient<'a>(&'a self, chat_id: &'a str) -> &'a str {
        let chat_id = chat_id.trim();
        if chat_id.is_empty() {
            self.config.home_channel.trim()
        } else {
            chat_id
        }
    }

    fn subject(&self) -> &str {
        let subject = self.config.subject.trim();
        if subject.is_empty() {
            "Hakimi Agent"
        } else {
            subject
        }
    }

    fn build_message(&self, to: &str, body: &str) -> anyhow::Result<Message> {
        let from: Mailbox = self.config.address.trim().parse()?;
        let to: Mailbox = to.trim().parse()?;
        Ok(Message::builder()
            .from(from)
            .to(to)
            .subject(self.subject())
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())?)
    }
}

#[async_trait]
impl PlatformAdapter for EmailAdapter {
    fn name(&self) -> &str {
        "email"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.config.smtp_host.trim().is_empty()
            || self.config.address.trim().is_empty()
            || self.config.password.trim().is_empty()
        {
            anyhow::bail!("Email gateway requires smtp_host, address, and password");
        }
        if !looks_like_email(&self.config.address) {
            anyhow::bail!("Email gateway address must be an email address");
        }
        info!(
            address = %redact_email(&self.config.address),
            smtp_host = %self.config.smtp_host,
            "Email adapter connected"
        );
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let recipient = self.recipient(chat_id);
        if recipient.is_empty() {
            anyhow::bail!("Email send_message requires a recipient email address");
        }
        if !looks_like_email(recipient) {
            anyhow::bail!(
                "Email send_message requires an email recipient, got '{}'",
                redact_email(recipient)
            );
        }

        for chunk in email_chunks(text) {
            let message = self.build_message(recipient, &chunk)?;
            self.mailer.send(message).await?;
        }

        info!(
            to = %redact_email(recipient),
            text_len = text.len(),
            "Email: message sent"
        );
        Ok(())
    }

    fn max_message_chars(&self) -> Option<usize> {
        Some(MAX_EMAIL_CHARS)
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        info!("Email adapter disconnected");
        Ok(())
    }
}

fn smtp_username(config: &EmailAdapterConfig) -> String {
    let username = config.username.trim();
    if username.is_empty() {
        config.address.trim().to_string()
    } else {
        username.to_string()
    }
}

fn looks_like_email(value: &str) -> bool {
    let value = value.trim();
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.trim().is_empty() && domain.contains('.') && !domain.trim().ends_with('.')
}

fn email_chunks(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0;
    for ch in text.chars() {
        if current_chars >= MAX_EMAIL_CHARS {
            chunks.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current.push(ch);
        current_chars += 1;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn redact_email(value: &str) -> String {
    let value = value.trim();
    let Some((local, domain)) = value.split_once('@') else {
        return "***".to_string();
    };
    let visible: String = local.chars().take(2).collect();
    format!("{visible}***@{domain}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> EmailAdapterConfig {
        EmailAdapterConfig {
            bot_id: "email".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            address: "agent@example.com".into(),
            password: "app-password".into(),
            username: String::new(),
            home_channel: "owner@example.com".into(),
            subject: "Hakimi Agent".into(),
        }
    }

    #[test]
    fn construction_sets_platform_identity() {
        let adapter = EmailAdapter::new(make_config()).unwrap();
        assert_eq!(adapter.name(), "email");
        assert_eq!(adapter.bot_id(), "email");
    }

    #[test]
    fn recipient_falls_back_to_home_channel() {
        let adapter = EmailAdapter::new(make_config()).unwrap();
        assert_eq!(adapter.recipient(""), "owner@example.com");
        assert_eq!(
            adapter.recipient("person@example.com"),
            "person@example.com"
        );
    }

    #[test]
    fn smtp_username_defaults_to_address() {
        let config = make_config();
        assert_eq!(smtp_username(&config), "agent@example.com");
    }

    #[test]
    fn smtp_username_accepts_override() {
        let mut config = make_config();
        config.username = "agent-login".into();
        assert_eq!(smtp_username(&config), "agent-login");
    }

    #[test]
    fn email_chunks_are_utf8_safe() {
        let input = "好".repeat(MAX_EMAIL_CHARS + 1);
        let chunks = email_chunks(&input);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), MAX_EMAIL_CHARS);
        assert_eq!(chunks[1], "好");
    }

    #[test]
    fn validates_basic_email_shape() {
        assert!(looks_like_email("person@example.com"));
        assert!(!looks_like_email("person"));
        assert!(!looks_like_email("@example.com"));
        assert!(!looks_like_email("person@example"));
    }

    #[test]
    fn redacts_email_addresses() {
        assert_eq!(redact_email("person@example.com"), "pe***@example.com");
        assert_eq!(redact_email("invalid"), "***");
    }

    #[test]
    fn take_receiver_once() {
        let mut adapter = EmailAdapter::new(make_config()).unwrap();
        assert!(adapter.take_receiver().is_some());
        assert!(adapter.take_receiver().is_none());
    }
}
