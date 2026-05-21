//! Thin wrapper that re-exports hakimi-cli's entry point.
//!
//! This crate exists so that users can install via `cargo install hakimi-agent`.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    hakimi_cli::entry::run().await
}
