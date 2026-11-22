//! Custom native Rust update engine for Nexora.
//!
//! Replaces `tauri-plugin-updater`'s Windows handoff (which can race with NSIS
//! `FindProcDLL` and abort with "Failed to kill Nexora") with a deterministic,
//! fully detached 3-stage pipeline:
//! 1. Fetch `update-feed.json` via `reqwest` and compare SemVer cleanly.
//! 2. Stream the NSIS per-user setup `.exe` into `%TEMP%` with real-time
//!    `update-progress` events and header validation.
//! 3. Spawn a hidden, detached Windows apply script (`CREATE_NO_WINDOW |
//!    DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`) that waits until
//!    `nexora.exe` has completely terminated and released all file locks,
//!    runs `setup.exe /P`, relaunches Nexora if needed, and deletes temp files.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

/// Rolling update feed published by `.github/scripts/publish.py`.
const UPDATE_FEED_URL: &str =
    "https://github.com/thaarfknight-star/nexora/releases/download/update-feed/update-feed.json";

/// Event emitted while an update downloads.
pub const PROGRESS_EVENT: &str = "update-progress";

/// Progress payload for the [`PROGRESS_EVENT`] event.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// What `check_updates` returns to the UI.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub update_available: bool,
    pub available_version: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateFeed {
    version: String,
    notes: Option<String>,
    platforms: HashMap<String, FeedPlatform>,
}

#[derive(Debug, Deserialize)]
struct FeedPlatform {
    url: String,
}

pub fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Parses a `major.minor.patch` (optional leading `v`) version tuple.
pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let clean = raw.trim().trim_start_matches(['v', 'V']);
    let core = clean.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

/// Returns `true` when `candidate` is strictly newer than `current`.
pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    match (parse_semver(current), parse_semver(candidate)) {
        (Some(cur), Some(cand)) => cand > cur,
        _ => false,
    }
}

/// Picks the best Windows installer URL from the feed (prefers per-user NSIS `.exe`).
fn pick_windows_installer_url(feed: &UpdateFeed) -> Option<String> {
    feed.platforms
        .get("windows-x86_64-nsis")
        .or_else(|| feed.platforms.get("windows-x86_64"))
        .or_else(|| feed.platforms.get("windows-x86_64-msi"))
        .map(|p| p.url.clone())
}

fn fetch_feed_blocking() -> Result<UpdateFeed, String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let url = format!("{UPDATE_FEED_URL}?t={ts}");
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .user_agent("Nexora-Updater/0.2")
        .build()
        .map_err(|e| format!("Failed to initialize updater HTTP client: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("Update check network error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Update feed returned HTTP {}", resp.status()));
    }
    let body = resp
        .text()
        .map_err(|e| format!("Failed reading update feed body: {e}"))?;
    serde_json::from_str::<UpdateFeed>(&body).map_err(|e| format!("Invalid update feed JSON: {e}"))
}

/// Check the update feed for a newer version.
pub async fn check(_app: AppHandle) -> Result<UpdateInfo, String> {
    let current = current_version();
    let feed = tauri::async_runtime::spawn_blocking(fetch_feed_blocking)
        .await
        .map_err(|e| format!("Update check task failed: {e}"))??;

    let newer =
        is_newer_version(&current, &feed.version) && pick_windows_installer_url(&feed).is_some();

    if newer {
        Ok(UpdateInfo {
            current_version: current,
            update_available: true,
            available_version: Some(feed.version),
            notes: feed.notes,
        })
    } else {
        Ok(UpdateInfo {
            current_version: current,
            update_available: false,
            available_version: None,
            notes: None,
        })
    }
}

/// Validates that the downloaded installer starts with a valid PE (`MZ`) or MSI header.
pub fn has_valid_installer_header(path: &Path, is_msi: bool) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() {
        return false;
    }
    if is_msi {
        magic == [0xD0, 0xCF, 0x11, 0xE0]
    } else {
        magic[0] == b'M' && magic[1] == b'Z'
    }
}

