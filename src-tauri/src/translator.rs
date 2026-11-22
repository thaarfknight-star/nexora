//! Optional downloadable real-time audio-to-subtitle translation engine.
//!
//! The engine is downloaded on demand into `<app_data_dir>/live-translate/`
//! using pure native Rust HTTP streaming (`reqwest`) and native ZIP extraction
//! (`zip`) — never spawning `curl.exe` or `powershell.exe` during download.
//!
//! Low-latency pipeline on Windows:
//! 1. **WASAPI Loopback Producer Thread**: Captures internal system audio
//!    (`"system"`) or a specific app's process-tree (`"app"`) with zero
//!    microphone input. Uses fast VAD pause endpointing (900–2,000 ms chunks)
//!    so phrases flush immediately on natural speech pauses.
//! 2. **Pipelined Inference & Translation Consumer Thread**: Keeps
//!    `whisper-server.exe` warm in RAM (no 75 MB model reload per chunk,
//!    in-memory WAV multipart POST, greedy `-bo 1 -bs 1 -nf -sns`), with
//!    automatic fallback to fast `whisper-cli.exe`, and translates over a
//!    persistent keep-alive `reqwest` HTTP client.

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const WHISPER_HF_COMMIT: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const WHISPER_ZIP_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/download/b5907/whisper-bin-x64.zip";
const EXPECTED_WHISPER_ZIP_SHA256: &str =
    "ecca07b277d943e9b9f4712856a144b857271a6b333104e511e1988e80db582b";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhisperModelSpec {
    pub filename: &'static str,
    pub sha256: &'static str,
    pub size_mb: u64,
    pub english_only: bool,
    /// Adaptive snapshot interval so heavier models do not stall CPU.
    pub snapshot_interval_ms: u64,
}

/// Verified official `ggerganov/whisper.cpp` GGML models pinned to `WHISPER_HF_COMMIT`.
pub const WHISPER_MODELS: &[WhisperModelSpec] = &[
    WhisperModelSpec {
        filename: "ggml-tiny.bin",
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        size_mb: 74,
        english_only: false,
        snapshot_interval_ms: 250,
    },
    WhisperModelSpec {
        filename: "ggml-tiny.en.bin",
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        size_mb: 74,
        english_only: true,
        snapshot_interval_ms: 250,
    },
    WhisperModelSpec {
        filename: "ggml-base.en.bin",
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        size_mb: 141,
        english_only: true,
        snapshot_interval_ms: 350,
    },
    WhisperModelSpec {
        filename: "ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size_mb: 547,
        english_only: false,
        snapshot_interval_ms: 750,
    },
];

pub fn whisper_model_spec(filename: &str) -> WhisperModelSpec {
    WHISPER_MODELS
        .iter()
        .copied()
        .find(|m| m.filename == filename)
        .unwrap_or(WHISPER_MODELS[0])
}

pub fn whisper_model_url(spec: WhisperModelSpec) -> String {
    format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/{WHISPER_HF_COMMIT}/{}",
        spec.filename
    )
}
const MAX_EXTRACT_BYTES: u64 = 150 * 1024 * 1024;
const MAX_EXTRACT_FILES: usize = 256;

const SUBTITLE_WIDTH: f64 = 820.0;
const SUBTITLE_HEIGHT: f64 = 128.0;
const SUBTITLE_BOTTOM_MARGIN: f64 = 72.0;

#[cfg(target_os = "windows")]
const CAPTURE_SAMPLE_RATE: usize = 16_000;
/// Minimum audio accumulated before emitting the very first partial words (~300 ms).
#[cfg(target_os = "windows")]
const MIN_STREAM_BYTES: usize = (CAPTURE_SAMPLE_RATE * 2 * 300) / 1000;
/// Snapshot interval while speech is active — updates subtitles word-by-word every 250 ms.
#[cfg(target_os = "windows")]
const STREAM_STEP_MS: u64 = 250;
/// Silence pause that finalizes the current sentence and starts a fresh rolling window.
#[cfg(target_os = "windows")]
const PHRASE_RESET_SILENCE_MS: u64 = 450;
/// Max rolling speech window (4.2 s); older 1.8 s trims smoothly during continuous speech.
#[cfg(target_os = "windows")]
const MAX_ROLLING_BYTES: usize = (CAPTURE_SAMPLE_RATE * 2 * 4200) / 1000;
#[cfg(target_os = "windows")]
const ROLLING_TRIM_BYTES: usize = (CAPTURE_SAMPLE_RATE * 2 * 1800) / 1000;
const SPEECH_RMS_THRESHOLD: f32 = 120.0;
/// Consecutive identical Whisper snapshots required before translating an unfinalized phrase.
pub const STABLE_SNAPSHOT_COUNT: usize = 2;
/// Maximum entries kept in the normalized translation cache.
pub const TRANSLATION_CACHE_MAX_ENTRIES: usize = 256;
/// Minimum spacing between remote translation calls to stay well below rate limits.
#[cfg(target_os = "windows")]
const TRANSLATION_MIN_INTERVAL_MS: u64 = 400;

static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static WHISPER_SERVER_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static TRANSLATE_HTTP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

static ENGINE_STATE: Mutex<EnginePrefs> = Mutex::new(EnginePrefs {
    source_lang: None,
    target_lang: None,
    capture_mode: None,
    online_translation: false,
    translate_provider: None,
    whisper_model: None,
    target_pid: None,
    target_app_name: None,
    last_error: None,
});

