//! Real Windows taskbar hide / restore.
//!
//! Nexora replaces the Windows taskbar with the floating island, so the real
//! taskbar window (`Shell_TrayWnd`, plus one `Shell_SecondaryTrayWnd` per
//! extra monitor) must be hidden while the island is the shell, and shown
//! again when Nexora quits — or if Nexora is killed, by the watchdog.
//!
//! `status()` always reports the live window state (`IsWindowVisible`), never
//! a cached flag, so it stays correct if Explorer restarts and recreates the
//! tray window. `restore()` is the exit / crash-recovery hook: it is
//! idempotent and always shows the taskbar again. The Tauri `RunEvent`
//! wiring that calls `restore()` on quit lives in `lib.rs` (coordinated
//! through DEVagent — this module only exposes the function).
//!
//! Only the Windows implementation talks to WinAPI (crate `windows`,
//! target-gated in `Cargo.toml`). On any other OS every function is a
//! no-op that reports the taskbar as visible / unsupported, so `cargo
//! check` and `cargo test` keep passing off Windows.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether Nexora itself hid the taskbar in this process.
///
/// `restore()` shows the taskbar unconditionally, but the watchdog uses this
/// flag to know whether a missing taskbar is Nexora's doing.
static HIDDEN_BY_NEXORA: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct TaskbarStatus {
    pub hidden: bool,
    pub mode: String,
}

#[cfg(target_os = "windows")]
fn mode_for(hidden: bool) -> String {
    if hidden {
        "island-shell".into()
    } else {
        "normal".into()
    }
}

/// Current taskbar state, read from the live window.
pub fn status() -> TaskbarStatus {
    #[cfg(target_os = "windows")]
    {
        match tray_visible() {
            Some(visible) => TaskbarStatus {
                hidden: !visible,
                mode: mode_for(!visible),
            },
            // No tray window at all: Explorer is not running, so there is
            // no taskbar to show.
            None => TaskbarStatus {
                hidden: true,
                mode: "explorer-missing".into(),
            },
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        TaskbarStatus {
            hidden: false,
            mode: "unsupported".into(),
        }
    }
}

/// Hide the taskbar (primary and secondary tray windows).
pub fn hide() -> TaskbarStatus {
    #[cfg(target_os = "windows")]
    {
        if set_tray_visible(false) {
            HIDDEN_BY_NEXORA.store(true, Ordering::SeqCst);
        }
    }
    status()
}

/// Show the taskbar again.
pub fn show() -> TaskbarStatus {
    #[cfg(target_os = "windows")]
    {
        if set_tray_visible(true) {
            HIDDEN_BY_NEXORA.store(false, Ordering::SeqCst);
        }
    }
    status()
}

/// Show the taskbar again. Alias of `show()` with recovery semantics: call
/// it on quit and from the watchdog when Nexora dies, so the user is never
/// left without a taskbar.
pub fn restore() -> TaskbarStatus {
    show()
}

/// Hide or show the taskbar, as requested.
pub fn set_hidden(hidden: bool) -> TaskbarStatus {
    if hidden {
        hide()
    } else {
        show()
    }
}

/// Whether Nexora hid the taskbar in this process and has not restored it.
pub fn hidden_by_nexora() -> bool {
    HIDDEN_BY_NEXORA.load(Ordering::SeqCst)
}

#[cfg(target_os = "windows")]
mod win {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOW,
    };

    /// Window classes of the Windows taskbar. `Shell_TrayWnd` is the primary
    /// taskbar; Windows creates one `Shell_SecondaryTrayWnd` per extra
    /// monitor that has a taskbar.
    const TRAY_CLASSES: [PCWSTR; 2] = [w!("Shell_TrayWnd"), w!("Shell_SecondaryTrayWnd")];

    /// Every taskbar window currently owned by Explorer, in creation order.
    pub fn tray_windows() -> Vec<HWND> {
        let mut found = Vec::new();
        for class in TRAY_CLASSES {
            let mut previous: Option<HWND> = None;
            loop {
                // SAFETY: `FindWindowExW` with a null parent searches
                // top-level windows; the class strings are valid
                // null-terminated UTF-16 literals.
                let hwnd = unsafe { FindWindowExW(None, previous, class, PCWSTR::null()) };
                match hwnd {
                    Ok(hwnd) => {
                        found.push(hwnd);
                        previous = Some(hwnd);
                    }
                    // No (more) window of this class: the API reports a
                    // null handle as an error, which ends the enumeration.
                    Err(_) => break,
                }
            }
        }
        found
    }

    /// Whether the primary taskbar window is currently visible.
    ///
    /// Returns `None` when Explorer owns no taskbar window at all.
    pub fn tray_visible() -> Option<bool> {
        let windows = tray_windows();
        windows.first().map(|hwnd| unsafe {
            // SAFETY: `hwnd` came from `FindWindowExW` in this call.
            IsWindowVisible(*hwnd).as_bool()
        })
    }

    /// Show or hide every taskbar window. Returns `true` if at least one
    /// taskbar window was found.
    pub fn set_tray_visible(visible: bool) -> bool {
        let windows = tray_windows();
        let cmd = if visible { SW_SHOW } else { SW_HIDE };
        for hwnd in &windows {
            // SAFETY: `hwnd` came from `FindWindowExW` in this call.
            // The return value only reports the previous visibility
            // state; `status()` re-queries the live state instead.
            unsafe {
                let _ = ShowWindow(*hwnd, cmd);
            }
        }
        !windows.is_empty()
    }
}

#[cfg(target_os = "windows")]
use win::{set_tray_visible, tray_visible};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reports_a_mode() {
        let status = status();
        assert!(!status.mode.is_empty());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_is_a_noop() {
        assert!(!status().hidden);
        assert_eq!(status().mode, "unsupported");
        assert!(!hide().hidden);
        assert!(!show().hidden);
        assert!(!restore().hidden);
        assert!(!set_hidden(true).hidden);
        assert!(!hidden_by_nexora());
    }

    // On Windows this test really hides the taskbar for a moment on the
    // machine running the tests (CI runners are ephemeral). `restore()` is
    // called before any assertion, so a failure cannot leave the taskbar
    // hidden.
    #[cfg(target_os = "windows")]
    #[test]
    fn hide_then_restore_round_trips() {
        let was_hidden = hide().hidden;
        let restored = restore();
        assert!(was_hidden, "hide() should hide the taskbar");
        assert!(!restored.hidden, "restore() should show the taskbar");
        assert_eq!(restored.mode, "normal");
        assert!(!hidden_by_nexora());
    }
}
