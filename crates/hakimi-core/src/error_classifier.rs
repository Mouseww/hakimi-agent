use std::collections::HashMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

use hakimi_common::HakimiError;

/// Reason for a provider failure, used to decide recovery strategy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailoverReason {
    /// 401 – invalid or expired API key.
    AuthError,
    /// 402 – payment required / billing issue.
    BillingError,
    /// 429 – too many requests.
    RateLimit,
    /// 529 – server overloaded (Anthropic-specific).
    Overloaded,
    /// Context window exceeded.
    ContextOverflow,
    /// 404 – model does not exist.
    ModelNotFound,
    /// Anthropic thinking-signature format error.
    ThinkingSignature,
    /// Connection or read timeout.
    NetworkTimeout,
    /// 400 – malformed / bad request.
    InvalidRequest,
    /// Content-policy / safety filter triggered.
    ContentFilter,
    /// 403 – permission denied.
    PermissionDenied,
    /// Geographic / region restriction.
    RegionBlocked,
    /// Monthly or daily quota exhausted.
    QuotaExceeded,
    /// 5xx server-side error.
    ServerError,
    /// Proxy connection failure.
    ProxyError,
    /// TLS / SSL handshake failure.
    SslError,
    /// Response body could not be parsed.
    ParsingError,
    /// SSE stream broke mid-response.
    StreamInterrupted,
    /// Unclassified / unknown error.
    Unknown,
}

/// Action the retry layer should take after classifying an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryAction {
    /// Retry immediately without delay.
    RetryImmediate,
    /// Retry with exponential backoff.
    RetryWithBackoff,
    /// Rotate to the next API key for the same provider.
    RotateCredential,
    /// Switch to a backup model.
    FallbackModel,
    /// Reduce context size and retry.
    CompressContext,
    /// Switch to an entirely different provider.
    ChangeProvider,
    /// Give up – non-recoverable.
    Abort,
}

/// Result of classifying a single error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorClassification {
    pub reason: FailoverReason,
    pub action: RecoveryAction,
    pub is_retryable: bool,
    pub message: String,
    /// Suggested wait time from `Retry-After` header, in milliseconds.
    pub retry_after_ms: Option<u64>,
}

/// A user-defined rule that overrides the default classification when the
/// error body matches a given regex pattern.
pub struct ErrorRule {
    pub pattern: Regex,
    pub reason: FailoverReason,
    pub action: RecoveryAction,
}

/// Stateful classifier that can be extended with custom rules.
pub struct ErrorClassifier {
    custom_rules: Vec<ErrorRule>,
}

impl ErrorClassifier {
    /// Create a classifier with sensible defaults (no custom rules).
    pub fn new() -> Self {
        Self {
            custom_rules: Vec::new(),
        }
    }

    /// Register a custom regex rule.  When the error body matches `pattern`,
    /// the given `reason` and `action` will be used instead of the defaults.
    ///
    /// # Panics
    /// Panics if `pattern` is not a valid regex.
    pub fn add_rule(&mut self, pattern: &str, reason: FailoverReason, action: RecoveryAction) {
        self.custom_rules.push(ErrorRule {
            pattern: Regex::new(pattern).expect("invalid regex pattern"),
            reason,
            action,
        });
    }

