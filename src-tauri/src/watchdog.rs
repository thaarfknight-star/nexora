//! Watchdog: keep the Windows taskbar safe while Nexora hides it.
//!
//! The taskbar branch (`feat/taskbar-hide-restore`, TAgent1) owns hiding the
//! taskbar. This module owns the safety net: the taskbar must come back when
//! Nexora exits cleanly **and** when it dies (crash / killed from Task
//! Manager / panic). An in-process thread cannot do the crash case — it dies
//! with the process — so the real guard is a separate watchdog process:
//!
//! * At startup (`start_watchdog`, called from the Tauri `setup` hook) Nexora
//!   re-invokes its own binary detached with `--nexora-watchdog <pid>`.
//!   That child only waits on the parent process handle; the moment the
//!   parent exits for any reason it shows the taskbar again and exits.
//! * On a clean exit the main process restores the taskbar synchronously
//!   first, then stops the child (`lib.rs` `RunEvent::Exit` handling).
//! * While Nexora runs, a monitor thread polls `explorer.exe` every 2s and
//!   counts restarts, so `check_health` reports live explorer state instead
//!   of the old hardcoded `healthy: true`.
//!
//! Implementation notes:
//! * No new dependency: the Win32 calls are declared directly with
//!   `extern "system"` (`user32`/`kernel32`) and are `cfg(windows)`-gated,
//!   so this branch touches no shared file (`Cargo.toml` untouched) and the
//!   Linux build keeps working with honest `false` values.
//! * `restore_taskbar` is intentionally independent of `taskbar.rs`: the
//!   safety net must never depend on the merge order of the taskbar branch.
//!   Once both land, DEVagent may point this at the canonical hide/show in
//!   `taskbar.rs`; until then this copy is the one the kill-app test relies
//!   on.
//! * Showing the taskbar is always safe to repeat: if it is already visible
//!   the call is a no-op, and if Explorer is not running there is no taskbar
//!   window yet (Explorer creates it visible when it restarts).

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
#[cfg(any(windows, test))]
use std::time::Duration;

/// Argument that re-invokes this binary in watchdog mode.
/// Usage: `nexora --nexora-watchdog <parent-pid>`. Handled in `main.rs`
/// before Tauri starts; a watchdog child never opens the island window.
pub const WATCHDOG_ARG: &str = "--nexora-watchdog";

/// How often the monitor thread checks `explorer.exe` while Nexora runs.
#[cfg(windows)]
const MONITOR_INTERVAL: Duration = Duration::from_secs(2);

/// Number of times the monitor saw Explorer die and come back.
static EXPLORER_RESTARTS: AtomicU32 = AtomicU32::new(0);

#[cfg(windows)]
static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);
static WATCHDOG_CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct WatchdogStatus {
    /// Explorer is running. When it is not, there is no taskbar at all.
    pub healthy: bool,
    pub explorer_ok: bool,
    /// The primary taskbar window is currently visible. Note: `false` here
    /// is not an error — Nexora hides the taskbar on purpose while it runs.
    pub taskbar_visible: bool,
    /// Times the monitor saw Explorer die and come back since startup.
    pub explorer_restarts: u32,
}

/// Live health check: real Explorer / taskbar state, no hardcoded values.
pub fn check_health() -> WatchdogStatus {
    let explorer_ok = explorer_running();
    WatchdogStatus {
        healthy: explorer_ok,
        explorer_ok,
        taskbar_visible: taskbar_visible(),
        explorer_restarts: EXPLORER_RESTARTS.load(Ordering::Relaxed),
    }
}

