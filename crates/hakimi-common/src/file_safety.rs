//! Shared file safety helpers for tool boundaries.

use std::path::{Component, Path, PathBuf};

const BLOCKED_PROJECT_ENV_BASENAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.development",
    ".env.production",
    ".env.test",
    ".env.staging",
    ".envrc",
];

const HAKIMI_CREDENTIAL_FILES: &[&[&str]] = &[
    &["config.yaml"],
    &["auth.json"],
    &["auth.lock"],
    &[".anthropic_oauth.json"],
    &[".env"],
    &["webhook_subscriptions.json"],
    &["auth", "google_oauth.json"],
    &["cache", "bws_cache.json"],
];

const WRITE_DENIED_PATHS: &[&str] = &[
    "/etc/passwd",
    "/etc/shadow",
    "/etc/sudoers",
    "/etc/ssh",
    "/private/etc",
    "/private/var",
    "/root/.ssh",
    "/root/.gnupg",
    "/root/.aws/credentials",
    "/root/.config/gcloud",
    "/boot",
    "/proc",
    "/sys",
    "/dev",
];

const WRITE_DENIED_PREFIXES: &[&str] = &[
    "/etc/",
    "/private/etc/",
    "/private/var/",
    "/root/.ssh/",
    "/root/.gnupg/",
    "/root/.aws/",
    "/root/.config/gcloud/",
    "/boot/",
    "/proc/",
    "/sys/",
    "/dev/",
];

const WRITE_SAFE_ROOT_ENV_VARS: &[&str] = &["HAKIMI_WRITE_SAFE_ROOT", "HERMES_WRITE_SAFE_ROOT"];

/// Return a user-facing read denial when a path points at known secret stores.
pub fn get_read_block_error(path: &Path) -> Option<String> {
    let home = default_home_dir();
    let hakimi_home = std::env::var_os("HAKIMI_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".hakimi")));
    let hakimi_root = home.as_ref().map(|h| h.join(".hakimi"));

    get_read_block_error_with_homes(path, hakimi_home.as_deref(), hakimi_root.as_deref())
}

/// Testable variant of [`get_read_block_error`] with explicit Hakimi homes.
pub fn get_read_block_error_with_homes(
    path: &Path,
    hakimi_home: Option<&Path>,
    hakimi_root: Option<&Path>,
) -> Option<String> {
    let resolved = resolve_for_safety(path);
    let display_path = path.display();

    for base in unique_hakimi_dirs(hakimi_home, hakimi_root) {
        if is_hakimi_credential_file(&resolved, &base)
            || is_hakimi_profile_credential_file(&resolved, &base)
        {
            return Some(format!(
                "Access denied: {display_path} is a Hakimi credential store and cannot be read directly. Provider tools consume these credentials through internal channels. (Defense-in-depth; terminal access can still bypass.)"
            ));
        }

        let mcp_tokens = base.join("mcp-tokens");
        if resolved == mcp_tokens || resolved.starts_with(&mcp_tokens) {
            return Some(format!(
                "Access denied: {display_path} is a Hakimi MCP token file and cannot be read directly. (Defense-in-depth; terminal access can still bypass.)"
            ));
        }
    }

    if resolved
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| BLOCKED_PROJECT_ENV_BASENAMES.contains(&name))
    {
        return Some(format!(
            "Access denied: {display_path} is a secret-bearing environment file and cannot be read to prevent credential leakage. If you need the shape, read .env.example instead. (Defense-in-depth; terminal access can still bypass.)"
        ));
    }

    None
}

/// Return a user-facing write denial for sensitive paths or safe-root escapes.
pub fn get_write_block_error(path: &Path) -> Option<String> {
    let safe_root = configured_write_safe_root();
    get_write_block_error_with_safe_root(path, safe_root.as_deref())
}

/// Testable variant of [`get_write_block_error`] with an explicit safe root.
pub fn get_write_block_error_with_safe_root(
    path: &Path,
    safe_root: Option<&Path>,
) -> Option<String> {
    let resolved = resolve_for_safety(path);
    let display_path = path.display();

    if is_static_write_denied(&resolved) {
        return Some(format!(
            "Access denied: {display_path} is a sensitive system or credential path and cannot be written by Hakimi. (Defense-in-depth; terminal access can still bypass.)"
        ));
    }

    if let Some(root) = safe_root {
        let resolved_root = resolve_for_safety(root);
        if !resolved.starts_with(&resolved_root) {
            return Some(format!(
                "Access denied: {display_path} is outside the configured write safe root '{}'. Set HAKIMI_WRITE_SAFE_ROOT or HERMES_WRITE_SAFE_ROOT to restrict file writes to a trusted workspace.",
                resolved_root.display()
            ));
        }
    }

    None
}

/// Return true when a write should be denied by static or safe-root policy.
pub fn is_write_denied(path: &Path) -> bool {
    get_write_block_error(path).is_some()
}

/// Testable variant of [`is_write_denied`] with an explicit safe root.
pub fn is_write_denied_with_safe_root(path: &Path, safe_root: Option<&Path>) -> bool {
    get_write_block_error_with_safe_root(path, safe_root).is_some()
}

fn is_hakimi_credential_file(resolved: &Path, base: &Path) -> bool {
    HAKIMI_CREDENTIAL_FILES
        .iter()
        .any(|parts| resolved == base.join(relative_path(parts)))
}

fn is_hakimi_profile_credential_file(resolved: &Path, base: &Path) -> bool {
    let profiles = base.join("profiles");
    let Ok(relative) = resolved.strip_prefix(&profiles) else {
        return false;
    };
    let mut components = relative.components();
    if !matches!(components.next(), Some(Component::Normal(_))) {
        return false;
    }
    let profile_relative = components.as_path();
    HAKIMI_CREDENTIAL_FILES
        .iter()
        .any(|parts| profile_relative == relative_path(parts))
}

