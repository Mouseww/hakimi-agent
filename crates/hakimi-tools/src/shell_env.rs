use std::path::{Path, PathBuf};

use tokio::process::Command;

const SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

pub fn stable_shell_path() -> String {
    let home = std::env::var("HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/root"));
    stable_shell_path_for_home(&home, std::env::var("PATH").ok().as_deref())
}

pub fn stable_shell_path_for_home(home: &Path, inherited_path: Option<&str>) -> String {
    let mut parts = path_entries(inherited_path.unwrap_or(SYSTEM_PATH));
    prepend_unique(
        &mut parts,
        unix_style_path(&home.join(".cargo").join("bin")),
    );
    prepend_unique(
        &mut parts,
        unix_style_path(&home.join(".hakimi").join("bin")),
    );
    parts.join(":")
}

pub fn apply_stable_path(command: &mut Command) {
    if cfg!(unix) {
        command.env("PATH", stable_shell_path());
    }
}

pub fn bash_program() -> &'static str {
    if cfg!(unix) {
        if Path::new("/usr/bin/bash").exists() {
            return "/usr/bin/bash";
        }
        if Path::new("/bin/bash").exists() {
            return "/bin/bash";
        }
    }
    "bash"
}

pub fn diagnose_shell_failure(
    stderr: &str,
    exit_code: Option<i32>,
    workdir: &str,
) -> Option<String> {
    let path = stable_shell_path();

    if exit_code == Some(127)
        && let Some(token) = extract_bash_error_token(stderr, "No such file or directory")
        && token.contains('/')
    {
        let resolved = resolve_command_path(&token, workdir);
        if !resolved.exists() {
            return Some(format!(
                "Command path does not exist: `{}`\nHakimi terminal uses PATH={path}. If this command works in an interactive shell, the systemd/Hakimi environment differs from that shell; use an absolute path or install the binary into the stable PATH above.",
                resolved.display()
            ));
        }
    }

    if let Some(token) = extract_bash_error_token(stderr, "cannot execute: Permission denied")
        .or_else(|| extract_bash_error_token(stderr, "Permission denied"))
    {
        if let Some(diagnostic) = diagnose_non_executable(&token, workdir) {
            return Some(diagnostic);
        }
    }

    if exit_code == Some(126)
        && let Some(token) = extract_bash_error_token(stderr, "Permission denied")
    {
        if let Some(diagnostic) = diagnose_non_executable(&token, workdir) {
            return Some(diagnostic);
        }
    }

    if exit_code == Some(127)
        && let Some(command) = extract_bash_error_token(stderr, "command not found")
    {
        if command.contains('/') {
            let resolved = resolve_command_path(&command, workdir);
            if !resolved.exists() {
                return Some(format!(
                    "Command path does not exist: `{}`\nHakimi terminal uses PATH={path}. If this command works in an interactive shell, the systemd/Hakimi environment differs from that shell; use an absolute path or install the binary into the stable PATH above.",
                    resolved.display()
                ));
            }
            if !is_executable(&resolved) {
                return Some(format!(
                    "Binary path exists but is not executable: `{}`\nMake it executable, for example: `chmod +x \"{}\"`.",
                    resolved.display(),
                    resolved.display()
                ));
            }
        }

        if let Some(candidate) = path_candidates(&command)
            .into_iter()
            .find(|candidate| candidate.exists() && !is_executable(candidate))
        {
            return Some(format!(
                "PATH resolved `{command}` to a non-executable binary: `{}`\nFix its permissions or remove the broken PATH entry.",
                candidate.display()
            ));
        }

        return Some(format!(
            "Command not found in PATH: `{command}`\nHakimi terminal uses PATH={path}. If this command works in an interactive shell, the systemd/Hakimi environment differs from that shell; install the binary into the stable PATH above or use an absolute path."
        ));
    }

    None
}

fn unix_style_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_entries(path: &str) -> Vec<String> {
    let mut entries = Vec::new();
    for entry in path.split(':').filter(|part| !part.trim().is_empty()) {
        let entry = entry.to_string();
        if !entries.iter().any(|existing| existing == &entry) {
            entries.push(entry);
        }
    }
    entries
}

