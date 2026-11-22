//! Official Tauri v2 update engine (`tauri-plugin-updater`), matching ReChat's
//! 3-part architecture: signed GitHub Releases + rolling `update-feed.json` +
//! minisign-verified passive install.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

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

pub fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Check the rolling `update-feed.json` via `tauri-plugin-updater`.
pub async fn check(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = current_version();
    let update = app
        .updater()
        .map_err(|e| format!("Updater is not configured: {e}"))?
        .check()
        .await
        .map_err(|e| format!("Update check failed: {e}"))?;
    match update {
        Some(update) => Ok(UpdateInfo {
            current_version: current,
            update_available: true,
            available_version: Some(update.version.clone()),
            notes: update.body.clone(),
        }),
        None => Ok(UpdateInfo {
            current_version: current,
            update_available: false,
            available_version: None,
            notes: None,
        }),
    }
}

/// Download, verify minisign signature, and install via `tauri-plugin-updater`
/// in passive mode, shutting down any live-translate helper first.
pub async fn download_and_install(app: AppHandle) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|e| format!("Updater is not configured: {e}"))?
        .check()
        .await
        .map_err(|e| format!("Update check failed: {e}"))?
        .ok_or_else(|| "No update is available.".to_string())?;

    crate::translator::shutdown_engine();

    let mut downloaded: u64 = 0;
    let progress_app = app.clone();
    let exit_app = app.clone();
    update
        .download_and_install(
            move |chunk_length, content_length| {
                downloaded += chunk_length as u64;
                let _ = progress_app.emit(
                    PROGRESS_EVENT,
                    UpdateProgress {
                        downloaded,
                        total: content_length,
                    },
                );
            },
            move || {
                exit_app.cleanup_before_exit();
                std::process::exit(0);
            },
        )
        .await
        .map_err(|e| format!("Update download/install failed: {e}"))?;
    app.restart()
}
