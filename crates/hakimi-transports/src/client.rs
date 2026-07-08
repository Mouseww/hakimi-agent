use std::time::Duration;

use reqwest::Client;

/// Default connect timeout for LLM HTTP requests.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Default read timeout for LLM HTTP requests.
///
/// This is a per-read timeout, so long streaming responses remain valid as long
/// as the provider keeps sending bytes such as SSE pings or deltas.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(90);

/// Build the shared HTTP client used for LLM and embedding providers.
pub fn build_llm_http_client() -> reqwest::Result<Client> {
    Client::builder()
        .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
        .read_timeout(DEFAULT_READ_TIMEOUT)
        .tcp_keepalive(Duration::from_secs(60))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(10)
        .build()
}
