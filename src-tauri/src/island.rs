//! Fixed island window.
//!
//! The island window never resizes: it is always one fixed transparent
//! canvas, top-center, and the pill / panel animate inside it with CSS
//! (frontend). The canvas is bigger than the panel so the glow is never
//! clipped: the expanded panel (560x360, approved design) sits in a
//! content box inset by `GLOW_PADDING` on every side. The compact pill
//! (~64px tall) and the hidden-state handle (120x4) render inside the
//! same window; only CSS moves them.
//!
//! Two runtime behaviors live here as well:
//!
//! * Hot zone: on Windows a background thread polls `GetCursorPos` and
//!   emits `island-hot-zone` when the mouse enters / leaves the top
//!   edge strip. The frontend shows the island from its hidden state
//!   on that event. Off Windows the watcher never fires.
//! * Click-through: `set_click_through` toggles
//!   `Window::set_ignore_cursor_events` so the empty parts of the
//!   canvas let clicks pass to the app below, while the pill / panel
//!   still receive clicks. The toggle is window-wide (Tauri has no
//!   per-region ignore), so the frontend owns the policy: hidden ->
//!   ignore, visible -> receive.
//!
//! And the anchor is self-verifying: after every `set_position`,
//! [`place_top_center`] reads the real window position back
//! (`outer_position`) and re-applies the anchor when it drifted —
//! `set_position` during setup can be silently overridden while the
//! window is still being created (on Taha's install the island
//! opened mid-screen, not top-center). Every application appends one
//! evidence line (computed position, real position, monitor geometry
//! + DPI factor, time) to [`ANCHOR_LOG_FILE`] in the app data
//! directory, so the next install shows what the anchor did instead
//! of guessing.

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};

/// Fixed window size, in logical pixels: expanded panel 560x360
/// (approved design) plus `GLOW_PADDING` on every side.
pub const WINDOW_WIDTH: u32 = 640;
pub const WINDOW_HEIGHT: u32 = 440;
/// Glow room around the content box, in logical pixels.
pub const GLOW_PADDING: u32 = 40;
/// Content box inside the window: the expanded panel lives here
/// (560x360, approved design).
pub const CONTENT_X: u32 = GLOW_PADDING;
pub const CONTENT_Y: u32 = GLOW_PADDING;
pub const CONTENT_WIDTH: u32 = WINDOW_WIDTH - 2 * GLOW_PADDING;
pub const CONTENT_HEIGHT: u32 = WINDOW_HEIGHT - 2 * GLOW_PADDING;
/// Compact pill height, in logical pixels (approved design). The pill
/// renders inside the same fixed window; its width is bounded by the
/// content box.
pub const COMPACT_HEIGHT: u32 = 64;
/// Hidden-state handle size, in logical pixels (approved design):
/// a 120x4 strip at the top edge. It renders inside the same fixed
/// window; the hot-zone watcher (not a click) wakes the island.
pub const HANDLE_WIDTH: u32 = 120;
pub const HANDLE_HEIGHT: u32 = 4;
/// Top edge of the window, in logical pixels from the monitor top.
/// The window sits flush at the top; the content inset provides the
/// visual gap (previously `TOP_MARGIN`, removed with the resize).
pub const WINDOW_TOP_Y: f64 = 0.0;

/// Hot zone: strip along the top edge of the monitor, in logical
/// pixels tall. The mouse entering it wakes the island from hidden.
pub const HOT_ZONE_HEIGHT: f64 = 6.0;
/// How often the watcher polls the cursor position.
pub const HOT_ZONE_POLL_MS: u64 = 100;
/// Event emitted when the cursor enters / leaves the hot zone.
pub const HOT_ZONE_EVENT: &str = "island-hot-zone";

/// Anchor: how far the real window position (physical pixels) may
/// sit from the computed anchor before it counts as drifted and gets
/// re-applied. Covers integer rounding across DPI scale factors.
pub const ANCHOR_TOLERANCE_PX: f64 = 2.0;
/// Anchor: delay after setup before the position is re-asserted once
/// more — the window is shown as it is created, and the window
/// manager can still move it after setup returns.
pub const ANCHOR_REAPPLY_DELAY_MS: u64 = 500;
/// Anchor: evidence log file name, inside the app data directory.
pub const ANCHOR_LOG_FILE: &str = "island-anchor.log";

