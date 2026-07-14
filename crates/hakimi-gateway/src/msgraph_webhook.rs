//! Microsoft Graph webhook platform adapter.
//!
//! Receives Microsoft Graph change notifications, validates `clientState`,
//! filters optional resource allowlists, de-duplicates notification IDs, and
//! injects accepted notifications into the normal gateway message stream.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::{GatewayMessage, PlatformAdapter};

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_WEBHOOK_PATH: &str = "/msgraph/webhook";
const DEFAULT_HEALTH_PATH: &str = "/health";
const DEFAULT_MAX_SEEN_RECEIPTS: usize = 5_000;
const RENDERED_NOTIFICATION_LIMIT: usize = 4_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MSGraphWebhookAdapterConfig {
    /// Bot / role identifier for this instance.
    #[serde(default = "default_msgraph_webhook_bot_id")]
    pub bot_id: String,
    /// Host to bind. Network-accessible hosts require allowed_source_cidrs.
    #[serde(default = "default_host")]
    pub host: String,
    /// Listener port.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Microsoft Graph validation and notification path.
    #[serde(default = "default_webhook_path")]
    pub webhook_path: String,
    /// Health endpoint path.
    #[serde(default = "default_health_path")]
    pub health_path: String,
    /// Shared Graph subscription clientState secret.
    #[serde(default)]
    pub client_state: String,
    /// Accepted resource prefixes. Empty means accept any resource.
    #[serde(default)]
    pub accepted_resources: Vec<String>,
    /// Optional source CIDR allowlist for public binds.
    #[serde(default)]
    pub allowed_source_cidrs: Vec<String>,
    /// Maximum explicit notification IDs remembered for dedupe.
    #[serde(default = "default_max_seen_receipts")]
    pub max_seen_receipts: usize,
    /// Optional prompt template with {resource}, {change_type},
    /// {subscription_id}, and {notification.<field>} placeholders.
    #[serde(default)]
    pub prompt: String,
}

fn default_msgraph_webhook_bot_id() -> String {
    "msgraph_webhook".to_string()
}

fn default_host() -> String {
    DEFAULT_HOST.to_string()
}

fn default_port() -> u16 {
    8646
}

fn default_webhook_path() -> String {
    DEFAULT_WEBHOOK_PATH.to_string()
}

fn default_health_path() -> String {
    DEFAULT_HEALTH_PATH.to_string()
}

fn default_max_seen_receipts() -> usize {
    DEFAULT_MAX_SEEN_RECEIPTS
}

impl Default for MSGraphWebhookAdapterConfig {
    fn default() -> Self {
        Self {
            bot_id: default_msgraph_webhook_bot_id(),
            host: default_host(),
            port: default_port(),
            webhook_path: default_webhook_path(),
            health_path: default_health_path(),
            client_state: String::new(),
            accepted_resources: Vec::new(),
            allowed_source_cidrs: Vec::new(),
            max_seen_receipts: DEFAULT_MAX_SEEN_RECEIPTS,
            prompt: String::new(),
        }
    }
}

pub struct MSGraphWebhookAdapter {
    config: MSGraphWebhookAdapterConfig,
    bot_id: String,
    sender: mpsc::UnboundedSender<GatewayMessage>,
    receiver: Option<mpsc::UnboundedReceiver<GatewayMessage>>,
    server_handle: Option<JoinHandle<()>>,
}

impl MSGraphWebhookAdapter {
    pub fn new(mut config: MSGraphWebhookAdapterConfig) -> Self {
        config.webhook_path = normalize_path(&config.webhook_path);
        config.health_path = normalize_path(&config.health_path);
        config.host = if config.host.trim().is_empty() {
            DEFAULT_HOST.to_string()
        } else {
            config.host.trim().to_string()
        };
        let (sender, receiver) = mpsc::unbounded_channel();
        let bot_id = config.bot_id.clone();
        Self {
            config,
            bot_id,
            sender,
            receiver: Some(receiver),
            server_handle: None,
        }
    }
}

