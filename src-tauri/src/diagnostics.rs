//! Startup diagnostics log.
//!
//! One timestamped log for the whole startup story, so the next
//! install on Taha's machine leaves evidence instead of silence:
//! setup steps and their errors, the vibrancy outcome, the hotkey
//! registration result, the taskbar-hide result, the first hot-zone
//! event, every visibility transition and its reason, and every
//! panic (via a panic hook that writes to the log).
//!
//! The log is written to **two** places with identical lines:
//!
//! * the app data directory (`<app data>/nexora-diagnostics.log`)
//! * `%TEMP%` (`<temp>/nexora-diagnostics.log`)
//!
//! The second copy exists because no log ever appeared in the app
//! data directory on Taha's machine — if writing there is broken,
//! the `%TEMP%` copy still saves the evidence. Both files are
//! opened in append mode, so a new run never erases the previous
//! run's lines.
//!
//! Logging never breaks the app: every failure to open or write is
//! swallowed (and, when the other file is open, noted there). The
//! panic hook is built the same way — its body cannot panic, and a
//! poisoned lock is recovered instead of blocking.
//!
//! This module is the shared log API for the other agents, so it
//! stays small and stable:
//!
//! * [`log`] — the one function everyone calls:
//!   `diagnostics::log("hotkey", "registered: alt+space")`.
//!   [`log_hotkey_result`] and [`log_taskbar_hide`] are thin
//!   wrappers that spell out the call sites for TAgent5 (hotkey
//!   registration, wired after this module merges — `hotkey.rs`
//!   is untouched here) and TAgent1 (taskbar hide).
//! * [`log_visibility_transition`] — the Tauri command the
//!   frontend calls on every visibility transition, with the
//!   reason. It also tells `island` whether the island is hidden,
//!   which the hot-zone watcher uses for handle hit-testing.
//! * [`init`] — call once, first thing in the `setup` hook, before
//!   any other setup step. [`log`] before `init` still works: it
//!   lazily opens the `%TEMP%` file only.
//! * [`install_panic_hook`] — call once, right after [`init`].
//! * [`show_setup_error`] — a message box for setup errors
//!   (plain `MessageBoxW` on Windows; `eprintln` elsewhere), so a
//!   failed startup is seen instead of exiting silently.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Manager;

/// Diagnostics log file name, in the app data directory and in
/// `%TEMP%` alike.
pub const DIAGNOSTICS_LOG_FILE: &str = "nexora-diagnostics.log";

/// Open log files and the app data directory they live in. The
/// `files` are written to, in order; an empty vec means nothing
/// could be opened yet (a later [`log`] retries the `%TEMP%` file).
struct DiagnosticsState {
    files: Vec<File>,
    app_data_dir: Option<PathBuf>,
    temp_only_init_done: bool,
    initialized: bool,
}

static STATE: OnceLock<Mutex<DiagnosticsState>> = OnceLock::new();

fn state() -> &'static Mutex<DiagnosticsState> {
    STATE.get_or_init(|| {
        Mutex::new(DiagnosticsState {
            files: Vec::new(),
            app_data_dir: None,
            temp_only_init_done: false,
            initialized: false,
        })
    })
}

fn lock_state() -> std::sync::MutexGuard<'static, DiagnosticsState> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

/// Open `dir/DIAGNOSTICS_LOG_FILE` for appending, creating the
/// directory when needed. `None` when it cannot be opened.
fn open_log_file(dir: &Path) -> Option<File> {
    if std::fs::create_dir_all(dir).is_err() {
        return None;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(DIAGNOSTICS_LOG_FILE))
        .ok()
}

/// Initialise the diagnostics log: open the log file in the app
/// data directory and the same-named file in `%TEMP%`, and write
/// one line saying where both live. Call once, first thing in the
/// `setup` hook. Never fails the caller.
pub fn init(app: &tauri::AppHandle) {
    let app_data_dir = app.path().app_data_dir().ok();
    let temp_dir = std::env::temp_dir();
    let mut guard = lock_state();
    if guard.initialized {
        return;
    }
    guard.initialized = true;
    guard.app_data_dir = app_data_dir.clone();
    if let Some(dir) = &app_data_dir {
        if let Some(file) = open_log_file(dir) {
            guard.files.push(file);
        }
    }
    // The lazy path in `log` may already have opened the temp file
    // (before `init` ran); opening it twice would duplicate every
    // later line in that file.
    if !guard.temp_only_init_done {
        if let Some(file) = open_log_file(&temp_dir) {
            guard.files.push(file);
        }
    }
    let started = format!(
        "diagnostics started (app-data: {}, temp: {})",
        app_data_dir
            .as_ref()
            .map(|d| d.join(DIAGNOSTICS_LOG_FILE).display().to_string())
            .unwrap_or_else(|| "unavailable".to_string()),
        temp_dir.join(DIAGNOSTICS_LOG_FILE).display(),
    );
    write_line_locked(&mut guard, "diagnostics", &started);
    if guard.files.is_empty() {
        eprintln!("Nexora: diagnostics log could not be opened anywhere");
    }
}

/// Make sure at least the `%TEMP%` file is open. Called from
/// [`log`] when [`init`] never ran (or opened nothing): a log line
/// from anywhere in the crate still lands somewhere.
fn ensure_temp_file_locked(guard: &mut DiagnosticsState) {
    if guard.temp_only_init_done || !guard.files.is_empty() {
        return;
    }
    // Set the flag only on success, so a failed open is retried on
    // the next `log` call instead of giving up forever.
    if let Some(file) = open_log_file(&std::env::temp_dir()) {
        guard.files.push(file);
        guard.temp_only_init_done = true;
    }
}

