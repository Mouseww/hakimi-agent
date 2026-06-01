//! Voice-mode helpers shared by CLI/TUI surfaces and audio tools.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::LazyLock,
    sync::Mutex,
    time::{Duration, Instant},
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

/// Hermes-style start-recording cue frequency.
pub const VOICE_CUE_START_FREQUENCY_HZ: u32 = 880;

/// Hermes-style stop-recording cue frequency.
pub const VOICE_CUE_STOP_FREQUENCY_HZ: u32 = 660;

/// Duration of each short voice cue tone.
pub const VOICE_CUE_DURATION_SECONDS: f32 = 0.12;

/// Gap between repeated voice cue tones.
pub const VOICE_CUE_GAP_SECONDS: f32 = 0.06;

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
static ACTIVE_PLAYBACK: LazyLock<Mutex<Option<Child>>> = LazyLock::new(|| Mutex::new(None));
const PLAYBACK_MAX_SECONDS: u64 = 300;

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
pub struct VoicePlaybackCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePlaybackStart {
    pub command: VoicePlaybackCommand,
    pub process_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCueKind {
    Start,
    Stop,
}

impl VoiceCueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
        }
    }

    pub fn frequency_hz(self) -> u32 {
        match self {
            Self::Start => VOICE_CUE_START_FREQUENCY_HZ,
            Self::Stop => VOICE_CUE_STOP_FREQUENCY_HZ,
        }
    }

    pub fn count(self) -> usize {
        match self {
            Self::Start => 1,
            Self::Stop => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCuePlan {
    pub kind: VoiceCueKind,
    pub output_path: PathBuf,
    pub frequency_hz: u32,
    pub count: usize,
    pub playback_backend: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCueStart {
    pub plan: VoiceCuePlan,
    pub playback: VoicePlaybackStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCaptureFormat {
    Pcm16Wav,
    EncodedAudio,
}

impl VoiceCaptureFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pcm16Wav => "pcm16_wav",
            Self::EncodedAudio => "encoded_audio",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCaptureCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceCapturePlan {
    pub command: VoiceCaptureCommand,
    pub output_path: PathBuf,
    pub backend: String,
    pub format: VoiceCaptureFormat,
    pub duration_seconds: f32,
    pub silence_threshold: u32,
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
            playback_command: find_voice_playback_command(Path::new("hakimi_voice_probe.mp3"))
                .map(|command| command.program),
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

/// Default directory for voice-mode start/stop cue files.
pub fn voice_cue_cache_dir() -> PathBuf {
    std::env::temp_dir().join("hakimi_voice")
}

/// Build a deterministic voice cue path for tests and callers with their own cache.
pub fn voice_cue_output_path_for(dir: &Path, kind: VoiceCueKind) -> PathBuf {
    dir.join(format!("cue_{}.wav", kind.as_str()))
}

/// Plan the cue WAV path and playback backend for a start/stop recording cue.
pub fn plan_voice_cue(kind: VoiceCueKind) -> VoiceCuePlan {
    plan_voice_cue_for_dir(kind, &voice_cue_cache_dir())
}

/// Plan a voice cue using a caller-provided cache directory.
pub fn plan_voice_cue_for_dir(kind: VoiceCueKind, dir: &Path) -> VoiceCuePlan {
    let output_path = voice_cue_output_path_for(dir, kind);
    let playback_backend = find_voice_playback_command(&output_path).map(|command| command.program);
    VoiceCuePlan {
        kind,
        output_path,
        frequency_hz: kind.frequency_hz(),
        count: kind.count(),
        playback_backend,
    }
}

/// Render the configured state of Hermes-style record start/stop cues.
pub fn render_voice_cue_status(enabled: bool) -> String {
    if enabled {
        format!(
            "Audio cues: enabled (start={}Hz x{}, stop={}Hz x{}, {:.2}s tones)",
            VoiceCueKind::Start.frequency_hz(),
            VoiceCueKind::Start.count(),
            VoiceCueKind::Stop.frequency_hz(),
            VoiceCueKind::Stop.count(),
            VOICE_CUE_DURATION_SECONDS
        )
    } else {
        "Audio cues: disabled by voice.beep_enabled=false".to_string()
    }
}

/// Write a short PCM16 WAV recording cue.
pub fn write_voice_cue(path: impl AsRef<Path>, kind: VoiceCueKind) -> io::Result<()> {
    let samples = voice_cue_samples(kind);
    write_pcm16_wav_with_threshold(path, &samples, 0).map(|_| ())
}

/// Write and start playing a Hermes-style recording cue.
pub fn start_voice_cue(kind: VoiceCueKind) -> io::Result<VoiceCueStart> {
    let plan = plan_voice_cue(kind);
    write_voice_cue(&plan.output_path, kind)?;
    let playback = start_voice_playback(&plan.output_path)?;
    Ok(VoiceCueStart { plan, playback })
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

/// Return Hermes-style local playback command candidates for an audio file.
pub fn voice_playback_command_candidates(path: &Path) -> Vec<VoicePlaybackCommand> {
    voice_playback_command_candidates_for_platform(path, std::env::consts::OS)
}

/// Return local playback command candidates for a specific platform string.
pub fn voice_playback_command_candidates_for_platform(
    path: &Path,
    platform: &str,
) -> Vec<VoicePlaybackCommand> {
    let path = path.to_string_lossy().to_string();
    let extension = Path::new(&path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_wav = extension == "wav";
    let is_mp3 = extension == "mp3";

    let mut commands = Vec::new();

    if platform == "macos" {
        commands.push(VoicePlaybackCommand {
            program: "afplay".to_string(),
            args: vec![path.clone()],
        });
    }

    commands.push(VoicePlaybackCommand {
        program: "ffplay".to_string(),
        args: vec![
            "-nodisp".to_string(),
            "-autoexit".to_string(),
            "-loglevel".to_string(),
            "quiet".to_string(),
            path.clone(),
        ],
    });

    if is_mp3 {
        commands.push(VoicePlaybackCommand {
            program: "mpg123".to_string(),
            args: vec!["-q".to_string(), path.clone()],
        });
    }

    if platform == "linux" && is_wav {
        commands.push(VoicePlaybackCommand {
            program: "aplay".to_string(),
            args: vec!["-q".to_string(), path.clone()],
        });
        commands.push(VoicePlaybackCommand {
            program: "paplay".to_string(),
            args: vec![path.clone()],
        });
        commands.push(VoicePlaybackCommand {
            program: "pw-play".to_string(),
            args: vec![path.clone()],
        });
    }

    if platform == "windows" && is_wav {
        commands.push(VoicePlaybackCommand {
            program: "powershell.exe".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "$p=$args[0]; $player=New-Object System.Media.SoundPlayer $p; $player.PlaySync()"
                    .to_string(),
                path,
            ],
        });
    }

    commands
}

/// Return the first installed playback command for an audio file.
pub fn find_voice_playback_command(path: &Path) -> Option<VoicePlaybackCommand> {
    voice_playback_command_candidates(path)
        .into_iter()
        .find(|command| command_exists(&command.program))
}

/// Start local audio playback through the first available system player.
pub fn start_voice_playback(path: impl AsRef<Path>) -> io::Result<VoicePlaybackStart> {
    let path = path.as_ref();
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("audio file not found: {}", path.display()),
        ));
    }

    let command = find_voice_playback_command(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no local audio playback command found",
        )
    })?;

    stop_voice_playback();

    let child = Command::new(&command.program)
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let process_id = child.id();

    {
        let mut active = ACTIVE_PLAYBACK
            .lock()
            .map_err(|_| io::Error::other("voice playback lock poisoned"))?;
        *active = Some(child);
    }
    reap_voice_playback_when_done(process_id);

    Ok(VoicePlaybackStart {
        command,
        process_id,
    })
}

/// Stop the currently active local audio playback process, if any.
pub fn stop_voice_playback() -> bool {
    let Ok(mut active) = ACTIVE_PLAYBACK.lock() else {
        return false;
    };
    let Some(mut child) = active.take() else {
        return false;
    };
    let _ = child.kill();
    let _ = child.wait();
    true
}

/// Default directory for temporary push-to-talk recordings.
pub fn voice_capture_cache_dir() -> PathBuf {
    std::env::temp_dir().join("hakimi_voice")
}

/// Build a deterministic voice recording path for tests and callers with their own clock.
pub fn voice_capture_output_path_for(
    dir: &Path,
    timestamp: &str,
    suffix: &str,
    ext: &str,
) -> PathBuf {
    dir.join(format!("recording_{timestamp}_{suffix}.{ext}"))
}

/// Generate a temporary voice recording path.
pub fn next_voice_capture_output_path(ext: &str) -> PathBuf {
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let suffix = uuid::Uuid::new_v4().to_string().replace('-', "");
    voice_capture_output_path_for(&voice_capture_cache_dir(), &timestamp, &suffix[..8], ext)
}

/// Return Hermes-style one-shot microphone capture command candidates.
pub fn voice_capture_command_candidates(
    output_path: Option<&Path>,
    duration_seconds: f32,
    silence_threshold: u32,
) -> Vec<VoiceCapturePlan> {
    voice_capture_command_candidates_for_platform(
        output_path,
        duration_seconds,
        silence_threshold,
        std::env::consts::OS,
        detect_termux(),
    )
}

/// Return microphone capture command candidates for a specific platform.
pub fn voice_capture_command_candidates_for_platform(
    output_path: Option<&Path>,
    duration_seconds: f32,
    silence_threshold: u32,
    platform: &str,
    is_termux: bool,
) -> Vec<VoiceCapturePlan> {
    let duration_seconds = sanitized_voice_capture_duration(duration_seconds);
    let duration_arg = format!("{duration_seconds:.3}");
    let duration_secs = duration_seconds.ceil().max(1.0).to_string();
    let wav_path = output_path
        .filter(|path| path.extension_is("wav"))
        .map(Path::to_path_buf)
        .unwrap_or_else(|| next_voice_capture_output_path("wav"));
    let aac_path = output_path
        .filter(|path| path.extension_is("aac"))
        .map(Path::to_path_buf)
        .unwrap_or_else(|| next_voice_capture_output_path("aac"));
    let wav = wav_path.to_string_lossy().to_string();
    let aac = aac_path.to_string_lossy().to_string();

    let mut candidates = Vec::new();
    if is_termux {
        candidates.push(VoiceCapturePlan {
            command: VoiceCaptureCommand {
                program: "termux-microphone-record".to_string(),
                args: vec![
                    "-f".to_string(),
                    aac.clone(),
                    "-l".to_string(),
                    duration_secs.clone(),
                    "-e".to_string(),
                    "aac".to_string(),
                    "-r".to_string(),
                    VOICE_SAMPLE_RATE.to_string(),
                    "-c".to_string(),
                    VOICE_CHANNELS.to_string(),
                ],
            },
            output_path: aac_path.clone(),
            backend: "termux-microphone-record".to_string(),
            format: VoiceCaptureFormat::EncodedAudio,
            duration_seconds,
            silence_threshold,
        });
    }

    if platform == "linux" {
        candidates.push(VoiceCapturePlan {
            command: VoiceCaptureCommand {
                program: "arecord".to_string(),
                args: vec![
                    "-q".to_string(),
                    "-d".to_string(),
                    duration_secs.clone(),
                    "-f".to_string(),
                    "S16_LE".to_string(),
                    "-r".to_string(),
                    VOICE_SAMPLE_RATE.to_string(),
                    "-c".to_string(),
                    VOICE_CHANNELS.to_string(),
                    wav.clone(),
                ],
            },
            output_path: wav_path.clone(),
            backend: "arecord".to_string(),
            format: VoiceCaptureFormat::Pcm16Wav,
            duration_seconds,
            silence_threshold,
        });
        candidates.push(VoiceCapturePlan {
            command: VoiceCaptureCommand {
                program: "rec".to_string(),
                args: vec![
                    "-q".to_string(),
                    "-r".to_string(),
                    VOICE_SAMPLE_RATE.to_string(),
                    "-c".to_string(),
                    VOICE_CHANNELS.to_string(),
                    "-b".to_string(),
                    "16".to_string(),
                    wav.clone(),
                    "trim".to_string(),
                    "0".to_string(),
                    duration_arg.clone(),
                ],
            },
            output_path: wav_path.clone(),
            backend: "sox-rec".to_string(),
            format: VoiceCaptureFormat::Pcm16Wav,
            duration_seconds,
            silence_threshold,
        });
        candidates.push(ffmpeg_capture_plan(
            wav_path.clone(),
            vec![
                "-f".to_string(),
                "pulse".to_string(),
                "-i".to_string(),
                "default".to_string(),
            ],
            "ffmpeg-pulse",
            duration_seconds,
            silence_threshold,
        ));
        candidates.push(ffmpeg_capture_plan(
            wav_path.clone(),
            vec![
                "-f".to_string(),
                "alsa".to_string(),
                "-i".to_string(),
                "default".to_string(),
            ],
            "ffmpeg-alsa",
            duration_seconds,
            silence_threshold,
        ));
    } else if platform == "macos" {
        candidates.push(ffmpeg_capture_plan(
            wav_path.clone(),
            vec![
                "-f".to_string(),
                "avfoundation".to_string(),
                "-i".to_string(),
                ":0".to_string(),
            ],
            "ffmpeg-avfoundation",
            duration_seconds,
            silence_threshold,
        ));
    } else if platform == "windows" {
        candidates.push(ffmpeg_capture_plan(
            wav_path.clone(),
            vec![
                "-f".to_string(),
                "dshow".to_string(),
                "-i".to_string(),
                "audio=default".to_string(),
            ],
            "ffmpeg-dshow",
            duration_seconds,
            silence_threshold,
        ));
    }

    candidates
}

/// Return the first installed one-shot microphone capture plan.
pub fn find_voice_capture_plan(
    output_path: Option<&Path>,
    duration_seconds: f32,
    silence_threshold: u32,
) -> Option<VoiceCapturePlan> {
    voice_capture_command_candidates(output_path, duration_seconds, silence_threshold)
        .into_iter()
        .find(|plan| command_exists(&plan.command.program))
}

fn reap_voice_playback_when_done(process_id: u32) {
    let _ = std::thread::spawn(move || {
        let started = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let Ok(mut active) = ACTIVE_PLAYBACK.lock() else {
                return;
            };
            let Some(child) = active.as_mut() else {
                return;
            };
            if child.id() != process_id {
                return;
            }
            if child.try_wait().ok().flatten().is_some() {
                active.take();
                return;
            }
            if started.elapsed() > Duration::from_secs(PLAYBACK_MAX_SECONDS) {
                if let Some(mut child) = active.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return;
            }
        }
    });
}

