use crate::{island, media, settings, stats, taskbar, tray, updater, watchdog};

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
pub fn get_settings(app: tauri::AppHandle) -> settings::NexoraSettings {
    settings::load(&app)
}

#[tauri::command]
pub fn set_settings(
    app: tauri::AppHandle,
    settings: settings::NexoraSettings,
) -> Result<settings::NexoraSettings, String> {
    settings::update(&app, settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_updates(app: tauri::AppHandle) -> Result<updater::UpdateInfo, String> {
    updater::check(app).await
}

#[tauri::command]
pub async fn download_and_install_update(app: tauri::AppHandle) -> Result<(), String> {
    updater::download_and_install(app).await
}