struct EnginePrefs {
    source_lang: Option<String>,
    target_lang: Option<String>,
    capture_mode: Option<String>,
    online_translation: bool,
    translate_provider: Option<String>,
    whisper_model: Option<String>,
    target_pid: Option<u32>,
    target_app_name: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateEngineStatus {
    pub installed: bool,
    pub downloading: bool,
    pub running: bool,
    pub model_size_mb: u64,
    pub source_lang: String,
    pub target_lang: String,
    pub capture_mode: String,
    pub online_translation: bool,
    pub translate_provider: String,
    pub whisper_model: String,
    pub target_pid: Option<u32>,
    pub target_app_name: Option<String>,
    pub model_path: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAppTarget {
    pub pid: u32,
    pub title: String,
    pub exe_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitlePayload {
    pub original: String,
    pub translated: String,
    pub target_lang: String,
    pub active: bool,
    /// `"ok"`, `"pending"`, `"failed"`, or `"offline"`.
    pub translation_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgressPayload {
    pub percent: u8,
    pub stage: String,
}

fn shared_http_client() -> &'static reqwest::blocking::Client {
    TRANSLATE_HTTP_CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(4)
            .tcp_nodelay(true)
            .user_agent("Nexora/0.2")
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

/// Builds a `Command` that never flashes a console window on Windows.
fn hidden_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn emit_status_changed(app: &AppHandle, status: &TranslateEngineStatus) {
    let _ = app.emit("translate-status-changed", status);
}

fn engine_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("nexora"))
        .join("live-translate")
}

fn model_path(dir: &Path, model_filename: &str) -> PathBuf {
    dir.join(whisper_model_spec(model_filename).filename)
}

fn find_whisper_binary(dir: &Path) -> Option<PathBuf> {
    let candidates = [
        dir.join("whisper-cli.exe"),
        dir.join("main.exe"),
        dir.join("Release").join("whisper-cli.exe"),
        dir.join("bin").join("whisper-cli.exe"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

#[cfg(target_os = "windows")]
fn find_whisper_server(dir: &Path) -> Option<PathBuf> {
    let candidates = [
        dir.join("whisper-server.exe"),
        dir.join("Release").join("whisper-server.exe"),
        dir.join("bin").join("whisper-server.exe"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn is_installed_in(dir: &Path, model_filename: &str) -> bool {
    model_path(dir, model_filename).exists() && find_whisper_binary(dir).is_some()
}

fn dir_size_mb(dir: &Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    total / (1024 * 1024)
}

fn build_status(app: &AppHandle) -> TranslateEngineStatus {
    let dir = engine_dir(app);
    let cfg = crate::settings::load(app);
    let prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let provider = prefs
        .translate_provider
        .clone()
        .unwrap_or_else(|| cfg.translate_provider.clone());
    let model = prefs
        .whisper_model
        .clone()
        .unwrap_or_else(|| cfg.whisper_model.clone());
    TranslateEngineStatus {
        installed: is_installed_in(&dir, &model),
        downloading: DOWNLOADING.load(Ordering::SeqCst),
        running: RUNNING.load(Ordering::SeqCst),
        model_size_mb: dir_size_mb(&dir),
        source_lang: prefs
            .source_lang
            .clone()
            .unwrap_or_else(|| cfg.translate_source_lang.clone()),
        target_lang: prefs
            .target_lang
            .clone()
            .unwrap_or_else(|| cfg.translate_target_lang.clone()),
        capture_mode: prefs
            .capture_mode
            .clone()
            .unwrap_or_else(|| "system".to_string()),
        online_translation: prefs.online_translation || provider != "none",
        translate_provider: provider,
        whisper_model: model,
        target_pid: prefs.target_pid,
        target_app_name: prefs.target_app_name.clone(),
        model_path: dir.to_string_lossy().to_string(),
        last_error: prefs.last_error.clone(),
    }
}

/// Creates (lazily) and positions the bottom-center floating subtitle overlay
/// on the primary monitor, marking it click-through so it never intercepts clicks.
pub fn position_subtitle_window(app: &AppHandle) {
    let win_opt = app.get_webview_window("subtitle").or_else(|| {
        tauri::WebviewWindowBuilder::new(
            app,
            "subtitle",
            tauri::WebviewUrl::App("index.html".into()),
        )
        .title("Nexora Subtitles")
        .inner_size(SUBTITLE_WIDTH, SUBTITLE_HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()
        .ok()
    });
    if let Some(win) = win_opt {
        let _ = win.set_ignore_cursor_events(true);
        if let Ok(Some(mon)) = win.primary_monitor() {
            let sf = mon.scale_factor().max(1.0);
            let mon_x = f64::from(mon.position().x) / sf;
            let mon_y = f64::from(mon.position().y) / sf;
            let mon_w = f64::from(mon.size().width) / sf;
            let mon_h = f64::from(mon.size().height) / sf;
            let w = SUBTITLE_WIDTH.min((mon_w - 32.0).max(360.0));
            let x = mon_x + ((mon_w - w) / 2.0).max(0.0);
            let y = mon_y + (mon_h - SUBTITLE_HEIGHT - SUBTITLE_BOTTOM_MARGIN).max(40.0);
            let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                w,
                SUBTITLE_HEIGHT,
            )));
            let _ = win.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(x, y)));
        }
    }
}

fn emit_subtitle(app: &AppHandle, payload: &SubtitlePayload) {
    let _ = app.emit("subtitle-update", payload);
}

/// Builds an in-memory 16-bit signed mono RIFF/WAVE buffer for zero-disk-I/O
/// inference against `whisper-server.exe`.
pub fn encode_pcm16_mono_wav_bytes(pcm_bytes: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_len = u32::try_from(pcm_bytes.len()).unwrap_or(u32::MAX - 36);
    let riff_size = data_len.saturating_add(36);
    let byte_rate = sample_rate.saturating_mul(2);
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&pcm_bytes[..data_len as usize]);
    out
}

/// Writes 16 kHz, 16-bit signed mono PCM samples into a standard RIFF/WAVE file.
pub fn write_pcm16_mono_wav(path: &Path, pcm_bytes: &[u8], sample_rate: u32) -> Result<(), String> {
    let wav = encode_pcm16_mono_wav_bytes(pcm_bytes, sample_rate);
    let mut file = File::create(path).map_err(|e| format!("Failed to create WAV file: {e}"))?;
    file.write_all(&wav).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())
}

/// Builds a lightweight RFC 7578 `multipart/form-data` payload for
/// `whisper-server.exe /inference`.
pub fn build_whisper_multipart_body(wav_bytes: &[u8], boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav_bytes.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"response_format\"\r\n\r\n\
             text\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"temperature\"\r\n\r\n\
             0.0\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"chunk.wav\"\r\n\
             Content-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// Parses plain-text or JSON (`{"text": "..."}`) output from `whisper-server.exe`.
pub fn parse_whisper_server_response(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(txt) = val.get("text").and_then(|v| v.as_str()) {
                return clean_whisper_output(txt);
            }
        }
    }
    clean_whisper_output(trimmed)
}

/// Computes the RMS amplitude of 16-bit little-endian signed PCM bytes.
pub fn pcm16_rms(pcm_bytes: &[u8]) -> f32 {
    let mut sum_sq: f64 = 0.0;
    let mut count: u64 = 0;
    let (chunks, _) = pcm_bytes.as_chunks::<2>();
    for chunk in chunks {
        let sample = f64::from(i16::from_le_bytes([chunk[0], chunk[1]]));
        sum_sq += sample * sample;
        count += 1;
    }
    if count == 0 {
        return 0.0;
    }
    (sum_sq / (count as f64)).sqrt() as f32
}

/// Returns `true` when 16-bit PCM audio exceeds the base silence gate RMS threshold.
pub fn has_audible_speech(pcm_bytes: &[u8]) -> bool {
    pcm16_rms(pcm_bytes) >= SPEECH_RMS_THRESHOLD
}

/// Adaptive background noise-floor VAD gate.
///
/// Tracks an exponential moving average of ambient/music RMS during non-speech
/// frames so steady game music or room hum does not continuously trigger Whisper,
/// while speech peaks above `noise_floor * multiplier` open the gate cleanly.
#[derive(Debug, Clone)]
pub struct AdaptiveVadGate {
    noise_floor: f32,
    in_speech: bool,
}

impl Default for AdaptiveVadGate {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveVadGate {
    const MIN_NOISE_FLOOR: f32 = 60.0;
    const MAX_NOISE_FLOOR: f32 = 900.0;
    const MIN_SPEECH_RMS: f32 = 140.0;
    const ENTER_MULTIPLIER: f32 = 2.4;
    const HOLD_MULTIPLIER: f32 = 1.6;
    const ALPHA: f32 = 0.08;

    pub fn new() -> Self {
        Self {
            noise_floor: 90.0,
            in_speech: false,
        }
    }

    pub fn is_voiced(&mut self, pcm_bytes: &[u8]) -> bool {
        let rms = pcm16_rms(pcm_bytes);
        let enter_thresh = (self.noise_floor * Self::ENTER_MULTIPLIER).max(Self::MIN_SPEECH_RMS);
        let hold_thresh = (self.noise_floor * Self::HOLD_MULTIPLIER).max(SPEECH_RMS_THRESHOLD);

        let voiced = if self.in_speech {
            rms >= hold_thresh
        } else {
            rms >= enter_thresh
        };

        if !voiced {
            self.noise_floor = ((1.0 - Self::ALPHA) * self.noise_floor + Self::ALPHA * rms)
                .clamp(Self::MIN_NOISE_FLOOR, Self::MAX_NOISE_FLOOR);
        }
        self.in_speech = voiced;
        voiced
    }

    pub fn reset(&mut self) {
        self.in_speech = false;
    }

    pub fn noise_floor(&self) -> f32 {
        self.noise_floor
    }
}

/// Extensible list of known Whisper silence/music hallucinations (normalized lowercase,
/// outer punctuation stripped).
pub const WHISPER_HALLUCINATIONS: &[&str] = &[
    "thank you",
    "thanks for watching",
    "thank you for watching",
    "please subscribe",
    "subscribe",
    "like and subscribe",
    "see you next time",
    "bye",
    "you",
    "mbc",
    "subtitles by",
    "translated by",
    "amara.org",
    "ممنون",
    "سپاسگزارم",
    "با تشکر",
    "زیرنویس",
];

/// Returns `true` when Whisper loops on the same short word or 2-3 word phrase
/// 4 or more consecutive times (a classic silence/music hallucination pattern).
pub fn has_excessive_repetition(text: &str) -> bool {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() < 4 {
        return false;
    }
    for n in 1..=3 {
        if words.len() < n * 4 {
            continue;
        }
        for start in 0..=(words.len() - n * 4) {
            let pat = &words[start..start + n];
            let mut repeats = 1usize;
            let mut pos = start + n;
            while pos + n <= words.len() && &words[pos..pos + n] == pat {
                repeats += 1;
                pos += n;
            }
            if repeats >= 4 {
                return true;
            }
        }
    }
    false
}

/// Strips Whisper silence hallucinations, bracketed non-speech markers,
/// and infinite phrase-repetition loops.
pub fn clean_whisper_output(raw: &str) -> String {
    let joined = raw
        .lines()
        .map(str::trim)
        .filter(|l| {
            if l.is_empty()
                || l.starts_with('[')
                || (l.starts_with('(') && l.ends_with(')'))
                || (l.starts_with('♪') || l.ends_with('♪'))
            {
                return false;
            }
            let norm = l
                .trim_matches(|c: char| {
                    matches!(
                        c,
                        '.' | ',' | '!' | '?' | ';' | ':' | '"' | '\'' | '…' | '-'
                    )
                })
                .trim()
                .to_lowercase();
            if norm.is_empty() {
                return false;
            }
            !WHISPER_HALLUCINATIONS.iter().any(|h| {
                norm == *h || norm.starts_with("subtitles by") || norm.starts_with("translated by")
            })
        })
        .collect::<Vec<_>>()
        .join(" ");

    if has_excessive_repetition(&joined) {
        String::new()
    } else {
        joined
    }
}

/// Normalizes text for cache and stability comparisons (collapses whitespace,
/// trims outer punctuation, and lowercases).
pub fn normalize_cache_text(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | '"' | '\''))
        .to_ascii_lowercase()
}

