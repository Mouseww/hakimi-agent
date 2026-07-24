//! Hakimi Studio Hub binary.
//!
//! Usage:
//!   hakimi-hub --bind 0.0.0.0:3010 --token secret --mode relay
//!   hakimi-hub --mode embedded   # demo with in-process StudioRuntime
//!
//! Env:
//!   HAKIMI_HUB_BIND, HAKIMI_HUB_TOKEN, HAKIMI_HUB_MODE=relay|embedded

use anyhow::Result;
use clap::Parser;
use hakimi_hub::{HubConfig, HubState, hub_router};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "hakimi-hub", about = "Hakimi Studio multi-device relay hub")]
struct Args {
    /// Bind address.
    #[arg(long, default_value = "0.0.0.0:3010")]
    bind: String,

    /// Optional shared token for hello.
    #[arg(long)]
    token: Option<String>,

    /// Hub mode: `relay` (pure, no tools) or `embedded` (demo runtime).
    #[arg(long, default_value = "embedded")]
    mode: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hakimi_hub=debug,hakimi_studio_api=info".into()),
        )
        .init();

    let mut cfg = HubConfig::from_env();
    let args = Args::parse();
    if args.bind != "0.0.0.0:3010" || std::env::var("HAKIMI_HUB_BIND").is_err() {
        cfg.bind = args.bind;
    }
    if args.token.is_some() {
        cfg.token = args.token;
    }
    if std::env::var("HAKIMI_HUB_MODE").is_err() || args.mode != "embedded" {
        cfg.mode = args.mode;
    }

    let state = match cfg.mode.as_str() {
        "relay" | "pure" => {
            info!("starting pure-relay hub (no embedded runtime)");
            HubState::new_relay(cfg.token.clone())
        }
        _ => {
            info!("starting embedded hub (demo StudioRuntime)");
            HubState::new_embedded(cfg.token.clone())
        }
    };

    let app = hub_router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    info!(
        bind = %cfg.bind,
        mode = %cfg.mode,
        token_required = cfg.token.is_some(),
        "hakimi-hub listening (no tools / no provider keys)"
    );
    axum::serve(listener, app).await?;
    Ok(())
}
