//! **Nexora-Voice v1** — Offline Persian Live Dubbing Engine.
//!
//! Captures either **Full System Audio** (excluding Nexora's own process tree
//! via `PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE` so dubbed output never
//! loops back) or a **Specific Window / Application** (`INCLUDE_TARGET_PROCESS_TREE`),
//! transcribes English speech via Whisper, translates to Persian via the active
//! Nexora translation provider (offline `Nexora-MT v2`, Gemini, Google Cloud, or
//! Google Free), synthesizes natural Persian speech with Piper ONNX neural voices
//! (`fa_IR-amir-medium` / `fa_IR-gyro-medium`), and smoothly ducks original audio
//! volume using Windows `ISimpleAudioVolume` while playing the dubbed voice.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const CAPTURE_SAMPLE_RATE: usize = 16_000;
const MIN_DUB_PCM_BYTES: usize = 16_000; // 0.5 s at 16 kHz mono 16-bit
const MAX_DUB_PCM_BYTES: usize = 16_000 * 2 * 10; // 10 s phrase cap
const SILENCE_FINALIZE_MS: u64 = 520;
const MAX_TTS_QUEUE: usize = 2;

/// Official Piper Windows x64 release archive (`2023.11.14-2`, 22,477,236 bytes).
pub const PIPER_ARCHIVE_URL: &str =
    "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip";
pub const PIPER_ARCHIVE_SHA256: &str =
    "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea";

#[derive(Debug, Clone, Copy)]
pub struct NexoraVoiceSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub onnx_filename: &'static str,
    pub json_filename: &'static str,
    pub onnx_url: &'static str,
    pub json_url: &'static str,
    pub onnx_sha256: &'static str,
    pub approx_mb: u32,
}

pub const PERSIAN_VOICES: &[NexoraVoiceSpec] = &[
    NexoraVoiceSpec {
        id: "fa_IR-amir-medium",
        label: "Amir (Persian Natural, 22 kHz)",
        onnx_filename: "fa_IR-amir-medium.onnx",
        json_filename: "fa_IR-amir-medium.onnx.json",
        onnx_url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/fa/fa_IR/amir/medium/fa_IR-amir-medium.onnx?download=true",
        json_url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/fa/fa_IR/amir/medium/fa_IR-amir-medium.onnx.json?download=true",
        onnx_sha256: "e8c2f4c8381bec787bec81cedf6d0b32ddeb5eaeb136cc1c554014614eefe443",
        approx_mb: 60,
    },
    NexoraVoiceSpec {
        id: "fa_IR-gyro-medium",
        label: "Gyro (Persian Expressive, 22 kHz)",
        onnx_filename: "fa_IR-gyro-medium.onnx",
        json_filename: "fa_IR-gyro-medium.onnx.json",
        onnx_url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/fa/fa_IR/gyro/medium/fa_IR-gyro-medium.onnx?download=true",
        json_url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/fa/fa_IR/gyro/medium/fa_IR-gyro-medium.onnx.json?download=true",
        onnx_sha256: "37dfae43c82ee38ca9e6aac4ffef76a74d6b282ccbc397b27761f35d355c99ba",
        approx_mb: 60,
    },
];

