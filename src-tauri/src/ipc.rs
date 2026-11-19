use tauri::Emitter;

use crate::{island, media, notifications, settings, stats, taskbar, tray, updater, watchdog};

#[tauri::command]
pub fn get_island_state() -> island::IslandState {
    island::current_state()
}

#[tauri::command]
pub fn set_island_expanded(
    window: tauri::Window,
    expanded: bool,
) -> Result<island::IslandState, String> {
    island::set_expanded_on_window(&window, expanded).map_err(|e| e.to_string())
}

/// Toggle click-through on the island window: with `ignore` the whole
/// fixed canvas lets clicks pass to the app below (including the pill
/// / panel), so the frontend turns it off while the island is visible
/// and back on when it hides to its handle.
#[tauri::command]
pub fn set_island_click_through(window: tauri::Window, ignore: bool) -> Result<(), String> {
    island::set_click_through(&window, ignore).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_system_stats() -> stats::SystemStats {
    stats::collect()
}

#[tauri::command]
pub async fn get_now_playing() -> media::MediaInfo {
    media::now_playing().await
}

#[tauri::command]
pub async fn media_play() -> bool {
    media::play().await
}

#[tauri::command]
pub async fn media_pause() -> bool {
    media::pause().await
}

#[tauri::command]
pub async fn media_next() -> bool {
    media::next().await
}

#[tauri::command]
pub async fn media_previous() -> bool {
    media::previous().await
}

#[tauri::command]
pub fn get_taskbar_status() -> taskbar::TaskbarStatus {
    taskbar::status()
}

#[tauri::command]
pub fn set_taskbar_hidden(hidden: bool) -> taskbar::TaskbarStatus {
    taskbar::set_hidden(hidden)
}

#[tauri::command]
pub fn get_watchdog_status() -> watchdog::WatchdogStatus {
    watchdog::check_health()
}

#[tauri::command]
pub fn get_tray_status() -> tray::TrayStatus {
    tray::status()
}

#[tauri::command]
pub fn get_notifications() -> Vec<notifications::Notification> {
    notifications::list()
}

#[tauri::command]
pub fn clear_notifications() {
    notifications::clear();
}

#[tauri::command]
pub fn dismiss_notification(id: String) -> bool {
    notifications::dismiss(&id)
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> settings::NexoraSettings {
    settings::load(&app)
}

#[tauri::command]
pub fn set_settings(
    app: tauri::AppHandle,
    settings: settings::NexoraSettings,
) -> Result<settings::NexoraSettings, String> {
    let stored = settings::update(&app, settings).map_err(|e| e.to_string())?;
    // Tell every window the settings changed, so the island applies the
    // new theme at once instead of waiting for a restart.
    let _ = app.emit("settings-changed", &stored);
    Ok(stored)
}

#[tauri::command]
pub fn open_settings(app: tauri::AppHandle) {
    crate::open_settings_window(&app);
}

#[tauri::command]
pub async fn check_updates(app: tauri::AppHandle) -> Result<updater::UpdateInfo, String> {
    let info = updater::check(app).await?;
    if info.update_available {
        if let Some(version) = &info.available_version {
            crate::notifications::add(
                &format!("Update available: {version}"),
                "A new Nexora version is ready to download.",
                crate::notifications::KIND_UPDATE,
            );
        }
    }
    Ok(info)
}

#[tauri::command]
pub async fn download_and_install_update(app: tauri::AppHandle) -> Result<(), String> {
    updater::download_and_install(app).await
}
