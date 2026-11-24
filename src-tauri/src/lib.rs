use tauri::{AppHandle, Manager};

pub mod diagnostics;
pub mod hotkey;
pub mod ipc;
pub mod island;
pub mod media;
pub mod nexora_mt;
pub mod notifications;
pub mod settings;
pub mod stats;
pub mod taskbar;
pub mod taskbar_content;
pub mod translate_provider;
pub mod translate_secrets;
pub mod dubbing;
mod translator;
pub mod tray;
pub mod updater;
pub mod vibrancy;
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

/// The setup steps themselves, extracted from the `.setup()`
/// closure so its error path can log and show a message box.
/// Every step leaves one line in the diagnostics log, so a failed
/// startup shows exactly how far it got. The hotkey registration
/// result is NOT logged here: TAgent5 wires that outcome with the
/// same diagnostics log function after this module merges — only
/// the request is logged here. The taskbar is not hidden at startup
/// at all (deferred until the island is shown, TAgent1's
/// `notify_island_shown` command): only the deferral is logged
/// here.
// NOTE: every setup hook (settings, taskbar, island, watchdog,
// tray) lives in this one function; do not add a second `.setup()`.
fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Notifications: wire the emitter once, so backend producers
    // (updater, watchdog) can add notifications from anywhere.
    crate::notifications::set_emitter(app.handle());
    // Stats: point nexora-stats.log at the app data directory
    // before the first poll, so every get_system_stats poll
    // leaves one evidence line (see stats.rs). Silent when the
    // directory cannot be resolved.
    if let Ok(dir) = app.handle().path().app_data_dir() {
        stats::set_log_dir(dir);
    }
    // Settings: load the persisted settings and apply the values
    // the backend owns (currently: always_on_top on the main
    // window). The theme is applied by the frontend, which reads
    // it through `get_settings`.
    let settings = settings::load(app.handle());
    settings::apply(app.handle(), &settings);
    crate::diagnostics::log(
        "setup",
        &format!(
            "settings loaded (auto_hide_taskbar={})",
            settings.auto_hide_taskbar
        ),
    );
    // Island: place the window at its startup position (fixed
    // size, top-center). Skipped when the main window is missing.
    if let Some(webview_window) = app.get_webview_window("main") {
        let window = webview_window.as_ref().window();
        island::setup_window(&window)?;
        crate::diagnostics::log("setup", "island window placed");
        // Island handle: hand the island window handle to the
        // taskbar module so the window list can recognise (and
        // exclude) the island itself. Windows-only; when the
        // handle is unknown the island just shows up in the list.
        #[cfg(target_os = "windows")]
        if let Ok(hwnd) = window.hwnd() {
            taskbar::set_island_window(hwnd.0 as isize);
        }
    }
    // Taskbar: do NOT hide at startup. The taskbar stays
    // visible until the island has been shown at least once
    // with its taskbar content (Taha's spec after the 18:53
    // build test): the frontend calls the
    // `notify_island_shown` command once the island first
    // renders, and only then — when the user asked for it —
    // the taskbar is hidden. Record the intent here; the
    // command performs the hide and logs the outcome to the
    // island anchor log. The RunEvent handler restores it on
    // exit, and the watchdog covers a crash. No-op off
    // Windows.
    taskbar::set_hide_intent(false);
    crate::diagnostics::log(
        "setup",
        &format!(
            "taskbar hide deferred until island shown (intended={})",
            settings.auto_hide_taskbar
        ),
    );
    // Island hot zone: a background thread watches the cursor
    // and emits `island-hot-zone` when it enters / leaves the
    // top edge, so the frontend can wake the island from hidden.
    // Windows-only in effect; off Windows it never fires.
    island::start_hot_zone_watcher(app.handle());
    crate::diagnostics::log("setup", "hot-zone watcher started");
    // Island hotkey (default Alt+Space, Taha's choice): a global
    // shortcut that toggles the island through the frontend
    // state machine. The key comes from settings, so the user
    // can change it with `set_island_hotkey`. Registration
    // failing (e.g. another app owns the key) is not fatal —
    // the island still works through the hot zone.
    crate::diagnostics::log(
        "setup",
        &format!("hotkey registration requested: {}", settings.island_hotkey),
    );
    if let Err(e) = hotkey::register_island_hotkey(app.handle(), &settings.island_hotkey) {
        eprintln!("Nexora: failed to register island hotkey: {e}");
    }
    // Taskbar hiding and watchdog are disabled; Nexora runs in floating companion mode.
    crate::diagnostics::log("setup", "floating companion mode (taskbar untouched)");
    tray::setup(app.handle())?;
    crate::diagnostics::log("setup", "tray setup ok");
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Diagnostics first: the log (app data + %TEMP%) and the
            // panic hook are up before any other setup step, so even
            // a failure in the very first step leaves evidence. A
            // setup error is also shown in a message box instead of
            // exiting silently.
            crate::diagnostics::init(app.handle());
            crate::diagnostics::install_panic_hook();
            crate::diagnostics::log("setup", "starting");
            match setup_app(app) {
                Ok(()) => {
                    crate::diagnostics::log("setup", "ok");
                    Ok(())
                }
                Err(e) => {
                    let message = format!("Nexora failed to start: {e}");
                    crate::diagnostics::log("setup", &format!("error: {e}"));
                    crate::diagnostics::show_setup_error(&message);
                    Err(e)
                }
            }
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::ScaleFactorChanged { .. } = event {
                    island::reapply_anchor_on_window(window);
                }
            }
        })
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            ipc::get_island_state,
            ipc::set_island_expanded,
            ipc::set_island_click_through,
            ipc::island_heartbeat,
            ipc::get_system_stats,
            ipc::get_now_playing,
            ipc::media_play,
            ipc::media_pause,
            ipc::media_next,
            ipc::media_previous,
            ipc::media_stop,
            ipc::get_taskbar_status,
            ipc::set_taskbar_hidden,
            taskbar::notify_island_shown,
            ipc::get_taskbar_windows,
            ipc::get_pinned_apps,
            ipc::activate_taskbar_window,
            ipc::open_start_menu,
            ipc::launch_pinned_app,
            ipc::restore_taskbar_now,
            ipc::get_watchdog_status,
            ipc::get_tray_status,
            ipc::get_settings,
            ipc::set_settings,
            ipc::open_settings,
            hotkey::set_island_hotkey,
            ipc::check_updates,
            ipc::prepare_for_update_exit,
            ipc::download_and_install_update,
            ipc::get_notifications,
            ipc::clear_notifications,
            ipc::dismiss_notification,
            diagnostics::log_visibility_transition,
            translator::get_translate_status,
            translator::list_audio_sources,
            translator::download_translate_engine,
            translator::download_local_translate_model,
            translator::remove_local_translate_model,
            translator::remove_translate_engine,
            translator::start_live_translate,
            translator::stop_live_translate,
            translator::test_subtitle_overlay,
            dubbing::get_dubbing_status,
            dubbing::download_dubbing_engine,
            dubbing::remove_dubbing_engine,
            dubbing::start_live_dubbing,
            dubbing::stop_live_dubbing,
            dubbing::test_dubbing_voice,
            translator::set_translate_api_key,
            translator::delete_translate_api_key,
            translator::get_translate_api_key_status,
            translator::test_translate_provider,
            translator::evaluate_translation_samples,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Nexora Tauri application")
        .run(|app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                // Restore taskbar first (if ever hidden) before tearing down helpers.
                if taskbar::hidden_by_nexora() {
                    taskbar::restore();
                }
                watchdog::stop_watchdog();
                translator::shutdown_engine();
                {
                    use tauri_plugin_global_shortcut::GlobalShortcutExt;
                    let _ = app_handle.global_shortcut().unregister_all();
                }
                // A clean exit leaves this line: when the next
                // install's log just stops, the process died
                // instead of exiting.
                crate::diagnostics::log("shutdown", "exit requested");
            }
        });
}
