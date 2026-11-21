//! Optional downloadable real-time audio-to-subtitle translation engine.
//!
//! The engine is downloaded on demand into `<app_data_dir>/live-translate/`
//! using pure native Rust HTTP streaming (`reqwest`) and native ZIP extraction
//! (`zip`) — never spawning `curl.exe` or `powershell.exe` during download.
//!
//! Audio capture on Windows uses native WASAPI loopback ONLY — supporting both:
//! - `"system"`: Full internal system playback audio (`Direction::Render` loopback)
//! - `"app"`: Specific application / window process-tree loopback
//!   (`AudioClient::new_application_loopback_client(pid, true)`)
//!
//! External microphone audio is NEVER captured.

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const MODEL_FILENAME: &str = "ggml-tiny.bin";
const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";
const WHISPER_ZIP_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/download/b5907/whisper-bin-x64.zip";

const SUBTITLE_WIDTH: f64 = 820.0;
const SUBTITLE_HEIGHT: f64 = 128.0;
const SUBTITLE_BOTTOM_MARGIN: f64 = 72.0;

#[cfg(target_os = "windows")]
const CAPTURE_SAMPLE_RATE: usize = 16_000;
#[cfg(target_os = "windows")]
const CHUNK_DURATION_MS: u64 = 3_200;
const SPEECH_RMS_THRESHOLD: f32 = 140.0;

static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

static ENGINE_STATE: Mutex<EnginePrefs> = Mutex::new(EnginePrefs {
    source_lang: None,
    target_lang: None,
    capture_mode: None,
    target_pid: None,
    target_app_name: None,
    last_error: None,
});

