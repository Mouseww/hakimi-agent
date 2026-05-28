//! Secret redaction helpers shared across runtime and tool-output boundaries.
//!
//! The patterns intentionally cover high-confidence credential shapes and
//! sensitive key/value carriers. Non-matching text passes through unchanged.

use regex::{Captures, Regex};
use std::sync::LazyLock;

const MASK_PLACEHOLDER: &str = "***";

const SENSITIVE_QUERY_KEYS: &[&str] = &[
    "access_token",
    "refresh_token",
    "id_token",
    "token",
    "api_key",
    "apikey",
    "client_secret",
    "password",
    "auth",
    "jwt",
    "session",
    "secret",
    "key",
    "code",
    "signature",
    "x-amz-signature",
];

static PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        (
            sk-[A-Za-z0-9_-]{10,}|
            ghp_[A-Za-z0-9]{10,}|
            github_pat_[A-Za-z0-9_]{10,}|
            gho_[A-Za-z0-9]{10,}|
            ghu_[A-Za-z0-9]{10,}|
            ghs_[A-Za-z0-9]{10,}|
            ghr_[A-Za-z0-9]{10,}|
            xox[baprs]-[A-Za-z0-9-]{10,}|
            AIza[A-Za-z0-9_-]{30,}|
            pplx-[A-Za-z0-9]{10,}|
            fal_[A-Za-z0-9_-]{10,}|
            fc-[A-Za-z0-9]{10,}|
            bb_live_[A-Za-z0-9_-]{10,}|
            gAAAA[A-Za-z0-9_=-]{20,}|
            AKIA[A-Z0-9]{16}|
            sk_live_[A-Za-z0-9]{10,}|
            sk_test_[A-Za-z0-9]{10,}|
            rk_live_[A-Za-z0-9]{10,}|
            SG\.[A-Za-z0-9_-]{10,}|
            hf_[A-Za-z0-9]{10,}|
            r8_[A-Za-z0-9]{10,}|
            npm_[A-Za-z0-9]{10,}|
            pypi-[A-Za-z0-9_-]{10,}|
            dop_v1_[A-Za-z0-9]{10,}|
            doo_v1_[A-Za-z0-9]{10,}|
            am_[A-Za-z0-9_-]{10,}|
            sk_[A-Za-z0-9_]{10,}|
            tvly-[A-Za-z0-9]{10,}|
            exa_[A-Za-z0-9]{10,}|
            gsk_[A-Za-z0-9]{10,}|
            syt_[A-Za-z0-9]{10,}|
            retaindb_[A-Za-z0-9]{10,}|
            hsk-[A-Za-z0-9]{10,}|
            mem0_[A-Za-z0-9]{10,}|
            brv_[A-Za-z0-9]{10,}|
            xai-[A-Za-z0-9]{30,}
        )
        ",
    )
    .expect("valid prefix redaction regex")
});

static ENV_ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"\b([A-Z0-9_]{0,50}(?:API_?KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|AUTH)[A-Z0-9_]{0,50})\s*=\s*(['"]?)([^'"\s]+)(['"]?)"#,
    )
    .expect("valid env assignment redaction regex")
});

static JSON_FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)("?(?:api_?key|apikey|token|secret|password|access_token|refresh_token|auth_token|bearer|secret_value|raw_secret|secret_input|key_material|authorization)"?)\s*:\s*"([^"]+)""#,
    )
    .expect("valid json field redaction regex")
});

static AUTH_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(Authorization:\s*Bearer\s+)(\S+)")
        .expect("valid authorization redaction regex")
});

static TELEGRAM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(bot)?(\d{8,}):([-A-Za-z0-9_]{30,})")
        .expect("valid telegram token redaction regex")
});

static PRIVATE_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----BEGIN[A-Z ]*PRIVATE KEY-----[\s\S]*?-----END[A-Z ]*PRIVATE KEY-----")
        .expect("valid private key redaction regex")
});

static DB_CONNSTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)((?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp)://[^:]+:)([^@]+)(@)")
        .expect("valid connection string redaction regex")
});

static URL_USERINFO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(https?|wss?|ftp)://([^/\s:@]+):([^/\s@]+)@")
        .expect("valid url userinfo redaction regex")
});

static URL_WITH_QUERY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(https?|wss?|ftp)://([^\s/?#]+)([^\s?#]*)\?([^\s#]+)(#\S*)?")
        .expect("valid url query redaction regex")
});

