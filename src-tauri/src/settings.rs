use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// File name of the persisted settings inside the app data dir.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

fn default_true() -> bool {
    true
}

fn default_always_on_top() -> bool {
    true
}

fn default_auto_hide_taskbar() -> bool {
    false
}

fn default_theme() -> String {
    "neon-glass".into()
}

fn default_language() -> String {
    "en".into()
}

fn default_island_hotkey() -> String {
    crate::hotkey::ISLAND_HOTKEY.to_string()
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

fn default_translate_provider() -> String {
    "none".into()
}

fn default_translate_source_lang() -> String {
    "en".into()
}

fn default_translate_target_lang() -> String {
    "fa".into()
}

fn default_whisper_model() -> String {
    "ggml-tiny.bin".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleCssPreset {
    pub name: String,
    pub css: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NexoraSettings {
    #[serde(default = "default_auto_hide_taskbar")]
    pub auto_hide_taskbar: bool,
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// UI language: `"en"` or `"fa"`. Any other value falls back to `"en"`
    /// in `normalized()`.
    #[serde(default = "default_language")]
    pub language: String,
    /// Island hotkey (global shortcut), e.g. `"Alt+Space"`. A value that
    /// does not parse falls back to the default in `normalized()`.
    #[serde(default = "default_island_hotkey")]
    pub island_hotkey: String,
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
    /// Legacy flag kept for backwards compatibility; when `translate_provider`
    /// is `"google_cloud"` or `"gemini"`, online translation is enabled.
    #[serde(default)]
    pub online_translation: bool,
    /// Active translation provider: `"none"`, `"google_cloud"`, `"gemini"`, or `"local"`.
    #[serde(default = "default_translate_provider")]
    pub translate_provider: String,
    /// Spoken audio language for Live Subtitles (e.g. `"en"`, `"fa"`, `"auto"`).
    #[serde(default = "default_translate_source_lang")]
    pub translate_source_lang: String,
    /// Target subtitle language (e.g. `"fa"`, `"en"`, `"ar"`, `"tr"`).
    #[serde(default = "default_translate_target_lang")]
    pub translate_target_lang: String,
    /// Selected Whisper GGML speech model filename.
    #[serde(default = "default_whisper_model")]
    pub whisper_model: String,
    /// User-defined custom CSS applied inside the floating subtitle overlay window.
    #[serde(default)]
    pub subtitle_custom_css: String,
    /// Saved named custom CSS presets for the floating subtitle overlay.
    #[serde(default)]
    pub subtitle_css_presets: Vec<SubtitleCssPreset>,
    /// Name of the currently selected subtitle CSS preset (empty = default/custom).
    #[serde(default)]
    pub active_subtitle_css_preset: String,
    /// Dynamic Island customization: show Start menu button.
    #[serde(default = "default_true")]
    pub island_show_start: bool,
    /// Dynamic Island customization: show open apps carousel (Taskbar + System Tray).
    #[serde(default = "default_true")]
    pub island_show_apps: bool,
    /// Dynamic Island customization: show system stats (CPU / RAM / GPU).
    #[serde(default = "default_true")]
    pub island_show_stats: bool,
    /// Dynamic Island customization: show GPU icon & percentage alongside CPU/RAM.
    #[serde(default = "default_true")]
    pub island_show_gpu: bool,
    /// Dynamic Island customization: show media playback controls.
    #[serde(default = "default_true")]
    pub island_show_media: bool,
    /// Dynamic Island customization: show clock.
    #[serde(default = "default_true")]
    pub island_show_clock: bool,
    /// Dynamic Island customization: show notifications bell button.
    #[serde(default = "default_true")]
    pub island_show_notifications: bool,
    /// Dynamic Island customization: show Local (Offline Subtitles / Nexora-MT) engine quick button.
    #[serde(default = "default_true")]
    pub island_show_engine_local: bool,
    /// Dynamic Island customization: show Online Translation engine quick button.
    #[serde(default = "default_true")]
    pub island_show_engine_online: bool,
    /// Dynamic Island customization: show Live Dubbing (Nexora-Voice) engine quick button.
    #[serde(default = "default_true")]
    pub island_show_engine_dubbing: bool,
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
        language: "en".into(),
        island_hotkey: default_island_hotkey(),
        island_blur_px: None,
        island_radius_px: None,
        clock_24h: true,
        show_seconds: false,
        date_format: "short".into(),
        poll_interval_secs: 3,
        stats_speed_unit: "auto".into(),
        stats_percent_style: "with".into(),
        online_translation: false,
        translate_provider: default_translate_provider(),
        translate_source_lang: default_translate_source_lang(),
        translate_target_lang: default_translate_target_lang(),
        whisper_model: default_whisper_model(),
        subtitle_custom_css: String::new(),
        subtitle_css_presets: Vec::new(),
        active_subtitle_css_preset: String::new(),
        island_show_start: true,
        island_show_apps: true,
        island_show_stats: true,
        island_show_gpu: true,
        island_show_media: true,
        island_show_clock: true,
        island_show_notifications: true,
        island_show_engine_local: true,
        island_show_engine_online: true,
        island_show_engine_dubbing: true,
    }
}

impl NexoraSettings {
    /// Repairs values that cannot be applied: an empty theme falls back
    /// to the default theme, an unsupported language falls back to
    /// English, and an unparseable hotkey falls back to the default
    /// hotkey. Theme names are not restricted here on purpose — the
    /// theme list lives in the frontend (`src/themes.ts`).
    fn normalized(mut self) -> Self {
        if self.theme.trim().is_empty() {
            self.theme = default_theme();
        } else {
            self.theme = self.theme.trim().to_string();
        }
        if self.language != "en" && self.language != "fa" {
            self.language = default_language();
        }
        if !crate::hotkey::is_valid_shortcut(&self.island_hotkey) {
            self.island_hotkey = default_island_hotkey();
        }
        self.poll_interval_secs = self.poll_interval_secs.clamp(1, 60);
        if let Some(blur) = self.island_blur_px {
            self.island_blur_px = Some(blur.min(64));
        }
        if let Some(radius) = self.island_radius_px {
            self.island_radius_px = Some(radius.min(48));
        }
        self.translate_provider =
            crate::translate_provider::TranslateProviderId::parse_id(&self.translate_provider)
                .as_str()
                .to_string();
        if self.translate_source_lang.trim().is_empty() {
            self.translate_source_lang = default_translate_source_lang();
        } else {
            self.translate_source_lang = self.translate_source_lang.trim().to_ascii_lowercase();
        }
        if self.translate_target_lang.trim().is_empty() {
            self.translate_target_lang = default_translate_target_lang();
        } else {
            self.translate_target_lang = self.translate_target_lang.trim().to_ascii_lowercase();
        }
        if !matches!(
            self.whisper_model.as_str(),
            "ggml-tiny.bin"
                | "ggml-tiny.en.bin"
                | "ggml-base.en.bin"
                | "ggml-large-v3-turbo-q5_0.bin"
        ) {
            self.whisper_model = default_whisper_model();
        }
        if self.subtitle_custom_css.len() > 8192 {
            self.subtitle_custom_css.truncate(8192);
        }
        self.subtitle_css_presets
            .retain(|p| !p.name.trim().is_empty());
        if self.subtitle_css_presets.len() > 30 {
            self.subtitle_css_presets.truncate(30);
        }
        for preset in &mut self.subtitle_css_presets {
            preset.name = preset.name.trim().chars().take(48).collect();
            if preset.css.len() > 8192 {
                preset.css.truncate(8192);
            }
        }
        self.active_subtitle_css_preset = self.active_subtitle_css_preset.trim().to_string();
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
            online_translation: false,
            translate_provider: "gemini".into(),
            translate_source_lang: "en".into(),
            translate_target_lang: "fa".into(),
            whisper_model: "ggml-base.en.bin".into(),
            subtitle_custom_css: String::new(),
            subtitle_css_presets: Vec::new(),
            active_subtitle_css_preset: String::new(),
            island_show_start: false,
            island_show_apps: true,
            island_show_stats: true,
            island_show_gpu: false,
            island_show_media: true,
            island_show_clock: true,
            island_show_notifications: false,
            island_show_engine_local: true,
            island_show_engine_online: false,
            island_show_engine_dubbing: true,
            language: "fa".into(),
            island_hotkey: "Ctrl+Alt+N".into(),
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
                online_translation: false,
                translate_provider: "none".into(),
                translate_source_lang: "en".into(),
                translate_target_lang: "fa".into(),
                whisper_model: "ggml-tiny.bin".into(),
                subtitle_custom_css: String::new(),
                subtitle_css_presets: Vec::new(),
                active_subtitle_css_preset: String::new(),
                island_show_start: true,
                island_show_apps: true,
                island_show_stats: true,
                island_show_gpu: true,
                island_show_media: true,
                island_show_clock: true,
                island_show_notifications: true,
                island_show_engine_local: true,
                island_show_engine_online: true,
                island_show_engine_dubbing: true,
                island_show_start: true,
                island_show_apps: true,
                island_show_stats: true,
                island_show_gpu: true,
                island_show_media: true,
                island_show_clock: true,
                island_show_notifications: true,
                island_show_engine_local: true,
                island_show_engine_online: true,
                island_show_engine_dubbing: true,
                language: "en".into(),
                island_hotkey: "Alt+Space".into(),
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
            online_translation: false,
            translate_provider: "none".into(),
            translate_source_lang: "en".into(),
            translate_target_lang: "fa".into(),
            whisper_model: "ggml-tiny.bin".into(),
            subtitle_custom_css: String::new(),
            subtitle_css_presets: Vec::new(),
            active_subtitle_css_preset: String::new(),
            island_show_start: true,
            island_show_apps: true,
            island_show_stats: true,
            island_show_gpu: true,
            island_show_media: true,
            island_show_clock: true,
            island_show_notifications: true,
            island_show_engine_local: true,
            island_show_engine_online: true,
            island_show_engine_dubbing: true,
            language: "en".into(),
            island_hotkey: "Alt+Space".into(),
        };
        save_to_dir(&dir, &settings).expect("save must succeed");
        assert_eq!(load_from_dir(&dir).theme, "neon-glass");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_hotkey_falls_back_to_default() {
        let dir = test_dir();
        let settings = NexoraSettings {
            auto_hide_taskbar: true,
            always_on_top: true,
            theme: "neon-glass".into(),
            language: "en".into(),
            island_blur_px: None,
            island_radius_px: None,
            clock_24h: true,
            show_seconds: false,
            date_format: "short".into(),
            poll_interval_secs: 3,
            stats_speed_unit: "auto".into(),
            stats_percent_style: "with".into(),
            online_translation: false,
            translate_provider: "none".into(),
            translate_source_lang: "en".into(),
            translate_target_lang: "fa".into(),
            whisper_model: "ggml-tiny.bin".into(),
            subtitle_custom_css: String::new(),
            subtitle_css_presets: Vec::new(),
            active_subtitle_css_preset: String::new(),
            island_show_start: true,
            island_show_apps: true,
            island_show_stats: true,
            island_show_gpu: true,
            island_show_media: true,
            island_show_clock: true,
            island_show_notifications: true,
            island_show_engine_local: true,
            island_show_engine_online: true,
            island_show_engine_dubbing: true,
            island_hotkey: "not-a-key".into(),
        };
        save_to_dir(&dir, &settings).expect("save must succeed");
        assert_eq!(load_from_dir(&dir).island_hotkey, "Alt+Space");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsupported_language_falls_back_to_english() {
        let dir = test_dir();
        let settings = NexoraSettings {
            auto_hide_taskbar: true,
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
            online_translation: false,
            translate_provider: "none".into(),
            translate_source_lang: "en".into(),
            translate_target_lang: "fa".into(),
            whisper_model: "ggml-tiny.bin".into(),
            subtitle_custom_css: String::new(),
            subtitle_css_presets: Vec::new(),
            active_subtitle_css_preset: String::new(),
            island_show_start: true,
            island_show_apps: true,
            island_show_stats: true,
            island_show_gpu: true,
            island_show_media: true,
            island_show_clock: true,
            island_show_notifications: true,
            island_show_engine_local: true,
            island_show_engine_online: true,
            island_show_engine_dubbing: true,
            language: "de".into(),
            island_hotkey: "Alt+Space".into(),
        };
        save_to_dir(&dir, &settings).expect("save must succeed");
        assert_eq!(load_from_dir(&dir).language, "en");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_online_translation_defaults_to_false() {
        let dir = test_dir();
        fs::create_dir_all(&dir).expect("create test dir");
        fs::write(settings_path(&dir), r#"{"theme": "neon-glass"}"#).expect("write legacy");
        let loaded = load_from_dir(&dir);
        assert!(!loaded.online_translation);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn numeric_settings_are_clamped_within_safe_bounds() {
        let dir = test_dir();
        fs::create_dir_all(&dir).expect("create test dir");
        fs::write(
            settings_path(&dir),
            r#"{"poll_interval_secs": 0, "island_blur_px": 255, "island_radius_px": 200}"#,
        )
        .expect("write out-of-range");
        let low = load_from_dir(&dir);
        assert_eq!(low.poll_interval_secs, 1);
        assert_eq!(low.island_blur_px, Some(64));
        assert_eq!(low.island_radius_px, Some(48));

        fs::write(settings_path(&dir), r#"{"poll_interval_secs": 9999}"#).expect("write high");
        let high = load_from_dir(&dir);
        assert_eq!(high.poll_interval_secs, 60);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_translate_provider_and_model_normalize_to_safe_defaults() {
        let dir = test_dir();
        fs::create_dir_all(&dir).expect("create test dir");
        fs::write(
            settings_path(&dir),
            r#"{"translate_provider": "gtx", "whisper_model": "../evil.bin"}"#,
        )
        .expect("write invalid");
        let loaded = load_from_dir(&dir);
        assert_eq!(loaded.translate_provider, "none");
        assert_eq!(loaded.whisper_model, "ggml-tiny.bin");
        assert_eq!(loaded.translate_source_lang, "en");
        assert_eq!(loaded.translate_target_lang, "fa");
        let _ = fs::remove_dir_all(&dir);
    }
}
