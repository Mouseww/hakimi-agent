use hakimi_common::redact_sensitive_text;
use regex::{Captures, Regex};
use std::sync::LazyLock;

const ERROR_SNIPPET_CHARS: usize = 200;

static MCP_ERROR_CREDENTIAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)
        \b(bearer\s+)([^\s&,;"']{4,})
        |
        \b((?:api_?key|token|password|secret|client_secret|access_token|refresh_token|key)=)([^\s&,;"']{1,255})
        "#,
    )
    .expect("valid MCP error credential regex")
});

pub(crate) fn sanitize_mcp_error(text: &str) -> String {
    let redacted = redact_sensitive_text(text);
    MCP_ERROR_CREDENTIAL_RE
        .replace_all(&redacted, |caps: &Captures<'_>| {
            if let Some(prefix) = caps.get(1) {
                format!("{}***", prefix.as_str())
            } else if let Some(prefix) = caps.get(3) {
                format!("{}***", prefix.as_str())
            } else {
                "***".to_string()
            }
        })
        .into_owned()
}

pub(crate) fn sanitized_error_snippet(text: &str) -> String {
    sanitize_mcp_error(text)
        .chars()
        .take(ERROR_SNIPPET_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_known_credentials_in_mcp_errors() {
        let token = format!("{}{}", "ghp_", "abcdefghijklmnopqrstuvwxyz123456");
        let text = format!("remote MCP failed: Authorization: Bearer {token}; password=secretpass");

        let redacted = sanitize_mcp_error(&text);

        assert!(!redacted.contains(&token));
        assert!(!redacted.contains("secretpass"));
        assert!(redacted.contains("Authorization: Bearer"));
        assert!(redacted.contains("password="));
    }

    #[test]
    fn snippets_are_char_bounded_without_leaking_tokens() {
        let token = format!("{}{}", "sk-", "abcdefghijklmnopqrstuvwxyz1234567890");
        let text = format!(
            "{} token={token}&state=ok {}",
            "界".repeat(188),
            "尾".repeat(300)
        );

        let snippet = sanitized_error_snippet(&text);

        assert!(!snippet.contains(&token));
        assert!(!snippet.contains("abcdefghijklmnopqrstuvwxyz1234567890"));
        assert!(snippet.contains("token="));
        assert!(snippet.chars().count() <= 200);
    }

    #[test]
    fn sanitizes_opaque_mcp_key_value_shapes() {
        let text = "remote MCP failed: token=opaque-value password=plain secret=s3";

        let redacted = sanitize_mcp_error(text);

        assert_eq!(
            redacted,
            "remote MCP failed: token=*** password=*** secret=***"
        );
    }
}