fn prepend_unique(entries: &mut Vec<String>, entry: String) {
    entries.retain(|existing| existing != &entry);
    entries.insert(0, entry);
}

fn extract_bash_error_token(stderr: &str, suffix: &str) -> Option<String> {
    for line in stderr.lines() {
        let trimmed = line.trim();
        let prefix = trimmed.strip_suffix(suffix)?;
        let prefix = prefix.trim_end().trim_end_matches(':').trim_end();
        let token = prefix.rsplit(": ").next()?.trim();
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }
    None
}

fn resolve_command_path(command: &str, workdir: &str) -> PathBuf {
    let path = PathBuf::from(command);
    if path.is_absolute() {
        path
    } else {
        Path::new(workdir).join(path)
    }
}

fn diagnose_non_executable(command: &str, workdir: &str) -> Option<String> {
    let resolved = resolve_command_path(command, workdir);
    if resolved.exists() && !is_executable(&resolved) {
        return Some(format!(
            "Binary path exists but is not executable: `{}`\nMake it executable, for example: `chmod +x \"{}\"`.",
            resolved.display(),
            resolved.display()
        ));
    }

    path_candidates(command)
        .into_iter()
        .find(|candidate| candidate.exists() && !is_executable(candidate))
        .map(|candidate| {
            format!(
                "PATH resolved `{command}` to a non-executable binary: `{}`\nFix its permissions or remove the broken PATH entry.",
                candidate.display()
            )
        })
}

fn path_candidates(command: &str) -> Vec<PathBuf> {
    stable_shell_path()
        .split(':')
        .filter(|part| !part.is_empty())
        .map(|part| Path::new(part).join(command))
        .collect()
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_shell_path_uses_managed_and_cargo_bins_before_system_bins() {
        assert_eq!(
            stable_shell_path_for_home(Path::new("/root"), None),
            "/root/.hakimi/bin:/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
        );
    }

    #[test]
    fn stable_shell_path_preserves_existing_path_after_managed_prefixes() {
        assert_eq!(
            stable_shell_path_for_home(Path::new("/root"), Some("/opt/bin:/usr/bin")),
            "/root/.hakimi/bin:/root/.cargo/bin:/opt/bin:/usr/bin"
        );
    }

    #[test]
    fn stable_shell_path_deduplicates_managed_prefixes() {
        assert_eq!(
            stable_shell_path_for_home(
                Path::new("/root"),
                Some("/usr/bin:/root/.hakimi/bin:/root/.cargo/bin")
            ),
            "/root/.hakimi/bin:/root/.cargo/bin:/usr/bin"
        );
    }

    #[test]
    fn diagnoses_command_not_found_in_stable_path() {
        let diagnostic = diagnose_shell_failure(
            "bash: line 1: hakimi-missing: command not found",
            Some(127),
            "/tmp",
        )
        .unwrap();

        assert!(diagnostic.contains("Command not found in PATH: `hakimi-missing`"));
        assert!(diagnostic.contains("systemd/Hakimi environment differs"));
    }

    #[test]
    fn diagnoses_missing_explicit_command_path() {
        let diagnostic = diagnose_shell_failure(
            "bash: line 1: /no/such/hakimi: No such file or directory",
            Some(127),
            "/tmp",
        )
        .unwrap();

        assert!(diagnostic.contains("Command path does not exist: `/no/such/hakimi`"));
    }

    #[cfg(unix)]
    #[test]
    fn diagnoses_non_executable_explicit_path() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("hakimi");
        std::fs::write(&binary, "not executable").unwrap();
        let stderr = format!("bash: line 1: {}: Permission denied", binary.display());

        let diagnostic =
            diagnose_shell_failure(&stderr, Some(126), temp.path().to_str().unwrap()).unwrap();

        assert!(diagnostic.contains("Binary path exists but is not executable"));
        assert!(diagnostic.contains(&binary.display().to_string()));
    }
}