/// Decides when a streaming partial transcript is stable enough to translate.
/// Live `original` subtitles are always shown immediately, while remote
/// translation only fires when a sentence is finalized OR remains identical
/// across `STABLE_SNAPSHOT_COUNT` consecutive snapshots.
#[derive(Debug, Default)]
pub struct SubtitleStabilityTracker {
    last_seen_norm: String,
    consecutive_hits: usize,
    last_translated_norm: String,
}

impl SubtitleStabilityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, text: &str, finalized: bool) -> bool {
        let norm = normalize_cache_text(text);
        if norm.is_empty() {
            return false;
        }
        if norm == self.last_translated_norm {
            if finalized {
                self.reset_phrase();
            }
            return false;
        }
        if finalized {
            self.last_translated_norm = norm;
            self.last_seen_norm.clear();
            self.consecutive_hits = 0;
            return true;
        }
        if norm == self.last_seen_norm {
            self.consecutive_hits += 1;
        } else {
            self.last_seen_norm = norm.clone();
            self.consecutive_hits = 1;
        }
        if self.consecutive_hits >= STABLE_SNAPSHOT_COUNT {
            self.last_translated_norm = norm;
            true
        } else {
            false
        }
    }

    pub fn reset_phrase(&mut self) {
        self.last_seen_norm.clear();
        self.consecutive_hits = 0;
    }
}

/// Bounded FIFO translation cache keyed by `(normalized_text, provider, target_lang)`.
pub struct TranslationCache {
    max_entries: usize,
    map: std::collections::HashMap<
        (
            String,
            crate::translate_provider::TranslateProviderId,
            String,
        ),
        String,
    >,
    order: std::collections::VecDeque<(
        String,
        crate::translate_provider::TranslateProviderId,
        String,
    )>,
}

impl TranslationCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            map: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
        }
    }

    pub fn get(
        &self,
        text: &str,
        provider: crate::translate_provider::TranslateProviderId,
        target_lang: &str,
    ) -> Option<String> {
        let key = (
            normalize_cache_text(text),
            provider,
            target_lang.to_ascii_lowercase(),
        );
        self.map.get(&key).cloned()
    }

    pub fn insert(
        &mut self,
        text: &str,
        provider: crate::translate_provider::TranslateProviderId,
        target_lang: &str,
        translated: String,
    ) {
        let norm = normalize_cache_text(text);
        if norm.is_empty() {
            return;
        }
        let key = (norm, provider, target_lang.to_ascii_lowercase());
        if !self.map.contains_key(&key) {
            if self.order.len() >= self.max_entries {
                if let Some(oldest) = self.order.pop_front() {
                    self.map.remove(&oldest);
                }
            }
            self.order.push_back(key.clone());
        }
        self.map.insert(key, translated);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Translates `text` using the configured official provider (`GoogleCloud`,
/// `Gemini`, or `Local`). Returns `Ok(text)` unchanged when `provider == None`.
pub fn translate_with_provider(
    provider_id: crate::translate_provider::TranslateProviderId,
    api_key: Option<&str>,
    text: &str,
    source_lang: &str,
    target_lang: &str,
    context: &[String],
) -> Result<String, crate::translate_provider::TranslateError> {
    use crate::translate_provider::{
        translate_with_retry_and_fallback, GeminiTranslateProvider, GoogleCloudTranslateProvider,
        LocalTranslateProvider, TranslateError, TranslateProviderId, TranslateRequest,
    };
    let clean = text.trim();
    if clean.is_empty() {
        return Err(TranslateError::EmptyResponse);
    }
    let req = TranslateRequest {
        text: clean,
        source_lang: if source_lang.is_empty() {
            "auto"
        } else {
            source_lang
        },
        target_lang: if target_lang.is_empty() {
            "fa"
        } else {
            target_lang
        },
        context,
    };
    match provider_id {
        TranslateProviderId::None => Ok(clean.to_string()),
        TranslateProviderId::GoogleCloud => {
            let key = api_key
                .filter(|k| !k.trim().is_empty())
                .ok_or(TranslateError::MissingApiKey)?;
            let provider = GoogleCloudTranslateProvider::new(key.to_string());
            translate_with_retry_and_fallback(&provider, None, &req)
        }
        TranslateProviderId::Gemini => {
            let key = api_key
                .filter(|k| !k.trim().is_empty())
                .ok_or(TranslateError::MissingApiKey)?;
            let provider = GeminiTranslateProvider::new(key.to_string());
            translate_with_retry_and_fallback(&provider, None, &req)
        }
        TranslateProviderId::Local => {
            let provider = LocalTranslateProvider;
            translate_with_retry_and_fallback(&provider, None, &req)
        }
    }
}

/// Streams an HTTP file to disk natively via `reqwest`, emitting smooth progress percentages.
#[allow(clippy::too_many_arguments)]
fn download_file_streaming_event(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    start_pct: u8,
    end_pct: u8,
    stage: &str,
    event_name: &str,
    app: &AppHandle,
) -> Result<(), String> {
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| format!("Network error downloading {stage}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} downloading {stage}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut file = File::create(dest).map_err(|e| format!("Failed to create file: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_emitted = start_pct;

    let _ = app.emit(
        event_name,
        DownloadProgressPayload {
            percent: start_pct,
            stage: stage.to_string(),
        },
    );

    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("Read error during {stage}: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Write error during {stage}: {e}"))?;
        downloaded = downloaded.saturating_add(n as u64);

        let span = u64::from(end_pct.saturating_sub(start_pct));
        if let Some(delta) = (downloaded * span).checked_div(total) {
            let pct = start_pct.saturating_add(delta as u8);
            if pct > last_emitted {
                last_emitted = pct;
                let _ = app.emit(
                    event_name,
                    DownloadProgressPayload {
                        percent: pct.min(end_pct),
                        stage: stage.to_string(),
                    },
                );
            }
        }
    }
    file.flush().map_err(|e| format!("Flush error: {e}"))?;
    Ok(())
}

fn download_file_streaming(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    start_pct: u8,
    end_pct: u8,
    stage: &str,
    app: &AppHandle,
) -> Result<(), String> {
    download_file_streaming_event(
        client,
        url,
        dest,
        start_pct,
        end_pct,
        stage,
        "translate-download-progress",
        app,
    )
}

/// Computes lowercase hex SHA-256 of a file on disk.
pub fn file_sha256_hex(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = File::open(path).map_err(|e| format!("Failed to open file for SHA-256: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed reading file for SHA-256: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    Ok(hex)
}

/// Fail-closed SHA-256 verifier: deletes `path` on mismatch or unconfigured TODO digest.
pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> Result<(), String> {
    let expected = expected_hex.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        let _ = fs::remove_file(path);
        return Err(format!(
            "SHA-256 digest is not configured ({expected_hex}); refusing unverified file"
        ));
    }
    let actual = file_sha256_hex(path)?;
    if actual != expected {
        let _ = fs::remove_file(path);
        return Err(format!(
            "SHA-256 mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

/// Removes any leftover `.part` temporary download files inside `dir`.
pub fn cleanup_partial_downloads(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("part") {
                let _ = fs::remove_file(p);
            }
        }
    }
}

/// Extracts a ZIP archive natively in Rust with zip-slip (`enclosed_name`) and
/// ZIP-bomb limits (`MAX_EXTRACT_BYTES` and `MAX_EXTRACT_FILES`).
pub fn extract_zip_native(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    extract_zip_with_limits(zip_path, dest_dir, MAX_EXTRACT_BYTES, MAX_EXTRACT_FILES)
}

pub fn extract_zip_with_limits(
    zip_path: &Path,
    dest_dir: &Path,
    max_bytes: u64,
    max_files: usize,
) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| format!("Failed to open archive: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP archive: {e}"))?;
    if archive.len() > max_files {
        return Err(format!(
            "ZIP archive exceeds max file count ({} > {max_files})",
            archive.len()
        ));
    }
    let mut total_written: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed reading archive entry: {e}"))?;
        let Some(rel_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest_dir.join(rel_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("Failed to create dir: {e}"))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {e}"))?;
            }
            let remaining = max_bytes.saturating_sub(total_written);
            let mut limited = entry.take(remaining.saturating_add(1));
            let mut out_file = File::create(&out_path)
                .map_err(|e| format!("Failed to write extracted file: {e}"))?;
            let copied = std::io::copy(&mut limited, &mut out_file)
                .map_err(|e| format!("Failed extracting file: {e}"))?;
            total_written = total_written.saturating_add(copied);
            if total_written > max_bytes {
                let _ = fs::remove_file(&out_path);
                return Err(format!(
                    "ZIP extraction exceeded safety limit ({max_bytes} bytes)"
                ));
            }
        }
    }
    Ok(())
}

/// Lists currently open top-level application windows that can be targeted for
/// per-application WASAPI process-tree audio loopback.
#[tauri::command]
pub fn list_audio_sources() -> Vec<AudioAppTarget> {
    #[cfg(target_os = "windows")]
    {
        list_windows_audio_targets()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "windows")]
