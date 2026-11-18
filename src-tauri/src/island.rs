use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;

/// Compact (collapsed) island size, in logical pixels.
/// Matches the initial window size in `tauri.conf.json`.
pub const COMPACT_WIDTH: u32 = 420;
pub const COMPACT_HEIGHT: u32 = 120;
/// Expanded island size, in logical pixels. Large enough for the
/// media controls / stats content the frontend shows when expanded.
pub const EXPANDED_WIDTH: u32 = 560;
pub const EXPANDED_HEIGHT: u32 = 340;
/// Gap between the top edge of the primary monitor and the island.
pub const TOP_MARGIN: f64 = 12.0;
/// Resize animation: number of steps and delay between steps
/// (12 steps x 15 ms = ~180 ms total).
pub const ANIMATION_STEPS: u32 = 12;
pub const ANIMATION_STEP_MS: u64 = 15;

#[derive(Debug, Clone, Serialize)]
pub struct IslandState {
    pub expanded: bool,
    pub mode: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

static EXPANDED: Mutex<bool> = Mutex::new(false);

/// Window size for a given mode.
pub fn size_for(expanded: bool) -> (u32, u32) {
    if expanded {
        (EXPANDED_WIDTH, EXPANDED_HEIGHT)
    } else {
        (COMPACT_WIDTH, COMPACT_HEIGHT)
    }
}

pub fn current_state() -> IslandState {
    let expanded = *EXPANDED.lock().unwrap_or_else(|e| e.into_inner());
    let (width, height) = size_for(expanded);
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
    }
}

/// State-only toggle, kept for the existing `set_island_expanded` command.
/// It updates the flag and the reported size but cannot move the window:
/// use [`set_expanded_on_window`] when a `Window` handle is available.
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

/// Linear interpolation between two sizes. Step `steps` returns `to`
/// exactly; step 0 returns `from`.
pub fn lerp_size(from: (u32, u32), to: (u32, u32), step: u32, steps: u32) -> (u32, u32) {
    if steps == 0 || step >= steps {
        return to;
    }
    let t = f64::from(step) / f64::from(steps);
    let lerp = |a: u32, b: u32| -> u32 {
        (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u32
    };
    (lerp(from.0, to.0), lerp(from.1, to.1))
}

/// Logical size of the monitor the window is currently on.
/// Falls back to 1920x1080 when no monitor can be queried (headless/CI).
fn logical_monitor_size(window: &tauri::Window) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    match monitor {
        Some(monitor) => {
            let size = monitor.size();
            (
                f64::from(size.width) / scale,
                f64::from(size.height) / scale,
            )
        }
        None => (1920.0, 1080.0),
    }
}

/// Top-center position for a window of `width` logical pixels:
/// horizontally centered on its monitor, `TOP_MARGIN` below the top edge.
pub fn top_center_position(window: &tauri::Window, width: u32) -> tauri::LogicalPosition<f64> {
    let (monitor_width, _) = logical_monitor_size(window);
    tauri::LogicalPosition::new(centered_x(monitor_width, width), TOP_MARGIN)
}

/// Move the window to the top-center position for its current size.
pub fn place_top_center(window: &tauri::Window) -> Result<(), tauri::Error> {
    let state = current_state();
    window.set_position(tauri::Position::Logical(top_center_position(
        window,
        state.width,
    )))
}

/// Current window size in logical pixels. Falls back to the opposite of
/// the target size so the animation still has a start point.
fn current_logical_size(window: &tauri::Window, target: (u32, u32)) -> (u32, u32) {
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    match window.inner_size() {
        Ok(size) => (
            (f64::from(size.width) / scale).round() as u32,
            (f64::from(size.height) / scale).round() as u32,
        ),
        Err(_) => {
            if target == size_for(true) {
                size_for(false)
            } else {
                size_for(true)
            }
        }
    }
}

/// Animate the window from its current size to `to` in small steps,
/// re-centering it horizontally at every step so it stays top-center
/// while it grows/shrinks. Runs on a detached thread; errors are
/// ignored per step because the window may be closed mid-animation.
fn animate_resize(window: tauri::Window, from: (u32, u32), to: (u32, u32)) {
    std::thread::spawn(move || {
        for step in 1..=ANIMATION_STEPS {
            let (width, height) = lerp_size(from, to, step, ANIMATION_STEPS);
            let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                f64::from(width),
                f64::from(height),
            )));
            let _ = window.set_position(tauri::Position::Logical(top_center_position(
                &window, width,
            )));
            std::thread::sleep(Duration::from_millis(ANIMATION_STEP_MS));
        }
        // Guarantee the exact final size and position.
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            f64::from(to.0),
            f64::from(to.1),
        )));
        let _ = window.set_position(tauri::Position::Logical(top_center_position(&window, to.0)));
    });
}

/// Expand/collapse the island on a real window: updates the state,
/// then animates the resize and keeps the window top-centered.
/// Returns the new state immediately; the animation finishes async
/// (~180 ms). This is the function IPC wiring should call
/// (owner: EAgent5, `ipc.rs`); `set_expanded` stays as the state-only
/// fallback for callers without a window handle.
pub fn set_expanded_on_window(
    window: &tauri::Window,
    expanded: bool,
) -> Result<IslandState, tauri::Error> {
    let state = set_expanded(expanded);
    let target = (state.width, state.height);
    let from = current_logical_size(window, target);
    if from != target {
        animate_resize(window.clone(), from, target);
    } else {
        place_top_center(window)?;
    }
    Ok(state)
}

/// Alias for [`set_expanded_on_window`].
pub fn apply_to_window(
    window: &tauri::Window,
    expanded: bool,
) -> Result<IslandState, tauri::Error> {
    set_expanded_on_window(window, expanded)
}

/// Place the island at its startup position: compact size, top-center.
/// Intended for the `setup` hook in `lib.rs` (coordinated via DEVagent).
pub fn setup_window(window: &tauri::Window) -> Result<IslandState, tauri::Error> {
    let state = set_expanded(false);
    window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
        f64::from(state.width),
        f64::from(state.height),
    )))?;
    place_top_center(window)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_match_config() {
        assert_eq!(size_for(false), (420, 120));
        assert_eq!(size_for(true), (560, 340));
        assert_ne!(size_for(false), size_for(true));
    }

    #[test]
    fn centered_x_is_half_the_gap() {
        assert_eq!(centered_x(1920.0, 420), 750.0);
        assert_eq!(centered_x(560.0, 560), 0.0);
        assert_eq!(centered_x(400.0, 560), 0.0);
    }

    #[test]
    fn lerp_size_steps_are_monotonic_and_exact() {
        let from = (420, 120);
        let to = (560, 340);
        assert_eq!(lerp_size(from, to, 0, 12), from);
        assert_eq!(lerp_size(from, to, 12, 12), to);
        let mid = lerp_size(from, to, 6, 12);
        assert_eq!(mid, (490, 230));
        let mut prev = from;
        for step in 1..=12 {
            let next = lerp_size(from, to, step, 12);
            assert!(next.0 >= prev.0 && next.1 >= prev.1);
            prev = next;
        }
    }
}
