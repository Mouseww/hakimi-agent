//! Contextual first-touch onboarding hints.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use hakimi_config::HakimiConfig;

pub const BUSY_INPUT_FLAG: &str = "busy_input_prompt";
pub const TOOL_PROGRESS_FLAG: &str = "tool_progress_prompt";
pub const OPENCLAW_RESIDUE_FLAG: &str = "openclaw_residue_cleanup";

pub fn busy_input_hint_gateway() -> &'static str {
    "First-time tip: this chat is already running a task, so Hakimi is handling your new message as concurrent input. Use `/stop` to cancel the current task before sending a replacement request. This notice will not appear again."
}

pub fn busy_input_hint_cli() -> &'static str {
    "(tip) Your message arrived while a previous task was still running. Use /stop before sending a replacement request. This tip only shows once."
}

pub fn tool_progress_hint_gateway() -> &'static str {
    "First-time tip: long-running tool progress can be noisy. Use `/verbose` to cycle progress display modes. This notice will not appear again."
}

pub fn tool_progress_hint_cli() -> &'static str {
    "(tip) Long-running tools may show progress updates. Use /verbose to cycle display modes. This tip only shows once."
}

pub fn openclaw_residue_hint_cli() -> &'static str {
    "A legacy OpenClaw directory was detected at ~/.openclaw/.\nRun `hakimi setup` or migrate state manually before removing it. If you archive the old directory, OpenClaw will stop using that state.\nThis tip only shows once."
}

pub fn detect_openclaw_residue(home: Option<&Path>) -> bool {
    let base = home
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".openclaw").is_dir()
}

pub fn should_show(config: &HakimiConfig, flag: &str) -> bool {
    !config.onboarding.is_seen(flag)
}

pub fn mark_seen(config: &mut HakimiConfig, config_path: &Path, flag: &str) -> Result<bool> {
    if config.onboarding.is_seen(flag) {
        return Ok(false);
    }
    config.onboarding.mark_seen(flag.to_string());
    write_onboarding_seen(config_path, flag)?;
    Ok(true)
}

fn write_onboarding_seen(config_path: &Path, flag: &str) -> Result<()> {
    let mut config = match std::fs::read_to_string(config_path) {
        Ok(contents) => serde_yaml::from_str::<HakimiConfig>(&contents)
            .with_context(|| format!("failed to parse {}", config_path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => HakimiConfig::default(),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };

    if config.onboarding.is_seen(flag) {
        return Ok(());
    }
    config.onboarding.mark_seen(flag.to_string());

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let yaml = serde_yaml::to_string(&config)?;
    std::fs::write(config_path, yaml)
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_openclaw_residue_in_home_override() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!detect_openclaw_residue(Some(tmp.path())));

        std::fs::create_dir(tmp.path().join(".openclaw")).unwrap();
        assert!(detect_openclaw_residue(Some(tmp.path())));
    }

    #[test]
    fn mark_seen_persists_flag_once() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.yaml");
        let mut config = HakimiConfig::default();

        assert!(mark_seen(&mut config, &config_path, BUSY_INPUT_FLAG).unwrap());
        assert!(config.onboarding.is_seen(BUSY_INPUT_FLAG));
        assert!(!mark_seen(&mut config, &config_path, BUSY_INPUT_FLAG).unwrap());

        let persisted: HakimiConfig =
            serde_yaml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
        assert!(persisted.onboarding.is_seen(BUSY_INPUT_FLAG));
    }

    #[test]
    fn hint_texts_name_their_controls() {
        assert!(busy_input_hint_gateway().contains("/stop"));
        assert!(tool_progress_hint_gateway().contains("/verbose"));
        assert!(openclaw_residue_hint_cli().contains("~/.openclaw/"));
    }
}