static HTTP_REQUEST_TARGET_QUERY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b((?:GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS|TRACE|CONNECT)\s+[^ \t\r\n"']*?)\?([^ \t\r\n"']+)"#,
    )
    .expect("valid request target redaction regex")
});

static FORM_BODY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z_][A-Za-z0-9_.-]*=[^&\s]*(?:&[A-Za-z_][A-Za-z0-9_.-]*=[^&\s]*)+$")
        .expect("valid form body redaction regex")
});

static JWT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}(?:\.[A-Za-z0-9_=-]{4,}){0,2}\b")
        .expect("valid jwt redaction regex")
});

/// Mask a secret while preserving a small prefix/suffix for diagnostics.
pub fn mask_secret(value: &str) -> String {
    if value.is_empty() || value.len() < 18 {
        return MASK_PLACEHOLDER.to_string();
    }

    let head: String = value.chars().take(6).collect();
    let tail: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}...{tail}")
}

/// Redact secrets in arbitrary text.
pub fn redact_sensitive_text(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut redacted = text.to_string();

    if has_known_prefix_hint(&redacted) {
        redacted = PREFIX_RE
            .replace_all(&redacted, |caps: &Captures<'_>| mask_secret(&caps[1]))
            .into_owned();
    }

    if redacted.contains('=') {
        redacted = ENV_ASSIGN_RE
            .replace_all(&redacted, |caps: &Captures<'_>| {
                let opening_quote = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let closing_quote = caps.get(4).map(|m| m.as_str()).unwrap_or(opening_quote);
                format!(
                    "{}={}{}{}",
                    &caps[1],
                    opening_quote,
                    mask_secret(&caps[3]),
                    closing_quote
                )
            })
            .into_owned();
    }

    if redacted.contains(':') && redacted.contains('"') {
        redacted = JSON_FIELD_RE
            .replace_all(&redacted, |caps: &Captures<'_>| {
                format!("{}: \"{}\"", &caps[1], mask_secret(&caps[2]))
            })
            .into_owned();
    }

    if redacted.contains("uthorization") || redacted.contains("UTHORIZATION") {
        redacted = AUTH_HEADER_RE
            .replace_all(&redacted, |caps: &Captures<'_>| {
                format!("{}{}", &caps[1], mask_secret(&caps[2]))
            })
            .into_owned();
    }

    if redacted.contains(':') {
        redacted = TELEGRAM_RE
            .replace_all(&redacted, |caps: &Captures<'_>| {
                let bot = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                format!("{}{}:{}", bot, &caps[2], MASK_PLACEHOLDER)
            })
            .into_owned();
    }

    if redacted.contains("BEGIN") && redacted.contains("-----") {
        redacted = PRIVATE_KEY_RE
            .replace_all(&redacted, "[REDACTED PRIVATE KEY]")
            .into_owned();
    }

    if redacted.contains("://") {
        redacted = DB_CONNSTR_RE
            .replace_all(&redacted, |caps: &Captures<'_>| {
                format!("{}{}{}", &caps[1], MASK_PLACEHOLDER, &caps[3])
            })
            .into_owned();
        redacted = URL_USERINFO_RE
            .replace_all(&redacted, |caps: &Captures<'_>| {
                format!("{}://{}:{}@", &caps[1], &caps[2], MASK_PLACEHOLDER)
            })
            .into_owned();

        if redacted.contains('?') {
            redacted = URL_WITH_QUERY_RE
                .replace_all(&redacted, |caps: &Captures<'_>| {
                    let fragment = caps.get(5).map(|m| m.as_str()).unwrap_or("");
                    format!(
                        "{}://{}{}?{}{}",
                        &caps[1],
                        &caps[2],
                        &caps[3],
                        redact_query_string(&caps[4]),
                        fragment
                    )
                })
                .into_owned();
        }
    }

    if redacted.contains("eyJ") {
        redacted = JWT_RE
            .replace_all(&redacted, |caps: &Captures<'_>| mask_secret(&caps[0]))
            .into_owned();
    }

    if redacted.contains('?') && redacted.contains('=') && has_http_method_hint(&redacted) {
        redacted = HTTP_REQUEST_TARGET_QUERY_RE
            .replace_all(&redacted, |caps: &Captures<'_>| {
                format!("{}?{}", &caps[1], redact_query_string(&caps[2]))
            })
            .into_owned();
    }

    if redacted.contains('&') && redacted.contains('=') {
        let trimmed = redacted.trim();
        if !trimmed.contains('\n') && FORM_BODY_RE.is_match(trimmed) {
            redacted = redact_query_string(trimmed);
        }
    }

    redacted
}

