use tauri::{AppHandle, Manager};

pub mod ipc;
pub mod island;
pub mod media;
pub mod notifications;
pub mod settings;
pub mod stats;
pub mod taskbar;
pub mod taskbar_content;
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
            // Notifications: wire the emitter once, so backend producers
            // (updater, watchdog) can add notifications from anywhere.
            crate::notifications::set_emitter(app.handle());
            // Settings: load the persisted settings and apply the values
            // the backend owns (currently: always_on_top on the main
            // window). The theme is applied by the frontend, which reads
            // it through `get_settings`.
            let settings = settings::load(app.handle());
            settings::apply(app.handle(), &settings);
            // Island: place the window at its startup position (fixed
            // size, top-center). Skipped when the main window is missing.
            if let Some(webview_window) = app.get_webview_window("main") {
                let window = webview_window.as_ref().window();
                island::setup_window(&window)?;
                // AppBar: hand the island window handle to the taskbar
                // module so hide() can register it with SHAppBarMessage
                // (maximized windows stay below the island).
                // Windows-only; when the handle is unknown hide() still
                // works, just without the reservation.
                #[cfg(target_os = "windows")]
                if let Ok(hwnd) = window.hwnd() {
                    taskbar::set_appbar_window(hwnd.0 as isize);
                }
            }
            // Taskbar: hide it at startup when the user asked for it.
            // hide() registers the island as a top AppBar and starts the
            // monitor that re-hides the taskbar if Windows shows it
            // again. The RunEvent handler restores it on exit, and the
            // watchdog covers a crash. No-op off Windows.
            if settings.auto_hide_taskbar {
                taskbar::set_hidden(true);
            }
            // Island hot zone: a background thread watches the cursor
            // and emits `island-hot-zone` when it enters / leaves the
            // top edge, so the frontend can wake the island from hidden.
            // Windows-only in effect; off Windows it never fires.
            island::start_hot_zone_watcher(app.handle());
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
            ipc::set_island_click_through,
            ipc::get_system_stats,
            ipc::get_now_playing,
            ipc::media_play,
            ipc::media_pause,
            ipc::media_next,
            ipc::media_previous,
            ipc::get_taskbar_status,
            ipc::set_taskbar_hidden,
            ipc::get_taskbar_windows,
            ipc::get_pinned_apps,
            ipc::activate_taskbar_window,
            ipc::open_start_menu,
            ipc::launch_pinned_app,
            ipc::get_watchdog_status,
            ipc::get_tray_status,
            ipc::get_settings,
            ipc::set_settings,
            ipc::open_settings,
            ipc::check_updates,
            ipc::download_and_install_update,
            ipc::get_notifications,
            ipc::clear_notifications,
            ipc::dismiss_notification,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Nexora Tauri application")
        .run(|_app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                // Clean exit: drop the AppBar reservation first (otherwise
                // the work-area reservation outlives Nexora until Explorer
                // restarts), then restore the taskbar synchronously, then
                // stop the external watchdog (it would restore anyway).
                taskbar::unregister_appbar();
                watchdog::restore_taskbar();
                watchdog::stop_watchdog();
            }
        });
}
