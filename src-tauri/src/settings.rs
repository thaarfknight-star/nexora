use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// File name of the persisted settings inside the app data dir.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

fn default_always_on_top() -> bool {
    true
}

fn default_theme() -> String {
    "neon-glass".into()
}

fn default_clock_24h() -> bool {
    true
}

fn default_date_format() -> String {
    "short".into()
}

fn default_poll_interval_secs() -> u64 {
    3
}

fn default_stats_speed_unit() -> String {
    "auto".into()
}

fn default_stats_percent_style() -> String {
    "with".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NexoraSettings {
    #[serde(default)]
    pub auto_hide_taskbar: bool,
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Island blur in px; `None` means the theme default, `Some(v)` is a
    /// user override (the frontend clamps the slider value).
    #[serde(default)]
    pub island_blur_px: Option<u8>,
    /// Island corner radius in px; same `None`/`Some` meaning as blur.
    #[serde(default)]
    pub island_radius_px: Option<u8>,
    /// Clock in 24-hour format (`false` is 12-hour).
    #[serde(default = "default_clock_24h")]
    pub clock_24h: bool,
    /// Clock shows seconds.
    #[serde(default)]
    pub show_seconds: bool,
    /// Date format: `"short"` or `"full"`.
    #[serde(default = "default_date_format")]
    pub date_format: String,
    /// How often system stats are polled, in seconds.
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    /// Stats speed unit: `"auto"`, `"kb"` or `"mb"`.
    #[serde(default = "default_stats_speed_unit")]
    pub stats_speed_unit: String,
    /// Stats percent style: `"with"` shows the `%` sign, `"without"` hides it.
    #[serde(default = "default_stats_percent_style")]
    pub stats_percent_style: String,
}

impl Default for NexoraSettings {
    fn default() -> Self {
        defaults()
    }
}

pub fn defaults() -> NexoraSettings {
    NexoraSettings {
        auto_hide_taskbar: false,
        always_on_top: true,
        theme: "neon-glass".into(),
        island_blur_px: None,
        island_radius_px: None,
        clock_24h: true,
        show_seconds: false,
        date_format: "short".into(),
        poll_interval_secs: 3,
        stats_speed_unit: "auto".into(),
        stats_percent_style: "with".into(),
    }
}

impl NexoraSettings {
    /// Repairs values that cannot be applied: an empty theme falls back
    /// to the default theme. Theme names are not restricted here on
    /// purpose — the theme list lives in the frontend (`src/themes.ts`).
    fn normalized(mut self) -> Self {
        if self.theme.trim().is_empty() {
            self.theme = default_theme();
        } else {
            self.theme = self.theme.trim().to_string();
        }
        self
    }
}

/// Full path of the settings file for the given app data dir.
pub fn settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SETTINGS_FILE_NAME)
}

fn app_data_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok()
}

/// Loads settings from the given app data dir. A missing, unreadable or
/// corrupt file falls back to [`defaults`], so the app always starts.
pub fn load_from_dir(app_data_dir: &Path) -> NexoraSettings {
    fs::read_to_string(settings_path(app_data_dir))
        .ok()
        .and_then(|contents| serde_json::from_str::<NexoraSettings>(&contents).ok())
        .map(NexoraSettings::normalized)
        .unwrap_or_else(defaults)
}

/// Loads settings for the running app. Falls back to [`defaults`] when
/// the app data dir cannot be resolved.
pub fn load(app: &AppHandle) -> NexoraSettings {
    app_data_dir(app).map_or_else(defaults, |dir| load_from_dir(&dir))
}

/// Persists settings into the given app data dir, creating it when needed.
/// The write is atomic: content goes to a temp file first and is then
/// renamed over `settings.json`.
pub fn save_to_dir(app_data_dir: &Path, settings: &NexoraSettings) -> io::Result<()> {
    fs::create_dir_all(app_data_dir)?;
    let settings = settings.clone().normalized();
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let path = settings_path(app_data_dir);
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, json)?;
    match fs::rename(&tmp_path, &path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp_path);
            Err(err)
        }
    }
}

/// Persists settings for the running app.
pub fn save(app: &AppHandle, settings: &NexoraSettings) -> io::Result<()> {
    let Some(dir) = app_data_dir(app) else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "app data dir is not available",
        ));
    };
    save_to_dir(&dir, settings)
}

/// Applies the settings that the backend owns to the running app.
/// `always_on_top` is applied to the main window; the theme is applied
/// by the frontend, which reads it through `get_settings`.
/// Never fails: there may be no main window yet (for example in tests).
pub fn apply(app: &AppHandle, settings: &NexoraSettings) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_always_on_top(settings.always_on_top);
    }
}

/// The real setter: normalizes, persists and applies the settings,
/// then returns the stored values. This is what the `set_settings`
/// command (owned by the IPC wiring) calls.
pub fn update(app: &AppHandle, settings: NexoraSettings) -> io::Result<NexoraSettings> {
    let settings = settings.normalized();
    save(app, &settings)?;
    apply(app, &settings);
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("nexora-settings-test-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_returns_defaults() {
        let dir = test_dir();
        assert_eq!(load_from_dir(&dir), defaults());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_persists_all_fields() {
        let dir = test_dir();
        let settings = NexoraSettings {
            auto_hide_taskbar: true,
            always_on_top: false,
            theme: "liquid-glass".into(),
            island_blur_px: Some(12),
            island_radius_px: Some(24),
            clock_24h: false,
            show_seconds: true,
            date_format: "full".into(),
            poll_interval_secs: 5,
            stats_speed_unit: "mb".into(),
            stats_percent_style: "without".into(),
        };
        save_to_dir(&dir, &settings).expect("save must succeed");
        assert_eq!(load_from_dir(&dir), settings);
        assert!(settings_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_creates_parent_dirs() {
        let base = test_dir();
        let dir = base.join("nested").join("deep");
        save_to_dir(&dir, &defaults()).expect("save must create parent dirs");
        assert_eq!(load_from_dir(&dir), defaults());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn partial_file_fills_missing_fields_with_defaults() {
        let dir = test_dir();
        fs::create_dir_all(&dir).expect("create test dir");
        fs::write(settings_path(&dir), r#"{"theme": "sunset"}"#).expect("write partial file");
        assert_eq!(
            load_from_dir(&dir),
            NexoraSettings {
                auto_hide_taskbar: false,
                always_on_top: true,
                theme: "sunset".into(),
                island_blur_px: None,
                island_radius_px: None,
                clock_24h: true,
                show_seconds: false,
                date_format: "short".into(),
                poll_interval_secs: 3,
                stats_speed_unit: "auto".into(),
                stats_percent_style: "with".into(),
            }
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_returns_defaults() {
        let dir = test_dir();
        fs::create_dir_all(&dir).expect("create test dir");
        fs::write(settings_path(&dir), "{not valid json").expect("write corrupt file");
        assert_eq!(load_from_dir(&dir), defaults());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_theme_falls_back_to_default() {
        let dir = test_dir();
        let settings = NexoraSettings {
            auto_hide_taskbar: false,
            always_on_top: true,
            theme: "   ".into(),
            island_blur_px: None,
            island_radius_px: None,
            clock_24h: true,
            show_seconds: false,
            date_format: "short".into(),
            poll_interval_secs: 3,
            stats_speed_unit: "auto".into(),
            stats_percent_style: "with".into(),
        };
        save_to_dir(&dir, &settings).expect("save must succeed");
        assert_eq!(load_from_dir(&dir).theme, "neon-glass");
        let _ = fs::remove_dir_all(&dir);
    }
}