#[derive(Debug, Clone, Serialize)]
pub struct IslandState {
    pub expanded: bool,
    pub mode: String,
    pub title: String,
    /// Fixed window size (never changes with expand / collapse).
    pub width: u32,
    pub height: u32,
    /// Content box where the pill / panel render.
    pub content_x: u32,
    pub content_y: u32,
    pub content_width: u32,
    pub content_height: u32,
    /// Compact pill height and hidden-state handle size (approved
    /// design), so the frontend reads the numbers from one place.
    pub compact_height: u32,
    pub handle_width: u32,
    pub handle_height: u32,
}

/// Payload of the [`HOT_ZONE_EVENT`] event.
#[derive(Debug, Clone, Serialize)]
pub struct HotZoneEvent {
    pub in_hot_zone: bool,
}

static EXPANDED: Mutex<bool> = Mutex::new(false);

/// The one fixed window size.
pub fn window_size() -> (u32, u32) {
    (WINDOW_WIDTH, WINDOW_HEIGHT)
}

/// Content box `(x, y, width, height)` inside the window.
pub fn content_rect() -> (u32, u32, u32, u32) {
    (CONTENT_X, CONTENT_Y, CONTENT_WIDTH, CONTENT_HEIGHT)
}

pub fn current_state() -> IslandState {
    let expanded = *EXPANDED.lock().unwrap_or_else(|e| e.into_inner());
    let (width, height) = window_size();
    IslandState {
        expanded,
        mode: if expanded {
            "expanded".into()
        } else {
            "compact".into()
        },
        title: "Nexora Island".into(),
        width,
        height,
        content_x: CONTENT_X,
        content_y: CONTENT_Y,
        content_width: CONTENT_WIDTH,
        content_height: CONTENT_HEIGHT,
        compact_height: COMPACT_HEIGHT,
        handle_width: HANDLE_WIDTH,
        handle_height: HANDLE_HEIGHT,
    }
}

/// State-only toggle, kept for the existing `set_island_expanded` command.
/// It updates the flag but never touches the window: use
/// [`set_expanded_on_window`] when a `Window` handle is available.
pub fn set_expanded(expanded: bool) -> IslandState {
    if let Ok(mut guard) = EXPANDED.lock() {
        *guard = expanded;
    }
    current_state()
}

/// X coordinate that centers a window of `window_width` logical pixels
/// on a monitor `monitor_width` logical pixels wide. Never negative.
pub fn centered_x(monitor_width: f64, window_width: u32) -> f64 {
    ((monitor_width - f64::from(window_width)) / 2.0).max(0.0)
}

/// Logical geometry `(x, y, width, height)` of the monitor the window
/// is currently on. Falls back to 1920x1080 at the origin when no
/// monitor can be queried (headless / CI).
fn logical_monitor_geometry(window: &tauri::Window) -> (f64, f64, f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    match monitor {
        Some(monitor) => {
            let size = monitor.size();
            let position = monitor.position();
            (
                f64::from(position.x) / scale,
                f64::from(position.y) / scale,
                f64::from(size.width) / scale,
                f64::from(size.height) / scale,
            )
        }
        None => (0.0, 0.0, 1920.0, 1080.0),
    }
}

/// Top-center position for the fixed window: horizontally centered on
/// its monitor, flush with the top edge.
pub fn top_center_position(window: &tauri::Window) -> tauri::LogicalPosition<f64> {
    let (_, _, monitor_width, _) = logical_monitor_geometry(window);
    tauri::LogicalPosition::new(centered_x(monitor_width, WINDOW_WIDTH), WINDOW_TOP_Y)
}

/// Logical position scaled to physical pixels.
pub fn expected_physical(logical: (f64, f64), scale: f64) -> (f64, f64) {
    (logical.0 * scale, logical.1 * scale)
}

/// Expected physical top-center position for the fixed window on the
/// monitor the window is on: the logical anchor times the DPI scale.
pub fn expected_physical_position(window: &tauri::Window) -> (f64, f64) {
    let pos = top_center_position(window);
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    expected_physical((pos.x, pos.y), scale)
}

