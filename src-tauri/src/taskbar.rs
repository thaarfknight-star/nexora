//! Real Windows taskbar hide / restore, AppBar registration and re-hide
//! monitor.
//!
//! Nexora replaces the Windows taskbar with the floating island, so the real
//! taskbar window (`Shell_TrayWnd`, plus one `Shell_SecondaryTrayWnd` per
//! extra monitor) must be hidden while the island is the shell, and shown
//! again when Nexora quits — or if Nexora is killed, by the watchdog.
//!
//! Two things keep maximized windows from sliding under the island:
//!
//! * **AppBar.** The island window is registered with `SHAppBarMessage`
//!   (`ABM_NEW` + `ABM_SETPOS`, edge `ABE_TOP`) so the system reserves the
//!   strip the island occupies and maximized windows start below it. The
//!   reservation is dropped (`ABM_REMOVE`) whenever the taskbar is shown
//!   again and on clean exit (`lib.rs` RunEvent handler). Tauri does not
//!   expose the island window procedure, so `ABN_*` notifications cannot
//!   be pumped; the monitor below re-asserts the reservation every tick
//!   instead. The callback message is therefore 0.
//! * **Monitor.** Windows 11 can show the taskbar again on its own (Win
//!   key, notifications) and Explorer recreates it visible after a
//!   restart. A background thread re-checks every 1.5 s while Nexora
//!   wants the taskbar hidden and hides it again, and re-registers the
//!   AppBar after an Explorer restart (the shell forgets every AppBar
//!   registration when Explorer restarts).
//!
//! `status()` always reports the live window state (`IsWindowVisible`), never
//! a cached flag, so it stays correct if Explorer restarts and recreates the
//! tray window. `restore()` is the exit / crash-recovery hook: it is
//! idempotent and always shows the taskbar again. The clean-exit path in
//! `lib.rs` drops the AppBar reservation and then calls the watchdog's
//! own `restore_taskbar()` (independent of this module, by design).
//!
//! Known limit: if Nexora is *killed* (not a clean exit) the AppBar
//! reservation outlives the process until Explorer restarts — the
//! watchdog restores the taskbar itself, but removing a dead AppBar
//! belongs to the watchdog module, which this branch does not touch.
//!
//! Only the Windows implementation talks to WinAPI (crate `windows`,
//! target-gated in `Cargo.toml`). On any other OS every function is a
//! no-op that reports the taskbar as visible / unsupported, so `cargo
//! check` and `cargo test` keep passing off Windows.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::Duration;

/// Whether Nexora wants the taskbar hidden in this process (set by
/// `hide()`, cleared by `show()` / `restore()`).
///
/// This is an *intent* flag, not a measurement: it stays set while
/// Explorer is down, so the monitor can hide the tray as soon as
/// Explorer recreates it. `status()` still reads the live window state.
static HIDDEN_BY_NEXORA: AtomicBool = AtomicBool::new(false);

/// Raw island window handle (`HWND` as `isize`; 0 = unknown).
/// Set once from `lib.rs` setup via [`set_appbar_window`]. Stored as an
/// integer so this module never needs a Tauri type.
static APPBAR_HWND: AtomicIsize = AtomicIsize::new(0);

/// Whether the island is currently registered as an AppBar with the shell.
static APPBAR_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Whether the re-hide monitor thread is running (Windows only; the
/// monitor never runs elsewhere).
#[cfg(target_os = "windows")]
static MONITOR_RUNNING: AtomicBool = AtomicBool::new(false);

/// How often the monitor re-checks the taskbar (assignment: 1–2 s).
/// Windows 11 can bring the taskbar back at any moment; 1.5 s bounds how
/// long it stays visible.
pub const MONITOR_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Serialize)]
pub struct TaskbarStatus {
    pub hidden: bool,
    pub mode: String,
}

/// Live tray state as the monitor sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    /// The primary tray window exists and is visible.
    Visible,
    /// The primary tray window exists and is hidden.
    Hidden,
    /// Explorer owns no tray window at all (Explorer down / restarting).
    Missing,
}