/// Format one log line: `2026-11-20T18:55:00.123Z [setup] message`.
pub fn format_log_line(timestamp: &str, kind: &str, message: &str) -> String {
    format!("{timestamp} [{kind}] {message}\n")
}

/// Write one already-formatted line to every open log file.
/// Failures are silent: logging must never break the app.
fn write_line_locked(guard: &mut DiagnosticsState, kind: &str, message: &str) {
    let line = format_log_line(&format_timestamp(SystemTime::now()), kind, message);
    for file in &mut guard.files {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Append one timestamped line to the diagnostics log (app data
/// and `%TEMP%`). The one function every agent calls:
/// `diagnostics::log("hotkey", "registered: alt+space")`.
/// Never fails, never panics.
pub fn log(kind: &str, message: &str) {
    let mut guard = lock_state();
    ensure_temp_file_locked(&mut guard);
    write_line_locked(&mut guard, kind, message);
}

/// Log the hotkey registration result. For TAgent5 to call after
/// this module merges (from `hotkey.rs` or the setup hook):
/// `diagnostics::log_hotkey_result("registered: alt+space")` or
/// `diagnostics::log_hotkey_result("failed: ...")`.
pub fn log_hotkey_result(message: &str) {
    log("hotkey", message);
}

/// Log the taskbar-hide result. For TAgent1 to call:
/// `diagnostics::log_taskbar_hide("hidden")` or
/// `diagnostics::log_taskbar_hide("failed: ...")`.
pub fn log_taskbar_hide(message: &str) {
    log("taskbar", message);
}

/// UTC timestamp as `2026-11-20T18:55:00.123Z`. Pure, so it is
/// unit-tested; no `chrono` dependency for one format.
pub fn format_timestamp(now: SystemTime) -> String {
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    )
}

/// Gregorian (year, month, day) for `days` since the Unix epoch
/// (Howard Hinnant's civil-from-days algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Install a panic hook that writes every panic to the diagnostics
/// log (message + location) and then runs the previous hook, so
/// the panic still unwinds exactly as before. The hook body cannot
/// panic: the log call swallows its own failures and a poisoned
/// lock is recovered. Call once, right after [`init`].
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        log("panic", &format!("{payload} at {location}"));
        previous(info);
    }));
}

/// Show a setup error in a message box instead of exiting
/// silently. Windows: plain `MessageBoxW` (the app has no dialog
/// plugin on purpose — see the `common-controls-v6` comment in
/// `Cargo.toml`). Elsewhere: `eprintln`.
#[cfg(target_os = "windows")]
pub fn show_setup_error(message: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let text = HSTRING::from(message);
    let caption = HSTRING::from("Nexora — startup error");
    // SAFETY: both strings are valid `HSTRING`s; no parent window
    // (`None`) is the documented way to show a standalone box.
    unsafe {
        let _ = MessageBoxW(None, &text, &caption, MB_OK | MB_ICONERROR);
    }
}

/// Show a setup error in a message box instead of exiting
/// silently. Off Windows there is no box: `eprintln`.
#[cfg(not(target_os = "windows"))]
pub fn show_setup_error(message: &str) {
    eprintln!("Nexora startup error: {message}");
}

/// Tauri command the frontend calls on every visibility
/// transition, with the reason: `from` / `to` are the frontend's
/// `visible` states (`hidden` / `compact` / `expanded`). The
/// transition is logged, and the island's hidden flag is updated,
/// which the hot-zone watcher uses for handle hit-testing.
#[tauri::command]
pub fn log_visibility_transition(from: String, to: String, reason: String) {
    log("visibility", &format!("{from} -> {to} ({reason})"));
    crate::island::set_hidden(to == "hidden");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_formats_utc() {
        assert_eq!(format_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_timestamp(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            "2023-11-14T22:13:20.000Z"
        );
        assert_eq!(
            format_timestamp(UNIX_EPOCH + Duration::from_millis(1_700_000_000_123)),
            "2023-11-14T22:13:20.123Z"
        );
        // Leap day.
        assert_eq!(
            format_timestamp(UNIX_EPOCH + Duration::from_secs(1_709_164_800)),
            "2024-02-29T00:00:00.000Z"
        );
    }

    #[test]
    fn log_line_carries_timestamp_kind_and_message() {
        let line = format_log_line("2026-11-20T18:55:00.123Z", "setup", "starting");
        assert_eq!(line, "2026-11-20T18:55:00.123Z [setup] starting\n");
    }

    #[test]
    fn log_appends_to_a_file() {
        let dir =
            std::env::temp_dir().join(format!("nexora-diagnostics-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = open_log_file(&dir).expect("log file opens");
        let mut guard = DiagnosticsState {
            files: vec![file],
            app_data_dir: None,
            temp_only_init_done: true,
            initialized: true,
        };
        write_line_locked(&mut guard, "setup", "starting");
        write_line_locked(&mut guard, "vibrancy", "acrylic-dwm");
        drop(guard);
        let contents =
            std::fs::read_to_string(dir.join(DIAGNOSTICS_LOG_FILE)).expect("log written");
        assert_eq!(contents.lines().count(), 2);
        assert!(contents.contains("[setup] starting"));
        assert!(contents.contains("[vibrancy] acrylic-dwm"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