fn ffmpeg_capture_plan(
    output_path: PathBuf,
    input_args: Vec<String>,
    backend: &str,
    duration_seconds: f32,
    silence_threshold: u32,
) -> VoiceCapturePlan {
    let mut args = vec![
        "-y".to_string(),
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
    ];
    args.extend(input_args);
    args.extend([
        "-t".to_string(),
        format!("{duration_seconds:.3}"),
        "-ac".to_string(),
        VOICE_CHANNELS.to_string(),
        "-ar".to_string(),
        VOICE_SAMPLE_RATE.to_string(),
        "-acodec".to_string(),
        "pcm_s16le".to_string(),
        output_path.to_string_lossy().to_string(),
    ]);
    VoiceCapturePlan {
        command: VoiceCaptureCommand {
            program: "ffmpeg".to_string(),
            args,
        },
        output_path,
        backend: backend.to_string(),
        format: VoiceCaptureFormat::Pcm16Wav,
        duration_seconds,
        silence_threshold,
    }
}

fn sanitized_voice_capture_duration(duration_seconds: f32) -> f32 {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return NO_SPEECH_TIMEOUT_SECONDS;
    }
    duration_seconds.clamp(MIN_SPEECH_RECORDING_SECONDS, 300.0)
}

fn voice_cue_samples(kind: VoiceCueKind) -> Vec<i16> {
    let tone_samples = (VOICE_SAMPLE_RATE as f32 * VOICE_CUE_DURATION_SECONDS)
        .round()
        .max(1.0) as usize;
    let gap_samples = (VOICE_SAMPLE_RATE as f32 * VOICE_CUE_GAP_SECONDS)
        .round()
        .max(0.0) as usize;
    let mut samples = Vec::with_capacity(
        tone_samples * kind.count() + gap_samples * kind.count().saturating_sub(1),
    );

    for index in 0..kind.count() {
        if index > 0 {
            samples.extend(std::iter::repeat_n(0, gap_samples));
        }
        append_voice_cue_tone(&mut samples, kind.frequency_hz(), tone_samples);
    }

    samples
}