/// Pure decision: should the monitor hide the taskbar on this tick?
///
/// Only ever hides what Nexora itself hid: when Nexora did not ask for
/// the taskbar to be hidden, a visible taskbar is left alone. When
/// Explorer owns no tray window there is nothing to hide; the next tick
/// catches the recreated window.
pub fn should_rehide(hidden_by_nexora: bool, tray: TrayState) -> bool {
    hidden_by_nexora && tray == TrayState::Visible
}

/// Pure decision: should the monitor re-register the AppBar on this tick?
///
/// Explorer forgets every AppBar registration when it restarts, so a
/// tray that reappears after being missing means the reservation died
/// with the old Explorer and must be made again.
pub fn should_reregister_appbar(hidden_by_nexora: bool, prev: TrayState, now: TrayState) -> bool {
    hidden_by_nexora && prev == TrayState::Missing && now != TrayState::Missing
}

/// Pure decision: should the monitor re-assert the AppBar reservation?
///
/// Re-asserting (`ABM_QUERYPOS` + `ABM_SETPOS`) keeps the reserved strip
/// in step with the island's current rect (expand/collapse resizes it)
/// and with work-area changes the shell would otherwise only report via
/// `ABN_POSCHANGED`, which cannot be pumped here.
pub fn should_reassert_appbar(appbar_registered: bool, hidden_by_nexora: bool) -> bool {
    appbar_registered && hidden_by_nexora
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
        // Intent first: the monitor enforces it even if Explorer is
        // down right now and the tray only reappears later.
        HIDDEN_BY_NEXORA.store(true, Ordering::SeqCst);
        set_tray_visible(false);
        // Best effort: hide() still reports the live state when the
        // island window is unknown (no AppBar) or registration fails.
        register_appbar();
        start_monitor();
    }
    status()
}