pub fn voice_spec(id: &str) -> &'static NexoraVoiceSpec {
    PERSIAN_VOICES
        .iter()
        .find(|v| v.id.eq_ignore_ascii_case(id.trim()))
        .unwrap_or(&PERSIAN_VOICES[0])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DubbingStatus {
    pub piper_installed: bool,
    pub voice_installed: bool,
    pub installed_voices: Vec<String>,
    pub ready: bool,
    pub downloading: bool,
    pub running: bool,
    pub voice_id: String,
    pub capture_mode: String,
    pub target_pid: Option<u32>,
    pub target_app_name: Option<String>,
    pub duck_percent: u8,
    pub speech_rate_pct: u16,
    pub last_spoken_fa: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DubbingProgressPayload {
    pub percent: u8,
    pub stage: String,
}

#[derive(Debug, Clone)]
struct DubbingPrefs {
    voice_id: String,
    capture_mode: String,
    target_pid: Option<u32>,
    target_app_name: Option<String>,
    duck_percent: u8,
    speech_rate_pct: u16,
    last_spoken_fa: String,
    last_error: Option<String>,
}

impl Default for DubbingPrefs {
    fn default() -> Self {
        Self {
            voice_id: PERSIAN_VOICES[0].id.to_string(),
            capture_mode: "system".to_string(),
            target_pid: None,
            target_app_name: None,
            duck_percent: 25,
            speech_rate_pct: 105,
            last_spoken_fa: String::new(),
            last_error: None,
        }
    }
}

static DUB_RUNNING: AtomicBool = AtomicBool::new(false);
static DUB_DOWNLOADING: AtomicBool = AtomicBool::new(false);
static DUB_PREFS: std::sync::LazyLock<Mutex<DubbingPrefs>> =
    std::sync::LazyLock::new(|| Mutex::new(DubbingPrefs::default()));

fn voice_dir(app: &AppHandle) -> PathBuf {
    crate::translator::engine_dir(app).join("voice")
}

fn find_piper_binary(dir: &Path) -> Option<PathBuf> {
    for rel in ["piper.exe", "piper/piper.exe", "piper", "piper/piper"] {
        let p = dir.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn build_status(app: &AppHandle) -> DubbingStatus {
    let dir = voice_dir(app);
    let prefs = DUB_PREFS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let spec = voice_spec(&prefs.voice_id);
    let piper_installed = find_piper_binary(&dir).is_some();
    let installed_voices: Vec<String> = PERSIAN_VOICES
        .iter()
        .filter(|v| dir.join(v.onnx_filename).is_file() && dir.join(v.json_filename).is_file())
        .map(|v| v.id.to_string())
        .collect();
    let voice_installed = installed_voices.iter().any(|id| id == spec.id);
    let whisper_ready = {
        let t_dir = crate::translator::engine_dir(app);
        crate::translator::find_whisper_binary(&t_dir).is_some()
    };
    DubbingStatus {
        piper_installed,
        voice_installed,
        installed_voices,
        ready: piper_installed && !installed_voices.is_empty() && whisper_ready,
        downloading: DUB_DOWNLOADING.load(Ordering::SeqCst),
        running: DUB_RUNNING.load(Ordering::SeqCst),
        voice_id: spec.id.to_string(),
        capture_mode: prefs.capture_mode,
        target_pid: prefs.target_pid,
        target_app_name: prefs.target_app_name,
        duck_percent: prefs.duck_percent,
        speech_rate_pct: prefs.speech_rate_pct,
        last_spoken_fa: prefs.last_spoken_fa,
        last_error: prefs.last_error,
    }
}

fn emit_status_changed(app: &AppHandle, st: &DubbingStatus) {
    let _ = app.emit("dubbing-status-changed", st);
}

/// Normalizes Persian text before Piper phonemization so Arabic variants and
/// noisy characters never degrade speech quality.
pub fn prepare_persian_tts_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    let mut prev_space = true;
    for ch in raw.chars() {
        let mapped = match ch {
            'ي' | 'ى' => 'ی',
            'ك' => 'ک',
            'ة' => 'ه',
            'أ' | 'إ' => 'ا',
            '\u{200b}' | '\u{feff}' => continue, // zero-width space / BOM
            '[' | ']' | '(' | ')' | '"' | '«' | '»' | '*' | '#' => ' ',
            _ => ch,
        };
        if mapped.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(mapped);
            prev_space = false;
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        return trimmed;
    }
    if !matches!(trimmed.chars().last(), Some('.' | '!' | '?' | '؟' | '،')) {
        format!("{trimmed}.")
    } else {
        trimmed
    }
}

/// Converts UI speech rate percentage (70..=150) to Piper's `--length_scale`
/// (inverse of speed: 100% -> 1.0, 125% -> 0.8).
pub fn speech_rate_to_length_scale(speech_rate_pct: u16) -> f32 {
    let clamped = speech_rate_pct.clamp(70, 150) as f32;
    100.0 / clamped
}

fn synthesize_piper_wav(
    piper_bin: &Path,
    onnx_path: &Path,
    json_path: &Path,
    text: &str,
    out_wav: &Path,
    speech_rate_pct: u16,
) -> Result<(), String> {
    let clean = prepare_persian_tts_text(text);
    if clean.is_empty() {
        return Err("Empty Persian text".to_string());
    }
    let scale = format!("{:.3}", speech_rate_to_length_scale(speech_rate_pct));
    let mut cmd = Command::new(piper_bin);
    if let Some(piper_dir) = piper_bin.parent() {
        cmd.current_dir(piper_dir);
        let espeak_dir = piper_dir.join("espeak-ng-data");
        if espeak_dir.is_dir() {
            cmd.arg("--espeak_data").arg(espeak_dir);
        }
    }
    cmd.arg("--model")
        .arg(onnx_path)
        .arg("--config")
        .arg(json_path)
        .arg("--output_file")
        .arg(out_wav)
        .arg("--length_scale")
        .arg(&scale)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed launching Piper TTS: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(clean.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("Failed writing text to Piper: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Piper wait failed: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Piper exited with {}: {}",
            output.status,
            err.trim()
        ));
    }
    if !out_wav.is_file() {
        return Err("Piper did not produce output WAV".to_string());
    }
    Ok(())
}

#[cfg(windows)]
mod win_audio {
    use std::path::Path;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        ISimpleAudioVolume, MMDeviceEnumerator, PlaySoundW, SND_FILENAME, SND_NODEFAULT, SND_SYNC,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    struct SavedSession {
        vol: ISimpleAudioVolume,
        prev_level: f32,
    }

    pub struct AudioDucker {
        saved: Vec<SavedSession>,
    }

    impl AudioDucker {
        pub fn engage(target_pid: Option<u32>, duck_percent: u8) -> Self {
            if duck_percent >= 100 {
                return Self { saved: Vec::new() };
            }
            let ratio = (duck_percent as f32 / 100.0).clamp(0.05, 1.0);
            let self_pid = std::process::id();
            let mut saved = Vec::new();

            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let Ok(enumr): Result<IMMDeviceEnumerator, _> =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                else {
                    return Self { saved };
                };
                let Ok(device) = enumr.GetDefaultAudioEndpoint(eRender, eMultimedia) else {
                    return Self { saved };
                };
                let Ok(mgr) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                    return Self { saved };
                };
                let Ok(sessions) = mgr.GetSessionEnumerator() else {
                    return Self { saved };
                };
                let count = sessions.GetCount().unwrap_or(0);
                for i in 0..count {
                    let Ok(ctrl) = sessions.GetSession(i) else {
                        continue;
                    };
                    let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else {
                        continue;
                    };
                    let pid = ctrl2.GetProcessId().unwrap_or(0);
                    if pid == 0 || pid == self_pid {
                        continue;
                    }
                    if target_pid.is_some_and(|wanted| pid != wanted) {
                        continue;
                    }
                    let Ok(vol) = ctrl.cast::<ISimpleAudioVolume>() else {
                        continue;
                    };
                    if let Ok(prev_level) = vol.GetMasterVolume() {
                        let new_level = (prev_level * ratio).clamp(0.02, prev_level.max(0.02));
                        if vol.SetMasterVolume(new_level, std::ptr::null()).is_ok() {
                            saved.push(SavedSession { vol, prev_level });
                        }
                    }
                }
            }
            Self { saved }
        }

        pub fn restore(&mut self) {
            for s in self.saved.drain(..) {
                unsafe {
                    let _ = s.vol.SetMasterVolume(s.prev_level, std::ptr::null());
                }
            }
        }
    }

    impl Drop for AudioDucker {
        fn drop(&mut self) {
            self.restore();
        }
    }

    pub fn play_wav_blocking(wav_path: &Path) -> Result<(), String> {
        let mut wide: Vec<u16> = wav_path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .collect();
        wide.push(0);
        let ok = unsafe {
            PlaySoundW(
                PCWSTR(wide.as_ptr()),
                None,
                SND_FILENAME | SND_SYNC | SND_NODEFAULT,
            )
        };
        if ok.as_bool() {
            Ok(())
        } else {
            Err("Windows PlaySoundW failed".to_string())
        }
    }
}

