use crate::auth::TokenManager;
use crate::error::{Error, Result};
use crate::model::*;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{interval, sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use tracing::{debug, error, info, warn};

const GATEWAY_URL: &str = "wss://api.sgroup.qq.com/websocket";

pub type EventSender = mpsc::UnboundedSender<GatewayEvent>;
pub type EventReceiver = mpsc::UnboundedReceiver<GatewayEvent>;

#[derive(Debug, Clone)]
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

#[async_trait]
pub trait EventHandler: Send + Sync {
    async fn on_ready(&self, _ready: &ReadyEvent) {}
    async fn on_message(&self, _message: &Message) {}
    async fn on_at_message(&self, _message: &Message) {}
    async fn on_direct_message(&self, _message: &Message) {}
    async fn on_c2c_message(&self, _message: &Message) {}
    async fn on_group_at_message(&self, _message: &Message) {}
}

#[derive(Clone)]
pub struct Gateway {
    token_manager: Arc<TokenManager>,
    intents: Intents,
    session_state: Arc<RwLock<SessionState>>,
    event_tx: EventSender,
}

#[derive(Debug, Clone)]
#[derive(Default)]
struct SessionState {
    session_id: Option<String>,
    seq: u64,
    should_resume: bool,
}


impl Gateway {
    pub fn new(token_manager: Arc<TokenManager>, intents: Intents) -> (Self, EventReceiver) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let gateway = Self {
            token_manager,
            intents,
            session_state: Arc::new(RwLock::new(SessionState::default())),
            event_tx,
        };

        (gateway, event_rx)
    }

    pub async fn connect(&self) -> Result<()> {
        let mut reconnect_delay = Duration::from_secs(1);
        let max_reconnect_delay = Duration::from_secs(60);

        loop {
            match self.connect_once().await {
                Ok(_) => {
                    info!("Gateway connection closed normally");
                    reconnect_delay = Duration::from_secs(1);
                }
                Err(Error::ReconnectRequired) => {
                    warn!("Gateway requested reconnect");
                    self.session_state.write().should_resume = true;
                }
                Err(e) => {
                    error!("Gateway error: {}", e);
                    self.session_state.write().should_resume = false;
                }
            }

            let _ = self.event_tx.send(GatewayEvent::Disconnected);

            info!("Reconnecting in {:?}", reconnect_delay);
            sleep(reconnect_delay).await;
            reconnect_delay = (reconnect_delay * 2).min(max_reconnect_delay);
        }
    }

    async fn connect_once(&self) -> Result<()> {
        let token = self.token_manager.get_token().await?;

        info!("Connecting to gateway: {}", GATEWAY_URL);
        let (ws_stream, _) = connect_async(GATEWAY_URL)
            .await
            .map_err(|e| Error::WebSocket(e.to_string()))?;

        let (mut write, mut read) = ws_stream.split();

        // 等待 Hello
        let hello_msg = read
            .next()
            .await
            .ok_or(Error::ConnectionClosed)?
            .map_err(|e| Error::WebSocket(e.to_string()))?;

        let hello_payload: GatewayPayload = match &hello_msg {
            WsMessage::Text(text) => {
                info!("📨 Received WS message: {}", text);
                serde_json::from_str(text)?
            }
            other => {
                warn!("⚠️  Received non-text WS message: {:?}", other);
                return Err(Error::InvalidPayload("Expected text message".to_string()));
            }
        };

        if hello_payload.op != OpCode::Hello {
            return Err(Error::InvalidPayload("Expected Hello opcode".to_string()));
        }

        let hello: HelloPayload = serde_json::from_value(
            hello_payload
                .d
                .ok_or(Error::InvalidPayload("Missing Hello data".to_string()))?,
        )?;

        info!(
            "Received Hello, heartbeat_interval: {}ms",
            hello.heartbeat_interval
        );

        // 发送 Identify 或 Resume
        let state = self.session_state.read().clone();
        if let (true, Some(session_id)) = (state.should_resume, state.session_id) {
            info!("Resuming session: {:?}", session_id);
            let resume_payload = GatewayPayload {
                op: OpCode::Resume,
                d: Some(json!(ResumePayload {
                    token: format!("QQBot {}", token),
                    session_id,
                    seq: state.seq,
                })),
                s: None,
                t: None,
            };
            write
                .send(WsMessage::Text(serde_json::to_string(&resume_payload)?))
                .await
                .map_err(|e| Error::WebSocket(e.to_string()))?;
        } else {
            info!("Identifying with intents: {}", self.intents.value());
            let identify_payload = GatewayPayload {
                op: OpCode::Identify,
                d: Some(json!(IdentifyPayload {
                    token: format!("QQBot {}", token),
                    intents: self.intents.value(),
                    shard: None,
                    properties: None,
                })),
                s: None,
                t: None,
            };
            write
                .send(WsMessage::Text(serde_json::to_string(&identify_payload)?))
                .await
                .map_err(|e| Error::WebSocket(e.to_string()))?;
        }

        // 启动心跳任务
        let heartbeat_interval = Duration::from_millis(hello.heartbeat_interval);
        let (heartbeat_tx, mut heartbeat_rx) = mpsc::unbounded_channel::<()>();
        let heartbeat_task = tokio::spawn({
            let mut write = write;
            async move {
                let mut ticker = interval(heartbeat_interval);
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            let heartbeat = GatewayPayload {
                                op: OpCode::Heartbeat,
                                d: None,
                                s: None,
                                t: None,
                            };
                            if let Ok(json) = serde_json::to_string(&heartbeat) {
                                if write.send(WsMessage::Text(json)).await.is_err() {
                                    break;
                                }
                                debug!("Sent heartbeat");
                            }
                        }
                        msg = heartbeat_rx.recv() => {
                            if msg.is_none() {
                                break;
                            }
                        }
                    }
                }
                write
            }
        });

        // 处理消息
        while let Some(msg) = read.next().await {
            let msg = msg.map_err(|e| Error::WebSocket(e.to_string()))?;

            match msg {
                WsMessage::Text(text) => match self.handle_payload(&text).await {
                    Ok(should_continue) => {
                        if !should_continue {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error handling payload: {}", e);
                    }
                },
                WsMessage::Close(_) => {
                    info!("Received close frame");
                    break;
                }
                WsMessage::Ping(_data) => {
                    debug!("Received ping, sending pong");
                    // tokio-tungstenite 自动处理 pong
                }
                _ => {}
            }
        }

        drop(heartbeat_tx);
        let _ = heartbeat_task.await;

        Ok(())
    }

    async fn handle_payload(&self, text: &str) -> Result<bool> {
        let payload: GatewayPayload = serde_json::from_str(text)?;

        // 更新序列号
        if let Some(s) = payload.s {
            self.session_state.write().seq = s;
        }

        match payload.op {
            OpCode::Dispatch => {
                if let Some(event_type) = payload.t.as_deref() {
                    self.handle_dispatch_event(event_type, payload.d).await?;
                }
            }
            OpCode::HeartbeatAck => {
                debug!("Received heartbeat ack");
            }
            OpCode::Reconnect => {
                info!("Server requested reconnect");
                return Err(Error::ReconnectRequired);
            }
            OpCode::InvalidSession => {
                warn!("Invalid session, will start fresh");
                self.session_state.write().should_resume = false;
                return Err(Error::ReconnectRequired);
            }
            _ => {
                debug!("Unhandled opcode: {:?}", payload.op);
            }
        }

        Ok(true)
    }

    async fn handle_dispatch_event(
        &self,
        event_type: &str,
        data: Option<serde_json::Value>,
    ) -> Result<()> {
        let data = data.ok_or_else(|| Error::InvalidPayload("Missing event data".to_string()))?;

        match event_type {
            "READY" => {
                let ready: ReadyEvent = serde_json::from_value(data)?;
                info!(
                    "Bot ready: {} (session: {})",
                    ready.user.username, ready.session_id
                );

                self.session_state.write().session_id = Some(ready.session_id.clone());
                self.session_state.write().should_resume = true;

                let _ = self.event_tx.send(GatewayEvent::Ready(ready));
            }
            "MESSAGE_CREATE" => {
                let msg: Message = serde_json::from_value(data)?;
                let _ = self.event_tx.send(GatewayEvent::MessageCreate(msg));
            }
            "AT_MESSAGE_CREATE" => {
                let msg: Message = serde_json::from_value(data)?;
                let _ = self.event_tx.send(GatewayEvent::AtMessageCreate(msg));
            }
            "DIRECT_MESSAGE_CREATE" => {
                let msg: Message = serde_json::from_value(data)?;
                let _ = self.event_tx.send(GatewayEvent::DirectMessageCreate(msg));
            }
            "C2C_MESSAGE_CREATE" => {
                let msg: Message = serde_json::from_value(data)?;
                let _ = self.event_tx.send(GatewayEvent::C2CMessageCreate(msg));
            }
            "GROUP_AT_MESSAGE_CREATE" => {
                let msg: Message = serde_json::from_value(data)?;
                let _ = self.event_tx.send(GatewayEvent::GroupAtMessageCreate(msg));
            }
            _ => {
                debug!("Unhandled event type: {}", event_type);
            }
        }

        Ok(())
    }
}
