//! Update engine — Tauri v2 updater plugin, wired to the rolling
//! update-feed release.
//!
//! Flow: [`check`] reads the feed from the endpoints in
//! `tauri.conf.json` (`plugins.updater.endpoints`) and compares versions;
//! [`download_and_install`] downloads the signed bundle (the signature is
//! verified against `plugins.updater.pubkey`), installs it and restarts
//! the app. Download progress is emitted on the [`PROGRESS_EVENT`] event
//! for the UI.
//!
//! Signing: the public key lives in `tauri.conf.json`
//! (`plugins.updater.pubkey`) as the base64-encoded minisign public key
//! (the two-line `.pub` file contents, encoded — the plugin decodes it
//! before verifying); the private key is only the
//! `TAURI_SIGNING_PRIVATE_KEY` GitHub secret (rotated 2026-11-18 by
//! DEVagent — agents never handle the private key).

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

/// Check the update feed for a newer version. `Ok` with
/// `update_available: false` means the app is current; `Err` only means
/// the check itself could not run (no network, no feed, updater not
/// configured).
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

/// Download the available update, verify it, install it and restart the
/// app. Emits [`PROGRESS_EVENT`] events while downloading.
pub async fn download_and_install(app: AppHandle) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|e| format!("Updater is not configured: {e}"))?
        .check()
        .await
        .map_err(|e| format!("Update check failed: {e}"))?
        .ok_or_else(|| "No update is available.".to_string())?;
    let mut downloaded: u64 = 0;
    let progress_app = app.clone();
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
            || {},
        )
        .await
        .map_err(|e| format!("Update download/install failed: {e}"))?;
    #[cfg(target_os = "windows")]
    {
        app.exit(0);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        app.restart()
    }
}
