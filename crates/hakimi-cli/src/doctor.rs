//! Diagnostics CLI command for troubleshooting Hakimi Agent setup.
//!
//! Checks configuration, dependencies, environment, API connectivity,
//! and reports each check with colored status indicators.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Diagnostic check severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStatus {
    /// Check passed.
    Pass,
    /// Check failed — something is broken.
    Fail,
    /// Warning — non-critical issue.
    Warn,
}

/// A single diagnostic check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResult {
    /// Name of the check.
    pub name: String,
    /// Status of the check.
    pub status: CheckStatus,
    /// Human-readable message.
    pub message: String,
    /// Suggested fix if the check failed or warned.
    pub fix: Option<String>,
}

// ---------------------------------------------------------------------------
// Hakimi directory helpers
// ---------------------------------------------------------------------------

fn hakimi_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".hakimi"))
        .unwrap_or_else(|| PathBuf::from(".hakimi"))
}

fn config_path() -> PathBuf {
    hakimi_dir().join("config.yaml")
}

fn managed_binary_path_for_home(home: &Path) -> PathBuf {
    home.join(".hakimi").join("bin").join("hakimi")
}

fn default_managed_binary_path() -> PathBuf {
    dirs::home_dir()
        .map(|home| managed_binary_path_for_home(&home))
        .unwrap_or_else(|| PathBuf::from("/root/.hakimi/bin/hakimi"))
}

fn session_db_path() -> PathBuf {
    hakimi_dir().join("sessions.db")
}

fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn path_status(path: &Path, label: &str) -> DiagnosticResult {
    if path.exists() {
        DiagnosticResult {
            name: label.to_string(),
            status: CheckStatus::Pass,
            message: format!("Found at {}", path.display()),
            fix: None,
        }
    } else {
        DiagnosticResult {
            name: label.to_string(),
            status: CheckStatus::Warn,
            message: format!("Not found at {}", path.display()),
            fix: None,
        }
    }
}

fn service_status_result(service: &str, label: &str) -> DiagnosticResult {
    let active = command_stdout("systemctl", &["is-active", service])
        .unwrap_or_else(|| "unavailable".to_string());
    let enabled = command_stdout("systemctl", &["is-enabled", service])
        .unwrap_or_else(|| "unavailable".to_string());
    let exec_start = command_stdout(
        "systemctl",
        &["show", service, "-p", "ExecStart", "--value"],
    )
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "ExecStart unavailable".to_string());
    let status = if active == "active" {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };

    DiagnosticResult {
        name: label.to_string(),
        status,
        message: format!("active={active}, enabled={enabled}, {exec_start}"),
        fix: None,
    }
}

fn check_managed_binary() -> DiagnosticResult {
    path_status(&default_managed_binary_path(), "Managed binary")
}

fn check_path_shim() -> DiagnosticResult {
    let shim = Path::new("/usr/local/bin/hakimi");
    let managed = default_managed_binary_path();
    if !shim.exists() {
        return DiagnosticResult {
            name: "PATH shim".to_string(),
            status: CheckStatus::Warn,
            message: "/usr/local/bin/hakimi not found".to_string(),
            fix: Some(format!(
                "Point /usr/local/bin/hakimi to {}",
                managed.display()
            )),
        };
    }

    let target = std::fs::read_link(shim).unwrap_or_else(|_| shim.to_path_buf());
    let status = if target == managed
        || std::fs::canonicalize(shim).ok() == std::fs::canonicalize(&managed).ok()
    {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };
    DiagnosticResult {
        name: "PATH shim".to_string(),
        status,
        message: format!("{} -> {}", shim.display(), target.display()),
        fix: (status != CheckStatus::Pass)
            .then(|| format!("Expected shim target: {}", managed.display())),
    }
}

fn check_hakimi_version() -> DiagnosticResult {
    DiagnosticResult {
        name: "Version".to_string(),
        status: CheckStatus::Pass,
        message: env!("CARGO_PKG_VERSION").to_string(),
        fix: None,
    }
}