fn unique_hakimi_dirs(hakimi_home: Option<&Path>, hakimi_root: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for candidate in [hakimi_home, hakimi_root].into_iter().flatten() {
        let normalized = resolve_for_safety(candidate);
        if !dirs.contains(&normalized) {
            dirs.push(normalized);
        }
    }
    dirs
}

fn relative_path(parts: &[&str]) -> PathBuf {
    parts.iter().fold(PathBuf::new(), |mut path, part| {
        path.push(part);
        path
    })
}

fn configured_write_safe_root() -> Option<PathBuf> {
    for key in WRITE_SAFE_ROOT_ENV_VARS {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(PathBuf::from(trimmed));
            }
        }
    }
    None
}

fn is_static_write_denied(path: &Path) -> bool {
    let path_str = path.to_string_lossy().replace('\\', "/");

    WRITE_DENIED_PATHS.iter().any(|denied| path_str == *denied)
        || WRITE_DENIED_PREFIXES
            .iter()
            .any(|prefix| path_str.starts_with(prefix))
}

fn default_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let path = std::env::var_os("HOMEPATH")?;
            let mut joined = PathBuf::from(drive);
            joined.push(path);
            Some(joined)
        })
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn expand_home(path: &Path) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path.to_path_buf();
    };

    if raw == "~" {
        return default_home_dir().unwrap_or_else(|| path.to_path_buf());
    }

    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\"))
        && let Some(home) = default_home_dir()
    {
        return home.join(rest);
    }

    path.to_path_buf()
}

fn resolve_for_safety(path: &Path) -> PathBuf {
    let expanded = expand_home(path);
    std::fs::canonicalize(&expanded).unwrap_or_else(|_| normalize_path(&expanded))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_bitwarden_secrets_manager_cache_file() {
        let home = Path::new("/home/user/.hakimi");
        let path = home.join("cache").join("bws_cache.json");

        let err = get_read_block_error_with_homes(&path, Some(home), Some(home)).unwrap();

        assert!(err.contains("credential store"));
    }

    #[test]
    fn does_not_block_other_cache_files() {
        let home = Path::new("/home/user/.hakimi");
        let path = home.join("cache").join("image.png");

        assert!(get_read_block_error_with_homes(&path, Some(home), Some(home)).is_none());
    }

    #[test]
    fn blocks_profile_and_root_credential_files() {
        let root = Path::new("/home/user/.hakimi");
        let profile = root.join("profiles").join("work");

        assert!(
            get_read_block_error_with_homes(
                &profile.join("auth.json"),
                Some(&profile),
                Some(root),
            )
            .is_some()
        );
        assert!(
            get_read_block_error_with_homes(
                &root.join("auth").join("google_oauth.json"),
                Some(&profile),
                Some(root),
            )
            .is_some()
        );
    }

    #[test]
    fn blocks_inactive_profile_credential_files_from_root() {
        let root = Path::new("/home/user/.hakimi");
        let path = root.join("profiles").join("work").join("config.yaml");

        let err = get_read_block_error_with_homes(&path, Some(root), Some(root)).unwrap();

        assert!(err.contains("credential store"));
    }

    #[test]
    fn blocks_mcp_token_files_by_prefix() {
        let home = Path::new("/home/user/.hakimi");
        let path = home.join("mcp-tokens").join("github.json");

        let err = get_read_block_error_with_homes(&path, Some(home), Some(home)).unwrap();

        assert!(err.contains("MCP token"));
    }

    #[test]
    fn blocks_project_env_files_anywhere() {
        let path = Path::new("/workspace/app/.env.local");

        let err = get_read_block_error_with_homes(path, None, None).unwrap();

        assert!(err.contains("environment file"));
    }

    #[test]
    fn allows_env_examples() {
        let path = Path::new("/workspace/app/.env.example");

        assert!(get_read_block_error_with_homes(path, None, None).is_none());
    }

    #[test]
    fn blocks_static_write_denied_paths() {
        assert!(is_write_denied_with_safe_root(
            Path::new("/etc/shadow"),
            None
        ));
        assert!(is_write_denied_with_safe_root(
            Path::new("/private/etc/hosts"),
            None
        ));
        assert!(is_write_denied_with_safe_root(
            Path::new("/root/.ssh/authorized_keys"),
            None
        ));
    }

    #[test]
    fn allows_regular_writes_without_safe_root() {
        assert!(!is_write_denied_with_safe_root(
            Path::new("/workspace/app/src/main.rs"),
            None
        ));
    }

    #[test]
    fn write_safe_root_allows_root_and_children() {
        let root = Path::new("/workspace/app");

        assert!(!is_write_denied_with_safe_root(root, Some(root)));
        assert!(!is_write_denied_with_safe_root(
            Path::new("/workspace/app/src/main.rs"),
            Some(root)
        ));
    }

    #[test]
    fn write_safe_root_denies_outside_siblings() {
        let root = Path::new("/workspace/app");

        let err = get_write_block_error_with_safe_root(
            Path::new("/workspace/app-other/file.txt"),
            Some(root),
        )
        .unwrap();

        assert!(err.contains("outside the configured write safe root"));
    }

    #[test]
    fn write_safe_root_does_not_override_static_deny() {
        let root = Path::new("/");

        let err =
            get_write_block_error_with_safe_root(Path::new("/etc/passwd"), Some(root)).unwrap();

        assert!(err.contains("sensitive system or credential path"));
    }
}
