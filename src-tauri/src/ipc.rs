use tauri::Emitter;

use crate::{
    island, media, notifications, settings, stats, taskbar, taskbar_content, tray, updater,
    watchdog,
};

/// Only `"main"` and `"settings"` windows may invoke privileged commands
/// (launcher, taskbar restore, updater, engine management).
pub fn is_privileged_window(label: &str) -> bool {
    label == "main" || label == "settings"
}

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
    island::record_ui_heartbeat();
    island::set_click_through(&window, ignore).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn island_heartbeat() {
    island::record_ui_heartbeat();
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
pub async fn media_stop() -> bool {
    media::stop().await
}

#[tauri::command]
pub fn get_taskbar_status() -> taskbar::TaskbarStatus {
    taskbar::status()
}

#[tauri::command]
pub fn set_taskbar_hidden(_hidden: bool) -> taskbar::TaskbarStatus {
    taskbar::status()
}

/// All open windows for the compact bar, in Z-order (frontmost first).
/// Icons are raw RGBA pixels as base64 (32x32, row-major top-down) —
/// not PNG data-URLs; `icon_size` is 0 when a window has no icon.
#[tauri::command]
pub fn get_taskbar_windows() -> Vec<taskbar_content::TaskbarWindow> {
    taskbar_content::list_windows()
}

/// All apps pinned to the user's taskbar, in file-name order. Same raw
/// RGBA icon format as `get_taskbar_windows`.
#[tauri::command]
pub fn get_pinned_apps() -> Vec<taskbar_content::PinnedApp> {
    taskbar_content::list_pinned()
}

/// Click on a taskbar window: minimize it when it is the foreground
/// window, otherwise bring it to the front. `id` is the `id` of a
/// `TaskbarWindow` from `get_taskbar_windows`.
#[tauri::command]
pub fn activate_taskbar_window(id: i64) -> bool {
    taskbar_content::activate_window(id)
}

/// Press the Windows key, which opens (or closes) the Start menu.
#[tauri::command]
pub fn open_start_menu() -> bool {
    taskbar_content::open_start_menu()
}

/// Launch a pinned app by its resolved target path. `true` means the
/// launch was invoked; `false` means it was not (empty path or error).
#[tauri::command]
pub fn launch_pinned_app(window: tauri::Window, target: String) -> bool {
    if !is_privileged_window(window.label()) {
        return false;
    }
    taskbar_content::launch_pinned_app(&target)
}

/// Show the taskbar again right now — the button the onboarding page
/// promises. Thin wrapper over `watchdog::restore_taskbar()`; `false`
/// off Windows, like the other taskbar commands.
#[tauri::command]
pub fn restore_taskbar_now(window: tauri::Window) -> bool {
    if !is_privileged_window(window.label()) {
        return false;
    }
    watchdog::restore_taskbar()
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
pub async fn check_updates(
    window: tauri::Window,
    app: tauri::AppHandle,
) -> Result<updater::UpdateInfo, String> {
    if !is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
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
pub fn prepare_for_update_exit(window: tauri::Window) -> Result<(), String> {
    if !is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    updater::prepare_for_update_exit("js-plugin-updater");
    Ok(())
}

#[tauri::command]
pub async fn download_and_install_update(
    window: tauri::Window,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !is_privileged_window(window.label()) {
        return Err("Unauthorized window".to_string());
    }
    updater::download_and_install(app).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn privileged_window_allows_only_main_and_settings() {
        assert!(super::is_privileged_window("main"));
        assert!(super::is_privileged_window("settings"));
        assert!(!super::is_privileged_window("subtitle"));
        assert!(!super::is_privileged_window("unknown"));
    }
}
