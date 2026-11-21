//! Backdrop material for the island window.
//!
//! On Windows, DWM system backdrops (`DWMWA_SYSTEMBACKDROP_TYPE` / SWCA
//! acrylic) paint across the entire rectangular 640x440 HWND — including the
//! transparent glow margin around the rounded pill — which creates a dark
//! rectangular border around the island. Keeping Windows on the CSS glass
//! fallback preserves per-pixel transparency outside the pill.

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
pub fn apply_island_vibrancy(window: &tauri::Window) -> &'static str {
    #[cfg(target_os = "macos")]
    return match window_vibrancy::apply_vibrancy(window, MACOS_MATERIAL, None, None) {
        Ok(()) => OUTCOME_MACOS,
        Err(e) => {
            eprintln!("[nexora] vibrancy unavailable: {e}");
            OUTCOME_CSS
        }
    };
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        OUTCOME_CSS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acrylic_tint_matches_default_theme() {
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
        assert_ne!(OUTCOME_DWM, OUTCOME_SWCA);
        assert_ne!(OUTCOME_DWM, OUTCOME_MACOS);
        assert_ne!(OUTCOME_DWM, OUTCOME_CSS);
        assert_ne!(OUTCOME_SWCA, OUTCOME_MACOS);
        assert_ne!(OUTCOME_SWCA, OUTCOME_CSS);
        assert_ne!(OUTCOME_MACOS, OUTCOME_CSS);
    }
}
