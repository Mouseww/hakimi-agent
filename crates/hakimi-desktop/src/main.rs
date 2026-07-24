//! Hakimi Studio desktop binary.
//!
//! Usage:
//!   cargo run -p hakimi-desktop -- --bind 127.0.0.1:3015
//!   cargo run -p hakimi-desktop --features gui   # requires webkit2gtk-4.1
//!
//! Env:
//!   RUST_LOG=info
//!   HAKIMI_DESKTOP_WORKSPACE=/path/to/project

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use hakimi_desktop::{BackendConfig, start_backend};
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "hakimi-desktop",
    about = "Hakimi Studio desktop shell (local backend + optional Tauri GUI)"
)]
struct Args {
    /// Bind address for the embedded Studio/WebUI backend.
    /// Use 127.0.0.1:0 for an ephemeral port.
    #[arg(long, default_value = "127.0.0.1:0")]
    bind: String,

    /// Workspace root for Studio path jail (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,

    /// Open the system browser to the local UI after start (headless mode).
    #[arg(long)]
    open: bool,

    /// Exit immediately after backend is ready (smoke / CI).
    #[arg(long)]
    once: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let workspace = args.workspace.or_else(|| {
        std::env::var("HAKIMI_DESKTOP_WORKSPACE")
            .ok()
            .map(PathBuf::from)
    });

    let handle = start_backend(BackendConfig {
        bind: args.bind,
        workspace,
    })
    .await?;

    info!(url = %handle.base_url, "Hakimi Studio ready");
    println!("Hakimi Studio → {}", handle.base_url);
    println!(
        "  Studio WS   → {}/v1/studio",
        handle.base_url.replace("http", "ws")
    );
    println!("  Health      → {}/v1/studio/health", handle.base_url);

    if args.open {
        let url = handle.base_url.clone();
        // Best-effort: xdg-open / open / start
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }

    #[cfg(feature = "gui")]
    {
        // Tauri takes over the main thread; backend already on tokio.
        let url = handle.base_url.clone();
        // Detach join handle so abort doesn't kill us from Drop.
        std::mem::forget(handle.join);
        hakimi_desktop::gui::run_gui(url);
        return Ok(());
    }

    #[cfg(not(feature = "gui"))]
    {
        if args.once {
            info!("--once: backend ready, exiting");
            handle.join.abort();
            tokio::time::sleep(Duration::from_millis(50)).await;
        } else {
            info!("headless mode — press Ctrl-C to stop (rebuild with --features gui for window)");
            tokio::signal::ctrl_c().await.ok();
            info!("shutting down");
            handle.join.abort();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(())
    }
}
