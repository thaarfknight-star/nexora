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
//! The hidden-state handle is drawn by the frontend at the very top
//! of the window (`HANDLE_Y_HIDDEN` = 0, stage offset 0 — it slides
//! to `CONTENT_Y` when the island opens). The backend owns the
//! numbers that must agree with that drawing: the visible bar
//! (`HANDLE_WIDTH` x `HANDLE_HEIGHT`), the larger interactive hit
//! rectangle around it (`HANDLE_HIT_WIDTH` x `HANDLE_HIT_HEIGHT`,
//! top-center at y=0), and the hot zone, which is exactly tall
//! enough to contain the hit rectangle (`HOT_ZONE_HEIGHT`).
//!
//! Three runtime behaviors live here as well:
//!
//! * Hot zone: on Windows a background thread polls `GetCursorPos` and
//!   emits `island-hot-zone` when the mouse enters / leaves the top
//!   edge strip. Entering only counts after a short hover-intent
//!   delay (`HOVER_INTENT_MS`) — a passing cursor must not wake the
//!   island by accident; leaving counts at once. The frontend shows
//!   the island from its hidden state on that event. Off Windows the
//!   watcher never fires.
//! * Click-through: `set_click_through` toggles
//!   `Window::set_ignore_cursor_events` so the empty parts of the
//!   canvas let clicks pass to the app below, while the pill / panel
//!   still receive clicks. When the island is hidden (the frontend
//!   reports every visibility transition through the
//!   `log_visibility_transition` command, which sets the hidden
//!   flag here), the watcher takes over the toggle: only the
//!   handle hit rectangle stays interactive and the rest of the
//!   window is click-through, so the handle can be grabbed while
//!   the desktop below stays usable. When the island is visible
//!   the frontend owns the flag, as before.
//! * Anchor (self-verifying): after every `set_position`,
//!   [`place_top_center`] reads the real window position back
//!   (`outer_position`) and re-applies the anchor when it drifted —
//!   `set_position` during setup can be silently overridden while the
//!   window is still being created (on Taha's install the island
//!   opened mid-screen, not top-center). Every application appends one
//!   evidence line (computed position, real position, window size and
//!   visibility, monitor geometry, DPI factor, and time) to
//!   [`ANCHOR_LOG_FILE`] in the app data directory, so the next
//!   install shows what the anchor did instead of guessing. The same
//!   file also gets one line per backdrop application (which path
//!   actually ran: DWM acrylic, SWCA acrylic, macOS vibrancy, or the
//!   CSS-only fallback), so a later screenshot can be matched to the
//!   exact window state — including whether the process was still
//!   alive when the island vanished.

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};

/// Fixed window size, in logical pixels: expanded panel 560x360
/// (approved design) plus `GLOW_PADDING` on every side.
pub const WINDOW_WIDTH: u32 = 760;
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
/// a 120x4 strip at the top edge of the window. The frontend draws
/// it at `HANDLE_Y_HIDDEN` (y=0, stage offset 0) while hidden; it
/// slides to `CONTENT_Y` when the island opens.
pub const HANDLE_WIDTH: u32 = 120;
pub const HANDLE_HEIGHT: u32 = 4;
/// Hidden-state handle top edge, in logical window pixels. The
/// window sits flush at the top of the monitor, so the handle is
/// drawn exactly at the screen edge — never 40px down at the content
/// box (the old mismatch: the hot zone was 6px at the screen edge
/// while the handle sat at y=40).
pub const HANDLE_Y_HIDDEN: u32 = 0;
/// Interactive handle rectangle, in logical pixels: a 160x12 area
/// around the visible 120x4 bar, top-center at y=0. In the hidden
/// state this rectangle stays clickable while the rest of the window
/// is click-through, so the thin bar can actually be grabbed.
pub const HANDLE_HIT_WIDTH: u32 = 160;
pub const HANDLE_HIT_HEIGHT: u32 = 12;
/// Top edge of the window, in logical pixels from the monitor top.
/// The window sits flush at the top; the content inset provides the
/// visual gap (previously `TOP_MARGIN`, removed with the resize).
pub const WINDOW_TOP_Y: f64 = 0.0;

