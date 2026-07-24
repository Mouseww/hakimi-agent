//! Hub worker client — connect a local StudioRuntime to a pure-relay hub.
//!
//! Flow:
//! 1. WS connect to hub `/v1/studio`
//! 2. `hello` as `DeviceKind::Server` (is_runner)
//! 3. On `worker_dispatch` → `runtime.handle_command_as(actor, cmd)`
//! 4. Publish returned envelopes via `worker_publish`

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use hakimi_studio_api::{
    DeviceKind, PROTOCOL_VERSION, StudioCommand, StudioEvent, StudioEventEnvelope, StudioRuntime,
    WorkerDispatch,
};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct HubWorkerConfig {
    /// e.g. `ws://127.0.0.1:3010/v1/studio`
    pub hub_url: String,
    pub device_id: String,
    pub device_name: Option<String>,
    pub token: Option<String>,
    /// Reconnect backoff base.
    pub reconnect_secs: u64,
}

impl HubWorkerConfig {
    pub fn from_env() -> Option<Self> {
        let hub_url = std::env::var("HAKIMI_HUB_URL").ok().filter(|s| !s.is_empty())?;
        let device_id = std::env::var("HAKIMI_HUB_DEVICE_ID").unwrap_or_else(|_| {
            format!(
                "runner-{}",
                uuid::Uuid::new_v4()
                    .to_string()
                    .split('-')
                    .next()
                    .unwrap_or("local")
            )
        });
        Some(Self {
            hub_url,
            device_id,
            device_name: std::env::var("HAKIMI_HUB_DEVICE_NAME")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| Some("Hakimi Server Worker".into())),
            token: std::env::var("HAKIMI_HUB_TOKEN").ok().filter(|s| !s.is_empty()),
            reconnect_secs: std::env::var("HAKIMI_HUB_RECONNECT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
        })
    }
}

/// Spawn a reconnecting hub worker loop. Returns a JoinHandle.
pub fn spawn_hub_worker(
    runtime: Arc<StudioRuntime>,
    cfg: HubWorkerConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = cfg.reconnect_secs.max(1);
        loop {
            match run_once(runtime.clone(), &cfg).await {
                Ok(()) => {
                    info!("hub worker disconnected cleanly; reconnecting");
                    backoff = cfg.reconnect_secs.max(1);
                }
                Err(e) => {
                    warn!(error = %e, backoff_secs = backoff, "hub worker error; reconnecting");
                }
            }
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(60);
        }
    })
}

async fn run_once(runtime: Arc<StudioRuntime>, cfg: &HubWorkerConfig) -> Result<()> {
    info!(url = %cfg.hub_url, device = %cfg.device_id, "connecting hub worker");
    let (ws, _) = connect_async(&cfg.hub_url)
        .await
        .with_context(|| format!("connect {}", cfg.hub_url))?;
    let (write, mut read) = ws.split();
    let write = Arc::new(Mutex::new(write));

    // Hello as server runner.
    let hello = StudioCommand::Hello {
        device_id: cfg.device_id.clone(),
        token: cfg.token.clone(),
        device_name: cfg.device_name.clone(),
        kind: DeviceKind::Server,
        protocol_version: PROTOCOL_VERSION,
    };
    {
        let text = serde_json::to_string(&hello)?;
        let mut w = write.lock().await;
        w.send(Message::Text(text.into())).await?;
    }

    // Wait for hello_ok (best-effort drain a few frames).
    for _ in 0..8 {
        match tokio::time::timeout(Duration::from_secs(5), read.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                if let Ok(env) = serde_json::from_str::<StudioEventEnvelope>(&t) {
                    match env.event {
                        StudioEvent::HelloOk { .. } => {
                            info!("hub worker hello_ok");
                            break;
                        }
                        StudioEvent::HelloError { message } => {
                            anyhow::bail!("hub hello rejected: {message}");
                        }
                        _ => {}
                    }
                }
            }
            Ok(Some(Ok(Message::Ping(p)))) => {
                let mut w = write.lock().await;
                let _ = w.send(Message::Pong(p)).await;
            }
            Ok(Some(Err(e))) => return Err(e.into()),
            Ok(None) => anyhow::bail!("hub closed during hello"),
            Err(_) => anyhow::bail!("hub hello timeout"),
            _ => {}
        }
    }

    while let Some(msg) = read.next().await {
        let msg = msg?;
        match msg {
            Message::Text(t) => {
                if let Ok(dispatch) = serde_json::from_str::<WorkerDispatch>(&t) {
                    if dispatch.kind == WorkerDispatch::TYPE {
                        handle_dispatch(
                            runtime.clone(),
                            write.clone(),
                            dispatch.actor_device_id.as_deref(),
                            dispatch.command,
                        )
                        .await;
                        continue;
                    }
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    if v.get("type").and_then(|x| x.as_str()) == Some("worker_dispatch") {
                        if let Ok(dispatch) = serde_json::from_value::<WorkerDispatch>(v) {
                            handle_dispatch(
                                runtime.clone(),
                                write.clone(),
                                dispatch.actor_device_id.as_deref(),
                                dispatch.command,
                            )
                            .await;
                        }
                    }
                }
            }
            Message::Ping(p) => {
                let mut w = write.lock().await;
                let _ = w.send(Message::Pong(p)).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    Ok(())
}

async fn handle_dispatch(
    runtime: Arc<StudioRuntime>,
    write: Arc<
        Mutex<
            futures::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
                Message,
            >,
        >,
    >,
    actor: Option<&str>,
    cmd: StudioCommand,
) {
    debug!(?actor, "worker handling dispatch");
    match runtime.handle_command_as(actor, cmd).await {
        Ok(events) if !events.is_empty() => {
            let publish = StudioCommand::WorkerPublish { events };
            match serde_json::to_string(&publish) {
                Ok(text) => {
                    let mut w = write.lock().await;
                    if let Err(e) = w.send(Message::Text(text.into())).await {
                        warn!(error = %e, "failed to worker_publish");
                    }
                }
                Err(e) => warn!(error = %e, "serialize worker_publish"),
            }
        }
        Ok(_) => {}
        Err(e) => {
            warn!(error = %e, "dispatch handle failed");
            let env = StudioEventEnvelope::new(
                0,
                None,
                StudioEvent::Error {
                    session_id: None,
                    message: e.to_string(),
                    code: Some("worker_error".into()),
                },
            );
            let publish = StudioCommand::WorkerPublish {
                events: vec![env],
            };
            if let Ok(text) = serde_json::to_string(&publish) {
                let mut w = write.lock().await;
                let _ = w.send(Message::Text(text.into())).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_dispatch_roundtrip() {
        let d = WorkerDispatch::new(
            Some("web-1".into()),
            StudioCommand::Ping {
                nonce: Some("n".into()),
            },
        );
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains("worker_dispatch"));
        let back: WorkerDispatch = serde_json::from_str(&s).unwrap();
        assert_eq!(back.kind, WorkerDispatch::TYPE);
        assert_eq!(back.actor_device_id.as_deref(), Some("web-1"));
    }
}
