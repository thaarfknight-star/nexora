pub mod ipc;
pub mod island;
pub mod media;
pub mod settings;
pub mod stats;
pub mod taskbar;
pub mod tray;
pub mod updater;
pub mod watchdog;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // Watchdog: a separate process watches Nexora and restores the
            // taskbar if Nexora crashes or is killed; a monitor thread also
            // watches explorer.exe while Nexora runs. See watchdog.rs.
            // NOTE: taskbar hide/restore hooks (feat/taskbar-hide-restore)
            // extend this same setup closure; do not add a second one.
            watchdog::start_watchdog();
            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            ipc::get_island_state,
            ipc::set_island_expanded,
            ipc::get_system_stats,
            ipc::get_now_playing,
            ipc::get_taskbar_status,
            ipc::get_watchdog_status,
            ipc::get_tray_status,
            ipc::get_settings,
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