/// Whether a real (physical) window position sits at the computed
/// anchor, within [`ANCHOR_TOLERANCE_PX`].
pub fn is_at_anchor(actual: (i32, i32), expected: (f64, f64)) -> bool {
    (f64::from(actual.0) - expected.0).abs() <= ANCHOR_TOLERANCE_PX
        && (f64::from(actual.1) - expected.1).abs() <= ANCHOR_TOLERANCE_PX
}

/// One anchor evidence line: time, computed position, real position,
/// monitor geometry (logical) and DPI scale. `actual` is `None` when
/// the real position could not be read.
pub fn format_anchor_line(
    now_secs: f64,
    expected: (f64, f64),
    actual: Option<(i32, i32)>,
    monitor: (f64, f64, f64, f64),
    scale: f64,
) -> String {
    let actual = match actual {
        Some((x, y)) => format!("({x},{y})"),
        None => "ERR".to_string(),
    };
    format!(
        "t={now_secs:.3} expected=({:.1},{:.1}) actual={actual} monitor=({:.0},{:.0},{:.0}x{:.0}) scale={scale:.2}\n",
        expected.0, expected.1, monitor.0, monitor.1, monitor.2, monitor.3
    )
}

/// Append one line to the anchor log in `dir`. Failures are silent:
/// evidence must never break the island.
fn write_anchor_log(dir: &Path, line: &str) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join(ANCHOR_LOG_FILE);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Append one anchor evidence line for this window to the log in the
/// app data directory. Silent when the directory cannot be resolved.
fn log_anchor(window: &tauri::Window, expected: (f64, f64), actual: Option<(i32, i32)>) {
    let (mon_x, mon_y, mon_w, mon_h) = logical_monitor_geometry(window);
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let line = format_anchor_line(
        now_secs,
        expected,
        actual,
        (mon_x, mon_y, mon_w, mon_h),
        scale,
    );
    let Ok(dir) = window.app_handle().path().app_data_dir() else {
        return;
    };
    write_anchor_log(&dir, &line);
}

/// Read the real window position back; when it is not at the anchor,
/// apply the position once more and read again. Always appends one
/// evidence line to the anchor log. Never fails the caller: a window
/// the OS moves anyway is repaired again on the next
/// `ensure_fixed_window` call.
fn verify_and_repair_anchor(window: &tauri::Window) {
    let expected = expected_physical_position(window);
    let mut actual = window.outer_position().ok().map(|p| (p.x, p.y));
    if let Some(pos) = actual {
        if !is_at_anchor(pos, expected) {
            let _ = window.set_position(tauri::Position::Logical(top_center_position(window)));
            actual = window.outer_position().ok().map(|p| (p.x, p.y));
        }
    }
    log_anchor(window, expected, actual);
}

/// Move the window to its fixed top-center position, then verify the
/// real position and repair it when it drifted (see the module docs).
pub fn place_top_center(window: &tauri::Window) -> Result<(), tauri::Error> {
    window.set_position(tauri::Position::Logical(top_center_position(window)))?;
    verify_and_repair_anchor(window);
    Ok(())
}

/// Force the window to the one fixed size and position. Idempotent:
/// calling it when the window is already fixed is a no-op visually.
/// This is the only place that calls `set_size` in the crate.
pub fn ensure_fixed_window(window: &tauri::Window) -> Result<(), tauri::Error> {
    window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
        f64::from(WINDOW_WIDTH),
        f64::from(WINDOW_HEIGHT),
    )))?;
    place_top_center(window)
}

/// Expand / collapse the island on a real window. The window itself
/// never resizes: only the state flag changes (the frontend animates
/// the pill / panel with CSS) and the fixed size / position are
/// re-asserted. This is the function IPC wiring calls
/// (owner: EAgent5, `ipc.rs`); `set_expanded` stays as the state-only
/// fallback for callers without a window handle.
pub fn set_expanded_on_window(
    window: &tauri::Window,
    expanded: bool,
) -> Result<IslandState, tauri::Error> {
    let state = set_expanded(expanded);
    ensure_fixed_window(window)?;
    Ok(state)
}