/// Small compatibility wrapper for callers that prefer an object.
#[derive(Debug, Default, Clone, Copy)]
pub struct SecretRedactor;

impl SecretRedactor {
    pub fn new() -> Self {
        Self
    }

    pub fn redact(&self, text: &str) -> String {
        redact_sensitive_text(text)
    }
}

fn redact_query_string(query: &str) -> String {
    query
        .split('&')
        .map(|pair| {
            let Some((key, value)) = pair.split_once('=') else {
                return pair.to_string();
            };
            if is_sensitive_query_key(key) {
                format!("{key}={MASK_PLACEHOLDER}")
            } else {
                format!("{key}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn is_sensitive_query_key(key: &str) -> bool {
    SENSITIVE_QUERY_KEYS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(key))
}

fn has_known_prefix_hint(text: &str) -> bool {
    [
        "sk-",
        "ghp_",
        "github_pat_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "xox",
        "AIza",
        "pplx-",
        "fal_",
        "fc-",
        "bb_live_",
        "gAAAA",
        "AKIA",
        "sk_live_",
        "sk_test_",
        "rk_live_",
        "SG.",
        "hf_",
        "r8_",
        "npm_",
        "pypi-",
        "dop_v1_",
        "doo_v1_",
        "am_",
        "sk_",
        "tvly-",
        "exa_",
        "gsk_",
        "syt_",
        "retaindb_",
        "hsk-",
        "mem0_",
        "brv_",
        "xai-",
    ]
    .iter()
    .any(|prefix| text.contains(prefix))
}

fn has_http_method_hint(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    [
        "GET ", "POST ", "PUT ", "PATCH ", "DELETE ", "HEAD ", "OPTIONS ", "TRACE ", "CONNECT ",
    ]
    .iter()
    .any(|method| upper.contains(method))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_known_provider_prefixes() {
        let token = format!("{}{}", "sk-proj-", "abcdefghijklmnopqrstuvwxyz123456");
        let text = format!("OPENAI_API_KEY={token}");
        let redacted = redact_sensitive_text(&text);

        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz123456"));
        assert!(redacted.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn masks_authorization_and_json_fields() {
        let bearer = format!("{}{}", "ghp_", "abcdefghijklmnopqrstuvwxyz1234567890");
        let jwt = format!("{}{}", "eyJ", "abcdefghijklmnopqrstuvwxyz.abcdefghi");
        let text = format!("Authorization: Bearer {bearer}\n{{\"access_token\": \"{jwt}\"}}");
        let redacted = redact_sensitive_text(&text);

        assert!(!redacted.contains(&bearer));
        assert!(!redacted.contains("eyJabcdefghijklmnopqrstuvwxyz"));
        assert!(redacted.contains("Authorization: Bearer"));
        assert!(redacted.contains(r#""access_token": "eyJabc...fghi""#));
    }

    #[test]
    fn redacts_sensitive_url_params_and_userinfo() {
        let text = "GET /hook?code=abc123&state=ok HTTP/1.1 https://user:pass@example.test/cb?api_key=opaque&x=1";
        let redacted = redact_sensitive_text(text);

        assert!(redacted.contains("code=***"));
        assert!(redacted.contains("state=ok"));
        assert!(redacted.contains("https://user:***@example.test/cb?api_key=***&x=1"));
    }

    #[test]
    fn redacts_private_keys_and_connection_strings() {
        let text = "postgres://app:secretpass@db.local/main\n-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----";
        let redacted = redact_sensitive_text(text);

        assert!(redacted.contains("postgres://app:***@db.local/main"));
        assert!(redacted.contains("[REDACTED PRIVATE KEY]"));
        assert!(!redacted.contains("secretpass"));
    }

    #[test]
    fn leaves_non_sensitive_counts_alone() {
        let text = "token_count=123&session_id=abc url=https://example.test/?state=ok";
        let redacted = redact_sensitive_text(text);

        assert_eq!(redacted, text);
    }
}
