use serde::{Deserialize, Serialize};

/// Parameters that can be sent alongside any completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestParams {
    /// Sampling temperature (0.0 – 2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Maximum number of tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Nucleus sampling top-p.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,

    /// Whether to use streaming.
    #[serde(default)]
    pub stream: bool,
}

impl Default for RequestParams {
    fn default() -> Self {
        Self {
            temperature: None,
            max_tokens: None,
            top_p: None,
            stop: None,
            stream: false,
        }
    }
}
