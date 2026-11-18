use tauri::{AppHandle, Manager};

pub mod ipc;
pub mod island;
pub mod media;
pub mod settings;
pub mod stats;
pub mod taskbar;
pub mod tray;
pub mod updater;
pub mod watchdog;

/// Shows the Settings window (defined in `tauri.conf.json` with label
/// `"settings"`; created hidden at startup) and focuses it. Called by
/// the `open_settings` command and by the tray menu item.
pub fn open_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // NOTE: every setup hook (settings, taskbar, island,
            // watchdog, tray) lives in this one closure; do not
            // add a second `.setup()`.
            // Settings: load the persisted settings and apply the values
            // the backend owns (currently: always_on_top on the main
            // window). The theme is applied by the frontend, which reads
            // it through `get_settings`.
            let settings = settings::load(app.handle());
            settings::apply(app.handle(), &settings);
            // Taskbar: hide it at startup when the user asked for it.
            // The RunEvent handler restores it on exit, and the watchdog
            // covers a crash. No-op off Windows.
            if settings.auto_hide_taskbar {
                taskbar::set_hidden(true);
            }
            // Island: place the window at its startup position (compact
            // size, top-center). Skipped when the main window is missing.
            if let Some(webview_window) = app.get_webview_window("main") {
                let window = webview_window.as_ref().window();
                island::setup_window(&window)?;
            }
            // Watchdog: a separate process watches Nexora and restores the
            // taskbar if Nexora crashes or is killed; a monitor thread also
            // watches explorer.exe while Nexora runs. See watchdog.rs.
            watchdog::start_watchdog();
            tray::setup(app.handle())?;
            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            ipc::get_island_state,
            ipc::set_island_expanded,
            ipc::get_system_stats,
            ipc::get_now_playing,
            ipc::media_play,
            ipc::media_pause,
            ipc::media_next,
            ipc::media_previous,
            ipc::get_taskbar_status,
            ipc::set_taskbar_hidden,
            ipc::get_watchdog_status,
            ipc::get_tray_status,
            ipc::get_settings,
            ipc::set_settings,
            ipc::open_settings,
            ipc::check_updates,
            ipc::download_and_install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Nexora Tauri application")
        .run(|_app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                // Clean exit: restore the taskbar synchronously first, then
                // stop the external watchdog (it would restore anyway).
                watchdog::restore_taskbar();
                watchdog::stop_watchdog();
            }
        });
}
