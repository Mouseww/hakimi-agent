//! Hakimi Desktop — local Studio backend + optional Tauri 2 shell.
//!
//! Default binary embeds a lightweight HTTP server (WebUI static + `/v1/studio` WS)
//! and either:
//! - prints the URL (headless / server mode), or
//! - with `--features gui`, opens a Tauri window navigating to that backend.
//!
//! See `docs/hakimi-studio/DESKTOP.md`.

pub mod backend;

#[cfg(feature = "gui")]
pub mod gui;

pub use backend::{BackendConfig, BackendHandle, start_backend};
