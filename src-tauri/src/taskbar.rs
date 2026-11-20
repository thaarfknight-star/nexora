//! Real Windows taskbar hide / restore and re-hide monitor.
//!
//! Nexora replaces the Windows taskbar with the floating island, so the real
//! taskbar window (`Shell_TrayWnd`, plus one `Shell_SecondaryTrayWnd` per
//! extra monitor) must be hidden while the island is the shell, and shown
//! again when Nexora quits — or if Nexora is killed, by the watchdog.
//!
//! The island is a plain overlay (`alwaysOnTop` in the Tauri config): it
//! reserves no workspace, so maximized windows may slide under it — that is
//! the accepted behaviour. A workspace reservation for the island's strip
//! used to exist and was removed because it shrank every maximized window
//! by the full window height (440 px).
//!
//! **Deferred startup hide (Taha's spec after the 18:53 build test).**
//! The taskbar is NOT hidden at startup: it stays visible until the
//! island has been shown at least once with its taskbar content
//! (running windows, pinned apps, Start). The frontend sends that
//! signal with the `notify_island_shown` command; only then — and
//! only when the user asked for hiding (`auto_hide_taskbar`) — the
//! taskbar is hidden. The outcome is written to the diagnostics
//! log (see `diagnostics::log_taskbar_hide`). An explicit
//! `set_taskbar_hidden` call still hides immediately: the gate only
//! applies to the deferred startup hide.
//!
//! **Monitor.** Windows 11 can show the taskbar again on its own (Win
//! key, notifications) and Explorer recreates it visible after a
//! restart. A background thread re-checks every 1.5 s while Nexora
//! wants the taskbar hidden and hides it again.
//!
//! `status()` always reports the live window state (`IsWindowVisible`), never
//! a cached flag, so it stays correct if Explorer restarts and recreates the
//! tray window. `restore()` is the exit / crash-recovery hook: it is
//! idempotent and always shows the taskbar again. The clean-exit path in
//! `lib.rs` calls the watchdog's own `restore_taskbar()` (independent of
//! this module, by design).
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
/// Set once from `lib.rs` setup via [`set_island_window`]. Stored as an
/// integer so this module never needs a Tauri type. It is only used to
/// recognise the island itself (see `taskbar_content.rs`, which excludes
/// it from the window list); it reserves nothing.
static ISLAND_HWND: AtomicIsize = AtomicIsize::new(0);

/// Whether the island has been shown at least once with its taskbar
/// content (set by the `notify_island_shown` command from the
/// frontend). The deferred startup hide waits for this.
static ISLAND_SHOWN: AtomicBool = AtomicBool::new(false);

/// Whether startup asked for the taskbar to be hidden (from
/// `settings.auto_hide_taskbar`). Recorded at startup, acted on only
/// after the island-shown signal — never hides on its own.
static HIDE_INTENT: AtomicBool = AtomicBool::new(false);

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
        start_monitor();
    }
    status()
}

/// Show the taskbar again.
pub fn show() -> TaskbarStatus {
    #[cfg(target_os = "windows")]
    {
        // Clear the intent even when Explorer is down: the user asked
        // for a visible taskbar, so the monitor must not re-hide the
        // tray Explorer recreates later. The monitor thread exits on its
        // next tick (flag cleared here).
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

/// Remember the island window handle. Called once from `lib.rs` setup
/// (Windows only); pass 0 to forget it. The handle is only used to
/// recognise the island itself (see `taskbar_content.rs`); it reserves
/// nothing.
pub fn set_island_window(hwnd: isize) {
    ISLAND_HWND.store(hwnd, Ordering::SeqCst);
}

/// The remembered island window handle, 0 when unknown.
pub fn island_window() -> isize {
    ISLAND_HWND.load(Ordering::SeqCst)
}

/// Record whether startup wants the taskbar hidden. Called once from
/// `lib.rs` setup; the hide itself waits for the island-shown signal.
pub fn set_hide_intent(intended: bool) {
    HIDE_INTENT.store(intended, Ordering::SeqCst);
}

/// Whether startup asked for the taskbar to be hidden.
pub fn hide_intended() -> bool {
    HIDE_INTENT.load(Ordering::SeqCst)
}

/// Whether the island has been shown at least once with its taskbar content.
pub fn island_shown() -> bool {
    ISLAND_SHOWN.load(Ordering::SeqCst)
}

/// Pure gate for the deferred startup hide: hide only once the island
/// has been shown and hiding was requested at startup.
pub fn should_hide_now(island_shown: bool, hide_intended: bool) -> bool {
    island_shown && hide_intended
}

/// First-signal-wins: returns `true` only for the first island-shown
/// signal. Later signals are no-ops — the deferred hide runs once.
fn mark_island_shown() -> bool {
    !ISLAND_SHOWN.swap(true, Ordering::SeqCst)
}

/// Frontend signal: the island has been shown at least once with its
/// taskbar content (running windows, pinned apps, Start). Only now —
/// and only when startup asked for hiding — the taskbar is hidden.
/// The outcome is appended to the diagnostics log. Later calls are
/// no-ops. Returns the resulting taskbar status.
#[tauri::command]
pub fn notify_island_shown() -> TaskbarStatus {
    if !mark_island_shown() {
        return status();
    }
    if should_hide_now(island_shown(), hide_intended()) {
        let hidden = set_hidden(true);
        crate::diagnostics::log_taskbar_hide(if hidden.hidden {
            "hidden"
        } else {
            "not hidden"
        });
        hidden
    } else {
        status()
    }
}

/// Start the background re-hide monitor (Windows only; no-op
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
            loop {
                std::thread::sleep(MONITOR_INTERVAL);
                if !hidden_by_nexora() {
                    break;
                }
                monitor_tick();
            }
            MONITOR_RUNNING.store(false, Ordering::SeqCst);
            // Close the race DEVagent flagged: hide() may have run between
            // the loop's intent check above and this store, seen
            // MONITOR_RUNNING still set and skipped spawning — so re-check
            // the intent here and restart the monitor instead of leaving
            // none running. (hide()'s own start_monitor() covers the
            // other interleavings; either call is a no-op when a monitor
            // is already running.)
            if hidden_by_nexora() {
                start_monitor();
            }
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

/// One monitor tick: re-hide the taskbar if Windows brought it back.
/// Only called while Nexora wants the taskbar hidden.
#[cfg(target_os = "windows")]
fn monitor_tick() {
    if !hidden_by_nexora() {
        return;
    }
    if should_rehide(hidden_by_nexora(), tray_state()) {
        set_tray_visible(false);
    }
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
    fn deferred_hide_gate_truth_table() {
        assert!(should_hide_now(true, true));
        assert!(!should_hide_now(true, false));
        assert!(!should_hide_now(false, true));
        assert!(!should_hide_now(false, false));
    }

    #[test]
    fn island_shown_signal_is_one_shot() {
        assert!(!island_shown());
        assert!(mark_island_shown());
        assert!(island_shown());
        assert!(!mark_island_shown());
        // Reset: no other test reads this flag, but leave it clean.
        ISLAND_SHOWN.store(false, Ordering::SeqCst);
        assert!(!island_shown());
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
        assert_eq!(island_window(), 0);
        set_island_window(12345);
        assert_eq!(island_window(), 12345);
        set_island_window(0);
        assert_eq!(island_window(), 0);
        assert!(!hide_intended());
        set_hide_intent(true);
        assert!(hide_intended());
        set_hide_intent(false);
        assert!(!hide_intended());
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