/// Alias for [`set_expanded_on_window`].
pub fn apply_to_window(
    window: &tauri::Window,
    expanded: bool,
) -> Result<IslandState, tauri::Error> {
    set_expanded_on_window(window, expanded)
}

/// Place the island at its startup position: fixed size, top-center.
/// Intended for the `setup` hook in `lib.rs` (coordinated via DEVagent).
/// The window is shown as it is created, and the window manager can
/// still move it after setup returns: the anchor is re-asserted once,
/// shortly after show, on a background thread. That call verifies and
/// logs again, like every other application.
pub fn setup_window(window: &tauri::Window) -> Result<IslandState, tauri::Error> {
    let state = set_expanded(false);
    ensure_fixed_window(window)?;
    // Real backdrop material (Acrylic on Windows, vibrancy on
    // macOS). Best effort and non-fatal: where it is unavailable
    // the CSS glass is the whole look, unchanged.
    crate::vibrancy::apply_island_vibrancy(window);
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ANCHOR_REAPPLY_DELAY_MS));
        let _ = ensure_fixed_window(&window);
    });
    Ok(state)
}

/// Whether a cursor at logical `(cursor_x, cursor_y)` on a monitor
/// `monitor_width` logical pixels wide is inside the hot zone: the
/// strip `0..=HOT_ZONE_HEIGHT` along the top edge.
pub fn is_in_hot_zone(cursor_x: f64, cursor_y: f64, monitor_width: f64) -> bool {
    (0.0..=monitor_width).contains(&cursor_x) && (0.0..=HOT_ZONE_HEIGHT).contains(&cursor_y)
}

/// Whether the cursor is currently inside the hot zone of the
/// monitor the window is on. On Windows this reads the live cursor
/// position with `GetCursorPos`; off Windows it is always `false`.
pub fn cursor_in_hot_zone(window: &tauri::Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        let Some((x, y)) = win::cursor_position() else {
            return false;
        };
        let (mon_x, mon_y, mon_w, _) = logical_monitor_geometry(window);
        let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
        let logical_x = (f64::from(x) - mon_x * scale) / scale;
        let logical_y = (f64::from(y) - mon_y * scale) / scale;
        is_in_hot_zone(logical_x, logical_y, mon_w)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        false
    }
}

/// Make the window ignore (or receive) cursor events. With `ignore`
/// the whole window is click-through, including the pill / panel, so
/// the frontend must turn it off whenever the island is visible and
/// back on when it hides to its handle. See the module docs.
pub fn set_click_through(window: &tauri::Window, ignore: bool) -> Result<(), tauri::Error> {
    window.set_ignore_cursor_events(ignore)
}

/// Start the hot-zone watcher: a background thread polls the cursor
/// position and emits [`HOT_ZONE_EVENT`] whenever the cursor enters
/// or leaves the top-edge strip (and once at startup, so the frontend
/// can sync its initial state). Call once from the `setup` hook in
/// `lib.rs`. The thread runs for the life of the process.
pub fn start_hot_zone_watcher(app: &tauri::AppHandle) {
    let Some(webview_window) = app.get_webview_window("main") else {
        return;
    };
    let window = webview_window.as_ref().window().clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let mut was_inside: Option<bool> = None;
        loop {
            let inside = cursor_in_hot_zone(&window);
            if was_inside != Some(inside) {
                was_inside = Some(inside);
                let _ = app.emit(
                    HOT_ZONE_EVENT,
                    HotZoneEvent {
                        in_hot_zone: inside,
                    },
                );
            }
            std::thread::sleep(Duration::from_millis(HOT_ZONE_POLL_MS));
        }
    });
}