fn check_gateway_processes() -> DiagnosticResult {
    let output = command_stdout("pgrep", &["-af", "hakimi.*gateway"]);
    let lines: Vec<String> = output
        .as_deref()
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains("pgrep -af"))
        .map(ToString::to_string)
        .collect();
    let status = if lines.len() <= 1 {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };
    let message = if lines.is_empty() {
        "No gateway process found".to_string()
    } else {
        format!("{} gateway process(es): {}", lines.len(), lines.join(" | "))
    };
    DiagnosticResult {
        name: "Duplicate gateway processes".to_string(),
        status,
        message,
        fix: (lines.len() > 1).then(|| "Keep only one hakimi.service gateway process".to_string()),
    }
}

fn check_port_3005() -> DiagnosticResult {
    let listener = command_stdout("ss", &["-ltnp", "sport", "=", ":3005"])
        .or_else(|| command_stdout("lsof", &["-i", ":3005"]));
    match listener.filter(|value| value.lines().count() > 1 || value.contains("LISTEN")) {
        Some(value) => DiagnosticResult {
            name: "Port 3005".to_string(),
            status: CheckStatus::Warn,
            message: value.lines().collect::<Vec<_>>().join(" | "),
            fix: Some("Verify only the intended Gateway owns port 3005".to_string()),
        },
        None => DiagnosticResult {
            name: "Port 3005".to_string(),
            status: CheckStatus::Pass,
            message: "No listener detected".to_string(),
            fix: None,
        },
    }
}

fn check_session_db() -> DiagnosticResult {
    path_status(&session_db_path(), "Session DB")
}

// ---------------------------------------------------------------------------
// Individual checks
// ---------------------------------------------------------------------------

fn check_hakimi_dir() -> DiagnosticResult {
    let dir = hakimi_dir();
    if dir.exists() && dir.is_dir() {
        DiagnosticResult {
            name: "~/.hakimi/ directory".to_string(),
            status: CheckStatus::Pass,
            message: format!("Found at {}", dir.display()),
            fix: None,
        }
    } else {
        DiagnosticResult {
            name: "~/.hakimi/ directory".to_string(),
            status: CheckStatus::Fail,
            message: "Directory does not exist".to_string(),
            fix: Some("Run `hakimi setup` to create it".to_string()),
        }
    }
}

fn check_config_file() -> DiagnosticResult {
    let path = config_path();
    if !path.exists() {
        return DiagnosticResult {
            name: "config.yaml".to_string(),
            status: CheckStatus::Fail,
            message: "Config file not found".to_string(),
            fix: Some("Run `hakimi setup` to create a config".to_string()),
        };
    }

    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return DiagnosticResult {
                name: "config.yaml".to_string(),
                status: CheckStatus::Fail,
                message: format!("Cannot read config file: {}", e),
                fix: Some("Check file permissions".to_string()),
            };
        }
    };

    // Validate YAML
    match serde_yaml::from_str::<serde_yaml::Value>(&contents) {
        Ok(_) => DiagnosticResult {
            name: "config.yaml".to_string(),
            status: CheckStatus::Pass,
            message: format!("Valid YAML at {}", path.display()),
            fix: None,
        },
        Err(e) => DiagnosticResult {
            name: "config.yaml".to_string(),
            status: CheckStatus::Fail,
            message: format!("Invalid YAML: {}", e),
            fix: Some("Fix the YAML syntax or run `hakimi setup` to regenerate".to_string()),
        },
    }
}