#[cfg(not(windows))]
mod win_audio {
    use std::path::Path;
    pub struct AudioDucker;
    impl AudioDucker {
        pub fn engage(_target_pid: Option<u32>, _duck_percent: u8) -> Self {
            Self
        }
    }
    pub fn play_wav_blocking(_wav_path: &Path) -> Result<(), String> {
        Err("Live dubbing audio playback is only supported on Windows".to_string())
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn run_dubbing_pipeline(
    app: AppHandle,
    whisper_dir: PathBuf,
    whisper_model_path: PathBuf,
    voice_onnx: PathBuf,
    voice_json: PathBuf,
    piper_bin: PathBuf,
    capture_mode: String,
    target_pid: Option<u32>,
    duck_percent: u8,
    speech_rate_pct: u16,
    provider_id: crate::translate_provider::TranslateProviderId,
) -> Result<(), String> {
    use wasapi::{
        initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode,
        WaveFormat,
    };

    let _ = initialize_mta();
    let desired_format = WaveFormat::new(16, 16, SampleType::Int, CAPTURE_SAMPLE_RATE, 1, None);
    let audio_client = if capture_mode == "app" {
        let pid = target_pid.ok_or_else(|| "No target application selected".to_string())?;
        let mut client = AudioClient::new_application_loopback_client(pid, true)
            .map_err(|e| format!("Failed attaching loopback to PID {pid}: {e}"))?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        };
        client
            .initialize_client(&desired_format, Direction::Capture, mode, None)
            .map_err(|e| format!("Failed initializing app loopback: {e}"))?;
        client
    } else {
        // Prefer excluding Nexora's own process tree so dubbed Persian output never
        // loops back into Whisper; fall back to default render loopback on older Windows 10.
        let excl_res = AudioClient::new_application_loopback_client(std::process::id(), false)
            .and_then(|mut client| {
                let mode = StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: 0,
                };
                client.initialize_client(&desired_format, Direction::Capture, mode, None)?;
                Ok(client)
            });
        match excl_res {
            Ok(client) => client,
            Err(_) => {
                let enumerator = DeviceEnumerator::new()
                    .map_err(|e| format!("Failed creating WASAPI enumerator: {e}"))?;
                let device = enumerator
                    .get_default_device(&Direction::Render)
                    .map_err(|e| format!("Failed getting default render device: {e}"))?;
                let mut client = device
                    .get_iaudioclient()
                    .map_err(|e| format!("Failed opening render audio client: {e}"))?;
                let mode = StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: 0,
                };
                client
                    .initialize_client(&desired_format, Direction::Capture, mode, None)
                    .map_err(|e| format!("Failed initializing fallback system loopback: {e}"))?;
                client
            }
        }
    };

    let h_event = audio_client
        .set_get_eventhandle()
        .map_err(|e| format!("Failed acquiring WASAPI event handle: {e}"))?;
    let capture_client = audio_client
        .get_audiocaptureclient()
        .map_err(|e| format!("Failed acquiring capture client: {e}"))?;
    audio_client
        .start_stream()
        .map_err(|e| format!("Failed starting WASAPI capture stream: {e}"))?;

    type PhraseSlot = Arc<(Mutex<(Option<Vec<u8>>, bool)>, Condvar)>;
    type TtsQueue = Arc<(Mutex<(VecDeque<String>, bool)>, Condvar)>;
    let phrase_slot: PhraseSlot = Arc::new((Mutex::new((None, false)), Condvar::new()));
    let tts_queue: TtsQueue = Arc::new((
        Mutex::new((VecDeque::with_capacity(MAX_TTS_QUEUE + 1), false)),
        Condvar::new(),
    ));

    // Thread 1: Playback + Audio Ducking worker
    let tts_rx = Arc::clone(&tts_queue);
    let app_tts = app.clone();
    let voice_dir_path = voice_dir(&app);
    let playback_handle = std::thread::spawn(move || {
        let (lock, cvar) = &*tts_rx;
        let mut slot_idx: usize = 0;
        loop {
            let text = {
                let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                while guard.0.is_empty() && !guard.1 && DUB_RUNNING.load(Ordering::SeqCst) {
                    guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
                }
                if guard.1 || !DUB_RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                guard.0.pop_front()
            };
            let Some(fa_text) = text else {
                continue;
            };
            let wav_out = voice_dir_path.join(format!("_nexora_dub_out_{}.wav", slot_idx % 2));
            slot_idx = slot_idx.wrapping_add(1);
            match synthesize_piper_wav(
                &piper_bin,
                &voice_onnx,
                &voice_json,
                &fa_text,
                &wav_out,
                speech_rate_pct,
            ) {
                Ok(()) if DUB_RUNNING.load(Ordering::SeqCst) => {
                    {
                        let mut prefs = DUB_PREFS.lock().unwrap_or_else(|e| e.into_inner());
                        prefs.last_spoken_fa = fa_text.clone();
                        prefs.last_error = None;
                    }
                    emit_status_changed(&app_tts, &build_status(&app_tts));
                    let duck_pid = if capture_mode == "app" {
                        target_pid
                    } else {
                        None
                    };
                    let _ducker = win_audio::AudioDucker::engage(duck_pid, duck_percent);
                    if let Err(err) = win_audio::play_wav_blocking(&wav_out) {
                        let mut prefs = DUB_PREFS.lock().unwrap_or_else(|e| e.into_inner());
                        prefs.last_error = Some(err);
                        drop(prefs);
                        emit_status_changed(&app_tts, &build_status(&app_tts));
                    }
                }
                Ok(()) => {}
                Err(err) => {
                    let mut prefs = DUB_PREFS.lock().unwrap_or_else(|e| e.into_inner());
                    prefs.last_error = Some(err);
                    drop(prefs);
                    emit_status_changed(&app_tts, &build_status(&app_tts));
                }
            }
        }
    });

    // Thread 2: Whisper + Translation worker (phrase-level for natural dubbing prosody)
    let phrase_rx = Arc::clone(&phrase_slot);
    let tts_tx = Arc::clone(&tts_queue);
    let server_bin = crate::translator::find_whisper_server(&whisper_dir);
    let cli_bin = crate::translator::find_whisper_binary(&whisper_dir)
        .ok_or_else(|| "Whisper CLI binary missing".to_string())?;
    let wav_in = voice_dir(&app).join("_nexora_dub_in.wav");
    let stt_handle = std::thread::spawn(move || {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().clamp(2, 6))
            .unwrap_or(4);
        let server = server_bin.as_deref().and_then(|sb| {
            crate::translator::ResidentWhisperServer::start(sb, &whisper_model_path, "en", threads)
        });
        let api_key = if provider_id.requires_api_key() {
            crate::translate_secrets::get_api_key(provider_id)
        } else {
            None
        };
        let local_model_path =
            whisper_dir.join(crate::translate_provider::LOCAL_TRANSLATE_MODEL_FILENAME);
        let mut rolling_ctx: VecDeque<String> = VecDeque::with_capacity(4);

        let (lock, cvar) = &*phrase_rx;
        loop {
            let pcm_opt = {
                let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                while guard.0.is_none() && !guard.1 && DUB_RUNNING.load(Ordering::SeqCst) {
                    guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
                }
                if guard.1 || !DUB_RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                guard.0.take()
            };
            let Some(pcm_bytes) = pcm_opt else {
                continue;
            };
            let text_opt = server
                .as_ref()
                .and_then(|srv| srv.transcribe_pcm(&pcm_bytes))
                .or_else(|| {
                    crate::translator::transcribe_cli_fallback(
                        &cli_bin,
                        &whisper_model_path,
                        &wav_in,
                        &pcm_bytes,
                        "en",
                        threads,
                    )
                });
            let Some(en_text) = text_opt else {
                continue;
            };
            let en_trim = en_text.trim();
            if en_trim.is_empty() {
                continue;
            }
            let fa_text = if provider_id == crate::translate_provider::TranslateProviderId::None {
                en_trim.to_string()
            } else {
                let ctx_vec: Vec<String> = rolling_ctx.iter().cloned().collect();
                match crate::translator::translate_with_provider(
                    provider_id,
                    api_key.as_deref(),
                    Some(local_model_path.as_path()),
                    en_trim,
                    "en",
                    "fa",
                    &ctx_vec,
                ) {
                    Ok(tr) if !tr.trim().is_empty() => {
                        if rolling_ctx.len() >= 3 {
                            rolling_ctx.pop_front();
                        }
                        rolling_ctx.push_back(en_trim.to_string());
                        tr
                    }
                    _ => continue,
                }
            };

            let (qlock, qcvar) = &*tts_tx;
            let mut qg = qlock.lock().unwrap_or_else(|e| e.into_inner());
            while qg.0.len() >= MAX_TTS_QUEUE {
                qg.0.pop_front();
            }
            qg.0.push_back(fa_text);
            qcvar.notify_one();
        }
    });

    // Producer loop: phrase-buffered capture with AdaptiveVadGate
    let mut rolling_pcm: VecDeque<u8> = VecDeque::with_capacity(MAX_DUB_PCM_BYTES + 16_384);
    let mut packet_buf: VecDeque<u8> = VecDeque::with_capacity(8_192);
    let mut vad = crate::translator::AdaptiveVadGate::new();
    let mut in_speech = false;
    let mut last_voice_at: Option<Instant> = None;

    while DUB_RUNNING.load(Ordering::SeqCst) {
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
                    let slice: Vec<u8> = packet_buf.drain(..).collect();
                    let voiced = vad.is_voiced(&slice);
                    let now = Instant::now();
                    if voiced {
                        in_speech = true;
                        last_voice_at = Some(now);
                        rolling_pcm.extend(slice);
                    } else if in_speech {
                        rolling_pcm.extend(slice);
                    }
                }
                _ => break,
            }
        }
        if !DUB_RUNNING.load(Ordering::SeqCst) {
            break;
        }
        if in_speech {
            let silence_ms = last_voice_at
                .map(|t| Instant::now().duration_since(t).as_millis() as u64)
                .unwrap_or(0);
            if silence_ms >= SILENCE_FINALIZE_MS || rolling_pcm.len() >= MAX_DUB_PCM_BYTES {
                if rolling_pcm.len() >= MIN_DUB_PCM_BYTES {
                    let pcm: Vec<u8> = rolling_pcm.iter().copied().collect();
                    if crate::translator::has_audible_speech(&pcm) {
                        let (plock, pcvar) = &*phrase_slot;
                        let mut pg = plock.lock().unwrap_or_else(|e| e.into_inner());
                        pg.0 = Some(pcm);
                        pcvar.notify_one();
                    }
                }
                rolling_pcm.clear();
                vad.reset();
                in_speech = false;
                last_voice_at = None;
            }
        }
    }

    {
        let (plock, pcvar) = &*phrase_slot;
        plock.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
        pcvar.notify_all();
    }
    {
        let (qlock, qcvar) = &*tts_queue;
        qlock.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
        qcvar.notify_all();
    }
    let _ = audio_client.stop_stream();
    let _ = stt_handle.join();
    let _ = playback_handle.join();
    Ok(())
}