#[cfg(target_os = "windows")]
mod win {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    /// Live cursor position in physical screen pixels, or `None` when
    /// the position cannot be read.
    pub fn cursor_position() -> Option<(i32, i32)> {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: `point` is a valid local `POINT`; `GetCursorPos`
        // writes the cursor coordinates into it.
        unsafe { GetCursorPos(&mut point) }.ok()?;
        Some((point.x, point.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_window_fits_panel_with_glow() {
        assert_eq!(window_size(), (640, 440));
        // The content box IS the expanded panel (560x360, approved
        // design), with equal glow padding on every side.
        assert_eq!(content_rect(), (40, 40, 560, 360));
        assert_eq!(CONTENT_X, GLOW_PADDING);
        assert_eq!(CONTENT_Y, GLOW_PADDING);
        assert_eq!(WINDOW_WIDTH - (CONTENT_X + CONTENT_WIDTH), GLOW_PADDING);
        assert_eq!(WINDOW_HEIGHT - (CONTENT_Y + CONTENT_HEIGHT), GLOW_PADDING);
    }

    #[test]
    fn state_reports_fixed_size() {
        let state = current_state();
        assert_eq!((state.width, state.height), (640, 440));
        assert_eq!(
            (
                state.content_x,
                state.content_y,
                state.content_width,
                state.content_height
            ),
            (40, 40, 560, 360)
        );
        assert_eq!(state.compact_height, 64);
        assert_eq!((state.handle_width, state.handle_height), (120, 4));
    }

    #[test]
    fn centered_x_is_half_the_gap() {
        assert_eq!(centered_x(1920.0, 640), 640.0);
        assert_eq!(centered_x(640.0, 640), 0.0);
        assert_eq!(centered_x(400.0, 640), 0.0);
    }

    #[test]
    fn anchor_tolerance_accepts_rounding() {
        assert!(is_at_anchor((640, 0), (640.0, 0.0)));
        assert!(is_at_anchor((641, 1), (640.0, 0.0)));
        assert!(is_at_anchor((638, 0), (640.0, 0.0)));
        assert!(!is_at_anchor((643, 0), (640.0, 0.0)));
        assert!(!is_at_anchor((640, 3), (640.0, 0.0)));
    }

    #[test]
    fn expected_physical_scales_with_dpi() {
        assert_eq!(expected_physical((640.0, 0.0), 1.0), (640.0, 0.0));
        assert_eq!(expected_physical((640.0, 0.0), 1.5), (960.0, 0.0));
        assert_eq!(expected_physical((640.0, 0.0), 1.25), (800.0, 0.0));
    }

    #[test]
    fn anchor_log_line_carries_evidence() {
        let line = format_anchor_line(
            1234.5,
            (640.0, 0.0),
            Some((640, 0)),
            (0.0, 0.0, 1920.0, 1080.0),
            1.25,
        );
        assert!(line.contains("t=1234.500"));
        assert!(line.contains("expected=(640.0,0.0)"));
        assert!(line.contains("actual=(640,0)"));
        assert!(line.contains("monitor=(0,0,1920x1080)"));
        assert!(line.contains("scale=1.25"));
        let missing = format_anchor_line(1.0, (640.0, 0.0), None, (0.0, 0.0, 1920.0, 1080.0), 1.0);
        assert!(missing.contains("actual=ERR"));
    }

    #[test]
    fn anchor_log_appends_to_file() {
        let dir =
            std::env::temp_dir().join(format!("nexora-anchor-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let line = format_anchor_line(
            1.5,
            (640.0, 0.0),
            Some((640, 0)),
            (0.0, 0.0, 1920.0, 1080.0),
            1.0,
        );
        write_anchor_log(&dir, &line);
        write_anchor_log(&dir, &line);
        let contents = std::fs::read_to_string(dir.join(ANCHOR_LOG_FILE)).expect("log written");
        assert_eq!(contents.lines().count(), 2);
        assert!(contents.contains("expected=(640.0,0.0) actual=(640,0)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hot_zone_is_the_top_edge_strip() {
        assert!(is_in_hot_zone(320.0, 0.0, 1920.0));
        assert!(is_in_hot_zone(320.0, 6.0, 1920.0));
        assert!(is_in_hot_zone(0.0, 3.0, 1920.0));
        assert!(is_in_hot_zone(1920.0, 3.0, 1920.0));
        assert!(!is_in_hot_zone(320.0, 6.1, 1920.0));
        assert!(!is_in_hot_zone(320.0, -1.0, 1920.0));
        assert!(!is_in_hot_zone(-1.0, 3.0, 1920.0));
        assert!(!is_in_hot_zone(1920.1, 3.0, 1920.0));
    }
}