fn list_windows_audio_targets() -> Vec<AudioAppTarget> {
    use std::collections::HashMap;
    use sysinfo::{Pid, System};
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
                return BOOL(1);
            }
            let mut buf = [0u16; 256];
            let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
            if len <= 0 {
                return BOOL(1);
            }
            let title = String::from_utf16_lossy(&buf[..len as usize])
                .trim()
                .to_string();
            if title.is_empty() || title == "Program Manager" {
                return BOOL(1);
            }
            let mut pid: u32 = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
            }
            if pid != 0 {
                let list = unsafe { &mut *(lparam.0 as *mut Vec<(u32, String)>) };
                list.push((pid, title));
            }
            BOOL(1)
        }))
        .unwrap_or(BOOL(0))
    }

    let mut raw_windows: Vec<(u32, String)> = Vec::new();
    let lparam = LPARAM((&mut raw_windows as *mut Vec<(u32, String)>) as isize);
    let _ = unsafe { EnumWindows(Some(enum_proc), lparam) };

    let self_pid = std::process::id();
    let sys = System::new_all();
    let mut by_root_pid: HashMap<u32, AudioAppTarget> = HashMap::new();

    for (win_pid, title) in raw_windows {
        if win_pid == self_pid {
            continue;
        }
        let mut root_pid = win_pid;
        let exe_name = if let Some(proc_info) = sys.process(Pid::from_u32(win_pid)) {
            let name = proc_info.name().to_string_lossy().to_string();
            let mut cur = proc_info;
            for _ in 0..6 {
                if let Some(parent_pid) = cur.parent() {
                    if let Some(parent) = sys.process(parent_pid) {
                        if parent.name().eq_ignore_ascii_case(cur.name()) {
                            root_pid = parent_pid.as_u32();
                            cur = parent;
                            continue;
                        }
                    }
                }
                break;
            }
            name
        } else {
            "App".to_string()
        };

        if root_pid == self_pid
            || exe_name.eq_ignore_ascii_case("nexora.exe")
            || exe_name.eq_ignore_ascii_case("TextInputHost.exe")
            || exe_name.eq_ignore_ascii_case("ApplicationFrameHost.exe")
        {
            continue;
        }

        let short_title: String = title.chars().take(64).collect();
        by_root_pid.entry(root_pid).or_insert(AudioAppTarget {
            pid: root_pid,
            title: short_title,
            exe_name,
        });
    }

    let mut out: Vec<AudioAppTarget> = by_root_pid.into_values().collect();
    out.sort_by(|a, b| {
        a.exe_name
            .to_lowercase()
            .cmp(&b.exe_name.to_lowercase())
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    out
}

#[cfg(target_os = "windows")]
struct ResidentWhisperServer {
    child: std::process::Child,
    endpoint: String,
}

#[cfg(target_os = "windows")]
impl ResidentWhisperServer {
    fn start(server_bin: &Path, model: &Path, source_lang: &str, threads: usize) -> Option<Self> {
        // Retry up to 3 times with fresh ephemeral ports to survive TOCTOU port collisions.
        // Note: whisper-server.exe (b5907) has no built-in auth/token flag; it binds to 127.0.0.1 only.
        for _attempt in 0..3 {
            let Ok(listener) = std::net::TcpListener::bind("127.0.0.1:0") else {
                continue;
            };
            let Ok(addr) = listener.local_addr() else {
                continue;
            };
            let port = addr.port();
            drop(listener);

            let mut args = vec![
                "-m".to_string(),
                model.to_string_lossy().to_string(),
                "--host".to_string(),
                "127.0.0.1".to_string(),
                "--port".to_string(),
                port.to_string(),
                "-t".to_string(),
                threads.to_string(),
                "-bo".to_string(),
                "1".to_string(),
                "-bs".to_string(),
                "1".to_string(),
                "-ac".to_string(),
                "256".to_string(),
                "-nt".to_string(),
                "-nf".to_string(),
                "-sns".to_string(),
            ];
            if !source_lang.is_empty() {
                args.push("-l".to_string());
                args.push(source_lang.to_string());
            }

            let Ok(mut child) = hidden_command(server_bin)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            else {
                continue;
            };

            // Wait up to 4 seconds for whisper-server.exe to finish loading ggml-tiny.bin into RAM.
            for _ in 0..40 {
                if !RUNNING.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                    WHISPER_SERVER_PID.store(child.id(), Ordering::SeqCst);
                    return Some(Self {
                        child,
                        endpoint: format!("http://127.0.0.1:{port}/inference"),
                    });
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            let _ = child.kill();
            let _ = child.wait();
        }
        None
    }

    fn transcribe_pcm(&self, pcm_bytes: &[u8]) -> Option<String> {
        let wav_bytes = encode_pcm16_mono_wav_bytes(pcm_bytes, CAPTURE_SAMPLE_RATE as u32);
        let boundary = "----NexoraWhisperBoundary9f8e7d";
        let body = build_whisper_multipart_body(&wav_bytes, boundary);
        let resp = shared_http_client()
            .post(&self.endpoint)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let raw = resp.text().ok()?;
        Some(parse_whisper_server_response(&raw))
    }
}

#[cfg(target_os = "windows")]
impl Drop for ResidentWhisperServer {
    fn drop(&mut self) {
        WHISPER_SERVER_PID.store(0, Ordering::SeqCst);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "windows")]
fn transcribe_cli_fallback(
    bin: &Path,
    model: &Path,
    wav_path: &Path,
    pcm_bytes: &[u8],
    source_lang: &str,
    threads: usize,
) -> Option<String> {
    write_pcm16_mono_wav(wav_path, pcm_bytes, CAPTURE_SAMPLE_RATE as u32).ok()?;
    let mut args = vec![
        "-m".to_string(),
        model.to_string_lossy().to_string(),
        "-f".to_string(),
        wav_path.to_string_lossy().to_string(),
        "-t".to_string(),
        threads.to_string(),
        "-bo".to_string(),
        "1".to_string(),
        "-bs".to_string(),
        "1".to_string(),
        "-ac".to_string(),
        "256".to_string(),
        "-nt".to_string(),
        "-nf".to_string(),
        "-sns".to_string(),
        "-np".to_string(),
    ];
    if source_lang != "auto" && !source_lang.is_empty() {
        args.push("-l".to_string());
        args.push(source_lang.to_string());
    }
    let out = hidden_command(bin).args(&args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(clean_whisper_output(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(target_os = "windows")]
#[allow(clippy::too_many_arguments)]
fn run_wasapi_loopback_worker(
    app: &AppHandle,
    dir: &Path,
    cli_bin: &Path,
    model: &Path,
    wav_path: &Path,
    source_lang: &str,
    target_lang: &str,
    capture_mode: &str,
    provider_id: crate::translate_provider::TranslateProviderId,
    target_pid: Option<u32>,
    snapshot_interval_ms: u64,
) -> Result<(), String> {
    use std::collections::VecDeque;
    use std::time::Instant;
    use wasapi::{
        initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode,
        WaveFormat,
    };

    let _ = initialize_mta();

    let desired_format = WaveFormat::new(16, 16, SampleType::Int, CAPTURE_SAMPLE_RATE, 1, None);
    let audio_client = if capture_mode == "app" {
        let pid = target_pid.ok_or_else(|| "No target application selected".to_string())?;
        let mut client = AudioClient::new_application_loopback_client(pid, true)
            .map_err(|e| format!("Failed to attach audio loopback to PID {pid}: {e}"))?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        };
        client
            .initialize_client(&desired_format, Direction::Capture, mode, None)
            .map_err(|e| format!("Failed to initialize app loopback stream: {e}"))?;
        client
    } else {
        let enumerator =
            DeviceEnumerator::new().map_err(|e| format!("WASAPI enumerator error: {e}"))?;
        let device = enumerator
            .get_default_device(Direction::Render)
            .map_err(|e| format!("No default playback device found: {e}"))?;
        let mut client = device
            .get_iaudioclient()
            .map_err(|e| format!("Failed to open playback audio client: {e}"))?;
        let (_def_time, min_time) = client.get_device_period().unwrap_or((100_000, 100_000));
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_time,
        };
        // Capturing from a Render endpoint enables WASAPI system loopback (no mic).
        client
            .initialize_client(&desired_format, Direction::Capture, mode, None)
            .map_err(|e| format!("Failed to initialize system loopback stream: {e}"))?;
        client
    };

    let h_event = audio_client
        .set_get_eventhandle()
        .map_err(|e| format!("Failed to bind WASAPI event handle: {e}"))?;
    let capture_client = audio_client
        .get_audiocaptureclient()
        .map_err(|e| format!("Failed to open WASAPI capture client: {e}"))?;

    audio_client
        .start_stream()
        .map_err(|e| format!("Failed to start WASAPI loopback stream: {e}"))?;

    // Three-stage latest-wins streaming pipeline:
    // 1. Producer: captures WASAPI loopback PCM, tags each snapshot with `finalized: bool`.
    // 2. Whisper worker: transcribes audio, emits `original` immediately, and forwards
    //    only stabilized or finalized phrases to the translator with a monotonic generation ID.
    // 3. Translator worker: checks cache, enforces latest-wins generation cancellation,
    //    passes 2-3 prior sentences as LLM context, and emits explicit `translation_status`.
    let audio_slot = std::sync::Arc::new((
        Mutex::new((None::<(Vec<u8>, bool)>, false)),
        std::sync::Condvar::new(),
    ));
    let text_slot = std::sync::Arc::new((
        Mutex::new((None::<(String, bool, u64)>, false)),
        std::sync::Condvar::new(),
    ));
    let latest_gen = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    let text_slot_rx = std::sync::Arc::clone(&text_slot);
    let latest_gen_tr = std::sync::Arc::clone(&latest_gen);
    let app_translator = app.clone();
    let src_for_tr = source_lang.to_string();
    let tgt_for_tr = target_lang.to_string();
    let translator_handle = std::thread::spawn(move || {
        let mut cache = TranslationCache::new(TRANSLATION_CACHE_MAX_ENTRIES);
        let mut rolling_context: VecDeque<String> = VecDeque::new();
        let mut last_remote_call = Instant::now()
            .checked_sub(Duration::from_secs(2))
            .unwrap_or_else(Instant::now);
        let (lock, cvar) = &*text_slot_rx;
        loop {
            let item = {
                let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                while guard.0.is_none() && !guard.1 && RUNNING.load(Ordering::SeqCst) {
                    guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
                }
                if guard.1 || !RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                guard.0.take()
            };
            let Some((text, finalized, gen_id)) = item else {
                continue;
            };
            // Latest-wins: drop stale work before making an HTTP request.
            if gen_id < latest_gen_tr.load(Ordering::SeqCst) {
                continue;
            }
            if provider_id == crate::translate_provider::TranslateProviderId::None {
                continue;
            }

            let outcome: Result<String, crate::translate_provider::TranslateError> =
                if let Some(cached) = cache.get(&text, provider_id, &tgt_for_tr) {
                    Ok(cached)
                } else {
                    let since = last_remote_call.elapsed();
                    if since < Duration::from_millis(TRANSLATION_MIN_INTERVAL_MS) {
                        std::thread::sleep(
                            Duration::from_millis(TRANSLATION_MIN_INTERVAL_MS) - since,
                        );
                    }
                    if !RUNNING.load(Ordering::SeqCst) {
                        break;
                    }
                    if gen_id < latest_gen_tr.load(Ordering::SeqCst) {
                        continue;
                    }
                    last_remote_call = Instant::now();
                    let api_key = if provider_id.requires_api_key() {
                        crate::translate_secrets::get_api_key(provider_id)
                    } else {
                        None
                    };
                    let ctx_vec: Vec<String> = rolling_context.iter().cloned().collect();
                    let res = translate_with_provider(
                        provider_id,
                        api_key.as_deref(),
                        &text,
                        &src_for_tr,
                        &tgt_for_tr,
                        &ctx_vec,
                    );
                    if let Ok(ref translated) = res {
                        cache.insert(&text, provider_id, &tgt_for_tr, translated.clone());
                    }
                    res
                };

            // Latest-wins: if a newer subtitle superseded this one while in flight, drop it.
            if !RUNNING.load(Ordering::SeqCst) || gen_id < latest_gen_tr.load(Ordering::SeqCst) {
                continue;
            }

            match outcome {
                Ok(translated) => {
                    if finalized {
                        if rolling_context.len()
                            >= crate::translate_provider::MAX_LLM_CONTEXT_SENTENCES
                        {
                            rolling_context.pop_front();
                        }
                        rolling_context.push_back(text.clone());
                    }
                    emit_subtitle(
                        &app_translator,
                        &SubtitlePayload {
                            original: text,
                            translated,
                            target_lang: tgt_for_tr.clone(),
                            active: true,
                            translation_status: "ok".to_string(),
                        },
                    );
                }
                Err(err) => {
                    emit_subtitle(
                        &app_translator,
                        &SubtitlePayload {
                            original: text,
                            translated: String::new(),
                            target_lang: tgt_for_tr.clone(),
                            active: true,
                            translation_status: err.to_subtitle_status().to_string(),
                        },
                    );
                }
            }
        }
    });

    let audio_slot_rx = std::sync::Arc::clone(&audio_slot);
    let text_slot_tx = std::sync::Arc::clone(&text_slot);
    let latest_gen_whisper = std::sync::Arc::clone(&latest_gen);
    let app_whisper = app.clone();
    let tgt_for_whisper = target_lang.to_string();
    let server_bin_opt = find_whisper_server(dir);
    let cli_bin_owned = cli_bin.to_path_buf();
    let model_owned = model.to_path_buf();
    let wav_owned = wav_path.to_path_buf();
    let src_owned = source_lang.to_string();

    let whisper_handle = std::thread::spawn(move || {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().clamp(2, 6))
            .unwrap_or(4);
        let server = server_bin_opt
            .as_deref()
            .and_then(|sb| ResidentWhisperServer::start(sb, &model_owned, &src_owned, threads));
        let mut stability = SubtitleStabilityTracker::new();

        let (lock, cvar) = &*audio_slot_rx;
        loop {
            let item = {
                let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                while guard.0.is_none() && !guard.1 && RUNNING.load(Ordering::SeqCst) {
                    guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
                }
                if guard.1 || !RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                guard.0.take()
            };
            let Some((pcm_bytes, finalized)) = item else {
                continue;
            };
            let text_opt = server
                .as_ref()
                .and_then(|srv| srv.transcribe_pcm(&pcm_bytes))
                .or_else(|| {
                    transcribe_cli_fallback(
                        &cli_bin_owned,
                        &model_owned,
                        &wav_owned,
                        &pcm_bytes,
                        &src_owned,
                        threads,
                    )
                });

            if let Some(text) = text_opt {
                if !text.is_empty() && RUNNING.load(Ordering::SeqCst) {
                    let gen_id = latest_gen_whisper.fetch_add(1, Ordering::SeqCst) + 1;
                    // Always display partial `original` immediately.
                    if provider_id == crate::translate_provider::TranslateProviderId::None {
                        emit_subtitle(
                            &app_whisper,
                            &SubtitlePayload {
                                original: String::new(),
                                translated: text,
                                target_lang: tgt_for_whisper.clone(),
                                active: true,
                                translation_status: "ok".to_string(),
                            },
                        );
                    } else {
                        emit_subtitle(
                            &app_whisper,
                            &SubtitlePayload {
                                original: text.clone(),
                                translated: String::new(),
                                target_lang: tgt_for_whisper.clone(),
                                active: true,
                                translation_status: "pending".to_string(),
                            },
                        );
                        if stability.observe(&text, finalized) {
                            let (tlock, tcvar) = &*text_slot_tx;
                            let mut tg = tlock.lock().unwrap_or_else(|e| e.into_inner());
                            tg.0 = Some((text, finalized, gen_id));
                            tcvar.notify_one();
                        }
                    }
                }
            }
        }
        let (tlock, tcvar) = &*text_slot_tx;
        let mut tg = tlock.lock().unwrap_or_else(|e| e.into_inner());
        tg.1 = true;
        tcvar.notify_all();
    });

    // Rolling-window producer: snapshots the growing phrase every 250 ms so words
    // stream live on screen as they are spoken — never waiting for sentence end.
    let mut rolling_pcm: VecDeque<u8> = VecDeque::with_capacity(MAX_ROLLING_BYTES + 16_384);
    let mut packet_buf: VecDeque<u8> = VecDeque::with_capacity(8_192);
    let mut vad = AdaptiveVadGate::new();
    let mut in_speech = false;
    let mut last_voice_at: Option<Instant> = None;
    let mut last_snapshot_at = Instant::now();
    let mut dirty_since_snapshot = false;
    let step_ms = snapshot_interval_ms.max(STREAM_STEP_MS);

    while RUNNING.load(Ordering::SeqCst) {
        let _ = h_event.wait_for_event(30);

        loop {
            match capture_client.get_next_packet_size() {
                Ok(Some(frames)) if frames > 0 => {
                    packet_buf.clear();
                    if capture_client
                        .read_from_device_to_deque(&mut packet_buf)
                        .is_err()
                    {
                        break;
                    }
                    if packet_buf.is_empty() {
                        break;
                    }
                    let pkt_slice: Vec<u8> = packet_buf.drain(..).collect();
                    let voiced = vad.is_voiced(&pkt_slice);
                    let now = Instant::now();
                    if voiced {
                        in_speech = true;
                        last_voice_at = Some(now);
                        rolling_pcm.extend(pkt_slice);
                        dirty_since_snapshot = true;
                    } else if in_speech {
                        rolling_pcm.extend(pkt_slice);
                        dirty_since_snapshot = true;
                    }
                }
                _ => break,
            }
        }

        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        if rolling_pcm.len() > MAX_ROLLING_BYTES {
            let drain_n = ROLLING_TRIM_BYTES.min(rolling_pcm.len());
            rolling_pcm.drain(..drain_n);
        }

        if in_speech {
            let now = Instant::now();
            let silence_ms = last_voice_at
                .map(|t| now.duration_since(t).as_millis() as u64)
                .unwrap_or(0);
            let step_ready = dirty_since_snapshot
                && rolling_pcm.len() >= MIN_STREAM_BYTES
                && now.duration_since(last_snapshot_at).as_millis() as u64 >= step_ms;
            let finalize_ready = silence_ms >= PHRASE_RESET_SILENCE_MS;

            if step_ready || finalize_ready {
                if rolling_pcm.len() >= MIN_STREAM_BYTES {
                    let snapshot: Vec<u8> = rolling_pcm.iter().copied().collect();
                    if has_audible_speech(&snapshot) {
                        let (alock, acvar) = &*audio_slot;
                        let mut ag = alock.lock().unwrap_or_else(|e| e.into_inner());
                        ag.0 = Some((snapshot, finalize_ready));
                        acvar.notify_one();
                        last_snapshot_at = now;
                        dirty_since_snapshot = false;
                    }
                }
                if finalize_ready {
                    rolling_pcm.clear();
                    vad.reset();
                    in_speech = false;
                    last_voice_at = None;
                    dirty_since_snapshot = false;
                }
            }
        }
    }

    {
        let (alock, acvar) = &*audio_slot;
        let mut ag = alock.lock().unwrap_or_else(|e| e.into_inner());
        ag.1 = true;
        acvar.notify_all();
    }
    let _ = audio_client.stop_stream();
    let _ = whisper_handle.join();
    let _ = translator_handle.join();
    Ok(())
}

#[tauri::command]
pub fn get_translate_status(app: AppHandle) -> TranslateEngineStatus {
    build_status(&app)
}

#[tauri::command]
pub async fn download_translate_engine(
    window: tauri::Window,
    app: AppHandle,
    whisper_model: Option<String>,
) -> Result<TranslateEngineStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    if DOWNLOADING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(build_status(&app));
    }

    let selected_model_name =
        whisper_model.unwrap_or_else(|| crate::settings::load(&app).whisper_model);
    let spec = whisper_model_spec(&selected_model_name);
    {
        let mut prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
        prefs.whisper_model = Some(spec.filename.to_string());
    }

    let app_clone = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let dir = engine_dir(&app_clone);
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create engine dir: {e}"))?;

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .user_agent("Nexora/0.2")
            .build()
            .map_err(|e| format!("Failed to initialize HTTP client: {e}"))?;

        cleanup_partial_downloads(&dir);

        if find_whisper_binary(&dir).is_none() {
            let zip_part = dir.join("whisper-bin-x64.zip.part");
            let zip_path = dir.join("whisper-bin-x64.zip");
            download_file_streaming(
                &client,
                WHISPER_ZIP_URL,
                &zip_part,
                5,
                35,
                "Downloading Whisper runtime...",
                &app_clone,
            )?;
            verify_file_sha256(&zip_part, EXPECTED_WHISPER_ZIP_SHA256)?;
            fs::rename(&zip_part, &zip_path)
                .map_err(|e| format!("Failed finalizing Whisper archive: {e}"))?;

            let _ = app_clone.emit(
                "translate-download-progress",
                DownloadProgressPayload {
                    percent: 40,
                    stage: "Extracting Whisper runtime...".to_string(),
                },
            );

            extract_zip_native(&zip_path, &dir)?;
            let _ = fs::remove_file(&zip_path);
        }

        let model_part = dir.join(format!("{}.part", spec.filename));
        let model_file = model_path(&dir, spec.filename);
        let url = whisper_model_url(spec);
        let stage_msg = format!("Downloading {} (~{} MB)...", spec.filename, spec.size_mb);
        download_file_streaming(&client, &url, &model_part, 45, 98, &stage_msg, &app_clone)?;
        verify_file_sha256(&model_part, spec.sha256)?;
        fs::rename(&model_part, &model_file)
            .map_err(|e| format!("Failed finalizing speech model: {e}"))?;

        let _ = app_clone.emit(
            "translate-download-progress",
            DownloadProgressPayload {
                percent: 100,
                stage: "Engine ready".to_string(),
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;

    DOWNLOADING.store(false, Ordering::SeqCst);
    {
        let mut prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
        prefs.last_error = result.as_ref().err().cloned();
    }
    if let Err(ref err) = result {
        crate::diagnostics::log("translator", &format!("download failed: {err}"));
    }
    result?;
    let st = build_status(&app);
    emit_status_changed(&app, &st);
    Ok(st)
}

#[tauri::command]
pub fn remove_translate_engine(
    window: tauri::Window,
    app: AppHandle,
) -> Result<TranslateEngineStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    RUNNING.store(false, Ordering::SeqCst);
    if let Some(win) = app.get_webview_window("subtitle") {
        let _ = win.hide();
    }
    let dir = engine_dir(&app);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("Failed to remove engine files: {e}"))?;
    }
    {
        let mut prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
        prefs.last_error = None;
    }
    let st = build_status(&app);
    emit_status_changed(&app, &st);
    Ok(st)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_live_translate(
    window: tauri::Window,
    app: AppHandle,
    source_lang: String,
    target_lang: String,
    capture_mode: Option<String>,
    online_translation: Option<bool>,
    translate_provider: Option<String>,
    target_pid: Option<u32>,
    target_app_name: Option<String>,
) -> Result<TranslateEngineStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    let loaded_settings = crate::settings::load(&app);
    let resolved_provider_id = match translate_provider {
        Some(ref raw) => crate::translate_provider::TranslateProviderId::parse_id(raw),
        None => {
            let from_settings = crate::translate_provider::TranslateProviderId::parse_id(
                &loaded_settings.translate_provider,
            );
            if online_translation == Some(false) {
                crate::translate_provider::TranslateProviderId::None
            } else {
                from_settings
            }
        }
    };
    let allow_online = resolved_provider_id.is_cloud();
    let resolved_mode = match capture_mode.as_deref() {
        Some("app") => "app".to_string(),
        _ => "system".to_string(),
    };

    if resolved_mode == "app" && target_pid.is_none() {
        let err_msg = if target_lang == "fa" {
            "ابتدا یک برنامه یا پنجره هدف را از تنظیمات موتور انتخاب کنید.".to_string()
        } else {
            "Please select a target window or application first.".to_string()
        };
        {
            let mut prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
            prefs.last_error = Some(err_msg.clone());
        }
        let st = build_status(&app);
        emit_status_changed(&app, &st);
        return Err(err_msg);
    }

    {
        let mut prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
        prefs.source_lang = Some(source_lang.clone());
        prefs.target_lang = Some(target_lang.clone());
        prefs.capture_mode = Some(resolved_mode.clone());
        prefs.online_translation = allow_online;
        prefs.translate_provider = Some(resolved_provider_id.as_str().to_string());
        prefs.target_pid = target_pid;
        prefs.target_app_name = target_app_name.clone();
        prefs.last_error = None;
    }

    position_subtitle_window(&app);
    if let Some(win) = app.get_webview_window("subtitle") {
        let _ = win.show();
    }

    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(build_status(&app));
    }

    let dir = engine_dir(&app);
    let model_spec = whisper_model_spec(&loaded_settings.whisper_model);
    let model = model_path(&dir, model_spec.filename);
    let whisper_bin = find_whisper_binary(&dir);
    let effective_source_lang = if model_spec.english_only {
        "en".to_string()
    } else {
        source_lang.clone()
    };

    let listening_banner = if resolved_mode == "app" {
        let label = target_app_name.as_deref().unwrap_or("App");
        if target_lang == "fa" {
            format!("در حال دریافت صدای «{label}» (بدون میکروفون)...")
        } else {
            format!("Listening to \"{label}\" audio (no mic)...")
        }
    } else if target_lang == "fa" {
        "در حال دریافت صدای کل سیستم (بدون میکروفون)...".to_string()
    } else {
        "Listening to system audio (no mic)...".to_string()
    };

    emit_subtitle(
        &app,
        &SubtitlePayload {
            original: String::new(),
            translated: listening_banner,
            target_lang: target_lang.clone(),
            active: true,
            translation_status: "ok".to_string(),
        },
    );

    if let Some(bin) = whisper_bin {
        let app_worker = app.clone();
        std::thread::spawn(move || {
            let wav_path = dir.join("chunk.wav");
            #[cfg(target_os = "windows")]
            {
                if let Err(err) = run_wasapi_loopback_worker(
                    &app_worker,
                    &dir,
                    &bin,
                    &model,
                    &wav_path,
                    &effective_source_lang,
                    &target_lang,
                    &resolved_mode,
                    resolved_provider_id,
                    target_pid,
                    model_spec.snapshot_interval_ms,
                ) {
                    crate::diagnostics::log("translator", &format!("loopback failed: {err}"));
                    RUNNING.store(false, Ordering::SeqCst);
                    {
                        let mut prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
                        prefs.last_error = Some(err.clone());
                    }
                    emit_subtitle(
                        &app_worker,
                        &SubtitlePayload {
                            original: String::new(),
                            translated: err,
                            target_lang: target_lang.clone(),
                            active: true,
                            translation_status: "failed".to_string(),
                        },
                    );
                    let st = build_status(&app_worker);
                    emit_status_changed(&app_worker, &st);
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (
                    &dir,
                    &bin,
                    &model,
                    &wav_path,
                    &app_worker,
                    &effective_source_lang,
                    &target_lang,
                    &resolved_mode,
                    resolved_provider_id,
                    target_pid,
                    model_spec.snapshot_interval_ms,
                );
                while RUNNING.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        });
    }

    let st = build_status(&app);
    emit_status_changed(&app, &st);
    Ok(st)
}

/// Signals the live translation worker to stop and synchronously terminates
/// any active `whisper-server.exe` child process so no child handle blocks exit or updates.
pub fn shutdown_engine() {
    RUNNING.store(false, Ordering::SeqCst);
    let pid = WHISPER_SERVER_PID.swap(0, Ordering::SeqCst);
    if pid != 0 {
        #[cfg(target_os = "windows")]
        {
            let _ = hidden_command("taskkill")
                .args(["/F", "/PID", &pid.to_string(), "/T"])
                .status();
        }
    }
}

#[tauri::command]
pub fn stop_live_translate(app: AppHandle) -> TranslateEngineStatus {
    RUNNING.store(false, Ordering::SeqCst);
    emit_subtitle(
        &app,
        &SubtitlePayload {
            original: String::new(),
            translated: String::new(),
            target_lang: "fa".to_string(),
            active: false,
            translation_status: "ok".to_string(),
        },
    );
    if let Some(win) = app.get_webview_window("subtitle") {
        let _ = win.hide();
    }
    let st = build_status(&app);
    emit_status_changed(&app, &st);
    st
}

#[tauri::command]
pub async fn test_subtitle_overlay(
    app: AppHandle,
    text: String,
    target_lang: String,
) -> Result<SubtitlePayload, String> {
    position_subtitle_window(&app);
    if let Some(win) = app.get_webview_window("subtitle") {
        let _ = win.show();
    }
    let cfg = crate::settings::load(&app);
    let provider_id =
        crate::translate_provider::TranslateProviderId::parse_id(&cfg.translate_provider);
    let clean = text.trim().to_string();
    let sl = cfg.translate_source_lang;
    let tl = target_lang.clone();
    let (translated, status) = tauri::async_runtime::spawn_blocking(move || {
        if provider_id == crate::translate_provider::TranslateProviderId::None {
            return (clean, "ok".to_string());
        }
        let key = if provider_id.requires_api_key() {
            crate::translate_secrets::get_api_key(provider_id)
        } else {
            None
        };
        match translate_with_provider(provider_id, key.as_deref(), &clean, &sl, &tl, &[]) {
            Ok(out) => (out, "ok".to_string()),
            Err(err) => (String::new(), err.to_subtitle_status().to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    let payload = SubtitlePayload {
        original: text,
        translated,
        target_lang,
        active: true,
        translation_status: status,
    };
    emit_subtitle(&app, &payload);
    let app_hide = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        if !RUNNING.load(Ordering::SeqCst) {
            emit_subtitle(
                &app_hide,
                &SubtitlePayload {
                    original: String::new(),
                    translated: String::new(),
                    target_lang: "fa".to_string(),
                    active: false,
                    translation_status: "ok".to_string(),
                },
            );
            if let Some(win) = app_hide.get_webview_window("subtitle") {
                let _ = win.hide();
            }
        }
    });
    Ok(payload)
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyStatus {
    pub google_cloud: bool,
    pub gemini: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestProviderResult {
    pub ok: bool,
    pub translated: Option<String>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// Saves a translation provider API key into Windows Credential Manager.
/// Restricted to `"main"` and `"settings"` windows; keys are never logged or returned.
#[tauri::command]
pub fn set_translate_api_key(
    window: tauri::Window,
    provider: String,
    api_key: String,
) -> Result<ApiKeyStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    let pid = crate::translate_provider::TranslateProviderId::parse_id(&provider);
    crate::translate_secrets::set_api_key(pid, &api_key)?;
    Ok(ApiKeyStatus {
        google_cloud: crate::translate_secrets::has_api_key(
            crate::translate_provider::TranslateProviderId::GoogleCloud,
        ),
        gemini: crate::translate_secrets::has_api_key(
            crate::translate_provider::TranslateProviderId::Gemini,
        ),
    })
}

/// Deletes a translation provider API key from Windows Credential Manager.
#[tauri::command]
pub fn delete_translate_api_key(
    window: tauri::Window,
    provider: String,
) -> Result<ApiKeyStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    let pid = crate::translate_provider::TranslateProviderId::parse_id(&provider);
    crate::translate_secrets::delete_api_key(pid)?;
    Ok(ApiKeyStatus {
        google_cloud: crate::translate_secrets::has_api_key(
            crate::translate_provider::TranslateProviderId::GoogleCloud,
        ),
        gemini: crate::translate_secrets::has_api_key(
            crate::translate_provider::TranslateProviderId::Gemini,
        ),
    })
}

/// Returns only whether each cloud provider has a stored API key (never the key itself).
#[tauri::command]
pub fn get_translate_api_key_status(window: tauri::Window) -> Result<ApiKeyStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    Ok(ApiKeyStatus {
        google_cloud: crate::translate_secrets::has_api_key(
            crate::translate_provider::TranslateProviderId::GoogleCloud,
        ),
        gemini: crate::translate_secrets::has_api_key(
            crate::translate_provider::TranslateProviderId::Gemini,
        ),
    })
}

/// Tests a configured translation provider by translating a short sample phrase
/// using its stored key from Windows Credential Manager.
#[tauri::command]
pub async fn test_translate_provider(
    window: tauri::Window,
    app: AppHandle,
    provider: String,
    target_lang: String,
) -> Result<TestProviderResult, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    let pid = crate::translate_provider::TranslateProviderId::parse_id(&provider);
    let ui_lang = crate::settings::load(&app).language;
    let tl = if target_lang.trim().is_empty() {
        "fa".to_string()
    } else {
        target_lang
    };
    let sample = if tl == "en" {
        "سلام دنیا"
    } else {
        "Hello, world!"
    };
    let sl = if tl == "en" { "fa" } else { "en" };

    let res = tauri::async_runtime::spawn_blocking(move || {
        let start = std::time::Instant::now();
        let api_key = if pid.requires_api_key() {
            crate::translate_secrets::get_api_key(pid)
        } else {
            None
        };
        let outcome = translate_with_provider(pid, api_key.as_deref(), sample, sl, &tl, &[]);
        let latency_ms = start.elapsed().as_millis() as u64;
        match outcome {
            Ok(translated) => TestProviderResult {
                ok: true,
                translated: Some(translated),
                latency_ms,
                error: None,
            },
            Err(err) => TestProviderResult {
                ok: false,
                translated: None,
                latency_ms,
                error: Some(err.user_message(&ui_lang)),
            },
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(res)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSampleItem {
    pub id: String,
    pub category: String,
    pub source_lang: String,
    pub target_lang: String,
    pub source_text: String,
    #[serde(default)]
    pub reference_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSampleFile {
    pub schema_version: u32,
    #[serde(default)]
    pub description: String,
    pub samples: Vec<EvalSampleItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalProviderOutput {
    pub provider: String,
    pub ok: bool,
    pub translated: Option<String>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalSampleResult {
    pub id: String,
    pub source_text: String,
    pub results: Vec<EvalProviderOutput>,
}

/// Dev-only quality & latency evaluation helper.
/// Reads `tests/translation-samples.json`, translates every sample with each
/// configured cloud provider (reading keys strictly from OS secure storage),
/// measures latency, and writes a JSON report to `output_path`.
/// Disabled in release builds (`cfg(not(debug_assertions))`).
#[tauri::command]
pub async fn evaluate_translation_samples(
    window: tauri::Window,
    samples_path: String,
    output_path: String,
) -> Result<Vec<EvalSampleResult>, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (samples_path, output_path);
        Err("evaluate_translation_samples is only available in debug builds".to_string())
    }
    #[cfg(debug_assertions)]
    {
        use crate::translate_provider::TranslateProviderId;
        tauri::async_runtime::spawn_blocking(move || {
            let raw = fs::read_to_string(&samples_path)
                .map_err(|e| format!("Failed reading samples file: {e}"))?;
            let parsed: EvalSampleFile =
                serde_json::from_str(&raw).map_err(|e| format!("Invalid samples JSON: {e}"))?;

            let providers = [
                TranslateProviderId::GoogleCloud,
                TranslateProviderId::Gemini,
            ];
            let mut report = Vec::with_capacity(parsed.samples.len());
            for sample in parsed.samples {
                let mut results = Vec::new();
                for pid in providers {
                    let Some(key) = crate::translate_secrets::get_api_key(pid) else {
                        continue;
                    };
                    let start = std::time::Instant::now();
                    let outcome = translate_with_provider(
                        pid,
                        Some(&key),
                        &sample.source_text,
                        &sample.source_lang,
                        &sample.target_lang,
                        &[],
                    );
                    let latency_ms = start.elapsed().as_millis() as u64;
                    match outcome {
                        Ok(translated) => results.push(EvalProviderOutput {
                            provider: pid.as_str().to_string(),
                            ok: true,
                            translated: Some(translated),
                            latency_ms,
                            error: None,
                        }),
                        Err(err) => results.push(EvalProviderOutput {
                            provider: pid.as_str().to_string(),
                            ok: false,
                            translated: None,
                            latency_ms,
                            error: Some(err.to_string()),
                        }),
                    }
                }
                report.push(EvalSampleResult {
                    id: sample.id,
                    source_text: sample.source_text,
                    results,
                });
            }
            let out_json = serde_json::to_string_pretty(&report)
                .map_err(|e| format!("Failed serializing evaluation report: {e}"))?;
            fs::write(&output_path, out_json)
                .map_err(|e| format!("Failed writing evaluation report: {e}"))?;
            Ok(report)
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_gate_filters_quiet_buffers() {
        let silence = vec![0u8; 3200];
        assert!(!has_audible_speech(&silence));
        let loud: Vec<u8> = (0..1600)
            .flat_map(|i| {
                let v: i16 = if i % 2 == 0 { 1200 } else { -1200 };
                v.to_le_bytes()
            })
            .collect();
        assert!(has_audible_speech(&loud));
    }

    #[test]
    fn cleans_whisper_hallucinations() {
        let raw = "[BLANK_AUDIO]\n(music)\nHello world\nThank you.\n";
        assert_eq!(clean_whisper_output(raw), "Hello world");
    }

    #[test]
    fn writes_valid_pcm16_mono_wav_header() {
        let tmp = std::env::temp_dir().join("nexora_test_pcm16.wav");
        let pcm = vec![0u8; 640];
        write_pcm16_mono_wav(&tmp, &pcm, 16_000).expect("wav write succeeds");
        let bytes = fs::read(&tmp).expect("wav read succeeds");
        let _ = fs::remove_file(&tmp);
        assert_eq!(bytes.len(), 44 + 640);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
    }

    #[test]
    fn builds_multipart_body_and_parses_server_json_or_text() {
        let body = build_whisper_multipart_body(b"RIFF", "BOUND");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("--BOUND\r\n"));
        assert!(text.contains("filename=\"chunk.wav\""));
        assert!(text.contains("--BOUND--\r\n"));
        assert_eq!(
            parse_whisper_server_response(r#"{"text":"  Hello fast world \n"}"#),
            "Hello fast world"
        );
        assert_eq!(
            parse_whisper_server_response("  Plain text line  \n"),
            "Plain text line"
        );
    }

    #[test]
    fn sha256_verification_accepts_valid_and_deletes_invalid() {
        let p = std::env::temp_dir().join("nexora_sha256_test.bin");
        fs::write(&p, b"abc").expect("write");
        // Known SHA-256 of "abc"
        let ok_hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_file_sha256(&p, ok_hash).is_ok());
        assert!(p.exists());
        assert!(verify_file_sha256(&p, "TODO_FILL_HASH").is_err());
        assert!(!p.exists());
        fs::write(&p, b"abc").expect("rewrite");
        assert!(verify_file_sha256(&p, &"0".repeat(64)).is_err());
        assert!(!p.exists());
    }

    #[test]
    fn zip_extraction_enforces_size_and_file_limits() {
        let zip_path = std::env::temp_dir().join("nexora_zip_limit_test.zip");
        let out_dir = std::env::temp_dir().join("nexora_zip_limit_out");
        let _ = fs::remove_dir_all(&out_dir);
        fs::create_dir_all(&out_dir).expect("mkdir");
        {
            let f = File::create(&zip_path).expect("create zip");
            let mut zw = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zw.start_file("a.txt", opts).expect("start a");
            zw.write_all(&[b'x'; 200]).expect("write a");
            zw.start_file("b.txt", opts).expect("start b");
            zw.write_all(&[b'y'; 200]).expect("write b");
            zw.finish().expect("finish");
        }
        assert!(extract_zip_with_limits(&zip_path, &out_dir, 500, 1).is_err());
        assert!(extract_zip_with_limits(&zip_path, &out_dir, 250, 10).is_err());
        assert!(extract_zip_with_limits(&zip_path, &out_dir, 1024, 10).is_ok());
        let _ = fs::remove_file(&zip_path);
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn stability_tracker_waits_for_two_identical_snapshots_or_finalization() {
        let mut st = SubtitleStabilityTracker::new();
        assert!(!st.observe("Hello", false));
        assert!(st.observe("  hello. ", false));
        // Duplicate already-translated text does not re-trigger.
        assert!(!st.observe("Hello!", false));
        // New partial phrase waits again.
        assert!(!st.observe("Hello world", false));
        // Finalized phrase triggers immediately.
        assert!(st.observe("Hello world", true));
    }

    #[test]
    fn translation_cache_normalizes_keys_and_evicts_oldest() {
        use crate::translate_provider::TranslateProviderId;
        let mut cache = TranslationCache::new(2);
        cache.insert(
            "  Hello, World! ",
            TranslateProviderId::Gemini,
            "fa",
            "سلام دنیا".into(),
        );
        assert_eq!(
            cache.get("hello, world", TranslateProviderId::Gemini, "FA"),
            Some("سلام دنیا".to_string())
        );
        assert_eq!(
            cache.get("hello, world", TranslateProviderId::GoogleCloud, "fa"),
            None
        );
        cache.insert("Second", TranslateProviderId::Gemini, "fa", "دوم".into());
        cache.insert("Third", TranslateProviderId::Gemini, "fa", "سوم".into());
        assert_eq!(cache.len(), 2);
        assert_eq!(
            cache.get("hello, world", TranslateProviderId::Gemini, "fa"),
            None
        );
        assert_eq!(
            cache.get("second", TranslateProviderId::Gemini, "fa"),
            Some("دوم".to_string())
        );
        assert_eq!(
            cache.get("third", TranslateProviderId::Gemini, "fa"),
            Some("سوم".to_string())
        );
    }

    #[test]
    fn adaptive_vad_raises_noise_floor_and_detects_speech_peaks() {
        let mut vad = AdaptiveVadGate::new();
        let make_pcm = |amp: i16| -> Vec<u8> {
            (0..800)
                .flat_map(|i| {
                    let v = if i % 2 == 0 { amp } else { -amp };
                    v.to_le_bytes()
                })
                .collect()
        };
        // Feed low steady music/hum: should stay unvoiced and adapt noise floor.
        let hum = make_pcm(100);
        for _ in 0..10 {
            assert!(!vad.is_voiced(&hum));
        }
        // Clear speech peak above adaptive threshold triggers voiced.
        let speech = make_pcm(1400);
        assert!(vad.is_voiced(&speech));
        // Drops back to unvoiced on silence.
        assert!(!vad.is_voiced(&vec![0u8; 1600]));
    }

    #[test]
    fn clean_whisper_output_filters_multilingual_hallucinations_and_repetitions() {
        assert_eq!(clean_whisper_output("Thanks for watching!"), "");
        assert_eq!(clean_whisper_output("ممنون."), "");
        assert_eq!(clean_whisper_output("♪ music ♪"), "");
        assert_eq!(clean_whisper_output("you you you you you"), "");
        assert_eq!(
            clean_whisper_output("the end, the end, the end, the end"),
            ""
        );
        assert_eq!(
            clean_whisper_output("Hello team, let's push mid."),
            "Hello team, let's push mid."
        );
    }

    #[test]
    fn whisper_model_catalog_has_valid_sha256_and_adaptive_intervals() {
        for spec in WHISPER_MODELS {
            assert_eq!(spec.sha256.len(), 64);
            assert!(spec.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(spec.snapshot_interval_ms >= 250);
        }
        let turbo = whisper_model_spec("ggml-large-v3-turbo-q5_0.bin");
        assert!(
            turbo.snapshot_interval_ms > whisper_model_spec("ggml-tiny.bin").snapshot_interval_ms
        );
        assert_eq!(whisper_model_spec("unknown.bin").filename, "ggml-tiny.bin");
    }
}
