use crate::{island, media, settings, stats, taskbar, tray, updater, watchdog};

#[tauri::command]
pub fn get_island_state() -> island::IslandState {
    island::current_state()
}

#[tauri::command]
pub fn set_island_expanded(expanded: bool) -> island::IslandState {
    island::set_expanded(expanded)
}

#[tauri::command]
pub fn get_system_stats() -> stats::SystemStats {
    stats::collect()
}

#[tauri::command]
pub fn get_now_playing() -> media::MediaInfo {
    media::now_playing()
}

#[tauri::command]
pub fn get_taskbar_status() -> taskbar::TaskbarStatus {
    taskbar::status()
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
pub fn get_settings() -> settings::NexoraSettings {
    settings::defaults()
}

#[tauri::command]
pub async fn check_updates(app: tauri::AppHandle) -> Result<updater::UpdateInfo, String> {
    updater::check(app).await
}

#[tauri::command]
pub async fn download_and_install_update(app: tauri::AppHandle) -> Result<(), String> {
    updater::download_and_install(app).await
}
