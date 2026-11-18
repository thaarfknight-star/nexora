use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuEvent},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

/// Menu item ids.
pub const MENU_SHOW_HIDE: &str = "show_hide";
pub const MENU_QUIT: &str = "quit";

/// Number of entries in the tray menu (toggle, separator, quit).
pub const MENU_ITEM_COUNT: usize = 3;

static TRAY_BUILT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct TrayStatus {
    pub active: bool,
    pub items: usize,
}

/// Real status: `active` is true only once the tray icon was actually
/// built in this process; `items` is the menu entry count (0 before that).
pub fn status() -> TrayStatus {
    let active = TRAY_BUILT.load(Ordering::SeqCst);
    TrayStatus {
        active,
        items: if active { MENU_ITEM_COUNT } else { 0 },
    }
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            Ok(false) => {
                let _ = window.show();
                let _ = window.set_focus();
            }
            Err(_) => {
                let _ = window.show();
            }
        }
    }
}

/// Restores the Windows taskbar by showing `Shell_TrayWnd` again.
/// Raw Win32 FFI on purpose: no extra crate is needed for two calls.
/// On other platforms this is a no-op (Nexora targets Windows).
#[cfg(windows)]
pub fn restore_taskbar() {
    use std::os::raw::{c_int, c_void};

    type Hwnd = *mut c_void;
    const SW_SHOW: c_int = 5;

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(lp_class_name: *const u16, lp_window_name: *const u16) -> Hwnd;
        fn ShowWindow(hwnd: Hwnd, n_cmd_show: c_int) -> c_int;
    }

    let class: Vec<u16> = "Shell_TrayWnd"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_SHOW);
        }
    }
}

/// Restores the Windows taskbar by showing `Shell_TrayWnd` again.
/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub fn restore_taskbar() {}

/// Quits Nexora, restoring the taskbar first so the desktop is never
/// left without a taskbar.
pub fn quit(app: &AppHandle) {
    restore_taskbar();
    app.exit(0);
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().0.as_str() {
        MENU_SHOW_HIDE => toggle_main_window(app),
        MENU_QUIT => quit(app),
        _ => {}
    }
}

/// Builds the real tray icon and menu. Called once from the `setup`
/// hook in `lib.rs`.
/// - Left-click toggles the island window; right-click opens the menu.
/// - "Show / Hide Nexora" toggles the island window.
/// - "Quit Nexora" restores the taskbar and exits.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(MENU_SHOW_HIDE, "Show / Hide Nexora")
        .separator()
        .text(MENU_QUIT, "Quit Nexora")
        .build()?;
    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    TrayIconBuilder::new()
        .menu(&menu)
        .icon(icon)
        .tooltip("Nexora")
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    toggle_main_window(tray.app_handle());
                }
            }
        })
        .build(app)?;
    TRAY_BUILT.store(true, Ordering::SeqCst);
    Ok(())
}