    /// Classify based on an HTTP status code, response headers, and body.
    pub fn classify_http(
        &self,
        status: u16,
        headers: &HashMap<String, String>,
        body: &str,
    ) -> ErrorClassification {
        // 1. Check custom rules first (body match).
        for rule in &self.custom_rules {
            if rule.pattern.is_match(body) {
                let retry_after = Self::extract_retry_after_ms(headers);
                let is_retryable = Self::is_retryable(&rule.reason);
                return ErrorClassification {
                    reason: rule.reason.clone(),
                    action: rule.action.clone(),
                    is_retryable,
                    message: format!("matched custom rule: {}", rule.pattern),
                    retry_after_ms: retry_after,
                };
            }
        }

        // 2. Check body patterns for well-known provider messages.
        let body_lower = body.to_lowercase();
        if body_lower.contains("context_length_exceeded")
            || body_lower.contains("context window")
            || body_lower.contains("maximum context length")
        {
            return ErrorClassification {
                reason: FailoverReason::ContextOverflow,
                action: RecoveryAction::CompressContext,
                is_retryable: false,
                message: "context window exceeded".into(),
                retry_after_ms: None,
            };
        }
        if body_lower.contains("content_policy") || body_lower.contains("content filter") {
            return ErrorClassification {
                reason: FailoverReason::ContentFilter,
                action: RecoveryAction::Abort,
                is_retryable: false,
                message: "content policy violation".into(),
                retry_after_ms: None,
            };
        }
        if body_lower.contains("model_not_found") || body_lower.contains("model not found") {
            return ErrorClassification {
                reason: FailoverReason::ModelNotFound,
                action: RecoveryAction::FallbackModel,
                is_retryable: false,
                message: "model not found".into(),
                retry_after_ms: None,
            };
        }
        if body_lower.contains("thinking_signature") || body_lower.contains("thinking signature") {
            return ErrorClassification {
                reason: FailoverReason::ThinkingSignature,
                action: RecoveryAction::Abort,
                is_retryable: false,
                message: "thinking signature error".into(),
                retry_after_ms: None,
            };
        }
        if body_lower.contains("quota_exceeded") || body_lower.contains("quota exceeded") {
            return ErrorClassification {
                reason: FailoverReason::QuotaExceeded,
                action: RecoveryAction::Abort,
                is_retryable: false,
                message: "quota exceeded".into(),
                retry_after_ms: None,
            };
        }
        if body_lower.contains("region") && body_lower.contains("blocked") {
            return ErrorClassification {
                reason: FailoverReason::RegionBlocked,
                action: RecoveryAction::ChangeProvider,
                is_retryable: false,
                message: "region blocked".into(),
                retry_after_ms: None,
            };
        }

        // 3. Fall back to status-code mapping.
        let (reason, action) = Self::classify_status(status);
        let retry_after = Self::extract_retry_after_ms(headers);
        let is_retryable = Self::is_retryable(&reason);
        ErrorClassification {
            reason,
            action,
            is_retryable,
            message: format!("HTTP {status}"),
            retry_after_ms: retry_after,
        }
    }

    /// Classify a transport-level (non-HTTP) error from a string message.
    pub fn classify_transport_error(&self, error: &str) -> ErrorClassification {
        let lower = error.to_lowercase();

        // Check custom rules first.
        for rule in &self.custom_rules {
            if rule.pattern.is_match(error) {
                return ErrorClassification {
                    reason: rule.reason.clone(),
                    action: rule.action.clone(),
                    is_retryable: Self::is_retryable(&rule.reason),
                    message: error.to_string(),
                    retry_after_ms: None,
                };
            }
        }

        if lower.contains("ssl") || lower.contains("tls") || lower.contains("certificate") {
            return ErrorClassification {
                reason: FailoverReason::SslError,
                action: RecoveryAction::ChangeProvider,
                is_retryable: false,
                message: error.to_string(),
                retry_after_ms: None,
            };
        }
        if lower.contains("proxy") {
            return ErrorClassification {
                reason: FailoverReason::ProxyError,
                action: RecoveryAction::ChangeProvider,
                is_retryable: false,
                message: error.to_string(),
                retry_after_ms: None,
            };
        }
        if lower.contains("timeout")
            || lower.contains("timed out")
            || lower.contains("deadline exceeded")
        {
            return ErrorClassification {
                reason: FailoverReason::NetworkTimeout,
                action: RecoveryAction::RetryWithBackoff,
                is_retryable: true,
                message: error.to_string(),
                retry_after_ms: None,
            };
        }
        if lower.contains("connection refused")
            || lower.contains("connection reset")
            || lower.contains("broken pipe")
            || lower.contains("network")
        {
            return ErrorClassification {
                reason: FailoverReason::NetworkTimeout,
                action: RecoveryAction::RetryWithBackoff,
                is_retryable: true,
                message: error.to_string(),
                retry_after_ms: None,
            };
        }
        if lower.contains("stream")
            && (lower.contains("interrupted")
                || lower.contains("broken")
                || lower.contains("closed"))
        {
            return ErrorClassification {
                reason: FailoverReason::StreamInterrupted,
                action: RecoveryAction::RetryImmediate,
                is_retryable: true,
                message: error.to_string(),
                retry_after_ms: None,
            };
        }
        if lower.contains("parse") || lower.contains("deserializ") || lower.contains("invalid json")
        {
            return ErrorClassification {
                reason: FailoverReason::ParsingError,
                action: RecoveryAction::Abort,
                is_retryable: false,
                message: error.to_string(),
                retry_after_ms: None,
            };
        }

        // Unknown transport error – retry with backoff as a safe default.
        ErrorClassification {
            reason: FailoverReason::Unknown,
            action: RecoveryAction::RetryWithBackoff,
            is_retryable: true,
            message: error.to_string(),
            retry_after_ms: None,
        }
    }