fn check_api_key() -> DiagnosticResult {
    // Check env vars first
    for var in &[
        "HAKIMI_API_KEY",
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
    ] {
        if let Ok(val) = std::env::var(var)
            && !val.is_empty()
        {
            let masked = format!(
                "{}...{}",
                &val[..4.min(val.len())],
                if val.len() > 4 {
                    &val[val.len() - 4..]
                } else {
                    ""
                }
            );
            return DiagnosticResult {
                name: "API key".to_string(),
                status: CheckStatus::Pass,
                message: format!("Found in ${} ({})", var, masked),
                fix: None,
            };
        }
    }

    // Check config file
    let path = config_path();
    if path.exists()
        && let Ok(contents) = std::fs::read_to_string(&path)
        && let Ok(config) = serde_yaml::from_str::<hakimi_config::HakimiConfig>(&contents)
        && !config.delegation.api_key.is_empty()
    {
        let key = &config.delegation.api_key;
        let masked = format!(
            "{}...{}",
            &key[..4.min(key.len())],
            if key.len() > 4 {
                &key[key.len() - 4..]
            } else {
                ""
            }
        );
        return DiagnosticResult {
            name: "API key".to_string(),
            status: CheckStatus::Pass,
            message: format!("Found in config ({})", masked),
            fix: None,
        };
    }

    DiagnosticResult {
        name: "API key".to_string(),
        status: CheckStatus::Fail,
        message: "No API key found in env vars or config".to_string(),
        fix: Some("Set HAKIMI_API_KEY env var or run `hakimi setup`".to_string()),
    }
}

fn check_api_connectivity() -> DiagnosticResult {
    // Only test if we have a readable config. Keep this check std-only because
    // `hakimi doctor` runs inside the Tokio CLI runtime; reqwest::blocking would
    // create and drop a nested runtime and panic.
    let path = config_path();
    if !path.exists() {
        return DiagnosticResult {
            name: "API connectivity".to_string(),
            status: CheckStatus::Warn,
            message: "Skipped - no config file".to_string(),
            fix: None,
        };
    }

    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            return DiagnosticResult {
                name: "API connectivity".to_string(),
                status: CheckStatus::Warn,
                message: "Skipped - cannot read config".to_string(),
                fix: None,
            };
        }
    };

    let config: hakimi_config::HakimiConfig = match serde_yaml::from_str(&contents) {
        Ok(c) => c,
        Err(_) => {
            return DiagnosticResult {
                name: "API connectivity".to_string(),
                status: CheckStatus::Warn,
                message: "Skipped - invalid config".to_string(),
                fix: None,
            };
        }
    };

    let base_url = if !config.model.base_url.is_empty() {
        config.model.base_url.clone()
    } else {
        "https://openrouter.ai/api/v1".to_string()
    };

    let Some((host, port)) = api_host_and_port(&base_url) else {
        return DiagnosticResult {
            name: "API connectivity".to_string(),
            status: CheckStatus::Warn,
            message: format!("Cannot parse API host from {base_url}"),
            fix: Some("Check your base URL configuration".to_string()),
        };
    };

    match std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
        .ok()
        .and_then(|mut addrs| addrs.next())
    {
        Some(addr) => {
            match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)) {
                Ok(_) => DiagnosticResult {
                    name: "API connectivity".to_string(),
                    status: CheckStatus::Pass,
                    message: format!("TCP reachable ({host}:{port})"),
                    fix: None,
                },
                Err(e) => DiagnosticResult {
                    name: "API connectivity".to_string(),
                    status: CheckStatus::Fail,
                    message: format!("Cannot reach {host}:{port}: {e}"),
                    fix: Some("Check your network connection and base URL".to_string()),
                },
            }
        }
        None => DiagnosticResult {
            name: "API connectivity".to_string(),
            status: CheckStatus::Fail,
            message: format!("Cannot resolve API host {host}"),
            fix: Some("Check DNS/network connectivity".to_string()),
        },
    }
}

