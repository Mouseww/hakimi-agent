use async_trait::async_trait;
use futures::stream::Stream;
use hakimi_common::{ApiMode, Message, NormalizedResponse, Result, ToolDefinition};
use std::pin::Pin;

use crate::params::RequestParams;
use crate::streaming::StreamEvent;

/// Core trait that every LLM transport must implement.
///
/// A transport is responsible for:
/// 1. Serializing messages + tools into the provider-specific wire format
/// 2. Sending the HTTP request
/// 3. Deserializing the response back into the common [`NormalizedResponse`]
#[async_trait]
pub trait ProviderTransport: Send + Sync {
    /// Which API mode / protocol this transport speaks.
    fn api_mode(&self) -> ApiMode;

    /// Human-readable provider name (e.g. `"openai"`, `"anthropic"`).
    fn provider_name(&self) -> &str;

    /// Execute a single (non-streaming) completion request.
    async fn execute(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> Result<NormalizedResponse>;

    /// Execute a streaming completion request.
    ///
    /// Returns a `Stream` of [`StreamEvent`]s as they arrive from the provider.
    /// The stream ends when the provider signals completion (e.g. `data: [DONE]`
    /// for OpenAI, `message_stop` for Anthropic).
    async fn execute_streaming(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamEvent, String>> + Send>>>;
}