/// Hot zone: strip along the top edge of the monitor, in logical
/// pixels tall. It is exactly tall enough to contain the handle hit
/// rectangle (`HANDLE_HIT_HEIGHT`), so the strip the watcher watches
/// and the strip the user grabs are the same strip. The mouse
/// entering it wakes the island from hidden.
pub const HOT_ZONE_HEIGHT: f64 = 12.0;
/// Hover intent: how long the cursor must stay inside the hot zone
/// before the watcher counts it as an entry (milliseconds). A
/// passing cursor must not wake the island by accident; leaving
/// counts at once, without a delay.
pub const HOVER_INTENT_MS: u64 = 120;
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
    /// Interactive handle rectangle and hot-zone height, so the
    /// frontend draws the handle where the backend hit-tests it.
    pub handle_hit_width: u32,
    pub handle_hit_height: u32,
    pub hot_zone_height: f64,
    pub hover_intent_ms: u64,
}

/// Payload of the [`HOT_ZONE_EVENT`] event.
#[derive(Debug, Clone, Serialize)]
pub struct HotZoneEvent {
    pub in_hot_zone: bool,
}

static EXPANDED: Mutex<bool> = Mutex::new(false);

/// Whether the island is currently hidden. The frontend is the
/// source of truth: it reports every visibility transition through
/// the `log_visibility_transition` command, which calls
/// [`set_hidden`]. The hot-zone watcher reads this flag to decide
/// whether it owns click-through (hidden: hit-test the handle) or
/// leaves the flag to the frontend (visible). Default `false`
/// (visible): at startup the island is shown, and the watcher must
/// not make the window click-through before the frontend has said
/// anything.
static HIDDEN: Mutex<bool> = Mutex::new(false);

/// Record whether the island is hidden. Called by the
/// `log_visibility_transition` command on every frontend
/// visibility transition.
pub fn set_hidden(hidden: bool) {
    if let Ok(mut guard) = HIDDEN.lock() {
        *guard = hidden;
    }
}

