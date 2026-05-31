//! Voice-mode helpers shared by CLI/TUI surfaces and audio tools.

use std::path::{Path, PathBuf};

/// Whisper-native recording sample rate used by Hermes voice mode.
pub const VOICE_SAMPLE_RATE: u32 = 16_000;

/// Mono channel count used for CLI voice capture.
pub const VOICE_CHANNELS: u16 = 1;

/// PCM sample width for 16-bit WAV recordings.
pub const VOICE_SAMPLE_WIDTH_BYTES: u16 = 2;

/// Default RMS threshold below which audio is treated as silence.
pub const DEFAULT_SILENCE_RMS_THRESHOLD: u32 = 200;

/// Default continuous silence duration before recording auto-stops.
pub const DEFAULT_SILENCE_DURATION_SECONDS: f32 = 3.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceEnvironmentReport {
    pub capture_available: bool,
    pub playback_available: bool,
    pub capture_backend: String,
    pub playback_backend: String,
    pub warnings: Vec<String>,
    pub notices: Vec<String>,
}

impl VoiceEnvironmentReport {
    pub fn available(&self) -> bool {
        self.capture_available && self.playback_available && self.warnings.is_empty()
    }

    pub fn render(&self) -> String {
        let capture = if self.capture_available {
            "ready"
        } else {
            "not ready"
        };
        let playback = if self.playback_available {
            "ready"
        } else {
            "not ready"
        };
        let status = if self.available() {
            "ready"
        } else {
            "needs setup"
        };

        let mut lines = vec![
            format!("Voice audio environment: {status}"),
            format!("Capture: {capture} ({})", self.capture_backend),
            format!("Playback: {playback} ({})", self.playback_backend),
            format!(
                "Recording format: {VOICE_SAMPLE_RATE} Hz, {VOICE_CHANNELS} channel, {VOICE_SAMPLE_WIDTH_BYTES}-byte PCM WAV"
            ),
        ];

        if !self.warnings.is_empty() {
            lines.push("Warnings:".to_string());
            lines.extend(self.warnings.iter().map(|warning| format!("  - {warning}")));
        }

        if !self.notices.is_empty() {
            lines.push("Notices:".to_string());
            lines.extend(self.notices.iter().map(|notice| format!("  - {notice}")));
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct VoiceEnvironmentProbe {
    is_ssh: bool,
    is_container: bool,
    is_wsl: bool,
    is_termux: bool,
    has_forwarded_audio: bool,
    has_termux_microphone: bool,
    has_arecord: bool,
    has_ffmpeg: bool,
    playback_command: Option<String>,
}

impl VoiceEnvironmentProbe {
    fn detect() -> Self {
        Self {
            is_ssh: env_any_present(&["SSH_CLIENT", "SSH_TTY", "SSH_CONNECTION"]),
            is_container: detect_container(),
            is_wsl: detect_wsl(),
            is_termux: detect_termux(),
            has_forwarded_audio: has_forwarded_audio(),
            has_termux_microphone: command_exists("termux-microphone-record"),
            has_arecord: command_exists("arecord") || command_exists("rec"),
            has_ffmpeg: command_exists("ffmpeg"),
            playback_command: first_existing_command(&[
                "ffplay",
                "afplay",
                "aplay",
                "paplay",
                "pw-play",
                "mpg123",
                "powershell.exe",
            ]),
        }
    }

    fn report(&self) -> VoiceEnvironmentReport {
        let mut warnings = Vec::new();
        let mut notices = Vec::new();

        if self.is_ssh && self.has_forwarded_audio {
            notices.push("SSH session has PulseAudio/PipeWire forwarding configured.".to_string());
        } else if self.is_ssh {
            warnings.push(
                "SSH session has no detected PulseAudio/PipeWire forwarding; local microphone capture is unlikely."
                    .to_string(),
            );
        }

        if self.is_container && self.has_forwarded_audio {
            notices.push("Container has host audio forwarding configured.".to_string());
        } else if self.is_container {
            warnings.push(
                "Container has no detected host audio forwarding; mount PulseAudio/PipeWire sockets for voice I/O."
                    .to_string(),
            );
        }

        if self.is_wsl && self.has_forwarded_audio {
            notices.push("WSL has a PulseAudio/PipeWire bridge configured.".to_string());
        } else if self.is_wsl {
            warnings.push(
                "WSL audio bridge not detected; set PULSE_SERVER or PIPEWIRE_REMOTE before push-to-talk."
                    .to_string(),
            );
        }

        let (capture_available, capture_backend) = if self.is_termux && self.has_termux_microphone {
            (true, "Termux:API microphone".to_string())
        } else if self.has_arecord {
            (true, "system recorder command".to_string())
        } else if self.has_ffmpeg {
            (true, "ffmpeg input backend candidate".to_string())
        } else if self.has_forwarded_audio {
            (
                true,
                "forwarded PulseAudio/PipeWire backend candidate".to_string(),
            )
        } else {
            warnings.push(
                "No microphone capture backend detected; install ffmpeg, ALSA arecord/rec, or Termux:API."
                    .to_string(),
            );
            (false, "none detected".to_string())
        };

        let (playback_available, playback_backend) = if let Some(command) =
            self.playback_command.as_deref()
        {
            (true, command.to_string())
        } else if self.has_forwarded_audio {
            (true, "forwarded audio backend candidate".to_string())
        } else {
            warnings.push(
                "No local playback command detected; install ffplay, afplay, aplay, paplay, pw-play, or mpg123."
                    .to_string(),
            );
            (false, "none detected".to_string())
        };

        VoiceEnvironmentReport {
            capture_available,
            playback_available,
            capture_backend,
            playback_backend,
            warnings,
            notices,
        }
    }
}

/// Detect whether local voice capture/playback dependencies look usable.
pub fn detect_voice_environment() -> VoiceEnvironmentReport {
    VoiceEnvironmentProbe::detect().report()
}

/// Render a human-readable voice environment report for CLI/TUI/gateway surfaces.
pub fn render_voice_environment_report() -> String {
    detect_voice_environment().render()
}

/// Return true when a transcript looks like a common Whisper hallucination on silence.
pub fn is_whisper_hallucination(transcript: &str) -> bool {
    let cleaned = normalize_transcript(transcript);
    if cleaned.is_empty() {
        return true;
    }

    const EXACT: &[&str] = &[
        "thank you",
        "thanks for watching",
        "subscribe to my channel",
        "like and subscribe",
        "please subscribe",
        "thank you for watching",
        "bye",
        "you",
        "the end",
        "продолжение следует",
        "sous titres",
        "sous titres réalisés par la communauté d amara org",
        "sous titres realises par la communaute d amara org",
        "sottotitoli creati dalla comunita amara org",
        "untertitel von stephanie geiges",
        "amara org",
        "www mooji org",
        "ご視聴ありがとうございました",
    ];

    if EXACT.contains(&cleaned.as_str()) {
        return true;
    }

    let allowed = ["thank", "you", "thanks", "bye", "ok", "okay", "the", "end"];
    let mut tokens = cleaned.split_whitespace().peekable();
    tokens.peek().is_some() && tokens.all(|token| allowed.contains(&token))
}

fn normalize_transcript(transcript: &str) -> String {
    transcript
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || !ch.is_ascii() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn env_any_present(names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

fn detect_termux() -> bool {
    env_any_present(&["TERMUX_VERSION"])
        || std::env::var("PREFIX").is_ok_and(|value| value.contains("com.termux"))
        || std::env::var("HOME").is_ok_and(|value| value.contains("com.termux"))
}

fn detect_container() -> bool {
    Path::new("/.dockerenv").exists()
        || std::fs::read_to_string("/proc/1/cgroup").is_ok_and(|content| {
            let lower = content.to_ascii_lowercase();
            lower.contains("docker")
                || lower.contains("kubepods")
                || lower.contains("containerd")
                || lower.contains("podman")
                || lower.contains("lxc")
        })
}

fn detect_wsl() -> bool {
    std::fs::read_to_string("/proc/version").is_ok_and(|content| {
        let lower = content.to_ascii_lowercase();
        lower.contains("microsoft") || lower.contains("wsl")
    })
}

fn has_forwarded_audio() -> bool {
    env_any_present(&["PULSE_SERVER", "PIPEWIRE_REMOTE"])
        || unix_socket_candidate_exists(&pulse_socket_candidates())
}

fn pulse_socket_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(pulse_runtime) = std::env::var("PULSE_RUNTIME_PATH")
        && !pulse_runtime.trim().is_empty()
    {
        candidates.push(PathBuf::from(pulse_runtime).join("native"));
    }

    if let Ok(xdg_runtime) = std::env::var("XDG_RUNTIME_DIR")
        && !xdg_runtime.trim().is_empty()
    {
        let runtime = PathBuf::from(xdg_runtime);
        candidates.push(runtime.join("pulse").join("native"));
        candidates.push(runtime.join("pipewire-0"));
    }

    candidates
}

#[cfg(unix)]
fn unix_socket_candidate_exists(paths: &[PathBuf]) -> bool {
    use std::os::unix::fs::FileTypeExt;

    paths
        .iter()
        .any(|path| std::fs::metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket()))
}

#[cfg(not(unix))]
fn unix_socket_candidate_exists(paths: &[PathBuf]) -> bool {
    let _ = paths;
    false
}

fn first_existing_command(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find(|name| command_exists(name))
        .map(|name| (*name).to_string())
}

fn command_exists(name: &str) -> bool {
    if name.contains(std::path::MAIN_SEPARATOR) && Path::new(name).is_file() {
        return true;
    }

    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };

    let extensions = command_extensions();
    std::env::split_paths(&path_var).any(|dir| {
        extensions.iter().any(|extension| {
            let candidate = if extension.is_empty() {
                dir.join(name)
            } else {
                dir.join(format!("{name}{extension}"))
            };
            candidate.is_file()
        })
    })
}

#[cfg(windows)]
fn command_extensions() -> Vec<String> {
    let mut extensions = vec![String::new()];
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    extensions.extend(pathext.split(';').filter_map(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_ascii_lowercase())
    }));
    extensions
}

#[cfg(not(windows))]
fn command_extensions() -> Vec<String> {
    vec![String::new()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_whisper_hallucinations() {
        assert!(is_whisper_hallucination(""));
        assert!(is_whisper_hallucination("Thank you."));
        assert!(is_whisper_hallucination("Thanks for watching!"));
        assert!(is_whisper_hallucination("Thank you. Thank you. you"));
        assert!(is_whisper_hallucination("ご視聴ありがとうございました"));
    }

    #[test]
    fn keeps_real_transcripts() {
        assert!(!is_whisper_hallucination(
            "Schedule the release after CI passes."
        ));
        assert!(!is_whisper_hallucination("thanks, now open the dashboard"));
    }

    #[test]
    fn probe_report_marks_forwarded_audio_as_candidate_backend() {
        let report = VoiceEnvironmentProbe {
            has_forwarded_audio: true,
            ..Default::default()
        }
        .report();

        assert!(report.capture_available);
        assert!(report.playback_available);
        assert!(report.render().contains("Recording format"));
    }
}
