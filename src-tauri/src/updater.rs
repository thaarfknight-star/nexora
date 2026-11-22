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
                prepare_for_update_exit("updater-on-before-exit");
                exit_app.cleanup_before_exit();
                std::process::exit(0);
            },
        )
        .await
        .map_err(|e| format!("Update download/install failed: {e}"))?;
    app.restart()
}

/// Canonical pre-exit cleanup stage order before installer handoff or shutdown.
pub const EXIT_CLEANUP_STAGES: [&str; 4] = [
    "restore_taskbar",
    "stop_watchdog",
    "shutdown_translator",
    "exit",
];

/// Runs the deterministic pre-exit sequence before an installer runs:
/// 1. Restore taskbar (always first for desktop safety)
/// 2. Stop & reap watchdog child (2 s bounded wait)
/// 3. Stop Live Translate & kill any `whisper-server.exe` child synchronously
/// 4. Log each stage with elapsed ms to `diagnostics.rs`
pub fn prepare_for_update_exit(reason: &str) {
    let t0 = std::time::Instant::now();
    crate::diagnostics::log("updater-exit", &format!("start reason={reason}"));

    let restored = crate::watchdog::restore_taskbar();
    crate::diagnostics::log(
        "updater-exit",
        &format!(
            "stage={} restored={restored} elapsed_ms={}",
            EXIT_CLEANUP_STAGES[0],
            t0.elapsed().as_millis()
        ),
    );

    crate::watchdog::stop_watchdog_with_timeout(std::time::Duration::from_secs(2));
    crate::diagnostics::log(
        "updater-exit",
        &format!(
            "stage={} elapsed_ms={}",
            EXIT_CLEANUP_STAGES[1],
            t0.elapsed().as_millis()
        ),
    );

    crate::translator::shutdown_engine();
    crate::diagnostics::log(
        "updater-exit",
        &format!(
            "stage={} elapsed_ms={}",
            EXIT_CLEANUP_STAGES[2],
            t0.elapsed().as_millis()
        ),
    );

    crate::diagnostics::log(
        "updater-exit",
        &format!(
            "stage={} elapsed_ms={}",
            EXIT_CLEANUP_STAGES[3],
            t0.elapsed().as_millis()
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_cleanup_stages_keep_taskbar_restore_first_and_exit_last() {
        assert_eq!(EXIT_CLEANUP_STAGES[0], "restore_taskbar");
        assert_eq!(EXIT_CLEANUP_STAGES[1], "stop_watchdog");
        assert_eq!(EXIT_CLEANUP_STAGES[2], "shutdown_translator");
        assert_eq!(EXIT_CLEANUP_STAGES[3], "exit");
    }
}