/// Whether the island is currently hidden, as last reported by the
/// frontend. `false` when the flag was never set.
pub fn is_hidden() -> bool {
    *HIDDEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Interactive handle rectangle `(x, y, width, height)` in logical
/// window pixels: 160x12, horizontally centered, top edge at y=0.
/// This is the area that stays clickable while the island is hidden.
pub fn handle_hit_rect() -> (f64, f64, f64, f64) {
    (
        (f64::from(WINDOW_WIDTH) - f64::from(HANDLE_HIT_WIDTH)) / 2.0,
        f64::from(HANDLE_Y_HIDDEN),
        f64::from(HANDLE_HIT_WIDTH),
        f64::from(HANDLE_HIT_HEIGHT),
    )
}

/// Visible handle bar `(x, y, width, height)` in logical window
/// pixels: 120x4, centered inside the hit rectangle, top edge at
/// y=0 — the bar the frontend draws while hidden.
pub fn handle_bar_rect() -> (f64, f64, f64, f64) {
    (
        (f64::from(WINDOW_WIDTH) - f64::from(HANDLE_WIDTH)) / 2.0,
        f64::from(HANDLE_Y_HIDDEN),
        f64::from(HANDLE_WIDTH),
        f64::from(HANDLE_HEIGHT),
    )
}

/// Whether rectangle `inner` lies fully inside rectangle `outer`.
/// Rectangles are `(x, y, width, height)`; edges count as inside.
pub fn rect_contains(outer: (f64, f64, f64, f64), inner: (f64, f64, f64, f64)) -> bool {
    inner.0 >= outer.0
        && inner.1 >= outer.1
        && inner.0 + inner.2 <= outer.0 + outer.2
        && inner.1 + inner.3 <= outer.1 + outer.3
}

/// Whether a cursor at logical window coordinates `(x, y)` is
/// inside the handle hit rectangle. Edges count as inside.
pub fn cursor_in_handle_hit_rect(x: f64, y: f64) -> bool {
    let (rx, ry, rw, rh) = handle_hit_rect();
    (rx..=rx + rw).contains(&x) && (ry..=ry + rh).contains(&y)
}

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
        handle_hit_width: HANDLE_HIT_WIDTH,
        handle_hit_height: HANDLE_HIT_HEIGHT,
        hot_zone_height: HOT_ZONE_HEIGHT,
        hover_intent_ms: HOVER_INTENT_MS,
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

/// Seconds since the Unix epoch, for evidence lines. `0.0` when the
/// clock cannot be read.
fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// One anchor evidence line: time, computed position, real position,
/// window size and visibility, monitor geometry (logical) and DPI
/// scale. `actual`, `size` and `visible` are `None` when they could
/// not be read.
pub fn format_anchor_line(
    now_secs: f64,
    expected: (f64, f64),
    actual: Option<(i32, i32)>,
    size: Option<(u32, u32)>,
    visible: Option<bool>,
    monitor: (f64, f64, f64, f64),
    scale: f64,
) -> String {
    let actual = match actual {
        Some((x, y)) => format!("({x},{y})"),
        None => "ERR".to_string(),
    };
    let size = match size {
        Some((w, h)) => format!("{w}x{h}"),
        None => "ERR".to_string(),
    };
    let visible = match visible {
        Some(v) => format!("{v}"),
        None => "ERR".to_string(),
    };
    format!(
        "t={now_secs:.3} expected=({:.1},{:.1}) actual={actual} size={size} visible={visible} monitor=({:.0},{:.0},{:.0}x{:.0}) scale={scale:.2}\n",
        expected.0, expected.1, monitor.0, monitor.1, monitor.2, monitor.3
    )
}

/// One vibrancy evidence line: time and which backdrop path actually
/// ran (`acrylic-dwm` / `acrylic-swca` / `vibrancy-macos` /
/// `css-fallback`). Written to the same anchor log, so one file
/// carries the whole window story for the next install.
pub fn format_vibrancy_line(now_secs: f64, outcome: &str) -> String {
    format!("t={now_secs:.3} vibrancy={outcome}\n")
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
    let size = window.outer_size().ok().map(|s| (s.width, s.height));
    let visible = window.is_visible().ok();
    let line = format_anchor_line(
        now_secs(),
        expected,
        actual,
        size,
        visible,
        (mon_x, mon_y, mon_w, mon_h),
        scale,
    );
    let Ok(dir) = window.app_handle().path().app_data_dir() else {
        return;
    };
    write_anchor_log(&dir, &line);
}

/// Append one vibrancy evidence line for this window to the log in
/// the app data directory. Silent when the directory cannot be
/// resolved.
fn log_vibrancy(window: &tauri::Window, outcome: &str) {
    let line = format_vibrancy_line(now_secs(), outcome);
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
/// This is the only place that calls `set_size` in the crate. The
/// backdrop is (re-)applied here too, because moving the window can
/// disturb the DWM backdrop — and because this is the one function
/// every window mutation goes through (setup, the anchor re-assert,
/// expand / collapse), so the backdrop survives all of them.
/// Re-applying is idempotent, and the outcome is logged next to the
/// anchor line.
pub fn ensure_fixed_window(window: &tauri::Window) -> Result<(), tauri::Error> {
    window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
        f64::from(WINDOW_WIDTH),
        f64::from(WINDOW_HEIGHT),
    )))?;
    place_top_center(window)?;
    // Real backdrop material (Acrylic on Windows, vibrancy on
    // macOS). Best effort and non-fatal: where it is unavailable
    // the CSS glass is the whole look, unchanged.
    let outcome = crate::vibrancy::apply_island_vibrancy(window);
    log_vibrancy(window, outcome);
    // The diagnostics log gets the same outcome: on the next
    // install it says which backdrop path actually ran, next to
    // the setup steps — the anchor log alone cannot prove that.
    crate::diagnostics::log("vibrancy", outcome);
    Ok(())
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
/// logs again, like every other application — and re-applies the
/// backdrop with it, through `ensure_fixed_window`.
pub fn setup_window(window: &tauri::Window) -> Result<IslandState, tauri::Error> {
    let state = set_expanded(false);
    ensure_fixed_window(window)?;
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ANCHOR_REAPPLY_DELAY_MS));
        let _ = ensure_fixed_window(&window);
    });
    Ok(state)
}

