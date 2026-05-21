use serde::{Deserialize, Serialize};

/// Reason a request failed, used for failover / retry logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailoverReason {
    /// Rate limit exceeded (HTTP 429).
    RateLimited,
    /// Server error (HTTP 5xx).
    ServerError,
    /// Auth / permission failure (HTTP 401 / 403).
    AuthError,
    /// Request timed out.
    Timeout,
    /// Model not found or unavailable (HTTP 404).
    ModelNotFound,
    /// Request too large (HTTP 413).
    RequestTooLarge,
    /// Unknown / unclassified error.
    Unknown,
}

/// Classify an HTTP error response into a [`FailoverReason`] and whether it
/// is worth retrying.
pub fn classify_error(status_code: u16, _body: &str) -> (FailoverReason, bool) {
    match status_code {
        429 => (FailoverReason::RateLimited, true),
        500..=599 => (FailoverReason::ServerError, true),
        401 | 403 => (FailoverReason::AuthError, false),
        404 => (FailoverReason::ModelNotFound, false),
        408 => (FailoverReason::Timeout, true),
        413 => (FailoverReason::RequestTooLarge, false),
        _ => (FailoverReason::Unknown, false),
    }
}

/// Error type for transport operations.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// HTTP-level error (connection, DNS, etc.).
    #[error("HTTP error: {0}")]
    Http(String),

    /// API returned an error response.
    #[error("API error {status} ({reason}, retryable={retryable}): {body}")]
    Api {
        status: u16,
        reason: String,
        retryable: bool,
        body: String,
    },

    /// Failed to parse the response.
    #[error("parse error: {0}")]
    Parse(String),
}

/// Convenience type alias for transport results.
pub type TransportResult<T> = Result<T, TransportError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rate_limited() {
        let (reason, retryable) = classify_error(429, "too many requests");
        assert_eq!(reason, FailoverReason::RateLimited);
        assert!(retryable);
    }

    #[test]
    fn test_classify_server_errors() {
        for code in [500, 502, 503, 504, 599] {
            let (reason, retryable) = classify_error(code, "server error");
            assert_eq!(reason, FailoverReason::ServerError, "expected ServerError for {code}");
            assert!(retryable, "expected retryable for {code}");
        }
    }

    #[test]
    fn test_classify_auth_errors() {
        for code in [401, 403] {
            let (reason, retryable) = classify_error(code, "unauthorized");
            assert_eq!(reason, FailoverReason::AuthError, "expected AuthError for {code}");
            assert!(!retryable, "expected not retryable for {code}");
        }
    }

    #[test]
    fn test_classify_model_not_found() {
        let (reason, retryable) = classify_error(404, "not found");
        assert_eq!(reason, FailoverReason::ModelNotFound);
        assert!(!retryable);
    }

    #[test]
    fn test_classify_timeout() {
        let (reason, retryable) = classify_error(408, "timeout");
        assert_eq!(reason, FailoverReason::Timeout);
        assert!(retryable);
    }

    #[test]
    fn test_classify_request_too_large() {
        let (reason, retryable) = classify_error(413, "payload too large");
        assert_eq!(reason, FailoverReason::RequestTooLarge);
        assert!(!retryable);
    }

    #[test]
    fn test_classify_unknown() {
        let (reason, retryable) = classify_error(418, "I'm a teapot");
        assert_eq!(reason, FailoverReason::Unknown);
        assert!(!retryable);
    }

    #[test]
    fn test_transport_error_display() {
        let err = TransportError::Http("connection refused".to_string());
        assert!(format!("{err}").contains("connection refused"));

        let err = TransportError::Api {
            status: 429,
            reason: "RateLimited".to_string(),
            retryable: true,
            body: "rate limit".to_string(),
        };
        assert!(format!("{err}").contains("429"));
        assert!(format!("{err}").contains("RateLimited"));

        let err = TransportError::Parse("invalid json".to_string());
        assert!(format!("{err}").contains("invalid json"));
    }
}
