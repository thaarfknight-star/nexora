//! Desktop wallpaper reader for the optional "Desktop refraction (beta)"
//! Liquid Glass mode.
//!
//! Reads the current Windows wallpaper path via `SystemParametersInfoW`
//! (`SPI_GETDESKWALLPAPER`) and fit mode (`WallpaperStyle` / `TileWallpaper`)
//! from `HKCU\Control Panel\Desktop`, together with the island window's live
//! monitor and screen coordinates so the frontend can align a bezel-ring
//! refraction canvas to the real desktop wallpaper.

use serde::Serialize;

/// Maximum wallpaper file size read into memory (16 MB) to prevent excessive
/// IPC payloads on unusually huge uncompressed BMP/PNG wallpapers.
const MAX_WALLPAPER_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct DesktopWallpaperInfo {
    /// Base64 data URL of the current wallpaper image (`None` when unavailable
    /// or off Windows).
    pub data_url: Option<String>,
    /// Normalized Windows fit mode: `"fill"`, `"fit"`, `"stretch"`, `"tile"`,
    /// `"center"`, or `"span"`.
    pub fit_style: String,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_w: u32,
    pub monitor_h: u32,
    pub window_x: i32,
    pub window_y: i32,
    pub scale_factor: f64,
}

/// Maps Windows registry `WallpaperStyle` + `TileWallpaper` values into a
/// clean fit mode string.
pub fn parse_wallpaper_fit(style: &str, tile: &str) -> &'static str {
    if tile.trim() == "1" {
        return "tile";
    }
    match style.trim() {
        "0" => "center",
        "2" => "stretch",
        "6" => "fit",
        "22" => "span",
        _ => "fill", // "10" and default Windows 10/11 Fill mode
    }
}

/// Infers an image MIME type from a wallpaper file path.
pub fn mime_for_wallpaper_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else {
        "image/jpeg"
    }
}

pub fn collect_for_window(window: &tauri::Window) -> DesktopWallpaperInfo {
    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let (window_x, window_y) = window
        .outer_position()
        .map(|p| (p.x, p.y))
        .unwrap_or((0, 0));
    let (monitor_x, monitor_y, monitor_w, monitor_h) = window
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let pos = m.position();
            let sz = m.size();
            (pos.x, pos.y, sz.width, sz.height)
        })
        .unwrap_or((0, 0, 1920, 1080));

    #[cfg(target_os = "windows")]
    let (data_url, fit_style) = win::read_wallpaper();
    #[cfg(not(target_os = "windows"))]
    let (data_url, fit_style) = (None, "fill".to_string());

    DesktopWallpaperInfo {
        data_url,
        fit_style,
        monitor_x,
        monitor_y,
        monitor_w,
        monitor_h,
        window_x,
        window_y,
        scale_factor,
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use base64::Engine as _;
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_reg_string(subkey: &str, value_name: &str) -> String {
        let sub = wide(subkey);
        let val = wide(value_name);
        let mut buf = [0u16; 64];
        let mut bytes: u32 = (buf.len() * std::mem::size_of::<u16>()) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR::from_raw(sub.as_ptr()),
                PCWSTR::from_raw(val.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut c_void),
                Some(&mut bytes),
            )
        };
        if status.is_err() {
            return String::new();
        }
        let chars = (bytes as usize) / 2;
        let end = buf[..chars.min(buf.len())]
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(chars.min(buf.len()));
        String::from_utf16_lossy(&buf[..end])
    }

    pub fn read_wallpaper() -> (Option<String>, String) {
        let style_raw = read_reg_string("Control Panel\\Desktop", "WallpaperStyle");
        let tile_raw = read_reg_string("Control Panel\\Desktop", "TileWallpaper");
        let fit_style = parse_wallpaper_fit(&style_raw, &tile_raw).to_string();

        let mut path_buf = [0u16; 1024];
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETDESKWALLPAPER,
                path_buf.len() as u32,
                Some(path_buf.as_mut_ptr() as *mut c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .is_ok();
        if !ok {
            return (None, fit_style);
        }
        let end = path_buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(path_buf.len());
        let path_str = String::from_utf16_lossy(&path_buf[..end]);
        if path_str.trim().is_empty() {
            return (None, fit_style);
        }
        let Ok(meta) = std::fs::metadata(&path_str) else {
            return (None, fit_style);
        };
        if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_WALLPAPER_BYTES {
            return (None, fit_style);
        }
        let Ok(bytes) = std::fs::read(&path_str) else {
            return (None, fit_style);
        };
        let mime = mime_for_wallpaper_path(&path_str);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        (Some(format!("data:{mime};base64,{b64}")), fit_style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_fit_parses_windows_registry_styles() {
        assert_eq!(parse_wallpaper_fit("10", "0"), "fill");
        assert_eq!(parse_wallpaper_fit("6", "0"), "fit");
        assert_eq!(parse_wallpaper_fit("2", "0"), "stretch");
        assert_eq!(parse_wallpaper_fit("0", "0"), "center");
        assert_eq!(parse_wallpaper_fit("22", "0"), "span");
        assert_eq!(parse_wallpaper_fit("0", "1"), "tile");
        assert_eq!(parse_wallpaper_fit("", ""), "fill");
    }

    #[test]
    fn wallpaper_mime_infers_common_formats() {
        assert_eq!(mime_for_wallpaper_path("C:\\wall.png"), "image/png");
        assert_eq!(mime_for_wallpaper_path("C:\\wall.WEBP"), "image/webp");
        assert_eq!(mime_for_wallpaper_path("C:\\wall.bmp"), "image/bmp");
        assert_eq!(
            mime_for_wallpaper_path("C:\\TranscodedWallpaper"),
            "image/jpeg"
        );
    }
}
