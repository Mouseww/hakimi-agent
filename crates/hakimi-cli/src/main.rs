//! Hakimi Agent CLI — interactive REPL and single-query mode.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    hakimi_cli::entry::run().await
}