/// Show the primary taskbar (and any secondary taskbars) again.
/// Safe to call repeatedly and on a clean exit. Returns `true` if at least
/// one taskbar window was found and shown.
pub fn restore_taskbar() -> bool {
    #[cfg(windows)]
    {
        win::restore_taskbar_windows()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Start the safety net: the Explorer monitor thread plus the external
/// watchdog child process. Idempotent; safe to call from the Tauri `setup`
/// hook. Never panics and never blocks app startup — a spawn failure just
/// leaves the external guard off (clean-exit restore still works).
pub fn start_watchdog() {
    start_monitor();
    if !WATCHDOG_STARTED.swap(true, Ordering::SeqCst) {
        #[cfg(windows)]
        {
            win::spawn_watchdog_child();
        }
    }
}

/// Stop the external watchdog child (clean exit only). Callers must restore
/// the taskbar first — see `lib.rs`. Idempotent.
pub fn stop_watchdog() {
    if let Ok(mut guard) = WATCHDOG_CHILD.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    WATCHDOG_STARTED.store(false, Ordering::SeqCst);
}

/// Watchdog-process entry point (runs in the detached child, never in the
/// main app). Waits for the parent process to exit, restores the taskbar,
/// then returns the process exit code. Returns `0` immediately if the
/// parent is already gone.
pub fn run_watchdog_process(parent_pid: u32) -> i32 {
    #[cfg(windows)]
    {
        win::run_watchdog_process(parent_pid)
    }
    #[cfg(not(windows))]
    {
        let _ = parent_pid;
        0
    }
}

/// Parse watchdog-mode arguments. `Some(pid)` only for exactly
/// `[binary, "--nexora-watchdog", "<pid>"]` with a numeric pid.
pub fn parse_watchdog_args(args: &[String]) -> Option<u32> {
    if args.len() == 3 && args[1] == WATCHDOG_ARG {
        args[2].parse::<u32>().ok()
    } else {
        None
    }
}

/// Poll `explorer.exe` in the background and count restarts. Idempotent;
/// a no-op on non-Windows builds.
fn start_monitor() {
    #[cfg(windows)]
    {
        if MONITOR_STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("nexora-watchdog".into())
            .spawn(|| {
                let mut was_running = explorer_running();
                loop {
                    std::thread::sleep(MONITOR_INTERVAL);
                    let running = explorer_running();
                    if !was_running && running {
                        EXPLORER_RESTARTS.fetch_add(1, Ordering::Relaxed);
                        // Explorer came back: it recreates the taskbar
                        // visible, so the taskbar is restored.
                        crate::notifications::add(
                            "Taskbar restored",
                            "Explorer restarted — the taskbar is visible again.",
                            crate::notifications::KIND_TASKBAR,
                        );
                    }
                    was_running = running;
                }
            });
    }
}

fn explorer_running() -> bool {
    #[cfg(windows)]
    {
        win::process_named_running("explorer.exe")
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn taskbar_visible() -> bool {
    #[cfg(windows)]
    {
        win::taskbar_visible()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod win {
    // The Win32 type names (HWND, BOOL, ...) are the official API spellings;
    // renaming them would make every signature unreadable against the docs.
    #![allow(clippy::upper_case_acronyms)]

    use super::{WATCHDOG_ARG, WATCHDOG_CHILD, WATCHDOG_STARTED};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    type HWND = isize;
    type BOOL = i32;
    type DWORD = u32;
    type HANDLE = isize;
    type LPARAM = isize;
    type LPCWSTR = *const u16;
    type LPWSTR = *mut u16;
    type WNDENUMPROC = unsafe extern "system" fn(HWND, LPARAM) -> BOOL;

    const TH32CS_SNAPPROCESS: DWORD = 0x0000_0002;
    const INVALID_HANDLE_VALUE: HANDLE = -1;
    const SW_SHOW: i32 = 5;
    const SYNCHRONIZE: DWORD = 0x0010_0000;
    const WAIT_OBJECT_0: DWORD = 0;
    const WAIT_FAILED: DWORD = 0xFFFF_FFFF;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(lp_class_name: LPCWSTR, lp_window_name: LPCWSTR) -> HWND;
        fn ShowWindow(h_wnd: HWND, n_cmd_show: i32) -> BOOL;
        fn IsWindowVisible(h_wnd: HWND) -> BOOL;
        fn EnumWindows(lp_enum_func: WNDENUMPROC, l_param: LPARAM) -> BOOL;
        fn GetClassNameW(h_wnd: HWND, lp_class_name: LPWSTR, n_max_count: i32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(dw_flags: DWORD, th32_process_id: DWORD) -> HANDLE;
        fn Process32FirstW(h_snapshot: HANDLE, lppe: *mut ProcessEntry32W) -> BOOL;
        fn Process32NextW(h_snapshot: HANDLE, lppe: *mut ProcessEntry32W) -> BOOL;
        fn CloseHandle(h_object: HANDLE) -> BOOL;
        fn OpenProcess(
            dw_desired_access: DWORD,
            b_inherit_handle: BOOL,
            dw_process_id: DWORD,
        ) -> HANDLE;
        fn WaitForSingleObject(h_handle: HANDLE, dw_milliseconds: DWORD) -> DWORD;
    }

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: DWORD,
        cnt_usage: DWORD,
        th32_process_id: DWORD,
        th32_default_heap_id: usize,
        th32_module_id: DWORD,
        cnt_threads: DWORD,
        th32_parent_process_id: DWORD,
        pc_pri_class_base: i32,
        dw_flags: DWORD,
        sz_exe_file: [u16; 260],
    }

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Case-insensitive `explorer.exe` comparison on one snapshot entry name.
    fn exe_name_matches(entry_name: &str, target: &str) -> bool {
        entry_name.eq_ignore_ascii_case(target)
    }

    pub fn process_named_running(target: &str) -> bool {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut entry: ProcessEntry32W = std::mem::zeroed();
            entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as DWORD;
            let mut found = false;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let len = entry
                        .sz_exe_file
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.sz_exe_file.len());
                    let name = String::from_utf16_lossy(&entry.sz_exe_file[..len]);
                    if exe_name_matches(&name, target) {
                        found = true;
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            found
        }
    }

    pub fn taskbar_visible() -> bool {
        unsafe {
            let class = wide("Shell_TrayWnd");
            let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
            hwnd != 0 && IsWindowVisible(hwnd) != 0
        }
    }

    /// `EnumWindows` callback: show every primary/secondary taskbar window.
    unsafe extern "system" fn show_taskbar_window(hwnd: HWND, _l_param: LPARAM) -> BOOL {
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            let name = String::from_utf16_lossy(&buf[..len as usize]);
            if name == "Shell_TrayWnd" || name == "Shell_SecondaryTrayWnd" {
                ShowWindow(hwnd, SW_SHOW);
            }
        }
        1 // keep enumerating
    }

    /// Show all taskbar windows once. Returns how many were found.
    fn show_all_taskbars() -> usize {
        // Count first via FindWindowW so the common single-monitor case
        // does not depend on the enumeration callback succeeding.
        let mut found = 0usize;
        unsafe {
            let primary = wide("Shell_TrayWnd");
            if FindWindowW(primary.as_ptr(), std::ptr::null()) != 0 {
                found += 1;
            }
            let secondary = wide("Shell_SecondaryTrayWnd");
            if FindWindowW(secondary.as_ptr(), std::ptr::null()) != 0 {
                found += 1;
            }
            EnumWindows(show_taskbar_window, 0);
        }
        found
    }

    /// Show the taskbar, retrying briefly: Explorer may be restarting at
    /// the same moment Nexora dies, so a missing window is retried instead
    /// of giving up. Returns `true` if a taskbar window was shown.
    pub fn restore_taskbar_windows() -> bool {
        for attempt in 0..5 {
            if show_all_taskbars() > 0 {
                return true;
            }
            if attempt < 4 {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        false
    }

    pub fn spawn_watchdog_child() {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let pid = std::process::id();
        let child = std::process::Command::new(exe)
            .arg(WATCHDOG_ARG)
            .arg(pid.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn();
        match child {
            Ok(child) => {
                if let Ok(mut guard) = WATCHDOG_CHILD.lock() {
                    *guard = Some(child);
                }
            }
            Err(_) => {
                // Spawn failed: allow a later retry instead of staying
                // permanently "started" with no child.
                WATCHDOG_STARTED.store(false, Ordering::SeqCst);
            }
        }
    }

    pub fn run_watchdog_process(parent_pid: u32) -> i32 {
        unsafe {
            let handle = OpenProcess(SYNCHRONIZE, 0, parent_pid);
            if handle == 0 || handle == INVALID_HANDLE_VALUE {
                // Parent is already gone: restore and exit.
                restore_taskbar_windows();
                return 0;
            }
            loop {
                let result = WaitForSingleObject(handle, 1000);
                if result == WAIT_OBJECT_0 || result == WAIT_FAILED {
                    break;
                }
                // WAIT_TIMEOUT: parent still alive, keep waiting.
            }
            CloseHandle(handle);
        }
        restore_taskbar_windows();
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_watchdog_args_accepts_exact_form() {
        assert_eq!(
            parse_watchdog_args(&args(&["nexora.exe", "--nexora-watchdog", "1234"])),
            Some(1234)
        );
    }

    #[test]
    fn parse_watchdog_args_rejects_wrong_forms() {
        assert_eq!(parse_watchdog_args(&args(&["nexora.exe"])), None);
        assert_eq!(
            parse_watchdog_args(&args(&["nexora.exe", "--nexora-watchdog"])),
            None
        );
        assert_eq!(
            parse_watchdog_args(&args(&["nexora.exe", "--nexora-watchdog", "abc"])),
            None
        );
        assert_eq!(
            parse_watchdog_args(&args(&["nexora.exe", "--nexora-watchdog", "1234", "extra"])),
            None
        );
        assert_eq!(
            parse_watchdog_args(&args(&["nexora.exe", "--other", "1234"])),
            None
        );
    }

    #[test]
    fn restore_and_health_never_panic() {
        // On non-Windows these are honest no-ops; on Windows they touch
        // real windows but must still not panic when nothing is hidden.
        let _ = restore_taskbar();
        let status = check_health();
        #[cfg(not(windows))]
        {
            assert!(!status.healthy);
            assert!(!status.explorer_ok);
            assert!(!status.taskbar_visible);
        }
        #[cfg(windows)]
        {
            assert_eq!(status.healthy, status.explorer_ok);
        }
    }

    #[test]
    fn start_stop_watchdog_is_idempotent() {
        start_watchdog();
        start_watchdog();
        stop_watchdog();
        stop_watchdog();
    }

    #[test]
    fn watchdog_process_exits_for_dead_parent() {
        // A pid that cannot exist: the child must restore (no-op here)
        // and return 0 right away instead of waiting forever.
        let start = std::time::Instant::now();
        assert_eq!(run_watchdog_process(u32::MAX - 1), 0);
        assert!(start.elapsed() < Duration::from_secs(10));
    }
}