/// Hover intent: the cursor must stay inside the hot zone for
/// [`HOVER_INTENT_MS`] before the entry counts. Pure and testable:
/// feed it the sampled inside/outside state and the sample time;
/// it returns `Some(true)` the moment the entry is confirmed,
/// `Some(false)` the moment the cursor leaves after a confirmed
/// entry, and `None` otherwise (still waiting, still inside, or a
/// leave before any entry was confirmed).
#[derive(Default)]
pub struct HoverIntent {
    entered_since: Option<Instant>,
    confirmed: bool,
}

impl HoverIntent {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, inside: bool, now: Instant) -> Option<bool> {
        if !inside {
            self.entered_since = None;
            if self.confirmed {
                self.confirmed = false;
                return Some(false);
            }
            return None;
        }
        if self.confirmed {
            return None;
        }
        match self.entered_since {
            None => {
                self.entered_since = Some(now);
                None
            }
            Some(since) => {
                if now.duration_since(since) >= Duration::from_millis(HOVER_INTENT_MS) {
                    self.confirmed = true;
                    Some(true)
                } else {
                    None
                }
            }
        }
    }
}

/// Whether a cursor at logical `(cursor_x, cursor_y)` on a monitor
/// `monitor_width` logical pixels wide is inside the hot zone: the
/// strip `0..=HOT_ZONE_HEIGHT` along the top edge.
pub fn is_in_hot_zone(cursor_x: f64, cursor_y: f64, monitor_width: f64) -> bool {
    (0.0..=monitor_width).contains(&cursor_x) && (0.0..=HOT_ZONE_HEIGHT).contains(&cursor_y)
}

/// Live cursor position in logical monitor coordinates
/// `(x, y, monitor_width)`, on Windows. `None` off Windows or
/// when the position cannot be read.
#[cfg(target_os = "windows")]
fn cursor_logical(window: &tauri::Window) -> Option<(f64, f64, f64)> {
    let (x, y) = win::cursor_position()?;
    let (mon_x, mon_y, mon_w, _) = logical_monitor_geometry(window);
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    Some((
        (f64::from(x) - mon_x * scale) / scale,
        (f64::from(y) - mon_y * scale) / scale,
        mon_w,
    ))
}

/// Whether the cursor is currently inside the hot zone of the
/// monitor the window is on. On Windows this reads the live cursor
/// position with `GetCursorPos`; off Windows it is always `false`.
pub fn cursor_in_hot_zone(window: &tauri::Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        let Some((x, y, mon_w)) = cursor_logical(window) else {
            return false;
        };
        is_in_hot_zone(x, y, mon_w)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        false
    }
}

/// Live cursor position in logical window coordinates, on Windows:
/// the monitor-logical position minus the window's top-center
/// origin. `None` off Windows or when the position cannot be read.
/// The window origin is `(centered_x, 0)`; the hit rectangle is
/// defined in these coordinates.
#[cfg(target_os = "windows")]
fn cursor_window_position(window: &tauri::Window) -> Option<(f64, f64)> {
    let (x, y, mon_w) = cursor_logical(window)?;
    Some((x - centered_x(mon_w, WINDOW_WIDTH), y - WINDOW_TOP_Y))
}