fn append_voice_cue_tone(samples: &mut Vec<i16>, frequency_hz: u32, sample_count: usize) {
    let fade_samples = ((VOICE_SAMPLE_RATE as f32 * 0.01).round() as usize)
        .min(sample_count / 4)
        .max(1);
    let amplitude = i16::MAX as f32 * 0.20;

    for index in 0..sample_count {
        let fade = if index < fade_samples {
            index as f32 / fade_samples as f32
        } else if sample_count - index <= fade_samples {
            (sample_count - index) as f32 / fade_samples as f32
        } else {
            1.0
        };
        let phase =
            std::f32::consts::TAU * frequency_hz as f32 * index as f32 / VOICE_SAMPLE_RATE as f32;
        samples.push((phase.sin() * amplitude * fade) as i16);
    }
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

/// Read and summarize a PCM16 mono WAV recording created by a capture backend.
pub fn summarize_pcm16_wav_file(
    path: impl AsRef<Path>,
    silence_threshold: u32,
) -> io::Result<VoiceRecordingSummary> {
    let bytes = std::fs::read(path)?;
    let data = extract_pcm16_mono_wav_data(&bytes)?;
    let samples = data
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    Ok(summarize_pcm16_recording(&samples, silence_threshold))
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

fn extract_pcm16_mono_wav_data(bytes: &[u8]) -> io::Result<&[u8]> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "recording is not a RIFF/WAVE file",
        ));
    }

    let mut offset = 12usize;
    let mut format_ok = false;
    let mut data_range = None;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let id = &bytes[offset..offset + 4];
        let len = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        let end = start
            .checked_add(len)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "WAV chunk is too large"))?;
        if end > bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "WAV chunk exceeds file size",
            ));
        }

        if id == b"fmt " {
            if len < 16 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "WAV fmt chunk is too short",
                ));
            }
            let audio_format = u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap());
            let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap());
            let sample_rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap());
            let bits_per_sample =
                u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap());
            format_ok = audio_format == 1
                && channels == VOICE_CHANNELS
                && sample_rate == VOICE_SAMPLE_RATE
                && bits_per_sample == VOICE_SAMPLE_WIDTH_BYTES * 8;
        } else if id == b"data" {
            data_range = Some(start..end);
        }

        offset = end + (len % 2);
    }

    if !format_ok {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "WAV recording is not 16 kHz mono PCM16",
        ));
    }
    let range = data_range.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "WAV recording has no data chunk",
        )
    })?;
    if range.len() % 2 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "WAV data chunk has an odd byte length",
        ));
    }
    Ok(&bytes[range])
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

