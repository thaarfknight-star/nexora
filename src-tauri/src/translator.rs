//! Optional downloadable real-time audio-to-subtitle translation engine.
//!
//! The engine is downloaded on demand into `<app_data_dir>/live-translate/`
//! so the base Nexora installer stays lightweight. When started, it captures
//! short audio chunks on Windows, transcribes them locally with `whisper-cli`,
//! translates into the chosen target language, and broadcasts `subtitle-update`
//! events to the click-through bottom-center `"subtitle"` overlay window.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

const MODEL_FILENAME: &str = "ggml-tiny.bin";
const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";
const WHISPER_ZIP_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/latest/download/whisper-bin-x64.zip";

const SUBTITLE_WIDTH: f64 = 820.0;
const SUBTITLE_HEIGHT: f64 = 128.0;
const SUBTITLE_BOTTOM_MARGIN: f64 = 72.0;

static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

static ENGINE_STATE: Mutex<EnginePrefs> = Mutex::new(EnginePrefs {
    source_lang: None,
    target_lang: None,
    last_error: None,
});

struct EnginePrefs {
    source_lang: Option<String>,
    target_lang: Option<String>,
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
    pub model_path: String,
    pub last_error: Option<String>,
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
        model_path: dir.to_string_lossy().to_string(),
        last_error: prefs.last_error.clone(),
    }
}

/// Positions the bottom-center floating subtitle overlay on the primary monitor
/// and marks it click-through so it never intercepts mouse clicks in games/videos.
pub fn position_subtitle_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("subtitle") {
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

/// Translates `text` into `target_lang` using curl.exe (built into Windows 10/11).
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
    let output = std::process::Command::new("curl")
        .args(["-sS", "--max-time", "6", &url])
        .output();
    if let Ok(out) = output {
        if out.status.success() {
            let body = String::from_utf8_lossy(&out.stdout);
            if let Some(translated) = parse_google_translate_response(&body) {
                return translated;
            }
        }
    }
    clean.to_string()
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

        let _ = app_clone.emit(
            "translate-download-progress",
            DownloadProgressPayload {
                percent: 10,
                stage: "Downloading Whisper runtime...".to_string(),
            },
        );

        let zip_path = dir.join("whisper-bin-x64.zip");
        let curl_bin = std::process::Command::new("curl")
            .args([
                "-L",
                "-f",
                "-sS",
                "-o",
                &zip_path.to_string_lossy(),
                WHISPER_ZIP_URL,
            ])
            .status()
            .map_err(|e| format!("Failed to launch curl: {e}"))?;

        if !curl_bin.success() {
            return Err("Failed to download whisper runtime archive".to_string());
        }

        let _ = app_clone.emit(
            "translate-download-progress",
            DownloadProgressPayload {
                percent: 40,
                stage: "Extracting Whisper runtime...".to_string(),
            },
        );

        #[cfg(target_os = "windows")]
        {
            let ps_cmd = format!(
                "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                zip_path.to_string_lossy(),
                dir.to_string_lossy()
            );
            let extract = std::process::Command::new("powershell")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_cmd])
                .status()
                .map_err(|e| format!("Failed to extract archive: {e}"))?;
            if !extract.success() {
                return Err("Failed to extract Whisper runtime zip".to_string());
            }
        }
        let _ = fs::remove_file(&zip_path);

        let _ = app_clone.emit(
            "translate-download-progress",
            DownloadProgressPayload {
                percent: 55,
                stage: "Downloading ggml-tiny speech model (~75 MB)...".to_string(),
            },
        );

        let model_file = model_path(&dir);
        let curl_model = std::process::Command::new("curl")
            .args([
                "-L",
                "-f",
                "-sS",
                "-o",
                &model_file.to_string_lossy(),
                MODEL_URL,
            ])
            .status()
            .map_err(|e| format!("Failed to download speech model: {e}"))?;

        if !curl_model.success() {
            return Err("Failed to download ggml-tiny speech model".to_string());
        }

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
    result?;
    Ok(build_status(&app))
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
    Ok(build_status(&app))
}

#[tauri::command]
pub fn start_live_translate(
    app: AppHandle,
    source_lang: String,
    target_lang: String,
) -> Result<TranslateEngineStatus, String> {
    {
        let mut prefs = ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner());
        prefs.source_lang = Some(source_lang.clone());
        prefs.target_lang = Some(target_lang.clone());
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

    emit_subtitle(
        &app,
        &SubtitlePayload {
            original: String::new(),
            translated: if target_lang == "fa" {
                "موتور ترجمه زنده فعال شد — در حال گوش دادن به صدا...".to_string()
            } else {
                "Live Translate active — listening to audio...".to_string()
            },
            target_lang: target_lang.clone(),
            active: true,
        },
    );

    if let Some(bin) = whisper_bin {
        let app_worker = app.clone();
        std::thread::spawn(move || {
            let wav_path = dir.join("chunk.wav");
            while RUNNING.load(Ordering::SeqCst) {
                #[cfg(target_os = "windows")]
                {
                    let recorded = record_windows_audio_chunk(&wav_path);
                    if !RUNNING.load(Ordering::SeqCst) {
                        break;
                    }
                    if recorded {
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
                            args.push(source_lang.clone());
                        }
                        if let Ok(out) = std::process::Command::new(&bin).args(&args).output() {
                            if out.status.success() {
                                let text = String::from_utf8_lossy(&out.stdout)
                                    .lines()
                                    .map(str::trim)
                                    .filter(|l| !l.is_empty() && !l.starts_with('['))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                if !text.is_empty() {
                                    let translated =
                                        translate_text(&text, &source_lang, &target_lang);
                                    emit_subtitle(
                                        &app_worker,
                                        &SubtitlePayload {
                                            original: text,
                                            translated,
                                            target_lang: target_lang.clone(),
                                            active: true,
                                        },
                                    );
                                }
                            }
                        }
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(800));
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = (&bin, &model, &wav_path, &app_worker);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        });
    }

    Ok(build_status(&app))
}

#[cfg(target_os = "windows")]
fn record_windows_audio_chunk(wav_path: &Path) -> bool {
    let out_file = wav_path.to_string_lossy();
    let lines = [
        "$ErrorActionPreference='SilentlyContinue'",
        r#"Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;using System.Text;public class WinMM{[DllImport("winmm.dll",CharSet=CharSet.Auto)]public static extern int mciSendString(string c,StringBuilder r,int l,IntPtr cb);}'"#,
        "[WinMM]::mciSendString('open new Type waveaudio Alias nxrec',$null,0,[IntPtr]::Zero)|Out-Null",
        "[WinMM]::mciSendString('set nxrec bitspersample 16 channels 1 samplespersec 16000',$null,0,[IntPtr]::Zero)|Out-Null",
        "[WinMM]::mciSendString('record nxrec',$null,0,[IntPtr]::Zero)|Out-Null",
        "Start-Sleep -Milliseconds 3500",
        &format!("[WinMM]::mciSendString('save nxrec "{out_file}"',$null,0,[IntPtr]::Zero)|Out-Null"),
        "[WinMM]::mciSendString('close nxrec',$null,0,[IntPtr]::Zero)|Out-Null",
    ];
    let script = lines.join("; ");
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .status()
        .map(|s| s.success() && wav_path.exists())
        .unwrap_or(false)
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
    build_status(&app)
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
}
