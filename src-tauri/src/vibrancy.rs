//! Real backdrop material for the island window: Acrylic on Windows,
//! vibrancy on macOS — with the CSS glass as the fallback.
//!
//! The CSS (`--island-bg` + `backdrop-filter: blur(...)`) is the
//! visual fallback and is never touched by this module: on Linux, on
//! old Windows, or when the OS refuses the effect, the island looks
//! exactly as it does today. Where the OS material is available it
//! only adds depth behind the semi-transparent card and in the
//! transparent glow margin around it — the design does not change.
//!
//! Windows, in detail (the gray-margin incident, 2026-11-20): the
//! acrylic call alone is not enough. Tao makes the window
//! transparent with `DwmEnableBlurBehindWindow` and an EMPTY blur
//! region; setting a system backdrop on top of that leaves DWM with
//! nothing to sample, so it renders only the backdrop's flat tint —
//! on Taha's install a uniform (84,84,84) slab over the whole
//! window rect, while the panel itself composited correctly on top
//! of it. So before setting the backdrop we clear the empty-region
//! blur-behind; the backdrop itself is the transparency from there
//! on. If the backdrop call fails (Windows 10, where the attribute
//! does not exist) we restore tao's empty-region blur-behind and
//! fall back to the legacy SWCA acrylic. Acrylic (not Mica) covers
//! both Windows 10 (v1809+) and Windows 11 with one call; Mica is
//! Windows 11-only and would need version gating for no visible
//! gain on this dark glass design. On Windows 11 the OS draws its
//! own acrylic and ignores the tint; on Windows 10 the tint below
//! is used.
//!
//! Application is best effort and non-fatal: setup must never fail
//! because of this, so errors are logged and the island keeps its
//! CSS glass. The return value says which path actually ran, so the
//! caller can write it to the anchor log — the next install shows
//! what happened instead of guessing.

/// Tint of the acrylic on Windows 10 (Windows 11 ignores it).
/// Matches the default theme (neon-glass `--island-bg`:
/// `rgb(24, 10, 54)`).
pub const ACRYLIC_TINT: window_vibrancy::Color = (24, 10, 54, 125);

/// Material of the vibrancy on macOS: the dark HUD material, the
/// closest match to the island's dark glass.
pub const MACOS_MATERIAL: window_vibrancy::NSVisualEffectMaterial =
    window_vibrancy::NSVisualEffectMaterial::HudWindow;

/// Whether this OS can have a real backdrop material at all.
pub const fn vibrancy_available() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

/// Outcome of [`apply_island_vibrancy`]: which path actually ran.
/// Written to the anchor log by the caller.
pub const OUTCOME_DWM: &str = "acrylic-dwm";
pub const OUTCOME_SWCA: &str = "acrylic-swca";
pub const OUTCOME_MACOS: &str = "vibrancy-macos";
pub const OUTCOME_CSS: &str = "css-fallback";

/// Apply the island's backdrop material. Best effort: logs and
/// continues on error; never panics. Returns what actually ran.
/// Called from `island::ensure_fixed_window`, after the fixed size
/// and position are in place — so it also re-applies after the
/// anchor re-assert and on every expand / collapse, because moving
/// the window can disturb the DWM backdrop (re-applying is
/// idempotent).
pub fn apply_island_vibrancy(window: &tauri::Window) -> &'static str {
    #[cfg(target_os = "windows")]
    return apply_island_vibrancy_windows(window);
    #[cfg(target_os = "macos")]
    return match window_vibrancy::apply_vibrancy(window, MACOS_MATERIAL, None, None) {
        Ok(()) => OUTCOME_MACOS,
        Err(e) => {
            eprintln!("[nexora] vibrancy unavailable: {e}");
            OUTCOME_CSS
        }
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = window;
        OUTCOME_CSS
    }
}