/// Whether the cursor is currently on the handle hit rectangle.
/// `false` off Windows or when the position cannot be read.
pub fn cursor_on_handle(window: &tauri::Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        let Some((x, y)) = cursor_window_position(window) else {
            return false;
        };
        cursor_in_handle_hit_rect(x, y)
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
/// position and emits [`HOT_ZONE_EVENT`] when the cursor enters
/// (after the hover-intent delay) or leaves the top-edge strip.
/// The very first poll also emits once at once when the cursor
/// starts outside, so the frontend can sync its initial state
/// without waiting; a cursor that starts inside still waits for
/// the intent delay, so the island is never woken by accident at
/// launch. The first emitted event is also written to the
/// diagnostics log, so the next install proves the watcher ran.
/// Call once from the `setup` hook in `lib.rs`. The thread runs for
/// the life of the process.
///
/// While the island is hidden, the same thread owns click-through:
/// only the handle hit rectangle stays interactive and the rest of
/// the window is click-through (re-applied only when the desired
/// value changes). While the island is visible the frontend owns
/// the flag and the watcher leaves it alone.
pub fn start_hot_zone_watcher(app: &tauri::AppHandle) {
    let Some(webview_window) = app.get_webview_window("main") else {
        return;
    };
    let window = webview_window.as_ref().window().clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let mut intent = HoverIntent::new();
        let mut first_poll = true;
        let mut first_event_logged = false;
        let mut last_ignore: Option<bool> = None;
        loop {
            let inside = cursor_in_hot_zone(&window);
            let mut emit: Option<bool> = intent.update(inside, Instant::now());
            if first_poll {
                first_poll = false;
                // Sync the initial state at once when the cursor
                // starts outside; inside still waits for intent.
                if !inside {
                    emit = Some(false);
                }
            }
            if let Some(in_hot_zone) = emit {
                let _ = app.emit(HOT_ZONE_EVENT, HotZoneEvent { in_hot_zone });
                if !first_event_logged {
                    first_event_logged = true;
                    crate::diagnostics::log(
                        "hot-zone",
                        &format!("first event: in_hot_zone={in_hot_zone}"),
                    );
                }
            }
            if is_hidden() {
                let ignore = !cursor_on_handle(&window);
                if last_ignore != Some(ignore) {
                    last_ignore = Some(ignore);
                    let _ = window.set_ignore_cursor_events(ignore);
                }
            } else {
                last_ignore = None;
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
        assert_eq!(window_size(), (760, 440));
        // The content box IS the expanded panel (560x360, approved
        // design), with equal glow padding on every side.
        assert_eq!(content_rect(), (40, 40, 680, 360));
        assert_eq!(CONTENT_X, GLOW_PADDING);
        assert_eq!(CONTENT_Y, GLOW_PADDING);
        assert_eq!(WINDOW_WIDTH - (CONTENT_X + CONTENT_WIDTH), GLOW_PADDING);
        assert_eq!(WINDOW_HEIGHT - (CONTENT_Y + CONTENT_HEIGHT), GLOW_PADDING);
    }

    #[test]
    fn state_reports_fixed_size() {
        let state = current_state();
        assert_eq!((state.width, state.height), (760, 440));
        assert_eq!(
            (
                state.content_x,
                state.content_y,
                state.content_width,
                state.content_height
            ),
            (40, 40, 680, 360)
        );
        assert_eq!(state.compact_height, 64);
        assert_eq!((state.handle_width, state.handle_height), (120, 4));
        assert_eq!((state.handle_hit_width, state.handle_hit_height), (160, 12));
        assert_eq!(state.hot_zone_height, 12.0);
        assert_eq!(state.hover_intent_ms, 120);
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
            Some((800, 550)),
            Some(true),
            (0.0, 0.0, 1920.0, 1080.0),
            1.25,
        );
        assert!(line.contains("t=1234.500"));
        assert!(line.contains("expected=(640.0,0.0)"));
        assert!(line.contains("actual=(640,0)"));
        assert!(line.contains("size=800x550"));
        assert!(line.contains("visible=true"));
        assert!(line.contains("monitor=(0,0,1920x1080)"));
        assert!(line.contains("scale=1.25"));
        let missing = format_anchor_line(
            1.0,
            (640.0, 0.0),
            None,
            None,
            None,
            (0.0, 0.0, 1920.0, 1080.0),
            1.0,
        );
        assert!(missing.contains("actual=ERR"));
        assert!(missing.contains("size=ERR"));
        assert!(missing.contains("visible=ERR"));
    }

    #[test]
    fn vibrancy_log_line_carries_outcome() {
        let line = format_vibrancy_line(1.5, "acrylic-dwm");
        assert!(line.contains("t=1.500"));
        assert!(line.contains("vibrancy=acrylic-dwm"));
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
            Some((800, 550)),
            Some(true),
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
        assert!(is_in_hot_zone(320.0, 12.0, 1920.0));
        assert!(is_in_hot_zone(0.0, 3.0, 1920.0));
        assert!(is_in_hot_zone(1920.0, 3.0, 1920.0));
        assert!(!is_in_hot_zone(320.0, 12.1, 1920.0));
        assert!(!is_in_hot_zone(320.0, -1.0, 1920.0));
        assert!(!is_in_hot_zone(-1.0, 3.0, 1920.0));
        assert!(!is_in_hot_zone(1920.1, 3.0, 1920.0));
    }

    #[test]
    fn handle_geometry_agrees() {
        // The visible bar sits at the very top of the window
        // (stage offset 0), centered; the hit rectangle around it
        // is top-center at y=0 too, with room to grab.
        assert_eq!(handle_bar_rect(), (320.0, 0.0, 120.0, 4.0));
        assert_eq!(handle_hit_rect(), (300.0, 0.0, 160.0, 12.0));
        assert!(rect_contains(handle_hit_rect(), handle_bar_rect()));
        // The hot zone is exactly tall enough to contain the whole
        // hit rectangle: the strip the watcher watches and the
        // strip the user grabs are the same strip.
        assert!(HOT_ZONE_HEIGHT >= HANDLE_HIT_HEIGHT as f64);
        assert_eq!(HOT_ZONE_HEIGHT, 12.0);
        assert!(rect_contains(
            (0.0, 0.0, f64::from(WINDOW_WIDTH), HOT_ZONE_HEIGHT),
            handle_hit_rect()
        ));
        // The bar is inside the hot zone as well.
        assert!(rect_contains(
            (0.0, 0.0, f64::from(WINDOW_WIDTH), HOT_ZONE_HEIGHT),
            handle_bar_rect()
        ));
    }

    #[test]
    fn hit_test_covers_only_the_handle() {
        // Inside: center, corners (edges count).
        assert!(cursor_in_handle_hit_rect(380.0, 6.0));
        assert!(cursor_in_handle_hit_rect(300.0, 0.0));
        assert!(cursor_in_handle_hit_rect(460.0, 12.0));
        // Outside: one pixel past every side.
        assert!(!cursor_in_handle_hit_rect(299.0, 6.0));
        assert!(!cursor_in_handle_hit_rect(461.0, 6.0));
        assert!(!cursor_in_handle_hit_rect(380.0, 13.0));
        assert!(!cursor_in_handle_hit_rect(380.0, -1.0));
        // The rest of the window (below the handle) is not a hit.
        assert!(!cursor_in_handle_hit_rect(380.0, 40.0));
        assert!(!cursor_in_handle_hit_rect(0.0, 0.0));
        assert!(!cursor_in_handle_hit_rect(760.0, 0.0));
    }

    #[test]
    fn hover_intent_waits_then_confirms() {
        let mut intent = HoverIntent::new();
        let t0 = Instant::now();
        // Entering: nothing until the delay has passed.
        assert_eq!(intent.update(true, t0), None);
        assert_eq!(intent.update(true, t0 + Duration::from_millis(100)), None);
        assert_eq!(intent.update(true, t0 + Duration::from_millis(119)), None);
        assert_eq!(
            intent.update(true, t0 + Duration::from_millis(120)),
            Some(true)
        );
        // Staying inside: no repeat events.
        assert_eq!(intent.update(true, t0 + Duration::from_millis(500)), None);
        // Leaving after a confirmed entry: one exit event.
        assert_eq!(
            intent.update(false, t0 + Duration::from_millis(600)),
            Some(false)
        );
        assert_eq!(intent.update(false, t0 + Duration::from_millis(700)), None);
        // Leaving before confirmation: no event at all, and the
        // next entry starts the timer over.
        let mut fresh = HoverIntent::new();
        assert_eq!(fresh.update(true, t0), None);
        assert_eq!(fresh.update(true, t0 + Duration::from_millis(50)), None);
        assert_eq!(fresh.update(false, t0 + Duration::from_millis(60)), None);
        assert_eq!(fresh.update(true, t0 + Duration::from_millis(70)), None);
        assert_eq!(fresh.update(true, t0 + Duration::from_millis(189)), None);
        assert_eq!(
            fresh.update(true, t0 + Duration::from_millis(190)),
            Some(true)
        );
    }

    #[test]
    fn hidden_flag_round_trips() {
        // The flag is set by the `log_visibility_transition`
        // command and read by the hot-zone watcher. It starts
        // `false` (visible at startup); the frontend reports the
        // first transition. Reset afterwards so test order cannot
        // leak state into other tests.
        set_hidden(true);
        assert!(is_hidden());
        set_hidden(false);
        assert!(!is_hidden());
    }
}