#[tauri::command]
pub fn get_dubbing_status(app: AppHandle) -> DubbingStatus {
    build_status(&app)
}

#[tauri::command]
pub async fn download_dubbing_engine(
    window: tauri::Window,
    app: AppHandle,
    voice_id: Option<String>,
) -> Result<DubbingStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    if DUB_DOWNLOADING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(build_status(&app));
    }

    let selected = voice_id.unwrap_or_else(|| {
        DUB_PREFS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .voice_id
            .clone()
    });
    let spec = voice_spec(&selected);
    {
        let mut prefs = DUB_PREFS.lock().unwrap_or_else(|e| e.into_inner());
        prefs.voice_id = spec.id.to_string();
        prefs.last_error = None;
    }

    let app_clone = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let dir = voice_dir(&app_clone);
        fs::create_dir_all(&dir).map_err(|e| format!("Failed creating voice dir: {e}"))?;

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .user_agent("Nexora/0.2")
            .build()
            .map_err(|e| format!("HTTP client error: {e}"))?;

        if find_piper_binary(&dir).is_none() {
            let zip_part = dir.join("piper_windows_amd64.zip.part");
            let zip_path = dir.join("piper_windows_amd64.zip");
            crate::translator::download_file_streaming_event(
                &client,
                PIPER_ARCHIVE_URL,
                &zip_part,
                5,
                40,
                "Downloading Piper TTS engine (~22 MB)...",
                "dubbing-download-progress",
                &app_clone,
            )?;
            crate::translator::verify_file_sha256(&zip_part, PIPER_ARCHIVE_SHA256)?;
            fs::rename(&zip_part, &zip_path)
                .map_err(|e| format!("Failed finalizing Piper archive: {e}"))?;

            let file = fs::File::open(&zip_path)
                .map_err(|e| format!("Failed opening Piper archive: {e}"))?;
            let mut archive =
                zip::ZipArchive::new(file).map_err(|e| format!("Invalid Piper ZIP: {e}"))?;
            for i in 0..archive.len() {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| format!("ZIP entry error: {e}"))?;
                let Some(rel) = entry.enclosed_name() else {
                    continue;
                };
                let dest = dir.join(rel);
                if entry.is_dir() {
                    let _ = fs::create_dir_all(&dest);
                } else {
                    if let Some(parent) = dest.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let mut out = fs::File::create(&dest)
                        .map_err(|e| format!("Failed extracting Piper file: {e}"))?;
                    std::io::copy(&mut entry, &mut out)
                        .map_err(|e| format!("Failed copying Piper file: {e}"))?;
                }
            }
            let _ = fs::remove_file(&zip_path);
        }

        let onnx_dest = dir.join(spec.onnx_filename);
        let json_dest = dir.join(spec.json_filename);
        if !onnx_dest.is_file() {
            let part = dir.join(format!("{}.part", spec.onnx_filename));
            crate::translator::download_file_streaming_event(
                &client,
                spec.onnx_url,
                &part,
                45,
                92,
                &format!(
                    "Downloading Persian voice {} (~{} MB)...",
                    spec.id, spec.approx_mb
                ),
                "dubbing-download-progress",
                &app_clone,
            )?;
            crate::translator::verify_file_sha256(&part, spec.onnx_sha256)?;
            fs::rename(&part, &onnx_dest)
                .map_err(|e| format!("Failed finalizing voice ONNX: {e}"))?;
        }
        if !json_dest.is_file() {
            let part = dir.join(format!("{}.part", spec.json_filename));
            crate::translator::download_file_streaming_event(
                &client,
                spec.json_url,
                &part,
                92,
                99,
                "Downloading voice config...",
                "dubbing-download-progress",
                &app_clone,
            )?;
            fs::rename(&part, &json_dest)
                .map_err(|e| format!("Failed finalizing voice config: {e}"))?;
        }
        let _ = app_clone.emit(
            "dubbing-download-progress",
            DubbingProgressPayload {
                percent: 100,
                stage: "Nexora-Voice v1 ready".to_string(),
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;

    DUB_DOWNLOADING.store(false, Ordering::SeqCst);
    {
        let mut prefs = DUB_PREFS.lock().unwrap_or_else(|e| e.into_inner());
        prefs.last_error = res.as_ref().err().cloned();
    }
    res?;
    let st = build_status(&app);
    emit_status_changed(&app, &st);
    Ok(st)
}

#[tauri::command]
pub fn remove_dubbing_engine(
    window: tauri::Window,
    app: AppHandle,
) -> Result<DubbingStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    DUB_RUNNING.store(false, Ordering::SeqCst);
    let dir = voice_dir(&app);
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    let st = build_status(&app);
    emit_status_changed(&app, &st);
    Ok(st)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn start_live_dubbing(
    window: tauri::Window,
    app: AppHandle,
    capture_mode: Option<String>,
    target_pid: Option<u32>,
    target_app_name: Option<String>,
    voice_id: Option<String>,
    duck_percent: Option<u8>,
    speech_rate_pct: Option<u16>,
    translate_provider: Option<String>,
) -> Result<DubbingStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    if DUB_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(build_status(&app));
    }

    let mode = capture_mode
        .unwrap_or_else(|| "system".to_string())
        .to_ascii_lowercase();
    let spec = voice_spec(voice_id.as_deref().unwrap_or("fa_IR-amir-medium"));
    let duck = duck_percent.unwrap_or(25).clamp(5, 100);
    let rate = speech_rate_pct.unwrap_or(105).clamp(70, 150);

    let vdir = voice_dir(&app);
    let Some(piper_bin) = find_piper_binary(&vdir) else {
        DUB_RUNNING.store(false, Ordering::SeqCst);
        return Err("Nexora-Voice v1 engine is not downloaded yet.".to_string());
    };
    let voice_onnx = vdir.join(spec.onnx_filename);
    let voice_json = vdir.join(spec.json_filename);
    if !voice_onnx.is_file() || !voice_json.is_file() {
        DUB_RUNNING.store(false, Ordering::SeqCst);
        return Err("Selected Persian voice model is not downloaded yet.".to_string());
    }

    let wdir = crate::translator::engine_dir(&app);
    let settings = crate::settings::load(&app);
    let wmodel = wdir.join(&settings.whisper_model);
    if !wmodel.is_file() {
        DUB_RUNNING.store(false, Ordering::SeqCst);
        return Err(
            "Whisper speech model is required for live dubbing. Download it in Engines first."
                .to_string(),
        );
    }

    let prov_str = translate_provider.unwrap_or_else(|| settings.translate_provider.clone());
    let parsed_prov = crate::translate_provider::TranslateProviderId::parse_id(&prov_str);
    let prov_id = if parsed_prov == crate::translate_provider::TranslateProviderId::None {
        let local_mt = wdir.join(crate::translate_provider::LOCAL_TRANSLATE_MODEL_FILENAME);
        if local_mt.is_file() {
            crate::translate_provider::TranslateProviderId::Local
        } else {
            crate::translate_provider::TranslateProviderId::GoogleFree
        }
    } else {
        parsed_prov
    };

    {
        let mut prefs = DUB_PREFS.lock().unwrap_or_else(|e| e.into_inner());
        prefs.voice_id = spec.id.to_string();
        prefs.capture_mode = mode.clone();
        prefs.target_pid = target_pid;
        prefs.target_app_name = target_app_name;
        prefs.duck_percent = duck;
        prefs.speech_rate_pct = rate;
        prefs.last_error = None;
    }

    let app_clone = app.clone();
    std::thread::spawn(move || {
        #[cfg(windows)]
        let res = run_dubbing_pipeline(
            app_clone.clone(),
            wdir,
            wmodel,
            voice_onnx,
            voice_json,
            piper_bin,
            mode,
            target_pid,
            duck,
            rate,
            prov_id,
        );
        #[cfg(not(windows))]
        let res: Result<(), String> = {
            let _ = (
                wdir, wmodel, voice_onnx, voice_json, piper_bin, mode, target_pid, duck, rate,
                prov_id,
            );
            Err("Live dubbing is supported on Windows 10/11 only.".to_string())
        };

        DUB_RUNNING.store(false, Ordering::SeqCst);
        if let Err(err) = res {
            let mut prefs = DUB_PREFS.lock().unwrap_or_else(|e| e.into_inner());
            prefs.last_error = Some(err);
        }
        emit_status_changed(&app_clone, &build_status(&app_clone));
    });

    let st = build_status(&app);
    emit_status_changed(&app, &st);
    Ok(st)
}