#[async_trait]
impl PlatformAdapter for MSGraphWebhookAdapter {
    fn name(&self) -> &str {
        "msgraph_webhook"
    }

    fn bot_id(&self) -> &str {
        &self.bot_id
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.config.client_state.trim().is_empty() {
            anyhow::bail!("MSGraph webhook requires client_state");
        }
        if bind_requires_source_allowlist(&self.config.host)
            && self.config.allowed_source_cidrs.is_empty()
        {
            anyhow::bail!(
                "MSGraph webhook binding to '{}' requires allowed_source_cidrs",
                self.config.host
            );
        }

        let state = Arc::new(MSGraphWebhookState::new(
            self.config.clone(),
            self.sender.clone(),
        )?);
        let app = Router::new()
            .route(&state.config.health_path, get(handle_health))
            .route(
                &state.config.webhook_path,
                get(handle_validation).post(handle_notification),
            )
            .with_state(state.clone());
        let bind_addr = format!("{}:{}", state.config.host, state.config.port);
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        let local_addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let result = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await;
            if let Err(err) = result {
                warn!(error = %err, "MSGraph webhook listener stopped with error");
            }
        });
        self.server_handle = Some(handle);
        info!(
            addr = %local_addr,
            path = %self.config.webhook_path,
            "MSGraph webhook adapter connected"
        );
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        info!(
            chat_id,
            text_len = text.len(),
            "MSGraph webhook response observed"
        );
        Ok(())
    }

    fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<GatewayMessage>> {
        self.receiver.take()
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
        }
        info!("MSGraph webhook adapter disconnected");
        Ok(())
    }
}

struct MSGraphWebhookState {
    config: MSGraphWebhookAdapterConfig,
    sender: mpsc::UnboundedSender<GatewayMessage>,
    seen: Mutex<SeenReceipts>,
    allowed_networks: Vec<IpNetwork>,
}

impl MSGraphWebhookState {
    fn new(
        config: MSGraphWebhookAdapterConfig,
        sender: mpsc::UnboundedSender<GatewayMessage>,
    ) -> anyhow::Result<Self> {
        let allowed_networks = parse_allowed_networks(&config.allowed_source_cidrs)?;
        Ok(Self {
            config,
            sender,
            seen: Mutex::new(SeenReceipts::default()),
            allowed_networks,
        })
    }

    fn source_allowed(&self, peer: SocketAddr) -> bool {
        if self.allowed_networks.is_empty() {
            return !bind_requires_source_allowlist(&self.config.host);
        }
        self.allowed_networks
            .iter()
            .any(|network| network.contains(peer.ip()))
    }

    fn resource_accepted(&self, resource: &str) -> bool {
        if self.config.accepted_resources.is_empty() {
            return true;
        }
        let resource = normalize_resource(resource);
        self.config.accepted_resources.iter().any(|pattern| {
            let pattern = normalize_resource(pattern);
            if pattern.is_empty() {
                return false;
            }
            if let Some(prefix) = pattern.strip_suffix('*') {
                let prefix = prefix.trim_end_matches('/');
                return resource == prefix || resource.starts_with(&format!("{prefix}/"));
            }
            resource == pattern || resource.starts_with(&format!("{pattern}/"))
        })
    }

    fn verify_client_state(&self, notification: &Value) -> bool {
        let Some(provided) = notification
            .get("clientState")
            .and_then(Value::as_str)
            .map(str::trim)
        else {
            return false;
        };
        constant_time_eq(
            provided.as_bytes(),
            self.config.client_state.trim().as_bytes(),
        )
    }