    /// Quick status-code → (reason, action) mapping.
    pub fn classify_status(status: u16) -> (FailoverReason, RecoveryAction) {
        match status {
            401 => (FailoverReason::AuthError, RecoveryAction::RotateCredential),
            402 => (FailoverReason::BillingError, RecoveryAction::Abort),
            403 => (FailoverReason::PermissionDenied, RecoveryAction::Abort),
            404 => (FailoverReason::ModelNotFound, RecoveryAction::FallbackModel),
            408 => (
                FailoverReason::NetworkTimeout,
                RecoveryAction::RetryWithBackoff,
            ),
            429 => (FailoverReason::RateLimit, RecoveryAction::RetryWithBackoff),
            500 => (
                FailoverReason::ServerError,
                RecoveryAction::RetryWithBackoff,
            ),
            502 | 503 => (FailoverReason::ServerError, RecoveryAction::RetryImmediate),
            504 => (
                FailoverReason::NetworkTimeout,
                RecoveryAction::RetryWithBackoff,
            ),
            529 => (FailoverReason::Overloaded, RecoveryAction::RetryWithBackoff),
            400 => (FailoverReason::InvalidRequest, RecoveryAction::Abort),
            451 => (
                FailoverReason::RegionBlocked,
                RecoveryAction::ChangeProvider,
            ),
            s if (500..600).contains(&s) => (
                FailoverReason::ServerError,
                RecoveryAction::RetryWithBackoff,
            ),
            _ => (FailoverReason::Unknown, RecoveryAction::Abort),
        }
    }

    /// Parse a `Retry-After` header value, handling both seconds and
    /// millisecond formats.  Returns `None` if absent or unparseable.
    pub fn extract_retry_after_ms(headers: &HashMap<String, String>) -> Option<u64> {
        // Try both casings.
        let value = headers
            .get("retry-after")
            .or_else(|| headers.get("Retry-After"))
            .or_else(|| headers.get("RETRY-AFTER"))?;

        let trimmed = value.trim();

        // If it looks like a date (RFC 7231), we can't easily parse it without
        // a full HTTP-date parser, so return a sensible default.
        // Try plain integer (seconds).
        if let Ok(secs) = trimmed.parse::<u64>() {
            return Some(secs * 1000);
        }
        // Try floating-point seconds.
        if let Ok(secs_f) = trimmed.parse::<f64>() {
            return Some((secs_f * 1000.0) as u64);
        }

        None
    }

    /// Whether a given `FailoverReason` is considered retryable by default.
    pub fn is_retryable(reason: &FailoverReason) -> bool {
        matches!(
            reason,
            FailoverReason::RateLimit
                | FailoverReason::Overloaded
                | FailoverReason::NetworkTimeout
                | FailoverReason::ServerError
                | FailoverReason::StreamInterrupted
        )
    }

    /// Suggest a default `RecoveryAction` for a given `FailoverReason`.
    pub fn suggest_fallback(reason: &FailoverReason) -> RecoveryAction {
        match reason {
            FailoverReason::AuthError => RecoveryAction::RotateCredential,
            FailoverReason::BillingError => RecoveryAction::Abort,
            FailoverReason::RateLimit => RecoveryAction::RetryWithBackoff,
            FailoverReason::Overloaded => RecoveryAction::RetryWithBackoff,
            FailoverReason::ContextOverflow => RecoveryAction::CompressContext,
            FailoverReason::ModelNotFound => RecoveryAction::FallbackModel,
            FailoverReason::ThinkingSignature => RecoveryAction::Abort,
            FailoverReason::NetworkTimeout => RecoveryAction::RetryWithBackoff,
            FailoverReason::InvalidRequest => RecoveryAction::Abort,
            FailoverReason::ContentFilter => RecoveryAction::Abort,
            FailoverReason::PermissionDenied => RecoveryAction::Abort,
            FailoverReason::RegionBlocked => RecoveryAction::ChangeProvider,
            FailoverReason::QuotaExceeded => RecoveryAction::Abort,
            FailoverReason::ServerError => RecoveryAction::RetryWithBackoff,
            FailoverReason::ProxyError => RecoveryAction::ChangeProvider,
            FailoverReason::SslError => RecoveryAction::ChangeProvider,
            FailoverReason::ParsingError => RecoveryAction::Abort,
            FailoverReason::StreamInterrupted => RecoveryAction::RetryImmediate,
            FailoverReason::Unknown => RecoveryAction::RetryWithBackoff,
        }
    }
}

