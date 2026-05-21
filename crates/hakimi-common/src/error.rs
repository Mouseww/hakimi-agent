use thiserror::Error;

/// The unified error type for the entire Hakimi Agent.
#[derive(Debug, Error)]
pub enum HakimiError {
    /// Error communicating with a remote service (LLM API, etc.).
    #[error("transport error: {0}")]
    Transport(String),

    /// Error during tool execution.
    #[error("tool error: {0}")]
    Tool(String),

    /// Invalid or missing configuration.
    #[error("config error: {0}")]
    Config(String),

    /// Session-related error (creation, lookup, etc.).
    #[error("session error: {0}")]
    Session(String),

    /// Context-related error (token limits, truncation, etc.).
    #[error("context error: {0}")]
    Context(String),

    /// File-system / I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization / deserialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Catch-all for other errors.
    #[error("{0}")]
    Other(String),
}

/// Convenience alias for `Result<T, HakimiError>`.
pub type Result<T> = std::result::Result<T, HakimiError>;