struct EnginePrefs {
    source_lang: Option<String>,
    target_lang: Option<String>,
    capture_mode: Option<String>,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgressPayload {
    pub percent: u8,
    pub stage: String,
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

fn model_path(dir: &Path) -> PathBuf {
    dir.join(MODEL_FILENAME)
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

fn is_installed_in(dir: &Path) -> bool {
    model_path(dir).exists() && find_whisper_binary(dir).is_some()
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
    let prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    TranslateEngineStatus {
        installed: is_installed_in(&dir),
        downloading: DOWNLOADING.load(Ordering::SeqCst),
        running: RUNNING.load(Ordering::SeqCst),
        model_size_mb: dir_size_mb(&dir),
        source_lang: prefs
            .source_lang
            .clone()
            .unwrap_or_else(|| "auto".to_string()),
        target_lang: prefs
            .target_lang
            .clone()
            .unwrap_or_else(|| "fa".to_string()),
        capture_mode: prefs
            .capture_mode
            .clone()
            .unwrap_or_else(|| "system".to_string()),
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

fn url_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Parses Google Translate's lightweight JSON array response (`[[["translated","orig",...]]]`).
pub fn parse_google_translate_response(raw: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(raw).ok()?;
    let sentences = val.get(0)?.as_array()?;
    let mut out = String::new();
    for seg in sentences {
        if let Some(piece) = seg.get(0).and_then(|v| v.as_str()) {
            out.push_str(piece);
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Writes 16 kHz, 16-bit signed mono PCM samples into a standard RIFF/WAVE file.
pub fn write_pcm16_mono_wav(path: &Path, pcm_bytes: &[u8], sample_rate: u32) -> Result<(), String> {
    let data_len = u32::try_from(pcm_bytes.len()).unwrap_or(u32::MAX - 36);
    let riff_size = data_len.saturating_add(36);
    let byte_rate = sample_rate.saturating_mul(2);
    let mut file = File::create(path).map_err(|e| format!("Failed to create WAV file: {e}"))?;
    file.write_all(b"RIFF").map_err(|e| e.to_string())?;
    file.write_all(&riff_size.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(b"WAVEfmt ").map_err(|e| e.to_string())?;
    file.write_all(&16u32.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(&1u16.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(&1u16.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(&sample_rate.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(&byte_rate.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(&2u16.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(&16u16.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(b"data").map_err(|e| e.to_string())?;
    file.write_all(&data_len.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(&pcm_bytes[..data_len as usize])
        .map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())
}

/// Returns `true` when 16-bit PCM audio exceeds the silence gate RMS threshold.
pub fn has_audible_speech(pcm_bytes: &[u8]) -> bool {
    let mut sum_sq: f64 = 0.0;
    let mut count: u64 = 0;
    for chunk in pcm_bytes.chunks_exact(2) {
        let sample = f64::from(i16::from_le_bytes([chunk[0], chunk[1]]));
        sum_sq += sample * sample;
        count += 1;
    }
    if count == 0 {
        return false;
    }
    let rms = (sum_sq / (count as f64)).sqrt() as f32;
    rms >= SPEECH_RMS_THRESHOLD
}

/// Strips Whisper silence hallucinations and bracketed non-speech markers.
pub fn clean_whisper_output(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('[')
                && !(l.starts_with('(') && l.ends_with(')'))
                && !l.eq_ignore_ascii_case("thank you.")
                && !l.eq_ignore_ascii_case("thanks for watching.")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Translates `text` into `target_lang` natively using `reqwest` (no external curl process).
fn translate_text(text: &str, source_lang: &str, target_lang: &str) -> String {
    let clean = text.trim();
    if clean.is_empty() {
        return String::new();
    }
    let sl = if source_lang.is_empty() {
        "auto"
    } else {
        source_lang
    };
    let tl = if target_lang.is_empty() {
        "fa"
    } else {
        target_lang
    };
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl={sl}&tl={tl}&dt=t&q={}",
        url_encode_query(clean)
    );
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(6))
        .user_agent("Nexora/0.2")
        .build()
    {
        Ok(c) => c,
        Err(_) => return clean.to_string(),
    };
    if let Ok(resp) = client.get(&url).send() {
        if resp.status().is_success() {
            if let Ok(body) = resp.text() {
                if let Some(translated) = parse_google_translate_response(&body) {
                    return translated;
                }
            }
        }
    }
    clean.to_string()
}

/// Streams an HTTP file to disk natively via `reqwest`, emitting smooth progress percentages.
fn download_file_streaming(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    start_pct: u8,
    end_pct: u8,
    stage: &str,
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
        "translate-download-progress",
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
                    "translate-download-progress",
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

/// Extracts a ZIP archive natively in Rust (no PowerShell process spawning).
fn extract_zip_native(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| format!("Failed to open archive: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP archive: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive
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
            let mut out_file = File::create(&out_path)
                .map_err(|e| format!("Failed to write extracted file: {e}"))?;
            std::io::copy(&mut entry, &mut out_file)
                .map_err(|e| format!("Failed extracting file: {e}"))?;
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
fn run_wasapi_loopback_worker(
    app: &AppHandle,
    bin: &Path,
    model: &Path,
    wav_path: &Path,
    source_lang: &str,
    target_lang: &str,
    capture_mode: &str,
    target_pid: Option<u32>,
) -> Result<(), String> {
    use std::collections::VecDeque;
    use std::time::Instant;
    use wasapi::{
        initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode,
        WaveFormat,
    };

    let _ = initialize_mta();

    let desired_format = WaveFormat::new(16, 16, SampleType::Int, CAPTURE_SAMPLE_RATE, 1, None);
    let mut audio_client = if capture_mode == "app" {
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

    let mut deque: VecDeque<u8> = VecDeque::with_capacity(CAPTURE_SAMPLE_RATE * 2 * 4);

    while RUNNING.load(Ordering::SeqCst) {
        deque.clear();
        let started = Instant::now();
        while RUNNING.load(Ordering::SeqCst)
            && started.elapsed() < Duration::from_millis(CHUNK_DURATION_MS)
        {
            // Short timeout so silence never blocks shutdown or stalls the loop.
            let _ = h_event.wait_for_event(200);
            loop {
                match capture_client.get_next_packet_size() {
                    Ok(Some(frames)) if frames > 0 => {
                        if capture_client
                            .read_from_device_to_deque(&mut deque)
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        }

        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }
        if deque.len() < CAPTURE_SAMPLE_RATE {
            continue;
        }
        let pcm_bytes: Vec<u8> = deque.drain(..).collect();
        if !has_audible_speech(&pcm_bytes) {
            continue;
        }
        if write_pcm16_mono_wav(wav_path, &pcm_bytes, CAPTURE_SAMPLE_RATE as u32).is_err() {
            continue;
        }

        let mut args = vec![
            "-m".to_string(),
            model.to_string_lossy().to_string(),
            "-f".to_string(),
            wav_path.to_string_lossy().to_string(),
            "-nt".to_string(),
            "-np".to_string(),
        ];
        if source_lang != "auto" && !source_lang.is_empty() {
            args.push("-l".to_string());
            args.push(source_lang.to_string());
        }

        if let Ok(out) = hidden_command(bin).args(&args).output() {
            if out.status.success() {
                let text = clean_whisper_output(&String::from_utf8_lossy(&out.stdout));
                if !text.is_empty() {
                    let translated = translate_text(&text, source_lang, target_lang);
                    emit_subtitle(
                        app,
                        &SubtitlePayload {
                            original: text,
                            translated,
                            target_lang: target_lang.to_string(),
                            active: true,
                        },
                    );
                }
            }
        }
    }

    let _ = audio_client.stop_stream();
    Ok(())
}

#[tauri::command]
pub fn get_translate_status(app: AppHandle) -> TranslateEngineStatus {
    build_status(&app)
}

#[tauri::command]
pub async fn download_translate_engine(app: AppHandle) -> Result<TranslateEngineStatus, String> {
    if DOWNLOADING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(build_status(&app));
    }

    let app_clone = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let dir = engine_dir(&app_clone);
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create engine dir: {e}"))?;

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .user_agent("Nexora/0.2")
            .build()
            .map_err(|e| format!("Failed to initialize HTTP client: {e}"))?;

        let zip_path = dir.join("whisper-bin-x64.zip");
        download_file_streaming(
            &client,
            WHISPER_ZIP_URL,
            &zip_path,
            5,
            35,
            "Downloading Whisper runtime...",
            &app_clone,
        )?;

        let _ = app_clone.emit(
            "translate-download-progress",
            DownloadProgressPayload {
                percent: 40,
                stage: "Extracting Whisper runtime...".to_string(),
            },
        );

        extract_zip_native(&zip_path, &dir)?;
        let _ = fs::remove_file(&zip_path);

        let model_file = model_path(&dir);
        download_file_streaming(
            &client,
            MODEL_URL,
            &model_file,
            45,
            98,
            "Downloading ggml-tiny speech model (~75 MB)...",
            &app_clone,
        )?;

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
pub fn remove_translate_engine(app: AppHandle) -> Result<TranslateEngineStatus, String> {
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
pub async fn start_live_translate(
    app: AppHandle,
    source_lang: String,
    target_lang: String,
    capture_mode: Option<String>,
    target_pid: Option<u32>,
    target_app_name: Option<String>,
) -> Result<TranslateEngineStatus, String> {
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
    let model = model_path(&dir);
    let whisper_bin = find_whisper_binary(&dir);

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
                    &bin,
                    &model,
                    &wav_path,
                    &source_lang,
                    &target_lang,
                    &resolved_mode,
                    target_pid,
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
                        },
                    );
                    let st = build_status(&app_worker);
                    emit_status_changed(&app_worker, &st);
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (
                    &bin,
                    &model,
                    &wav_path,
                    &app_worker,
                    &source_lang,
                    &target_lang,
                    &resolved_mode,
                    target_pid,
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
    let clean = text.trim().to_string();
    let tl = target_lang.clone();
    let translated =
        tauri::async_runtime::spawn_blocking(move || translate_text(&clean, "auto", &tl))
            .await
            .map_err(|e| e.to_string())?;

    let payload = SubtitlePayload {
        original: text,
        translated,
        target_lang,
        active: true,
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
                },
            );
            if let Some(win) = app_hide.get_webview_window("subtitle") {
                let _ = win.hide();
            }
        }
    });
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_google_translate_segments() {
        let sample =
            r#"[[["سلام دنیا","Hello world",null,null,1],["!","!",null,null,1]],null,"en"]"#;
        assert_eq!(
            parse_google_translate_response(sample),
            Some("سلام دنیا!".to_string())
        );
    }

    #[test]
    fn rejects_empty_or_invalid_translate_json() {
        assert_eq!(parse_google_translate_response("not json"), None);
        assert_eq!(parse_google_translate_response("[]"), None);
    }

    #[test]
    fn encodes_query_spaces_and_symbols() {
        assert_eq!(url_encode_query("Hello world!"), "Hello%20world%21");
    }

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
}
