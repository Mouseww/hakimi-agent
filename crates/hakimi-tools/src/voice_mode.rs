//! Voice-mode helpers shared by CLI/TUI surfaces and audio tools.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

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

/// Minimum speech recording duration before captured audio is worth transcribing.
pub const MIN_SPEECH_RECORDING_SECONDS: f32 = 0.3;

/// Hermes-style maximum wait for speech before an interactive recording auto-stops.
pub const NO_SPEECH_TIMEOUT_SECONDS: f32 = 15.0;

/// Maximum text length sent to the voice playback TTS path.
pub const VOICE_TTS_MAX_CHARS: usize = 4_000;

static FENCED_CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?s)```.*?```").expect("valid fenced-code regex"));
static MARKDOWN_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^)]+\)").expect("valid markdown-link regex"));
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://\S+").expect("valid url regex"));
static BOLD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*\*(.+?)\*\*").expect("valid bold regex"));
static ITALIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*(.+?)\*").expect("valid italic regex"));
static INLINE_CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`(.+?)`").expect("valid inline-code regex"));
static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?m)^#+\\s*").expect("valid heading regex"));
static LIST_BULLET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?m)^\\s*[-*]\\s+").expect("valid list-bullet regex"));
static HORIZONTAL_RULE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("-{3,}").expect("valid horizontal-rule regex"));
static EXCESS_NEWLINES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("\n{3,}").expect("valid newline regex"));

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceRecordingSummary {
    pub samples: usize,
    pub duration_seconds: f32,
    pub peak_rms: u32,
    pub accepted: bool,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceTtsPlaybackPlan {
    pub text: String,
    pub output_path: PathBuf,
    pub playback_backend: Option<String>,
    pub cleanup_paths: Vec<PathBuf>,
}

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

/// Strip Markdown and URLs before sending an assistant response to TTS playback.
pub fn prepare_voice_tts_text(text: &str) -> Option<String> {
    let capped: String = text.chars().take(VOICE_TTS_MAX_CHARS).collect();
    let mut cleaned = capped;

    cleaned = FENCED_CODE_RE.replace_all(&cleaned, " ").into_owned();
    cleaned = MARKDOWN_LINK_RE.replace_all(&cleaned, "$1").into_owned();
    cleaned = URL_RE.replace_all(&cleaned, "").into_owned();
    cleaned = BOLD_RE.replace_all(&cleaned, "$1").into_owned();
    cleaned = ITALIC_RE.replace_all(&cleaned, "$1").into_owned();
    cleaned = INLINE_CODE_RE.replace_all(&cleaned, "$1").into_owned();
    cleaned = HEADING_RE.replace_all(&cleaned, "").into_owned();
    cleaned = LIST_BULLET_RE.replace_all(&cleaned, "").into_owned();
    cleaned = HORIZONTAL_RULE_RE.replace_all(&cleaned, "").into_owned();
    cleaned = EXCESS_NEWLINES_RE
        .replace_all(&cleaned, "\n\n")
        .into_owned();

    let cleaned = cleaned.trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Default directory for voice-mode TTS files that are intended for playback.
pub fn voice_tts_cache_dir() -> PathBuf {
    std::env::temp_dir().join("hakimi_voice")
}

/// Generate an MP3 output path for voice-mode TTS playback.
pub fn next_voice_tts_output_path() -> PathBuf {
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let suffix = uuid::Uuid::new_v4().to_string().replace('-', "");
    voice_tts_output_path_for(&voice_tts_cache_dir(), &timestamp, &suffix[..8])
}

/// Build a deterministic voice TTS path for tests and callers with their own clock.
pub fn voice_tts_output_path_for(dir: &Path, timestamp: &str, suffix: &str) -> PathBuf {
    dir.join(format!("tts_{timestamp}_{suffix}.mp3"))
}

/// Return the generated audio paths Hermes-style playback should clean up.
pub fn voice_tts_cleanup_paths(output_path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![output_path.to_path_buf()];
    if output_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"))
    {
        paths.push(output_path.with_extension("ogg"));
    }
    paths
}

/// Prepare sanitized TTS text, output path, and playback metadata.
pub fn plan_voice_tts_playback(
    text: &str,
    output_path: Option<PathBuf>,
    auto_play: bool,
) -> Option<VoiceTtsPlaybackPlan> {
    let text = prepare_voice_tts_text(text)?;
    let output_path = output_path.unwrap_or_else(next_voice_tts_output_path);
    let playback_backend = auto_play
        .then(|| detect_voice_environment().playback_backend)
        .filter(|backend| backend != "none detected");
    let cleanup_paths = voice_tts_cleanup_paths(&output_path);

    Some(VoiceTtsPlaybackPlan {
        text,
        output_path,
        playback_backend,
        cleanup_paths,
    })
}

/// Summarize PCM16 mono recording data using Hermes-compatible speech gates.
pub fn summarize_pcm16_recording(samples: &[i16], silence_threshold: u32) -> VoiceRecordingSummary {
    let peak_rms = peak_pcm16_rms(samples);
    let duration_seconds = samples.len() as f32 / VOICE_SAMPLE_RATE as f32;
    let rejection_reason = if samples.len() < minimum_voice_samples() {
        Some(format!(
            "recording shorter than {MIN_SPEECH_RECORDING_SECONDS:.1}s"
        ))
    } else if peak_rms < silence_threshold {
        Some(format!(
            "recording peak RMS {peak_rms} is below threshold {silence_threshold}"
        ))
    } else {
        None
    };

    VoiceRecordingSummary {
        samples: samples.len(),
        duration_seconds,
        peak_rms,
        accepted: rejection_reason.is_none(),
        rejection_reason,
    }
}

/// Write PCM16 mono samples as a WAV file and return the transcription gate summary.
pub fn write_pcm16_wav(
    path: impl AsRef<Path>,
    samples: &[i16],
) -> io::Result<VoiceRecordingSummary> {
    write_pcm16_wav_with_threshold(path, samples, DEFAULT_SILENCE_RMS_THRESHOLD)
}

/// Write PCM16 mono samples as a WAV file with a caller-provided silence gate.
pub fn write_pcm16_wav_with_threshold(
    path: impl AsRef<Path>,
    samples: &[i16],
    silence_threshold: u32,
) -> io::Result<VoiceRecordingSummary> {
    let path = path.as_ref();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let data_len = samples
        .len()
        .checked_mul(VOICE_SAMPLE_WIDTH_BYTES as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WAV data is too large"))?;
    let data_len = u32::try_from(data_len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "WAV data is too large"))?;
    let riff_len = 36u32
        .checked_add(data_len)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WAV data is too large"))?;
    let byte_rate =
        VOICE_SAMPLE_RATE * u32::from(VOICE_CHANNELS) * u32::from(VOICE_SAMPLE_WIDTH_BYTES);
    let block_align = VOICE_CHANNELS * VOICE_SAMPLE_WIDTH_BYTES;

    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&riff_len.to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&VOICE_CHANNELS.to_le_bytes())?;
    file.write_all(&VOICE_SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&(VOICE_SAMPLE_WIDTH_BYTES * 8).to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    file.flush()?;

    Ok(summarize_pcm16_recording(samples, silence_threshold))
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

fn minimum_voice_samples() -> usize {
    (VOICE_SAMPLE_RATE as f32 * MIN_SPEECH_RECORDING_SECONDS).round() as usize
}

fn peak_pcm16_rms(samples: &[i16]) -> u32 {
    let frame_samples = (VOICE_SAMPLE_RATE as usize / 50).max(1);
    samples
        .chunks(frame_samples)
        .map(chunk_rms)
        .max()
        .unwrap_or(0)
}

fn chunk_rms(samples: &[i16]) -> u32 {
    if samples.is_empty() {
        return 0;
    }

    let sum_squares: f64 = samples
        .iter()
        .map(|sample| {
            let value = f64::from(*sample);
            value * value
        })
        .sum();
    (sum_squares / samples.len() as f64).sqrt().round() as u32
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
    fn prepares_voice_tts_text_like_hermes_playback() {
        let input = "# Release\n\n- **Ship** [Hakimi](https://example.com)\n- Run `cargo test`\n\n```rust\nfn main() {}\n```\n\nhttps://example.com/raw";
        let prepared = prepare_voice_tts_text(input).expect("prepared text");

        assert!(prepared.contains("Release"));
        assert!(prepared.contains("Ship Hakimi"));
        assert!(prepared.contains("Run cargo test"));
        assert!(!prepared.contains("https://"));
        assert!(!prepared.contains("fn main"));
        assert!(!prepared.contains("**"));
        assert!(!prepared.contains('`'));
    }

    #[test]
    fn prepares_voice_tts_text_caps_by_chars() {
        let input = format!("{}tail", "你".repeat(VOICE_TTS_MAX_CHARS));
        let prepared = prepare_voice_tts_text(&input).expect("prepared text");

        assert_eq!(prepared.chars().count(), VOICE_TTS_MAX_CHARS);
        assert!(!prepared.contains("tail"));
    }

    #[test]
    fn plans_voice_tts_playback_paths_and_cleanup() {
        let dir = Path::new("/tmp/hakimi-voice-test");
        let path = voice_tts_output_path_for(dir, "20260601_010203", "abcdef12");
        let plan =
            plan_voice_tts_playback("hello", Some(path.clone()), false).expect("playback plan");

        assert_eq!(plan.text, "hello");
        assert_eq!(plan.output_path, path);
        assert_eq!(
            plan.cleanup_paths,
            vec![
                dir.join("tts_20260601_010203_abcdef12.mp3"),
                dir.join("tts_20260601_010203_abcdef12.ogg"),
            ]
        );
        assert_eq!(plan.playback_backend, None);
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

    #[test]
    fn writes_pcm16_wav_with_hermes_recording_header() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("recording.wav");
        let summary = write_pcm16_wav(&path, &[0, 1_000, -1_000]).expect("write wav");
        let bytes = std::fs::read(path).expect("read wav");

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[22..24], &VOICE_CHANNELS.to_le_bytes());
        assert_eq!(&bytes[24..28], &VOICE_SAMPLE_RATE.to_le_bytes());
        assert_eq!(
            &bytes[34..36],
            &(VOICE_SAMPLE_WIDTH_BYTES * 8).to_le_bytes()
        );
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        assert_eq!(summary.samples, 3);
    }

    #[test]
    fn recording_summary_rejects_short_input() {
        let samples = vec![1_000; minimum_voice_samples() - 1];
        let summary = summarize_pcm16_recording(&samples, DEFAULT_SILENCE_RMS_THRESHOLD);

        assert!(!summary.accepted);
        assert!(
            summary
                .rejection_reason
                .as_deref()
                .unwrap_or_default()
                .contains("shorter")
        );
    }

    #[test]
    fn recording_summary_rejects_quiet_input() {
        let samples = vec![10; minimum_voice_samples()];
        let summary = summarize_pcm16_recording(&samples, DEFAULT_SILENCE_RMS_THRESHOLD);

        assert!(!summary.accepted);
        assert!(summary.peak_rms < DEFAULT_SILENCE_RMS_THRESHOLD);
    }

    #[test]
    fn recording_summary_accepts_loud_input() {
        let samples = vec![1_000; minimum_voice_samples()];
        let summary = summarize_pcm16_recording(&samples, DEFAULT_SILENCE_RMS_THRESHOLD);

        assert!(summary.accepted);
        assert!(summary.rejection_reason.is_none());
        assert!(summary.duration_seconds >= MIN_SPEECH_RECORDING_SECONDS);
    }
}