#[cfg(target_os = "windows")]
fn spawn_detached_windows_installer(installer_path: &Path, is_msi: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let pid = std::process::id();
    let current_exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let installer_ps = installer_path.to_string_lossy().replace('\'', "''");
    let exe_ps = current_exe.replace('\'', "''");

    let run_installer = if is_msi {
        format!(
            "Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', '{installer_ps}', '/passive', '/norestart') -Wait"
        )
    } else {
        format!("Start-Process -FilePath '{installer_ps}' -ArgumentList '/P' -Wait")
    };

    let relaunch = if exe_ps.is_empty() {
        String::new()
    } else {
        format!(
            "if (-not (Get-Process -Name 'nexora' -ErrorAction SilentlyContinue)) {{              if (Test-Path -LiteralPath '{exe_ps}') {{ Start-Process -FilePath '{exe_ps}' }}              }}"
        )
    };

    // Pure non-interactive PowerShell: uses kernel Wait-Process (no cmd.exe /
    // timeout.exe stdin blocking or Ctrl+C hangs), kills any lingering helper,
    // runs the passive installer, relaunches Nexora, and removes the temp file.
    let ps_script = format!(
        "$ErrorActionPreference = 'SilentlyContinue';          Wait-Process -Id {pid} -Timeout 8;          Get-Process -Name 'whisper-server','whisper-cli','llama-server','nexora' | Stop-Process -Force;          Start-Sleep -Milliseconds 600;          {run_installer};          Start-Sleep -Milliseconds 800;          {relaunch};          Remove-Item -LiteralPath '{installer_ps}' -Force"
    );

    // CREATE_NO_WINDOW (0x0800_0000) | DETACHED_PROCESS (0x0000_0008) |
    // CREATE_NEW_PROCESS_GROUP (0x0000_0200)
    const DETACHED_HIDDEN_FLAGS: u32 = 0x0800_0000 | 0x0000_0008 | 0x0000_0200;

    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &ps_script,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(DETACHED_HIDDEN_FLAGS)
        .spawn()
        .map_err(|e| format!("Failed to launch detached updater helper: {e}"))?;

    Ok(())
}

/// Download the latest Windows installer with live progress events, then hand
/// off cleanly to the detached updater script and exit Nexora immediately.
pub async fn download_and_install(app: AppHandle) -> Result<(), String> {
    let current = current_version();
    let app_dl = app.clone();

    let (installer_path, is_msi) = tauri::async_runtime::spawn_blocking(
        move || -> Result<(std::path::PathBuf, bool), String> {
            let feed = fetch_feed_blocking()?;
            if !is_newer_version(&current, &feed.version) {
                return Err("No update is available.".to_string());
            }
            let url = pick_windows_installer_url(&feed)
                .ok_or_else(|| "No Windows installer found in update feed.".to_string())?;
            let is_msi = url.to_ascii_lowercase().ends_with(".msi");
            let ext = if is_msi { "msi" } else { "exe" };
            let dest =
                std::env::temp_dir().join(format!("nexora-update-{}-setup.{ext}", feed.version));

            let client = reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(300))
                .user_agent("Nexora-Updater/0.2")
                .build()
                .map_err(|e| format!("HTTP client init failed: {e}"))?;

            let mut resp = client
                .get(&url)
                .send()
                .map_err(|e| format!("Failed to download update: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Update download returned HTTP {}", resp.status()));
            }

            let total = resp.content_length();
            let mut file = File::create(&dest)
                .map_err(|e| format!("Failed to create temp installer file: {e}"))?;
            let mut buf = [0u8; 64 * 1024];
            let mut downloaded: u64 = 0;

            let _ = app_dl.emit(
                PROGRESS_EVENT,
                UpdateProgress {
                    downloaded: 0,
                    total,
                },
            );

            loop {
                let n = resp
                    .read(&mut buf)
                    .map_err(|e| format!("Error reading update stream: {e}"))?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])
                    .map_err(|e| format!("Error writing update file: {e}"))?;
                downloaded = downloaded.saturating_add(n as u64);
                let _ = app_dl.emit(PROGRESS_EVENT, UpdateProgress { downloaded, total });
            }
            file.flush().map_err(|e| format!("Flush error: {e}"))?;
            drop(file);

            if !has_valid_installer_header(&dest, is_msi) {
                let _ = std::fs::remove_file(&dest);
                return Err("Downloaded update file is corrupted or incomplete.".to_string());
            }

            Ok((dest, is_msi))
        },
    )
    .await
    .map_err(|e| format!("Update download task failed: {e}"))??;

    // Stop Live Translate / whisper-server before exiting.
    crate::translator::shutdown_engine();

    #[cfg(target_os = "windows")]
    {
        spawn_detached_windows_installer(&installer_path, is_msi)?;
        crate::diagnostics::log(
            "updater",
            &format!("handed off to detached installer {:?}", installer_path),
        );
        app.cleanup_before_exit();
        std::process::exit(0);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (installer_path, is_msi);
        Err("Automatic installer handoff is supported on Windows only.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_versions_accurately() {
        assert!(is_newer_version("0.2.7", "0.2.8"));
        assert!(is_newer_version("0.2.7", "v0.3.0"));
        assert!(is_newer_version("0.2.9", "0.2.10"));
        assert!(!is_newer_version("0.2.8", "0.2.8"));
        assert!(!is_newer_version("0.2.8", "0.2.7"));
    }

    #[test]
    fn validates_pe_and_msi_headers() {
        let exe = std::env::temp_dir().join("nexora_test_mz.exe");
        std::fs::write(&exe, b"MZ\x90\x00test").expect("write exe");
        assert!(has_valid_installer_header(&exe, false));
        assert!(!has_valid_installer_header(&exe, true));
        let _ = std::fs::remove_file(&exe);
    }
}