#[tauri::command]
pub fn stop_live_dubbing(window: tauri::Window, app: AppHandle) -> Result<DubbingStatus, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    DUB_RUNNING.store(false, Ordering::SeqCst);
    let st = build_status(&app);
    emit_status_changed(&app, &st);
    Ok(st)
}

#[tauri::command]
pub async fn test_dubbing_voice(
    window: tauri::Window,
    app: AppHandle,
    text: Option<String>,
    voice_id: Option<String>,
    speech_rate_pct: Option<u16>,
) -> Result<String, String> {
    if !crate::ipc::is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    let sample =
        text.unwrap_or_else(|| "سلام! موتور دوبله هوشمند نوکسورا آماده به کار است.".to_string());
    let spec = voice_spec(voice_id.as_deref().unwrap_or("fa_IR-amir-medium"));
    let rate = speech_rate_pct.unwrap_or(105);
    let vdir = voice_dir(&app);
    let piper_bin = find_piper_binary(&vdir)
        .ok_or_else(|| "Nexora-Voice v1 engine is not downloaded yet.".to_string())?;
    let onnx = vdir.join(spec.onnx_filename);
    let json = vdir.join(spec.json_filename);
    if !onnx.is_file() || !json.is_file() {
        return Err("Selected Persian voice model is not downloaded yet.".to_string());
    }
    let wav = vdir.join("_nexora_dub_test.wav");

    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        synthesize_piper_wav(&piper_bin, &onnx, &json, &sample, &wav, rate)?;
        win_audio::play_wav_blocking(&wav)?;
        Ok(prepare_persian_tts_text(&sample))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persian_tts_text_normalizes_arabic_chars_and_adds_period() {
        let out = prepare_persian_tts_text("  سلام علي، حالت چطوره\u{200b}  ");
        assert_eq!(out, "سلام علی، حالت چطوره.");
    }

    #[test]
    fn speech_rate_maps_to_inverse_length_scale() {
        assert!((speech_rate_to_length_scale(100) - 1.0).abs() < 1e-4);
        assert!(speech_rate_to_length_scale(125) < 0.85);
        assert!(speech_rate_to_length_scale(50) <= 1.45);
    }

    #[test]
    fn voice_spec_falls_back_to_amir() {
        assert_eq!(voice_spec("fa_IR-gyro-medium").id, "fa_IR-gyro-medium");
        assert_eq!(voice_spec("unknown").id, "fa_IR-amir-medium");
    }
}