    fn build_message(&self, notification: &Value) -> GatewayMessage {
        let subscription_id = notification
            .get("subscriptionId")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        GatewayMessage {
            platform: "msgraph_webhook".to_string(),
            bot_id: self.config.bot_id.clone(),
            chat_id: format!("msgraph:{subscription_id}"),
            user_id: "msgraph".to_string(),
            text: render_prompt(notification, &self.config.prompt),
            media: None,
            callback_data: None,
                reply_to_message_id: None,
                reply_to_text: None,
            }
    }
}

#[derive(Default)]
struct SeenReceipts {
    ids: HashSet<String>,
    order: VecDeque<String>,
    accepted_count: usize,
    duplicate_count: usize,
}

impl SeenReceipts {
    fn has_seen(&self, key: &str) -> bool {
        self.ids.contains(key)
    }

    fn remember(&mut self, key: String, max_seen: usize) {
        self.ids.insert(key.clone());
        self.order.push_back(key);
        let max_seen = max_seen.max(1);
        while self.order.len() > max_seen {
            if let Some(oldest) = self.order.pop_front() {
                self.ids.remove(&oldest);
            }
        }
    }
}

async fn handle_health(
    State(state): State<Arc<MSGraphWebhookState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !state.source_allowed(peer) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let seen = state.seen.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "platform": "msgraph_webhook",
        "webhook_path": &state.config.webhook_path,
        "accepted": seen.accepted_count,
        "duplicates": seen.duplicate_count,
    }))
    .into_response()
}

async fn handle_validation(
    State(state): State<Arc<MSGraphWebhookState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if !state.source_allowed(peer) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(token) = query
        .get("validationToken")
        .filter(|value| !value.is_empty())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    ([(header::CONTENT_TYPE, "text/plain")], token.clone()).into_response()
}

async fn handle_notification(
    State(state): State<Arc<MSGraphWebhookState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    if !state.source_allowed(peer) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(token) = query
        .get("validationToken")
        .filter(|value| !value.is_empty())
    {
        return ([(header::CONTENT_TYPE, "text/plain")], token.clone()).into_response();
    }

    let Ok(body) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(notifications) = body.get("value").and_then(Value::as_array) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let mut accepted = 0usize;
    let mut duplicates = 0usize;
    let mut auth_rejected = 0usize;
    let mut other_rejected = 0usize;

    for notification in notifications {
        let resource = notification
            .get("resource")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !state.resource_accepted(resource) {
            other_rejected += 1;
            continue;
        }
        if !state.verify_client_state(notification) {
            auth_rejected += 1;
            continue;
        }

        let receipt_key = notification
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| format!("id:{id}"));

        if let Some(key) = receipt_key {
            let mut seen = state.seen.lock().await;
            if seen.has_seen(&key) {
                duplicates += 1;
                seen.duplicate_count += 1;
                continue;
            }
            seen.remember(key, state.config.max_seen_receipts);
        }

        let _ = state.sender.send(state.build_message(notification));
        accepted += 1;
    }

    if accepted > 0 {
        let mut seen = state.seen.lock().await;
        seen.accepted_count += accepted;
    }

    if accepted > 0 || duplicates > 0 {
        return StatusCode::ACCEPTED.into_response();
    }
    if auth_rejected > 0 && other_rejected == 0 {
        return StatusCode::FORBIDDEN.into_response();
    }
    StatusCode::BAD_REQUEST.into_response()
}

fn render_prompt(notification: &Value, template: &str) -> String {
    let template = template.trim();
    if template.is_empty() {
        let rendered = serde_json::to_string_pretty(notification).unwrap_or_else(|_| "{}".into());
        return format!(
            "Microsoft Graph change notification:\n\n```json\n{}\n```",
            truncate_chars(&rendered, RENDERED_NOTIFICATION_LIMIT)
        );
    }

    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let (before, after_start) = rest.split_at(start);
        output.push_str(before);
        if let Some(end) = after_start.find('}') {
            let key = &after_start[1..end];
            output.push_str(&resolve_template_key(notification, key));
            rest = &after_start[end + 1..];
        } else {
            output.push_str(after_start);
            rest = "";
        }
    }
    output.push_str(rest);
    output
}

