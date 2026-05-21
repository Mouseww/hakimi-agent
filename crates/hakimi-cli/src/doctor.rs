//! Diagnostics CLI command for troubleshooting setup issues.
//!
//! Checks dependencies, configuration, environment variables,
//! and API connectivity.

use serde::{Deserialize, Serialize};
use tracing::info;

/// A single diagnostic check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResult {
    /// Name of the check.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Human-readable message.
    pub message: String,
    /// Suggested fix if the check failed.
    pub fix: Option<String>,
}

/// Run all diagnostic checks.
pub fn run_diagnostics() -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    // Check git availability.
    results.push(check_git());

    // Check config file.
    results.push(check_config_file());

    // Check API key environment variables.
    results.push(check_env_var("ANTHROPIC_API_KEY", "Set your Anthropic API key"));
    results.push(check_env_var("OPENAI_API_KEY", "Set your OpenAI API key"));

    // Check working directory.
    results.push(check_workdir());

    // Check Python availability (for tools).
    results.push(check_command("python3", "Install Python 3"));

    // Check Node.js availability (for MCP servers).
    results.push(check_command("node", "Install Node.js"));

    info!(checks = results.len(), "Diagnostics completed");
    results
}

/// Format diagnostic results as a human-readable report.
pub fn format_report(results: &[DiagnosticResult]) -> String {
    let mut report = String::from("Hakimi Agent Diagnostics
========================

");

    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;

    for result in results {
        let icon = if result.passed { "✓" } else { "✗" };
        report.push_str(&format!("{} {}: {}
", icon, result.name, result.message));
        if let Some(ref fix) = result.fix {
            if !result.passed {
                report.push_str(&format!("  → Fix: {}
", fix));
            }
        }
    }

    report.push_str(&format!("
{} passed, {} failed
", passed, failed));
    report
}

fn check_git() -> DiagnosticResult {
    match std::process::Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => DiagnosticResult {
            name: "git".to_string(),
            passed: true,
            message: "Git is available".to_string(),
            fix: None,
        },
        _ => DiagnosticResult {
            name: "git".to_string(),
            passed: false,
            message: "Git not found".to_string(),
            fix: Some("Install git: https://git-scm.com/".to_string()),
        },
    }
}

fn check_config_file() -> DiagnosticResult {
    let home = dirs::home_dir().unwrap_or_default();
    let config_path = home.join(".hakimi").join("config.yaml");
    if config_path.exists() {
        DiagnosticResult {
            name: "config".to_string(),
            passed: true,
            message: format!("Config found at {}", config_path.display()),
            fix: None,
        }
    } else {
        DiagnosticResult {
            name: "config".to_string(),
            passed: false,
            message: "No config file found".to_string(),
            fix: Some("Run `hakimi setup` to create a config".to_string()),
        }
    }
}

fn check_env_var(var: &str, fix_msg: &str) -> DiagnosticResult {
    if std::env::var(var).is_ok() {
        DiagnosticResult {
            name: var.to_string(),
            passed: true,
            message: format!("{} is set", var),
            fix: None,
        }
    } else {
        DiagnosticResult {
            name: var.to_string(),
            passed: false,
            message: format!("{} is not set", var),
            fix: Some(fix_msg.to_string()),
        }
    }
}

fn check_workdir() -> DiagnosticResult {
    match std::env::current_dir() {
        Ok(dir) => DiagnosticResult {
            name: "workdir".to_string(),
            passed: true,
            message: format!("Working directory: {}", dir.display()),
            fix: None,
        },
        Err(e) => DiagnosticResult {
            name: "workdir".to_string(),
            passed: false,
            message: format!("Cannot determine working directory: {}", e),
            fix: Some("Check filesystem permissions".to_string()),
        },
    }
}

fn check_command(cmd: &str, fix_msg: &str) -> DiagnosticResult {
    match std::process::Command::new(cmd).arg("--version").output() {
        Ok(output) if output.status.success() => DiagnosticResult {
            name: cmd.to_string(),
            passed: true,
            message: format!("{} is available", cmd),
            fix: None,
        },
        _ => DiagnosticResult {
            name: cmd.to_string(),
            passed: false,
            message: format!("{} not found", cmd),
            fix: Some(fix_msg.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_diagnostics_returns_results() {
        let results = run_diagnostics();
        assert!(!results.is_empty());
        // At least git and workdir should pass on most systems.
        assert!(results.iter().any(|r| r.name == "git"));
        assert!(results.iter().any(|r| r.name == "workdir"));
    }

    #[test]
    fn test_format_report() {
        let results = vec![
            DiagnosticResult {
                name: "test1".to_string(),
                passed: true,
                message: "OK".to_string(),
                fix: None,
            },
            DiagnosticResult {
                name: "test2".to_string(),
                passed: false,
                message: "Failed".to_string(),
                fix: Some("Do something".to_string()),
            },
        ];
        let report = format_report(&results);
        assert!(report.contains("✓"));
        assert!(report.contains("✗"));
        assert!(report.contains("1 passed, 1 failed"));
    }

    #[test]
    fn test_check_git() {
        let result = check_git();
        // Git should be available on the test system.
        assert!(result.passed);
    }

    #[test]
    fn test_diagnostic_result_serialization() {
        let result = DiagnosticResult {
            name: "test".to_string(),
            passed: true,
            message: "OK".to_string(),
            fix: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test"));
    }
}
