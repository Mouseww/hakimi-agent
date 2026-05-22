use regex::Regex;

pub struct TerminalAutoFixer;

#[derive(Debug, Clone)]
pub struct FixSuggestion {
    pub reason: String,
    pub command: String,
}

impl TerminalAutoFixer {
    pub fn analyze(stdout: &str, stderr: &str, exit_code: i32) -> Option<FixSuggestion> {
        let combined = format!("{}\n{}", stdout, stderr);

        // 1. Missing binary (bash error 127)
        if exit_code == 127 {
            if let Some(caps) = Regex::new(r"bash: ([^:]+): command not found").ok()?.captures(&combined) {
                let bin = &caps[1];
                return Some(FixSuggestion {
                    reason: format!("Command '{}' is not installed.", bin),
                    command: format!("sudo apt update && sudo apt install -y {}", bin),
                });
            }
        }

        // 2. Python missing module
        if combined.contains("ModuleNotFoundError: No module named") || combined.contains("ImportError: No module named") {
            if let Some(caps) = Regex::new(r"No module named '([^']+)'").ok()?.captures(&combined) {
                let module = &caps[1];
                return Some(FixSuggestion {
                    reason: format!("Python module '{}' is missing.", module),
                    command: format!("pip install {}", module),
                });
            }
        }

        // 3. NPM missing package
        if combined.contains("Error: Cannot find module") {
            if let Some(caps) = Regex::new(r"Cannot find module '([^']+)'").ok()?.captures(&combined) {
                let package = &caps[1];
                return Some(FixSuggestion {
                    reason: format!("NPM package '{}' is missing.", package),
                    command: format!("npm install {}", package),
                });
            }
        }

        None
    }
}
