//! Hakimi Agent configuration.
//!
//! Provides [`HakimiConfig`] — the top-level configuration struct — along with
//! per-section config types and helper utilities.

pub mod config;
pub mod defaults;

pub use config::*;
pub use defaults::{DEFAULT_FLAT, default_config_value};