fn api_host_and_port(base_url: &str) -> Option<(String, u16)> {
    let trimmed = base_url.trim();
    let (scheme, rest) = trimmed.split_once("://").unwrap_or(("https", trimmed));
    let authority = rest.split('/').next()?.split('@').next_back()?;
    let (host, explicit_port) = authority
        .rsplit_once(':')
        .map_or((authority, None), |(h, p)| (h, p.parse::<u16>().ok()));
    let host = host.trim_matches(['[', ']']).trim();
    if host.is_empty() {
        return None;
    }
    let port = explicit_port.unwrap_or_else(|| {
        if scheme.eq_ignore_ascii_case("http") {
            80
        } else {
            443
        }
    });
    Some((host.to_string(), port))
}

fn check_rust_toolchain() -> DiagnosticResult {
    match std::process::Command::new("rustc")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            DiagnosticResult {
                name: "Rust toolchain".to_string(),
                status: CheckStatus::Pass,
                message: version,
                fix: None,
            }
        }
        _ => DiagnosticResult {
            name: "Rust toolchain".to_string(),
            status: CheckStatus::Warn,
            message: "rustc not found".to_string(),
            fix: Some("Install Rust: https://rustup.rs/".to_string()),
        },
    }
}

fn check_disk_space() -> DiagnosticResult {
    let dir = hakimi_dir();
    // Use statvfs-style check via df command
    match std::process::Command::new("df")
        .arg("-h")
        .arg(dir.parent().unwrap_or(&dir))
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse the last line for available space
            let lines: Vec<&str> = stdout.trim().lines().collect();
            if lines.len() >= 2 {
                let parts: Vec<&str> = lines[1].split_whitespace().collect();
                if parts.len() >= 4 {
                    let available = parts[3];
                    DiagnosticResult {
                        name: "Disk space".to_string(),
                        status: CheckStatus::Pass,
                        message: format!("{} available", available),
                        fix: None,
                    }
                } else {
                    DiagnosticResult {
                        name: "Disk space".to_string(),
                        status: CheckStatus::Warn,
                        message: "Could not parse df output".to_string(),
                        fix: None,
                    }
                }
            } else {
                DiagnosticResult {
                    name: "Disk space".to_string(),
                    status: CheckStatus::Warn,
                    message: "Empty df output".to_string(),
                    fix: None,
                }
            }
        }
        _ => DiagnosticResult {
            name: "Disk space".to_string(),
            status: CheckStatus::Warn,
            message: "Cannot check disk space".to_string(),
            fix: None,
        },
    }
}

fn check_sqlite() -> DiagnosticResult {
    // Try to create a temporary SQLite database
    let tmp = match tempfile::NamedTempFile::new() {
        Ok(f) => f,
        Err(e) => {
            return DiagnosticResult {
                name: "SQLite".to_string(),
                status: CheckStatus::Fail,
                message: format!("Cannot create temp file: {}", e),
                fix: None,
            };
        }
    };

    let db_path = tmp.path().with_extension("db");

    // Use the session DB crate to verify SQLite works
    match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            match conn.execute_batch("CREATE TABLE test (id INTEGER PRIMARY KEY); DROP TABLE test;")
            {
                Ok(_) => {
                    let _ = std::fs::remove_file(&db_path);
                    DiagnosticResult {
                        name: "SQLite".to_string(),
                        status: CheckStatus::Pass,
                        message: "SQLite is working".to_string(),
                        fix: None,
                    }
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&db_path);
                    DiagnosticResult {
                        name: "SQLite".to_string(),
                        status: CheckStatus::Fail,
                        message: format!("SQLite error: {}", e),
                        fix: Some("Install SQLite or check permissions".to_string()),
                    }
                }
            }
        }
        Err(e) => DiagnosticResult {
            name: "SQLite".to_string(),
            status: CheckStatus::Fail,
            message: format!("Cannot open SQLite: {}", e),
            fix: Some("Install SQLite: https://sqlite.org/".to_string()),
        },
    }
}

