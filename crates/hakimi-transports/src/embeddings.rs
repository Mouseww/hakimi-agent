use async_trait::async_trait;
use hakimi_common::{HakimiError, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, warn};

/// Common trait for text embedding providers.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate one embedding vector for each input text.
    async fn embed(&self, input: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Embedding model name.
    fn model_name(&self) -> &str;

    /// Dense embedding dimension.
    fn dimension(&self) -> usize;
}

/// OpenAI-compatible embeddings client.
///
/// Works with providers exposing `POST /v1/embeddings` or a `base_url` that
/// already ends in `/v1`. This is intentionally model-agnostic, so online
/// models such as `BAAI/bge-m3` can reuse the same site and API key as chat.
pub struct OpenAICompatibleEmbeddingProvider {
    base_url: String,
    api_key: String,
    model: String,
    dimension: usize,
    normalize: bool,
    client: Client,
}

impl OpenAICompatibleEmbeddingProvider {
    pub fn new(
        base_url: String,
        api_key: String,
        model: String,
        dimension: usize,
        normalize: bool,
        client: Client,
    ) -> Self {
        Self {
            base_url,
            api_key,
            model,
            dimension,
            normalize,
            client,
        }
    }

    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/embeddings")
        } else {
            format!("{base}/v1/embeddings")
        }
    }

    fn normalize_vector(vector: &mut [f32]) {
        let norm = vector
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            .sqrt();
        if norm > 0.0 {
            for v in vector.iter_mut() {
                *v = (*v as f64 / norm) as f32;
            }
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAICompatibleEmbeddingProvider {
    async fn embed(&self, input: &[String]) -> Result<Vec<Vec<f32>>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let url = self.endpoint();
        let body = json!({
            "model": self.model,
            "input": input,
        });

        debug!(url = %url, model = %self.model, count = input.len(), "sending embeddings request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| HakimiError::Transport(format!("embedding HTTP request failed: {e}")))?;

        let status = response.status();
        let response_text = response.text().await.map_err(|e| {
            HakimiError::Transport(format!("failed to read embedding response body: {e}"))
        })?;

        if !status.is_success() {
            warn!(status = status.as_u16(), body = %response_text, "embeddings API returned error");
            return Err(HakimiError::Transport(format!(
                "embeddings API error {}: {}",
                status.as_u16(),
                response_text
            )));
        }

        let parsed: EmbeddingResponse = serde_json::from_str(&response_text).map_err(|e| {
            warn!(error = %e, body = %response_text, "failed to parse embeddings response JSON");
            HakimiError::Transport(format!("failed to parse embeddings response: {e}"))
        })?;

        let mut data = parsed.data;
        data.sort_by_key(|item| item.index.unwrap_or(usize::MAX));

        let mut embeddings: Vec<Vec<f32>> = data.into_iter().map(|item| item.embedding).collect();

        if embeddings.len() != input.len() {
            return Err(HakimiError::Transport(format!(
                "embedding response count mismatch: expected {}, got {}",
                input.len(),
                embeddings.len()
            )));
        }

        for embedding in &mut embeddings {
            if self.dimension > 0 && embedding.len() != self.dimension {
                return Err(HakimiError::Transport(format!(
                    "embedding dimension mismatch for model {}: expected {}, got {}",
                    self.model,
                    self.dimension,
                    embedding.len()
                )));
            }
            if self.normalize {
                Self::normalize_vector(embedding);
            }
        }

        Ok(embeddings)
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    #[serde(default)]
    index: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_adds_v1_when_missing() {
        let p = OpenAICompatibleEmbeddingProvider::new(
            "https://api.example.com".into(),
            "sk-test".into(),
            "BAAI/bge-m3".into(),
            1024,
            true,
            Client::new(),
        );
        assert_eq!(p.endpoint(), "https://api.example.com/v1/embeddings");
    }

    #[test]
    fn endpoint_does_not_double_v1() {
        let p = OpenAICompatibleEmbeddingProvider::new(
            "https://api.example.com/v1".into(),
            "sk-test".into(),
            "BAAI/bge-m3".into(),
            1024,
            true,
            Client::new(),
        );
        assert_eq!(p.endpoint(), "https://api.example.com/v1/embeddings");
    }

    #[test]
    fn normalize_vector_to_unit_length() {
        let mut v = vec![3.0_f32, 4.0_f32];
        OpenAICompatibleEmbeddingProvider::normalize_vector(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
    }
}