trait VoicePathExt {
    fn extension_is(&self, expected: &str) -> bool;
}

impl VoicePathExt for Path {
    fn extension_is(&self, expected: &str) -> bool {
        self.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
    }
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
    fn playback_candidates_prefer_afplay_on_macos_mp3() {
        let commands =
            voice_playback_command_candidates_for_platform(Path::new("/tmp/reply.mp3"), "macos");

        assert_eq!(commands[0].program, "afplay");
        assert_eq!(commands[0].args, vec!["/tmp/reply.mp3"]);
        assert!(commands.iter().any(|command| command.program == "ffplay"));
        assert!(commands.iter().any(|command| command.program == "mpg123"));
    }

    #[test]
    fn playback_candidates_include_linux_wav_players() {
        let commands =
            voice_playback_command_candidates_for_platform(Path::new("/tmp/reply.wav"), "linux");
        let programs: Vec<&str> = commands
            .iter()
            .map(|command| command.program.as_str())
            .collect();

        assert!(programs.contains(&"ffplay"));
        assert!(programs.contains(&"aplay"));
        assert!(programs.contains(&"paplay"));
        assert!(programs.contains(&"pw-play"));
        assert!(!programs.contains(&"mpg123"));
    }

    #[test]
    fn playback_candidates_include_windows_wav_soundplayer() {
        let commands = voice_playback_command_candidates_for_platform(
            Path::new("C:/tmp/reply.wav"),
            "windows",
        );
        let powershell = commands
            .iter()
            .find(|command| command.program == "powershell.exe")
            .expect("powershell candidate");

        assert!(
            powershell
                .args
                .iter()
                .any(|arg| arg.contains("SoundPlayer"))
        );
    }

