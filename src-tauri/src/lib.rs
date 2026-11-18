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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Nexora Tauri application");
}