fn check_git() -> DiagnosticResult {
    match std::process::Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            DiagnosticResult {
                name: "Git".to_string(),
                status: CheckStatus::Pass,
                message: version,
                fix: None,
            }
        }
        _ => DiagnosticResult {
            name: "Git".to_string(),
            status: CheckStatus::Warn,
            message: "Git not found".to_string(),
            fix: Some("Install git: https://git-scm.com/".to_string()),
        },
    }
}

fn check_node() -> DiagnosticResult {
    match std::process::Command::new("node").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            DiagnosticResult {
                name: "Node.js".to_string(),
                status: CheckStatus::Pass,
                message: version,
                fix: None,
            }
        }
        _ => DiagnosticResult {
            name: "Node.js".to_string(),
            status: CheckStatus::Warn,
            message: "Node.js not found (needed for MCP servers)".to_string(),
            fix: Some("Install Node.js: https://nodejs.org/".to_string()),
        },
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run all diagnostic checks and return results.
pub fn run_diagnostics() -> Vec<DiagnosticResult> {
    vec![
        check_managed_binary(),
        check_path_shim(),
        check_hakimi_version(),
        service_status_result("hakimi.service", "Systemd hakimi.service"),
        service_status_result("hakimi-webui.service", "WebUI service"),
        check_gateway_processes(),
        check_port_3005(),
        check_hakimi_dir(),
        check_config_file(),
        check_session_db(),
        check_api_key(),
        check_api_connectivity(),
        check_rust_toolchain(),
        check_disk_space(),
        check_sqlite(),
        check_git(),
        check_node(),
    ]
}

/// Format diagnostic results with colored output.
pub fn format_report(results: &[DiagnosticResult]) -> String {
    let mut report = String::new();

    report.push_str("\n━━━ Hakimi Doctor ━━━\n\n");

    for result in results {
        let (icon, color) = match result.status {
            CheckStatus::Pass => ("✓", "\x1b[32m"), // green
            CheckStatus::Fail => ("✗", "\x1b[31m"), // red
            CheckStatus::Warn => ("⚠", "\x1b[33m"), // yellow
        };
        let reset = "\x1b[0m";

        report.push_str(&format!(
            "  {}{}{} {} — {}\n",
            color, icon, reset, result.name, result.message
        ));

        if let Some(ref fix) = result.fix
            && result.status != CheckStatus::Pass
        {
            report.push_str(&format!("    → {}\n", fix));
        }
    }

    let passed = results
        .iter()
        .filter(|r| r.status == CheckStatus::Pass)
        .count();
    let failed = results
        .iter()
        .filter(|r| r.status == CheckStatus::Fail)
        .count();
    let warned = results
        .iter()
        .filter(|r| r.status == CheckStatus::Warn)
        .count();

    report.push_str(&format!(
        "\n  Summary: {} passed, {} failed, {} warnings\n",
        passed, failed, warned
    ));

    if failed == 0 {
        report.push_str("\n  \x1b[32mAll critical checks passed!\x1b[0m\n");
    } else {
        report.push_str("\n  \x1b[31mSome checks failed — see fixes above.\x1b[0m\n");
    }

    report.push('\n');
    report
}

/// Format diagnostic results without ANSI color codes for gateway/chat output.
pub fn format_plain_report(results: &[DiagnosticResult]) -> String {
    let mut report = String::new();

    report.push_str("Hakimi Doctor\n\n");

    for result in results {
        let icon = match result.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Warn => "WARN",
        };

        report.push_str(&format!(
            "  [{icon}] {} - {}\n",
            result.name, result.message
        ));

        if let Some(ref fix) = result.fix
            && result.status != CheckStatus::Pass
        {
            report.push_str(&format!("    -> {}\n", fix));
        }
    }

    let passed = results
        .iter()
        .filter(|r| r.status == CheckStatus::Pass)
        .count();
    let failed = results
        .iter()
        .filter(|r| r.status == CheckStatus::Fail)
        .count();
    let warned = results
        .iter()
        .filter(|r| r.status == CheckStatus::Warn)
        .count();

    report.push_str(&format!(
        "\n  Summary: {} passed, {} failed, {} warnings\n",
        passed, failed, warned
    ));

    if failed == 0 {
        report.push_str("\n  All critical checks passed!\n");
    } else {
        report.push_str("\n  Some checks failed - see fixes above.\n");
    }

    report
}