    #[test]
    fn voice_cue_plans_hermes_style_start_and_stop_tones() {
        let dir = Path::new("/tmp/hakimi-voice-cue-test");
        let start = plan_voice_cue_for_dir(VoiceCueKind::Start, dir);
        let stop = plan_voice_cue_for_dir(VoiceCueKind::Stop, dir);

        assert_eq!(start.output_path, dir.join("cue_start.wav"));
        assert_eq!(start.frequency_hz, VOICE_CUE_START_FREQUENCY_HZ);
        assert_eq!(start.count, 1);
        assert_eq!(stop.output_path, dir.join("cue_stop.wav"));
        assert_eq!(stop.frequency_hz, VOICE_CUE_STOP_FREQUENCY_HZ);
        assert_eq!(stop.count, 2);
    }

    #[test]
    fn voice_cue_wav_contains_short_pcm16_tone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cue_stop.wav");

        write_voice_cue(&path, VoiceCueKind::Stop).expect("write cue");
        let summary = summarize_pcm16_wav_file(&path, 0).expect("summarize cue");

        assert!(summary.accepted);
        assert_eq!(summary.samples, voice_cue_samples(VoiceCueKind::Stop).len());
        assert!(summary.peak_rms > 0);
    }

    #[test]
    fn voice_cue_status_respects_config_flag() {
        assert!(render_voice_cue_status(true).contains("start=880Hz x1"));
        assert!(render_voice_cue_status(true).contains("stop=660Hz x2"));
        assert_eq!(
            render_voice_cue_status(false),
            "Audio cues: disabled by voice.beep_enabled=false"
        );
    }

    #[test]
    fn capture_candidates_include_linux_pcm16_backends() {
        let path = Path::new("/tmp/hakimi-voice.wav");
        let plans = voice_capture_command_candidates_for_platform(
            Some(path),
            2.25,
            DEFAULT_SILENCE_RMS_THRESHOLD,
            "linux",
            false,
        );
        let backends = plans
            .iter()
            .map(|plan| plan.backend.as_str())
            .collect::<Vec<_>>();

        assert!(backends.contains(&"arecord"));
        assert!(backends.contains(&"sox-rec"));
        assert!(backends.contains(&"ffmpeg-pulse"));
        assert!(
            plans
                .iter()
                .all(|plan| plan.format == VoiceCaptureFormat::Pcm16Wav)
        );
        assert!(plans.iter().all(|plan| plan.output_path == path));
        assert!(
            plans
                .iter()
                .any(|plan| plan.command.args.iter().any(|arg| arg == "16000"))
        );
    }

    #[test]
    fn capture_candidates_include_termux_encoded_backend() {
        let path = Path::new("/tmp/hakimi-voice.aac");
        let plans = voice_capture_command_candidates_for_platform(
            Some(path),
            1.0,
            DEFAULT_SILENCE_RMS_THRESHOLD,
            "linux",
            true,
        );
        let termux = plans
            .iter()
            .find(|plan| plan.backend == "termux-microphone-record")
            .expect("termux plan");

        assert_eq!(termux.output_path, path);
        assert_eq!(termux.format, VoiceCaptureFormat::EncodedAudio);
        assert!(termux.command.args.iter().any(|arg| arg == "aac"));
    }

    #[test]
    fn capture_duration_is_clamped_to_safe_bounds() {
        let plan = voice_capture_command_candidates_for_platform(
            Some(Path::new("/tmp/hakimi-voice.wav")),
            f32::NAN,
            DEFAULT_SILENCE_RMS_THRESHOLD,
            "macos",
            false,
        )
        .into_iter()
        .next()
        .expect("capture plan");

        assert_eq!(plan.duration_seconds, NO_SPEECH_TIMEOUT_SECONDS);
        assert!(plan.command.args.iter().any(|arg| arg == "15.000"));
    }

    #[test]
    fn start_voice_playback_rejects_missing_file() {
        let err = start_voice_playback(Path::new("/tmp/hakimi-missing-audio.mp3"))
            .expect_err("missing file should be rejected");

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
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
    fn summarizes_pcm16_wav_file_written_by_capture_backend() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("recording.wav");
        let samples = vec![1_000; minimum_voice_samples()];
        write_pcm16_wav(&path, &samples).expect("write wav");

        let summary =
            summarize_pcm16_wav_file(&path, DEFAULT_SILENCE_RMS_THRESHOLD).expect("summary");

        assert!(summary.accepted);
        assert_eq!(summary.samples, samples.len());
        assert!(summary.peak_rms >= DEFAULT_SILENCE_RMS_THRESHOLD);
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