/// Windows: clear tao's empty-region blur-behind, set the modern
/// DWM backdrop, fall back to the legacy SWCA acrylic on Windows 10.
#[cfg(target_os = "windows")]
fn apply_island_vibrancy_windows(window: &tauri::Window) -> &'static str {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
    };

    // The raw handle, not `window.hwnd()`: tauri's `hwnd()` returns
    // the `windows` 0.62 `HWND`, while this crate uses the 0.61 one
    // — the two types are identical but distinct, so passing one to
    // the other does not compile. The raw pointer is the same either
    // way.
    let Ok(handle) = window.window_handle() else {
        eprintln!("[nexora] acrylic unavailable: no window handle");
        return OUTCOME_CSS;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        eprintln!("[nexora] acrylic unavailable: not a Win32 window");
        return OUTCOME_CSS;
    };
    let hwnd = windows::Win32::Foundation::HWND(win32.hwnd.get() as *mut std::ffi::c_void);

    // Clear tao's empty-region blur-behind (see the module docs):
    // it conflicts with the system backdrop and leaves DWM with
    // nothing to sample, so the backdrop renders as a flat tint.
    disable_blur_behind(hwnd);

    // Modern path first, on every Windows version: the backdrop
    // type exists since Windows 11 (22000). It fails on Windows 10,
    // where the legacy path below takes over.
    let backdrop = DWMSBT_TRANSIENTWINDOW;
    let set = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&backdrop) as u32,
        )
    };
    if set.is_ok() {
        return OUTCOME_DWM;
    }
    eprintln!("[nexora] DWM backdrop unavailable ({set:?}); restoring transparency and trying SWCA acrylic");

    // The backdrop failed (Windows 10): put tao's transparency back
    // first, then the legacy acrylic.
    restore_blur_behind(hwnd);
    match window_vibrancy::apply_acrylic(window, Some(ACRYLIC_TINT)) {
        Ok(()) => OUTCOME_SWCA,
        Err(e) => {
            eprintln!("[nexora] acrylic unavailable: {e}");
            OUTCOME_CSS
        }
    }
}

/// Disable DWM blur-behind on `hwnd` entirely.
#[cfg(target_os = "windows")]
fn disable_blur_behind(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Dwm::{DwmEnableBlurBehindWindow, DWM_BB_ENABLE, DWM_BLURBEHIND};
    let off = DWM_BLURBEHIND {
        dwFlags: DWM_BB_ENABLE,
        fEnable: false.into(),
        hRgnBlur: Default::default(),
        fTransitionOnMaximized: false.into(),
    };
    let _ = unsafe { DwmEnableBlurBehindWindow(hwnd, &off) };
}

/// Restore tao's transparency recipe: blur-behind enabled with an
/// EMPTY region, so the window is fully transparent (the desktop
/// shows through where the page is transparent). This is the exact
/// recipe from tao's window creation (`CreateRectRgn(0, 0, -1, -1)`).
#[cfg(target_os = "windows")]
fn restore_blur_behind(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Dwm::{
        DwmEnableBlurBehindWindow, DWM_BB_BLURREGION, DWM_BB_ENABLE, DWM_BLURBEHIND,
    };
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject};
    let region = unsafe { CreateRectRgn(0, 0, -1, -1) };
    let bb = DWM_BLURBEHIND {
        dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
        fEnable: true.into(),
        hRgnBlur: region,
        fTransitionOnMaximized: false.into(),
    };
    let _ = unsafe { DwmEnableBlurBehindWindow(hwnd, &bb) };
    let _ = unsafe { DeleteObject(region.into()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acrylic_tint_matches_default_theme() {
        // neon-glass `--island-bg` is rgb(24, 10, 54); the alpha is
        // the acrylic tint strength (the crate's own example uses 125).
        assert_eq!(ACRYLIC_TINT, (24, 10, 54, 125));
    }

    #[test]
    fn macos_material_is_the_dark_hud_material() {
        assert_eq!(
            MACOS_MATERIAL,
            window_vibrancy::NSVisualEffectMaterial::HudWindow
        );
    }

    #[test]
    fn vibrancy_availability_matches_the_platform() {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        assert!(vibrancy_available());
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        assert!(!vibrancy_available());
    }

    #[test]
    fn vibrancy_outcomes_are_distinct_labels() {
        // The anchor log tells the paths apart by these strings;
        // two paths must never share one.
        assert_ne!(OUTCOME_DWM, OUTCOME_SWCA);
        assert_ne!(OUTCOME_DWM, OUTCOME_MACOS);
        assert_ne!(OUTCOME_DWM, OUTCOME_CSS);
        assert_ne!(OUTCOME_SWCA, OUTCOME_MACOS);
        assert_ne!(OUTCOME_SWCA, OUTCOME_CSS);
        assert_ne!(OUTCOME_MACOS, OUTCOME_CSS);
    }
}