impl Default for ErrorClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait that adds a `classify()` method to [`HakimiError`].
pub trait Classifiable {
    fn classify(&self) -> ErrorClassification;
}

impl Classifiable for HakimiError {
    fn classify(&self) -> ErrorClassification {
        let classifier = ErrorClassifier::new();
        match self {
            HakimiError::Transport(msg) => classifier.classify_transport_error(msg),
            HakimiError::Io(err) => classifier.classify_transport_error(&err.to_string()),
            HakimiError::Context(msg) => ErrorClassification {
                reason: FailoverReason::ContextOverflow,
                action: RecoveryAction::CompressContext,
                is_retryable: false,
                message: msg.clone(),
                retry_after_ms: None,
            },
            HakimiError::Json(err) => ErrorClassification {
                reason: FailoverReason::ParsingError,
                action: RecoveryAction::Abort,
                is_retryable: false,
                message: err.to_string(),
                retry_after_ms: None,
            },
            other => ErrorClassification {
                reason: FailoverReason::Unknown,
                action: RecoveryAction::Abort,
                is_retryable: false,
                message: other.to_string(),
                retry_after_ms: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_classifier() -> ErrorClassifier {
        ErrorClassifier::new()
    }

    fn empty_headers() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn test_classify_401_auth() {
        let c = make_classifier();
        let r = c.classify_http(401, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::AuthError);
        assert_eq!(r.action, RecoveryAction::RotateCredential);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_402_billing() {
        let c = make_classifier();
        let r = c.classify_http(402, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::BillingError);
        assert_eq!(r.action, RecoveryAction::Abort);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_403_permission() {
        let c = make_classifier();
        let r = c.classify_http(403, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::PermissionDenied);
        assert_eq!(r.action, RecoveryAction::Abort);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_404_model_not_found() {
        let c = make_classifier();
        let r = c.classify_http(404, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::ModelNotFound);
        assert_eq!(r.action, RecoveryAction::FallbackModel);
    }

    #[test]
    fn test_classify_429_rate_limit() {
        let c = make_classifier();
        let r = c.classify_http(429, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::RateLimit);
        assert_eq!(r.action, RecoveryAction::RetryWithBackoff);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_classify_429_with_retry_after() {
        let c = make_classifier();
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "5".to_string());
        let r = c.classify_http(429, &headers, "");
        assert_eq!(r.reason, FailoverReason::RateLimit);
        assert_eq!(r.retry_after_ms, Some(5000));
    }

    #[test]
    fn test_classify_500_server_error() {
        let c = make_classifier();
        let r = c.classify_http(500, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::ServerError);
        assert_eq!(r.action, RecoveryAction::RetryWithBackoff);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_classify_529_overloaded() {
        let c = make_classifier();
        let r = c.classify_http(529, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::Overloaded);
        assert_eq!(r.action, RecoveryAction::RetryWithBackoff);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_classify_context_overflow_body() {
        let c = make_classifier();
        let r = c.classify_http(
            400,
            &empty_headers(),
            r#"{"error": {"message": "context_length_exceeded: maximum 200000 tokens"}}"#,
        );
        assert_eq!(r.reason, FailoverReason::ContextOverflow);
        assert_eq!(r.action, RecoveryAction::CompressContext);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_content_filter_body() {
        let c = make_classifier();
        let r = c.classify_http(
            400,
            &empty_headers(),
            r#"{"error": "content_policy_violation detected"}"#,
        );
        assert_eq!(r.reason, FailoverReason::ContentFilter);
        assert_eq!(r.action, RecoveryAction::Abort);
    }

    #[test]
    fn test_classify_network_timeout() {
        let c = make_classifier();
        let r = c.classify_transport_error("connection timed out after 30s");
        assert_eq!(r.reason, FailoverReason::NetworkTimeout);
        assert_eq!(r.action, RecoveryAction::RetryWithBackoff);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_classify_ssl_error() {
        let c = make_classifier();
        let r = c.classify_transport_error("SSL certificate verification failed");
        assert_eq!(r.reason, FailoverReason::SslError);
        assert_eq!(r.action, RecoveryAction::ChangeProvider);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_stream_interrupted() {
        let c = make_classifier();
        let r = c.classify_transport_error("SSE stream interrupted unexpectedly");
        assert_eq!(r.reason, FailoverReason::StreamInterrupted);
        assert_eq!(r.action, RecoveryAction::RetryImmediate);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_classify_unknown() {
        let c = make_classifier();
        let r = c.classify_http(418, &empty_headers(), "I'm a teapot");
        assert_eq!(r.reason, FailoverReason::Unknown);
        assert_eq!(r.action, RecoveryAction::Abort);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_custom_rule() {
        let mut c = make_classifier();
        c.add_rule(
            "special_error_42",
            FailoverReason::QuotaExceeded,
            RecoveryAction::Abort,
        );
        let r = c.classify_http(400, &empty_headers(), "got special_error_42 in response");
        assert_eq!(r.reason, FailoverReason::QuotaExceeded);
        assert_eq!(r.action, RecoveryAction::Abort);
    }

    #[test]
    fn test_is_retryable_true() {
        assert!(ErrorClassifier::is_retryable(&FailoverReason::RateLimit));
        assert!(ErrorClassifier::is_retryable(&FailoverReason::Overloaded));
        assert!(ErrorClassifier::is_retryable(
            &FailoverReason::NetworkTimeout
        ));
        assert!(ErrorClassifier::is_retryable(&FailoverReason::ServerError));
        assert!(ErrorClassifier::is_retryable(
            &FailoverReason::StreamInterrupted
        ));
    }

    #[test]
    fn test_is_retryable_false() {
        assert!(!ErrorClassifier::is_retryable(&FailoverReason::AuthError));
        assert!(!ErrorClassifier::is_retryable(
            &FailoverReason::BillingError
        ));
        assert!(!ErrorClassifier::is_retryable(
            &FailoverReason::ContentFilter
        ));
        assert!(!ErrorClassifier::is_retryable(
            &FailoverReason::PermissionDenied
        ));
        assert!(!ErrorClassifier::is_retryable(
            &FailoverReason::ModelNotFound
        ));
        assert!(!ErrorClassifier::is_retryable(
            &FailoverReason::ContextOverflow
        ));
        assert!(!ErrorClassifier::is_retryable(
            &FailoverReason::ParsingError
        ));
    }

    #[test]
    fn test_extract_retry_after_seconds() {
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "10".to_string());
        assert_eq!(
            ErrorClassifier::extract_retry_after_ms(&headers),
            Some(10000)
        );
    }

    #[test]
    fn test_extract_retry_after_ms() {
        // Floating-point format: 0.5 seconds = 500 ms
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "0.5".to_string());
        assert_eq!(ErrorClassifier::extract_retry_after_ms(&headers), Some(500));
    }

    #[test]
    fn test_extract_retry_after_missing() {
        assert_eq!(
            ErrorClassifier::extract_retry_after_ms(&empty_headers()),
            None
        );
    }

    #[test]
    fn test_suggest_fallback() {
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::AuthError),
            RecoveryAction::RotateCredential
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::ContextOverflow),
            RecoveryAction::CompressContext
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::ModelNotFound),
            RecoveryAction::FallbackModel
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::RegionBlocked),
            RecoveryAction::ChangeProvider
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::StreamInterrupted),
            RecoveryAction::RetryImmediate
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::SslError),
            RecoveryAction::ChangeProvider
        );
    }

    #[test]
    fn test_all_failover_reasons_have_recovery() {
        // Every FailoverReason must map to some RecoveryAction.
        let reasons = vec![
            FailoverReason::AuthError,
            FailoverReason::BillingError,
            FailoverReason::RateLimit,
            FailoverReason::Overloaded,
            FailoverReason::ContextOverflow,
            FailoverReason::ModelNotFound,
            FailoverReason::ThinkingSignature,
            FailoverReason::NetworkTimeout,
            FailoverReason::InvalidRequest,
            FailoverReason::ContentFilter,
            FailoverReason::PermissionDenied,
            FailoverReason::RegionBlocked,
            FailoverReason::QuotaExceeded,
            FailoverReason::ServerError,
            FailoverReason::ProxyError,
            FailoverReason::SslError,
            FailoverReason::ParsingError,
            FailoverReason::StreamInterrupted,
            FailoverReason::Unknown,
        ];
        for reason in reasons {
            let action = ErrorClassifier::suggest_fallback(&reason);
            // Just ensure it doesn't panic and returns something.
            let _ = format!("{action:?}");
        }
    }

    #[test]
    fn test_classification_serialization() {
        let c = make_classifier();
        let classification = c.classify_http(429, &empty_headers(), "");
        // Serialize → deserialize round-trip.
        let json = serde_json::to_string(&classification).expect("serialize");
        let deser: ErrorClassification = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deser.reason, classification.reason);
        assert_eq!(deser.action, classification.action);
        assert_eq!(deser.is_retryable, classification.is_retryable);
        assert_eq!(deser.retry_after_ms, classification.retry_after_ms);
    }

    #[test]
    fn test_classifier_default_rules() {
        // Default classifier has zero custom rules.
        let c = ErrorClassifier::default();
        assert!(c.custom_rules.is_empty());
    }

    #[test]
    fn test_transport_error_classification() {
        let c = make_classifier();

        let r = c.classify_transport_error("proxy connection refused");
        assert_eq!(r.reason, FailoverReason::ProxyError);

        let r = c.classify_transport_error("connection refused by remote");
        assert_eq!(r.reason, FailoverReason::NetworkTimeout);
        assert!(r.is_retryable);

        let r = c.classify_transport_error("failed to parse JSON response");
        assert_eq!(r.reason, FailoverReason::ParsingError);
        assert!(!r.is_retryable);

        // Completely unrecognised message → Unknown but retryable (safe default).
        let r = c.classify_transport_error("something weird happened");
        assert_eq!(r.reason, FailoverReason::Unknown);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_classify_502_and_503() {
        let c = make_classifier();
        let r502 = c.classify_http(502, &empty_headers(), "");
        assert_eq!(r502.reason, FailoverReason::ServerError);
        assert_eq!(r502.action, RecoveryAction::RetryImmediate);

        let r503 = c.classify_http(503, &empty_headers(), "");
        assert_eq!(r503.reason, FailoverReason::ServerError);
        assert_eq!(r503.action, RecoveryAction::RetryImmediate);
    }

    #[test]
    fn test_classify_408_timeout() {
        let c = make_classifier();
        let r = c.classify_http(408, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::NetworkTimeout);
        assert_eq!(r.action, RecoveryAction::RetryWithBackoff);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_body_pattern_overrides_status() {
        let c = make_classifier();
        // Even on a 200, a body containing model_not_found should be caught.
        let r = c.classify_http(
            200,
            &empty_headers(),
            r#"{"error": "model_not_found: gpt-5 does not exist"}"#,
        );
        assert_eq!(r.reason, FailoverReason::ModelNotFound);
    }

    #[test]
    fn test_classifiable_trait() {
        use super::Classifiable;
        let err = HakimiError::Transport("connection timed out".into());
        let c = err.classify();
        assert_eq!(c.reason, FailoverReason::NetworkTimeout);
        assert!(c.is_retryable);
    }

    #[test]
    fn test_classifiable_context_error() {
        use super::Classifiable;
        let err = HakimiError::Context("token limit exceeded".into());
        let c = err.classify();
        assert_eq!(c.reason, FailoverReason::ContextOverflow);
        assert_eq!(c.action, RecoveryAction::CompressContext);
    }

    #[test]
    fn test_classifiable_json_error() {
        use super::Classifiable;
        let err = HakimiError::Json(serde_json::from_str::<String>("invalid").unwrap_err());
        let c = err.classify();
        assert_eq!(c.reason, FailoverReason::ParsingError);
        assert!(!c.is_retryable);
    }

    #[test]
    fn test_retry_after_case_insensitive_header() {
        let mut headers = HashMap::new();
        headers.insert("Retry-After".to_string(), "3".to_string());
        assert_eq!(
            ErrorClassifier::extract_retry_after_ms(&headers),
            Some(3000)
        );
    }

    #[test]
    fn test_classify_status_451() {
        let (reason, action) = ErrorClassifier::classify_status(451);
        assert_eq!(reason, FailoverReason::RegionBlocked);
        assert_eq!(action, RecoveryAction::ChangeProvider);
    }

    // --- Additional tests (12+) ---

    #[test]
    fn test_classify_504_gateway_timeout() {
        let c = make_classifier();
        let r = c.classify_http(504, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::NetworkTimeout);
        assert_eq!(r.action, RecoveryAction::RetryWithBackoff);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_classify_400_invalid_request() {
        let c = make_classifier();
        let r = c.classify_http(400, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::InvalidRequest);
        assert_eq!(r.action, RecoveryAction::Abort);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_body_model_not_found_alternate() {
        let c = make_classifier();
        // "model not found" variant (with spaces, not underscore)
        let r = c.classify_http(
            404,
            &empty_headers(),
            r#"{"error": "model not found: xyz-99"}"#,
        );
        assert_eq!(r.reason, FailoverReason::ModelNotFound);
        assert_eq!(r.action, RecoveryAction::FallbackModel);
    }

    #[test]
    fn test_classify_body_thinking_signature() {
        let c = make_classifier();
        let r = c.classify_http(
            400,
            &empty_headers(),
            r#"{"error": "thinking_signature validation failed"}"#,
        );
        assert_eq!(r.reason, FailoverReason::ThinkingSignature);
        assert_eq!(r.action, RecoveryAction::Abort);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_body_thinking_signature_alt() {
        let c = make_classifier();
        let r = c.classify_http(400, &empty_headers(), "Error: thinking signature mismatch");
        assert_eq!(r.reason, FailoverReason::ThinkingSignature);
    }

    #[test]
    fn test_classify_body_quota_exceeded() {
        let c = make_classifier();
        let r = c.classify_http(
            429,
            &empty_headers(),
            r#"{"error": "quota_exceeded: monthly limit reached"}"#,
        );
        assert_eq!(r.reason, FailoverReason::QuotaExceeded);
        assert_eq!(r.action, RecoveryAction::Abort);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_body_quota_exceeded_alt() {
        let c = make_classifier();
        let r = c.classify_http(429, &empty_headers(), "quota exceeded for this account");
        assert_eq!(r.reason, FailoverReason::QuotaExceeded);
    }

    #[test]
    fn test_classify_body_region_blocked() {
        let c = make_classifier();
        let r = c.classify_http(
            403,
            &empty_headers(),
            "Access denied: this region is blocked",
        );
        assert_eq!(r.reason, FailoverReason::RegionBlocked);
        assert_eq!(r.action, RecoveryAction::ChangeProvider);
    }

    #[test]
    fn test_classify_body_context_window_variant() {
        let c = make_classifier();
        let r = c.classify_http(
            400,
            &empty_headers(),
            "Error: maximum context length is 128000 tokens",
        );
        assert_eq!(r.reason, FailoverReason::ContextOverflow);
        assert_eq!(r.action, RecoveryAction::CompressContext);
    }

    #[test]
    fn test_classify_body_content_filter_alt() {
        let c = make_classifier();
        let r = c.classify_http(
            400,
            &empty_headers(),
            "The request was blocked by the content filter",
        );
        assert_eq!(r.reason, FailoverReason::ContentFilter);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_transport_tls_keyword() {
        let c = make_classifier();
        let r = c.classify_transport_error("TLS handshake failed with peer");
        assert_eq!(r.reason, FailoverReason::SslError);
        assert_eq!(r.action, RecoveryAction::ChangeProvider);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_transport_certificate_keyword() {
        let c = make_classifier();
        let r = c.classify_transport_error("certificate has expired");
        assert_eq!(r.reason, FailoverReason::SslError);
    }

    #[test]
    fn test_transport_deadline_exceeded() {
        let c = make_classifier();
        let r = c.classify_transport_error("request deadline exceeded");
        assert_eq!(r.reason, FailoverReason::NetworkTimeout);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_transport_connection_reset() {
        let c = make_classifier();
        let r = c.classify_transport_error("connection reset by peer");
        assert_eq!(r.reason, FailoverReason::NetworkTimeout);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_transport_broken_pipe() {
        let c = make_classifier();
        let r = c.classify_transport_error("broken pipe on write");
        assert_eq!(r.reason, FailoverReason::NetworkTimeout);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_transport_stream_broken() {
        let c = make_classifier();
        let r = c.classify_transport_error("SSE stream broken mid-response");
        assert_eq!(r.reason, FailoverReason::StreamInterrupted);
        assert_eq!(r.action, RecoveryAction::RetryImmediate);
    }

    #[test]
    fn test_transport_stream_closed() {
        let c = make_classifier();
        let r = c.classify_transport_error("stream closed unexpectedly");
        assert_eq!(r.reason, FailoverReason::StreamInterrupted);
        assert!(r.is_retryable);
    }

    #[test]
    fn test_transport_deserialize_error() {
        let c = make_classifier();
        let r = c.classify_transport_error("failed to deserialize response body");
        assert_eq!(r.reason, FailoverReason::ParsingError);
        assert_eq!(r.action, RecoveryAction::Abort);
    }

    #[test]
    fn test_transport_invalid_json() {
        let c = make_classifier();
        let r = c.classify_transport_error("invalid JSON at line 1 col 10");
        assert_eq!(r.reason, FailoverReason::ParsingError);
        assert!(!r.is_retryable);
    }

    #[test]
    fn test_classify_status_other_5xx() {
        // 501, 505, 599 – all should be ServerError via the catch-all 5xx arm
        for status in [501, 505, 506, 599] {
            let (reason, action) = ErrorClassifier::classify_status(status);
            assert_eq!(reason, FailoverReason::ServerError, "status {status}");
            assert_eq!(action, RecoveryAction::RetryWithBackoff, "status {status}");
        }
    }

    #[test]
    fn test_classify_status_unknown_codes() {
        // Non-standard codes → Unknown + Abort
        for status in [100, 200, 201, 204, 301, 302, 418, 499] {
            let (reason, action) = ErrorClassifier::classify_status(status);
            assert_eq!(reason, FailoverReason::Unknown, "status {status}");
            assert_eq!(action, RecoveryAction::Abort, "status {status}");
        }
    }

    #[test]
    fn test_retry_after_all_caps_header() {
        let mut headers = HashMap::new();
        headers.insert("RETRY-AFTER".to_string(), "7".to_string());
        assert_eq!(
            ErrorClassifier::extract_retry_after_ms(&headers),
            Some(7000)
        );
    }

    #[test]
    fn test_retry_after_unparseable_returns_none() {
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "not-a-number".to_string());
        assert_eq!(ErrorClassifier::extract_retry_after_ms(&headers), None);
    }

    #[test]
    fn test_retry_after_with_whitespace() {
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "  15  ".to_string());
        assert_eq!(
            ErrorClassifier::extract_retry_after_ms(&headers),
            Some(15000)
        );
    }

    #[test]
    fn test_custom_rule_on_transport_error() {
        let mut c = make_classifier();
        c.add_rule(
            "custom_transport_fail",
            FailoverReason::QuotaExceeded,
            RecoveryAction::Abort,
        );
        let r = c.classify_transport_error("got custom_transport_fail error");
        assert_eq!(r.reason, FailoverReason::QuotaExceeded);
        assert_eq!(r.action, RecoveryAction::Abort);
    }

    #[test]
    fn test_custom_rule_overrides_body_pattern() {
        let mut c = make_classifier();
        // Custom rule should take precedence over built-in body patterns
        c.add_rule(
            "context_length_exceeded",
            FailoverReason::RateLimit,
            RecoveryAction::RetryWithBackoff,
        );
        let r = c.classify_http(400, &empty_headers(), "context_length_exceeded: limit hit");
        assert_eq!(r.reason, FailoverReason::RateLimit);
        assert_eq!(r.action, RecoveryAction::RetryWithBackoff);
    }

    #[test]
    fn test_classify_500_with_retry_after_header() {
        let c = make_classifier();
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "30".to_string());
        let r = c.classify_http(500, &headers, "");
        assert_eq!(r.reason, FailoverReason::ServerError);
        assert_eq!(r.retry_after_ms, Some(30000));
        assert!(r.is_retryable);
    }

    #[test]
    fn test_classify_empty_body_and_headers() {
        let c = make_classifier();
        let r = c.classify_http(200, &empty_headers(), "");
        assert_eq!(r.reason, FailoverReason::Unknown);
        assert_eq!(r.action, RecoveryAction::Abort);
        assert!(!r.is_retryable);
        assert_eq!(r.retry_after_ms, None);
    }

    #[test]
    fn test_suggest_fallback_all_remaining_reasons() {
        // Cover the remaining reasons not in test_suggest_fallback
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::BillingError),
            RecoveryAction::Abort
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::RateLimit),
            RecoveryAction::RetryWithBackoff
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::Overloaded),
            RecoveryAction::RetryWithBackoff
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::ThinkingSignature),
            RecoveryAction::Abort
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::NetworkTimeout),
            RecoveryAction::RetryWithBackoff
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::InvalidRequest),
            RecoveryAction::Abort
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::ContentFilter),
            RecoveryAction::Abort
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::PermissionDenied),
            RecoveryAction::Abort
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::QuotaExceeded),
            RecoveryAction::Abort
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::ServerError),
            RecoveryAction::RetryWithBackoff
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::ProxyError),
            RecoveryAction::ChangeProvider
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::ParsingError),
            RecoveryAction::Abort
        );
        assert_eq!(
            ErrorClassifier::suggest_fallback(&FailoverReason::Unknown),
            RecoveryAction::RetryWithBackoff
        );
    }
}