fn resolve_template_key(notification: &Value, key: &str) -> String {
    match key {
        "resource" => notification_value(notification, "resource"),
        "change_type" => notification_value(notification, "changeType"),
        "subscription_id" => notification_value(notification, "subscriptionId"),
        key if key.starts_with("notification.") => {
            let mut value = notification;
            for part in key.trim_start_matches("notification.").split('.') {
                let Some(next) = value.get(part) else {
                    return format!("{{{key}}}");
                };
                value = next;
            }
            value_to_template_string(value)
        }
        _ => format!("{{{key}}}"),
    }
}

fn notification_value(notification: &Value, key: &str) -> String {
    notification
        .get(key)
        .map(value_to_template_string)
        .unwrap_or_default()
}

fn value_to_template_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn normalize_resource(resource: &str) -> String {
    resource.trim().trim_matches('/').to_string()
}

fn normalize_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return "/".to_string();
    }
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn bind_requires_source_allowlist(host: &str) -> bool {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(addr)) => !(addr.is_loopback()),
        Ok(IpAddr::V6(addr)) => !(addr.is_loopback()),
        Err(_) => true,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let max_len = a.len().max(b.len());
    let mut diff = a.len() ^ b.len();
    for i in 0..max_len {
        diff |= usize::from(*a.get(i).unwrap_or(&0) ^ *b.get(i).unwrap_or(&0));
    }
    diff == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IpNetwork {
    V4(u32, u8),
    V6(u128, u8),
}

impl IpNetwork {
    fn contains(&self, ip: IpAddr) -> bool {
        match (self, ip) {
            (IpNetwork::V4(network, prefix), IpAddr::V4(addr)) => {
                let addr = u32::from(addr);
                let mask = prefix_mask_v4(*prefix);
                (addr & mask) == (*network & mask)
            }
            (IpNetwork::V6(network, prefix), IpAddr::V6(addr)) => {
                let addr = u128::from(addr);
                let mask = prefix_mask_v6(*prefix);
                (addr & mask) == (*network & mask)
            }
            _ => false,
        }
    }
}

fn parse_allowed_networks(raw: &[String]) -> anyhow::Result<Vec<IpNetwork>> {
    raw.iter()
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| parse_ip_network(entry))
        .collect()
}

fn parse_ip_network(value: &str) -> anyhow::Result<IpNetwork> {
    let value = value.trim();
    let (addr, prefix) = match value.split_once('/') {
        Some((addr, prefix)) => (addr, Some(prefix)),
        None => (value, None),
    };
    let ip: IpAddr = addr.parse()?;
    match ip {
        IpAddr::V4(addr) => {
            let prefix = parse_prefix(prefix, 32)?;
            Ok(IpNetwork::V4(u32::from(addr), prefix))
        }
        IpAddr::V6(addr) => {
            let prefix = parse_prefix(prefix, 128)?;
            Ok(IpNetwork::V6(u128::from(addr), prefix))
        }
    }
}

fn parse_prefix(value: Option<&str>, max: u8) -> anyhow::Result<u8> {
    let Some(value) = value else {
        return Ok(max);
    };
    let prefix = value.parse::<u8>()?;
    if prefix > max {
        anyhow::bail!("CIDR prefix {prefix} exceeds {max}");
    }
    Ok(prefix)
}

fn prefix_mask_v4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