/// Run diagnostics and print the report. Returns the results.
pub fn run_and_print_diagnostics() -> Vec<DiagnosticResult> {
    let results = run_diagnostics();
    print!("{}", format_report(&results));
    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_diagnostics_returns_results() {
        let results = run_diagnostics();
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.name == "Rust toolchain"));
        assert!(results.iter().any(|r| r.name == "Git"));
        assert!(results.iter().any(|r| r.name == "Disk space"));
        assert!(results.iter().any(|r| r.name == "SQLite"));
    }

    #[test]
    fn test_format_report() {
        let results = vec![
            DiagnosticResult {
                name: "test-pass".to_string(),
                status: CheckStatus::Pass,
                message: "OK".to_string(),
                fix: None,
            },
            DiagnosticResult {
                name: "test-fail".to_string(),
                status: CheckStatus::Fail,
                message: "Failed".to_string(),
                fix: Some("Fix it".to_string()),
            },
            DiagnosticResult {
                name: "test-warn".to_string(),
                status: CheckStatus::Warn,
                message: "Warning".to_string(),
                fix: None,
            },
        ];
        let report = format_report(&results);
        assert!(report.contains("✓"));
        assert!(report.contains("✗"));
        assert!(report.contains("⚠"));
        assert!(report.contains("1 passed, 1 failed, 1 warnings"));
    }

    #[test]
    fn test_format_plain_report_uses_chat_safe_text() {
        let results = vec![
            DiagnosticResult {
                name: "test-pass".to_string(),
                status: CheckStatus::Pass,
                message: "OK".to_string(),
                fix: None,
            },
            DiagnosticResult {
                name: "test-fail".to_string(),
                status: CheckStatus::Fail,
                message: "Failed".to_string(),
                fix: Some("Fix it".to_string()),
            },
        ];
        let report = format_plain_report(&results);
        assert!(report.contains("[PASS] test-pass - OK"));
        assert!(report.contains("[FAIL] test-fail - Failed"));
        assert!(report.contains("-> Fix it"));
        assert!(!report.contains("\x1b["));
    }

    #[test]
    fn test_check_git() {
        let result = check_git();
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.message.contains("git"));
    }

    #[test]
    fn test_check_rust_toolchain() {
        let result = check_rust_toolchain();
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.message.contains("rustc"));
    }

    #[test]
    fn test_check_sqlite() {
        let result = check_sqlite();
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn test_diagnostic_result_serialization() {
        let result = DiagnosticResult {
            name: "test".to_string(),
            status: CheckStatus::Pass,
            message: "OK".to_string(),
            fix: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("Pass"));
    }

    #[test]
    fn test_check_disk_space() {
        let result = check_disk_space();
        // Should at least not crash
        assert!(result.status == CheckStatus::Pass || result.status == CheckStatus::Warn);
    }

    #[test]
    fn test_check_status_equality() {
        assert_eq!(CheckStatus::Pass, CheckStatus::Pass);
        assert_eq!(CheckStatus::Fail, CheckStatus::Fail);
        assert_eq!(CheckStatus::Warn, CheckStatus::Warn);
        assert_ne!(CheckStatus::Pass, CheckStatus::Fail);
        assert_ne!(CheckStatus::Pass, CheckStatus::Warn);
        assert_ne!(CheckStatus::Fail, CheckStatus::Warn);
    }

    #[test]
    fn test_format_report_all_pass() {
        let results = vec![
            DiagnosticResult {
                name: "check1".to_string(),
                status: CheckStatus::Pass,
                message: "OK".to_string(),
                fix: None,
            },
            DiagnosticResult {
                name: "check2".to_string(),
                status: CheckStatus::Pass,
                message: "Fine".to_string(),
                fix: None,
            },
        ];
        let report = format_report(&results);
        assert!(report.contains("✓"));
        assert!(!report.contains("✗"));
        assert!(report.contains("2 passed, 0 failed, 0 warnings"));
        assert!(report.contains("All critical checks passed"));
    }

    #[test]
    fn test_format_report_empty() {
        let results: Vec<DiagnosticResult> = vec![];
        let report = format_report(&results);
        assert!(report.contains("Hakimi Doctor"));
        assert!(report.contains("0 passed, 0 failed, 0 warnings"));
    }

    #[test]
    fn test_format_report_shows_fixes() {
        let results = vec![DiagnosticResult {
            name: "broken".to_string(),
            status: CheckStatus::Fail,
            message: "Missing".to_string(),
            fix: Some("Install it".to_string()),
        }];
        let report = format_report(&results);
        assert!(report.contains("→ Install it"));
        assert!(report.contains("Some checks failed"));
    }

    #[test]
    fn test_run_diagnostics_contains_all_expected_checks() {
        let results = run_diagnostics();
        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"Managed binary"));
        assert!(names.contains(&"PATH shim"));
        assert!(names.contains(&"Version"));
        assert!(names.contains(&"Systemd hakimi.service"));
        assert!(names.contains(&"WebUI service"));
        assert!(names.contains(&"Duplicate gateway processes"));
        assert!(names.contains(&"Port 3005"));
        assert!(names.contains(&"~/.hakimi/ directory"));
        assert!(names.contains(&"config.yaml"));
        assert!(names.contains(&"Session DB"));
        assert!(names.contains(&"API key"));
        assert!(names.contains(&"API connectivity"));
        assert!(names.contains(&"Rust toolchain"));
        assert!(names.contains(&"Disk space"));
        assert!(names.contains(&"SQLite"));
        assert!(names.contains(&"Git"));
        assert!(names.contains(&"Node.js"));
        assert_eq!(results.len(), 17);
    }

    #[test]
    fn test_managed_binary_path_uses_official_install_location() {
        assert_eq!(
            managed_binary_path_for_home(Path::new("/root")),
            PathBuf::from("/root/.hakimi/bin/hakimi")
        );
    }

    #[test]
    fn test_doctor_report_names_hakimi_doctor() {
        let report = format_plain_report(&[DiagnosticResult {
            name: "Version".to_string(),
            status: CheckStatus::Pass,
            message: "0.5.test".to_string(),
            fix: None,
        }]);
        assert!(report.starts_with("Hakimi Doctor"));
        assert!(report.contains("[PASS] Version - 0.5.test"));
    }

    #[test]
    fn test_diagnostic_result_fix_is_none_for_pass() {
        let result = DiagnosticResult {
            name: "test".to_string(),
            status: CheckStatus::Pass,
            message: "OK".to_string(),
            fix: None,
        };
        assert!(result.fix.is_none());
    }

    #[test]
    fn test_check_node() {
        let result = check_node();
        // Node may or may not be installed; just verify structure
        assert_eq!(result.name, "Node.js");
        assert!(
            result.status == CheckStatus::Pass || result.status == CheckStatus::Warn,
            "Node check should be Pass or Warn, got {:?}",
            result.status
        );
        if result.status == CheckStatus::Pass {
            assert!(result.fix.is_none());
        } else {
            assert!(result.fix.is_some());
        }
    }

    #[test]
    fn test_format_report_warn_has_no_fix_displayed_when_none() {
        let results = vec![DiagnosticResult {
            name: "warn-check".to_string(),
            status: CheckStatus::Warn,
            message: "Heads up".to_string(),
            fix: None,
        }];
        let report = format_report(&results);
        assert!(report.contains("⚠"));
        assert!(report.contains("Heads up"));
        // No arrow since fix is None
        assert!(!report.contains("→"));
    }
}