/// Show the taskbar again.
pub fn show() -> TaskbarStatus {
    #[cfg(target_os = "windows")]
    {
        // Drop the work-area reservation before showing the taskbar;
        // the monitor thread exits on its next tick (flag cleared below).
        unregister_appbar();
        // Clear the intent even when Explorer is down: the user asked
        // for a visible taskbar, so the monitor must not re-hide the
        // tray Explorer recreates later.
        HIDDEN_BY_NEXORA.store(false, Ordering::SeqCst);
        set_tray_visible(true);
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

/// Remember the island window handle for AppBar registration.
/// Called once from `lib.rs` setup (Windows only); pass 0 to forget it.
/// The handle is only used for `SHAppBarMessage` and `GetWindowRect`.
pub fn set_appbar_window(hwnd: isize) {
    APPBAR_HWND.store(hwnd, Ordering::SeqCst);
}

/// The remembered island window handle, 0 when unknown.
pub fn appbar_window() -> isize {
    APPBAR_HWND.load(Ordering::SeqCst)
}

/// Whether the island is currently registered as an AppBar.
pub fn appbar_registered() -> bool {
    APPBAR_REGISTERED.load(Ordering::SeqCst)
}

/// Register the island window as a top AppBar (no-op when the window
/// handle is unknown or off Windows). Returns whether the island is
/// registered afterwards; calling it while already registered is a no-op.
pub fn register_appbar() -> bool {
    #[cfg(target_os = "windows")]
    {
        if APPBAR_REGISTERED.load(Ordering::SeqCst) {
            return true;
        }
        win::register_appbar()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Drop the AppBar reservation (no-op when not registered or off
/// Windows). Idempotent; safe to call on every exit path.
pub fn unregister_appbar() {
    #[cfg(target_os = "windows")]
    {
        win::unregister_appbar();
    }
}

/// Start the background re-hide / AppBar monitor (Windows only; no-op
/// elsewhere). Safe to call repeatedly: at most one thread runs, and it
/// exits by itself once Nexora no longer wants the taskbar hidden.
pub fn start_monitor() {
    #[cfg(target_os = "windows")]
    {
        if MONITOR_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        std::thread::spawn(|| {
            // Unknown at start: the first tick treats the tray as freshly
            // (re)appeared, so the AppBar gets registered even if hide()
            // ran before the window handle was known.
            let mut prev = TrayState::Missing;
            loop {
                std::thread::sleep(MONITOR_INTERVAL);
                if !hidden_by_nexora() {
                    break;
                }
                monitor_tick(&mut prev);
            }
            MONITOR_RUNNING.store(false, Ordering::SeqCst);
        });
    }
}

/// Current tray state, read from the live window.
pub fn tray_state() -> TrayState {
    #[cfg(target_os = "windows")]
    {
        match tray_visible() {
            Some(true) => TrayState::Visible,
            Some(false) => TrayState::Hidden,
            None => TrayState::Missing,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Off Windows there is no tray window; report it as visible to
        // match status(). The monitor never runs here.
        TrayState::Visible
    }
}

/// One monitor tick: re-hide the taskbar if Windows brought it back and
/// maintain the AppBar reservation. Only called while Nexora wants the
/// taskbar hidden.
#[cfg(target_os = "windows")]
fn monitor_tick(prev: &mut TrayState) {
    if !hidden_by_nexora() {
        return;
    }
    let now = tray_state();
    if should_rehide(hidden_by_nexora(), now) {
        set_tray_visible(false);
    }
    if appbar_window() != 0 {
        if should_reregister_appbar(hidden_by_nexora(), *prev, now) {
            win::reregister_appbar();
        } else if !appbar_registered() {
            win::register_appbar();
        } else if should_reassert_appbar(appbar_registered(), hidden_by_nexora()) {
            win::reassert_appbar();
        }
    }
    *prev = now;
}

#[cfg(target_os = "windows")]
mod win {
    use std::sync::atomic::Ordering;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::Shell::{
        SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetWindowRect, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOW,
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

    /// APPBARDATA for a top-edge bar over `rect`.
    ///
    /// The callback message is 0 on purpose: Tauri does not expose the
    /// island window procedure, so `ABN_*` notifications (notably
    /// `ABN_POSCHANGED`) cannot be pumped; the monitor thread re-asserts
    /// the reservation every tick instead.
    fn appbar_data(hwnd: HWND, rect: RECT) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: 0,
            uEdge: ABE_TOP,
            rc: rect,
            lParam: LPARAM(0),
        }
    }

    /// The island window's current screen rect, in physical pixels.
    /// `None` when the window handle is dead.
    fn island_rect(hwnd: HWND) -> Option<RECT> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `hwnd` is the live island window handle and `rect`
        // points at a valid RECT.
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
        Some(rect)
    }

    /// Register the island as a top AppBar and commit the reserved strip.
    /// Returns true when the shell accepted the registration.
    pub fn register_appbar() -> bool {
        let raw = super::APPBAR_HWND.load(Ordering::SeqCst);
        if raw == 0 {
            return false;
        }
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        let Some(rect) = island_rect(hwnd) else {
            return false;
        };
        // SAFETY: `hwnd` is the live island window; APPBARDATA is a
        // plain struct whose cbSize matches its layout.
        let mut data = appbar_data(hwnd, rect);
        if (unsafe { SHAppBarMessage(ABM_NEW, &mut data) }) == 0 {
            return false;
        }
        // Ask the shell where the bar goes (it anchors the rect to the
        // top edge and stretches it full-width), then commit that rect
        // so maximized windows start below the island. The island window
        // itself is NOT moved: it already sits inside the strip and
        // island.rs owns its position.
        if (unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut data) }) == 0 {
            return false;
        }
        if (unsafe { SHAppBarMessage(ABM_SETPOS, &mut data) }) == 0 {
            return false;
        }
        super::APPBAR_REGISTERED.store(true, Ordering::SeqCst);
        true
    }

    /// Re-register after an Explorer restart: the new Explorer never saw
    /// the old registration, so the stale flag is cleared first.
    pub fn reregister_appbar() -> bool {
        super::APPBAR_REGISTERED.store(false, Ordering::SeqCst);
        register_appbar()
    }

    /// Re-assert the reserved strip for the island's current rect
    /// (expand/collapse changes it). Returns false when nothing is
    /// registered or the window handle is dead.
    pub fn reassert_appbar() -> bool {
        if !super::APPBAR_REGISTERED.load(Ordering::SeqCst) {
            return false;
        }
        let raw = super::APPBAR_HWND.load(Ordering::SeqCst);
        if raw == 0 {
            return false;
        }
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        let Some(rect) = island_rect(hwnd) else {
            return false;
        };
        // SAFETY: as in `register_appbar`.
        let mut data = appbar_data(hwnd, rect);
        if (unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut data) }) == 0 {
            return false;
        }
        (unsafe { SHAppBarMessage(ABM_SETPOS, &mut data) }) != 0
    }

    /// Drop the AppBar reservation. Best effort: the return value is
    /// ignored because this runs on exit paths too.
    pub fn unregister_appbar() {
        if !super::APPBAR_REGISTERED.swap(false, Ordering::SeqCst) {
            return;
        }
        let raw = super::APPBAR_HWND.load(Ordering::SeqCst);
        if raw == 0 {
            return;
        }
        let mut data = appbar_data(
            HWND(raw as *mut core::ffi::c_void),
            RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
        );
        // SAFETY: as in `register_appbar`.
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut data);
        }
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

    #[test]
    fn monitor_interval_is_within_one_to_two_seconds() {
        assert!(MONITOR_INTERVAL.as_millis() >= 1000);
        assert!(MONITOR_INTERVAL.as_millis() <= 2000);
    }

    #[test]
    fn rehide_only_when_nexora_hid_it_and_tray_is_visible() {
        assert!(should_rehide(true, TrayState::Visible));
        assert!(!should_rehide(true, TrayState::Hidden));
        assert!(!should_rehide(true, TrayState::Missing));
        assert!(!should_rehide(false, TrayState::Visible));
        assert!(!should_rehide(false, TrayState::Hidden));
        assert!(!should_rehide(false, TrayState::Missing));
    }

    #[test]
    fn explorer_restart_sequence_rehides_when_tray_reappears() {
        // Explorer dies: the tray is Missing, so there is nothing to hide.
        assert!(!should_rehide(true, TrayState::Missing));
        // Explorer is back and recreated the tray visible: hide it again.
        assert!(should_rehide(true, TrayState::Visible));
        // After the re-hide the tray is Hidden; the next tick stays quiet.
        assert!(!should_rehide(true, TrayState::Hidden));
        // If Nexora did not hide it, a reappearing tray is left alone.
        assert!(!should_rehide(false, TrayState::Visible));
    }

    #[test]
    fn appbar_reregisters_only_after_explorer_restart() {
        // Missing -> reappeared: the old registration died with Explorer.
        assert!(should_reregister_appbar(
            true,
            TrayState::Missing,
            TrayState::Visible
        ));
        assert!(should_reregister_appbar(
            true,
            TrayState::Missing,
            TrayState::Hidden
        ));
        // No restart happened: never re-register.
        assert!(!should_reregister_appbar(
            true,
            TrayState::Visible,
            TrayState::Visible
        ));
        assert!(!should_reregister_appbar(
            true,
            TrayState::Hidden,
            TrayState::Visible
        ));
        assert!(!should_reregister_appbar(
            true,
            TrayState::Hidden,
            TrayState::Hidden
        ));
        // Still missing: nothing to register against yet.
        assert!(!should_reregister_appbar(
            true,
            TrayState::Missing,
            TrayState::Missing
        ));
        // Nexora does not want the taskbar hidden: leave it alone.
        assert!(!should_reregister_appbar(
            false,
            TrayState::Missing,
            TrayState::Visible
        ));
    }

    #[test]
    fn appbar_reassert_needs_registration_and_hidden_intent() {
        assert!(should_reassert_appbar(true, true));
        assert!(!should_reassert_appbar(false, true));
        assert!(!should_reassert_appbar(true, false));
        assert!(!should_reassert_appbar(false, false));
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
        assert!(!register_appbar());
        assert!(!appbar_registered());
        unregister_appbar();
        assert_eq!(appbar_window(), 0);
        set_appbar_window(0);
        assert_eq!(appbar_window(), 0);
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
