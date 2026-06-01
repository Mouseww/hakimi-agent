use std::collections::VecDeque;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_LOG_FILE: &str = "gateway-events.log";
const MAX_FIELD_CHARS: usize = 2048;

/// Return the persistent gateway lifecycle event log path.
///
/// `HAKIMI_GATEWAY_EVENTS_LOG` is intentionally supported for operators and
/// tests that need a profile-specific or service-specific diagnostics file.
pub fn gateway_events_log_path() -> PathBuf {
    if let Ok(path) = env::var("HAKIMI_GATEWAY_EVENTS_LOG") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hakimi")
        .join("logs")
        .join(DEFAULT_LOG_FILE)
}

/// Best-effort gateway lifecycle event recording.
///
/// Logging failures never affect message delivery. The structured single-line
/// format keeps the file tail-friendly while remaining portable on Windows.
pub fn record_gateway_event(
    event: &str,
    platform: Option<&str>,
    bot_id: Option<&str>,
    chat_id: Option<&str>,
    detail: impl AsRef<str>,
) {
    if let Err(err) = append_gateway_event(
        event,
        platform.unwrap_or("-"),
        bot_id.unwrap_or("-"),
        chat_id.unwrap_or("-"),
        detail.as_ref(),
    ) {
        tracing::debug!(error = %err, event, "failed to write gateway lifecycle event");
    }
}

pub fn append_gateway_event(
    event: &str,
    platform: &str,
    bot_id: &str,
    chat_id: &str,
    detail: &str,
) -> io::Result<()> {
    append_gateway_event_to(
        &gateway_events_log_path(),
        event,
        platform,
        bot_id,
        chat_id,
        detail,
    )
}

fn append_gateway_event_to(
    path: &Path,
    event: &str,
    platform: &str,
    bot_id: &str,
    chat_id: &str,
    detail: &str,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "ts={} event={} platform={} bot_id={} chat_id={} detail={}",
        unix_timestamp_secs(),
        clean_field(event),
        clean_field(platform),
        clean_field(bot_id),
        clean_field(chat_id),
        clean_field(detail)
    )
}

pub fn read_recent_gateway_events(limit: usize) -> io::Result<String> {
    read_recent_lines(&gateway_events_log_path(), limit)
}

/// Read the last `limit` lines from a text log without shelling out to `tail`.
pub fn read_recent_lines(path: &Path, limit: usize) -> io::Result<String> {
    if limit == 0 || !path.exists() {
        return Ok(String::new());
    }

    let file = OpenOptions::new().read(true).open(path)?;
    let mut recent = VecDeque::with_capacity(limit);
    for line in BufReader::new(file).lines() {
        let line = line?;
        if recent.len() == limit {
            recent.pop_front();
        }
        recent.push_back(line);
    }

    Ok(recent.into_iter().collect::<Vec<_>>().join("\n"))
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn clean_field(value: &str) -> String {
    let normalized = value
        .replace(['\r', '\n', '\t'], " ")
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    let redacted = redact_sensitive_text(&normalized);
    if redacted.chars().count() <= MAX_FIELD_CHARS {
        return redacted;
    }

    let mut capped = redacted.chars().take(MAX_FIELD_CHARS).collect::<String>();
    capped.push_str("...");
    capped
}

fn redact_sensitive_text(input: &str) -> String {
    input
        .split_whitespace()
        .map(redact_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_word(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if lower == "authorization:" {
        return "Authorization:***".to_string();
    }
    if lower == "bearer" {
        return word.to_string();
    }
    if lower.starts_with("bearer ") {
        return "Bearer ***".to_string();
    }
    if lower.starts_with("sk-")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("xoxb-")
        || lower.starts_with("xoxp-")
    {
        return "***".to_string();
    }

    for marker in [
        "token",
        "secret",
        "api_key",
        "apikey",
        "password",
        "authorization",
        "access_key",
    ] {
        if lower.contains(marker) {
            if let Some((key, _)) = word.split_once('=') {
                return format!("{key}=***");
            }
            if let Some((key, _)) = word.split_once(':') {
                return format!("{key}:***");
            }
            return "***".to_string();
        }
    }

    word.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "hakimi-gateway-{name}-{}.log",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn lifecycle_log_redacts_sensitive_detail() {
        let path = temp_log_path("redact");
        append_gateway_event_to(
            &path,
            "route.error",
            "telegram",
            "default",
            "chat",
            "failed token=secret-value Authorization: Bearer sk-test",
        )
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("event=route.error"));
        assert!(content.contains("token=***"));
        assert!(content.contains("Authorization:***"));
        assert!(!content.contains("secret-value"));
        assert!(!content.contains("sk-test"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_recent_lines_without_tail() {
        let path = temp_log_path("recent");
        fs::write(&path, "one\ntwo\nthree\nfour\n").unwrap();

        let recent = read_recent_lines(&path, 2).unwrap();
        assert_eq!(recent, "three\nfour");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_log_returns_empty_text() {
        let path = temp_log_path("missing");
        let recent = read_recent_lines(&path, 10).unwrap();
        assert!(recent.is_empty());
    }
}
