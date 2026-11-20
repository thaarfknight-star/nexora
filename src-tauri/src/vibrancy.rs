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
//! Acrylic (not Mica) was chosen because it covers both Windows 10
//! (v1809+) and Windows 11 with one call; Mica is Windows 11-only
//! and would need version gating for no visible gain on this dark
//! glass design. On Windows 11 the OS draws its own acrylic and
//! ignores the tint; on Windows 10 the tint below is used.
//!
//! Application is best effort and non-fatal: setup must never fail
//! because of this, so errors are logged and the island keeps its
//! CSS glass.

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

/// Apply the island's backdrop material. Best effort: logs and
/// continues on error; never panics. Call once from
/// `island::setup_window`, after the fixed window is in place.
pub fn apply_island_vibrancy(window: &tauri::Window) {
    #[cfg(target_os = "windows")]
    {
        if let Err(e) = window_vibrancy::apply_acrylic(window, Some(ACRYLIC_TINT)) {
            eprintln!("[nexora] acrylic unavailable: {e}");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = window_vibrancy::apply_vibrancy(window, MACOS_MATERIAL, None, None) {
            eprintln!("[nexora] vibrancy unavailable: {e}");
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = window;
    }
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
}