fn prefix_mask_v6(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformAdapter;

    fn make_config() -> MSGraphWebhookAdapterConfig {
        MSGraphWebhookAdapterConfig {
            bot_id: "msgraph".into(),
            host: "127.0.0.1".into(),
            port: 8646,
            webhook_path: "/msgraph/webhook".into(),
            health_path: "/health".into(),
            client_state: "secret-state".into(),
            accepted_resources: vec!["users/123/messages".into()],
            allowed_source_cidrs: Vec::new(),
            max_seen_receipts: 2,
            prompt: String::new(),
        }
    }

    #[test]
    fn construction_sets_identity() {
        let mut config = make_config();
        config.webhook_path = "msgraph/webhook".into();
        let adapter = MSGraphWebhookAdapter::new(config);
        assert_eq!(adapter.name(), "msgraph_webhook");
        assert_eq!(adapter.bot_id(), "msgraph");
        assert_eq!(adapter.config.webhook_path, "/msgraph/webhook");
    }

    #[test]
    fn public_binds_require_source_allowlist() {
        assert!(bind_requires_source_allowlist("0.0.0.0"));
        assert!(bind_requires_source_allowlist("::"));
        assert!(bind_requires_source_allowlist("example.com"));
        assert!(!bind_requires_source_allowlist("127.0.0.1"));
        assert!(!bind_requires_source_allowlist("::1"));
        assert!(!bind_requires_source_allowlist("localhost"));
    }

    #[test]
    fn resource_allowlist_accepts_prefixes_and_wildcards() {
        let (sender, _) = mpsc::unbounded_channel();
        let state = MSGraphWebhookState::new(make_config(), sender).unwrap();
        assert!(state.resource_accepted("users/123/messages/AAMk"));
        assert!(!state.resource_accepted("users/456/messages"));

        let mut config = make_config();
        config.accepted_resources = vec!["users/*".into()];
        let (sender, _) = mpsc::unbounded_channel();
        let state = MSGraphWebhookState::new(config, sender).unwrap();
        assert!(state.resource_accepted("users/456/events"));
    }

    #[test]
    fn client_state_compare_is_exact() {
        let (sender, _) = mpsc::unbounded_channel();
        let state = MSGraphWebhookState::new(make_config(), sender).unwrap();
        assert!(state.verify_client_state(&serde_json::json!({
            "clientState": "secret-state"
        })));
        assert!(!state.verify_client_state(&serde_json::json!({
            "clientState": "secret-stat"
        })));
    }

    #[test]
    fn render_default_prompt_truncates_large_notification() {
        let notification = serde_json::json!({
            "resource": "users/123/messages/abc",
            "payload": "x".repeat(RENDERED_NOTIFICATION_LIMIT + 100),
        });
        let rendered = render_prompt(&notification, "");
        assert!(rendered.starts_with("Microsoft Graph change notification:"));
        assert!(rendered.chars().count() < RENDERED_NOTIFICATION_LIMIT + 200);
    }

    #[test]
    fn render_template_resolves_graph_fields() {
        let notification = serde_json::json!({
            "resource": "users/123/messages/abc",
            "changeType": "created",
            "subscriptionId": "sub-1",
            "nested": { "id": 42 },
        });
        let rendered = render_prompt(
            &notification,
            "Graph {change_type} {resource} {subscription_id} {notification.nested.id}",
        );
        assert_eq!(rendered, "Graph created users/123/messages/abc sub-1 42");
    }

    #[test]
    fn seen_receipts_evicts_old_entries() {
        let mut seen = SeenReceipts::default();
        seen.remember("id:1".into(), 2);
        seen.remember("id:2".into(), 2);
        seen.remember("id:3".into(), 2);
        assert!(!seen.has_seen("id:1"));
        assert!(seen.has_seen("id:2"));
        assert!(seen.has_seen("id:3"));
    }

    #[test]
    fn cidr_parser_matches_ipv4_and_ipv6() {
        let v4 = parse_ip_network("192.168.1.0/24").unwrap();
        assert!(v4.contains("192.168.1.42".parse().unwrap()));
        assert!(!v4.contains("192.168.2.1".parse().unwrap()));

        let v6 = parse_ip_network("2001:db8::/32").unwrap();
        assert!(v6.contains("2001:db8::1".parse().unwrap()));
        assert!(!v6.contains("2001:db9::1".parse().unwrap()));
    }
}
