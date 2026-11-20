//! Global hotkey for the island: Alt+Space by default (Taha's choice).
//!
//! The default key lives behind one constant ([`ISLAND_HOTKEY`]); the
//! configured key lives in settings (`NexoraSettings::island_hotkey`,
//! default [`ISLAND_HOTKEY`]). Startup registers the configured key
//! ([`register_island_hotkey`]); the [`set_island_hotkey`] command
//! validates a new key, re-registers it, and persists it only when
//! registration succeeds, so a failed change leaves the old key
//! registered and stored.
//!
//! Registration is backend-only, through the plugin's Rust manager
//! (`GlobalShortcutExt::on_shortcut`). That path calls the OS hotkey
//! manager directly and never goes through a Tauri command, so no
//! capability entry is granted: giving the webview
//! `global-shortcut:allow-register` would only let the frontend
//! register arbitrary global shortcuts, which is the opposite of
//! scoping. The frontend never calls the plugin; it only listens for
//! [`HOTKEY_EVENT`] and invokes [`set_island_hotkey`] like any other
//! settings command.
//!
//! Alt+Space trade-off (Taha's choice): while Nexora runs, the
//! shortcut is registered globally, so Alt+Space reaches the island
//! in every app and no longer opens the system menu of the focused
//! window. Inside the island the behaviour is still correct: the
//! island window is frameless (`decorations: false`), so it has no
//! system menu to open, and the shortcut fires before the OS could
//! deliver the key — pressing Alt+Space while the island is focused
//! just hides it (the toggle below), never a menu.
//!
//! The handler does not decide anything: it only emits
//! [`HOTKEY_EVENT`] on press (releases are ignored). The frontend's
//! existing island state machine performs the toggle
//! (hidden -> compact, compact / expanded -> hidden) through
//! `showIsland`; there is no second state machine.

use std::str::FromStr;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

/// The default island hotkey, as one accelerator string. The plugin
/// parses it case-insensitively (`Alt+Space` and `alt+space` are the
/// same shortcut); settings store the string as given, so the default
/// keeps its display form (see the unit test).
pub const ISLAND_HOTKEY: &str = "Alt+Space";

/// Event emitted to the frontend when the hotkey is pressed.
pub const HOTKEY_EVENT: &str = "island-hotkey";

/// The toggle handler shared by every registration: only emits
/// [`HOTKEY_EVENT`] on press (releases are ignored).
fn on_hotkey_press(app: &AppHandle, _shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state == ShortcutState::Pressed {
        let _ = app.emit(HOTKEY_EVENT, ());
    }
}

/// Whether the string parses into a shortcut (modifiers first, one
/// main key; case-insensitive). Used by settings to repair an
/// unparseable stored hotkey.
pub fn is_valid_shortcut(shortcut: &str) -> bool {
    Shortcut::from_str(shortcut).is_ok()
}

/// Register the island hotkey with its toggle handler. Call once
/// from the `setup` hook in `lib.rs`, with the configured shortcut
/// (`settings.island_hotkey`), after the plugin itself is
/// registered. Returns an error when the OS refuses the registration
/// (e.g. another app already owns the key); the caller logs it and
/// keeps running — the island still works through the hot zone.
pub fn register_island_hotkey(
    app: &AppHandle,
    shortcut: &str,
) -> Result<(), tauri_plugin_global_shortcut::Error> {
    app.global_shortcut().on_shortcut(shortcut, on_hotkey_press)
}

/// Register a shortcut with the toggle handler.
fn register(app: &AppHandle, shortcut: &str) -> Result<(), tauri_plugin_global_shortcut::Error> {
    app.global_shortcut().on_shortcut(shortcut, on_hotkey_press)
}

/// Persist the new hotkey and tell every window the settings
/// changed, so the frontend picks the new value up at once.
/// Returns `false` when the write fails.
fn store(app: &AppHandle, current: &crate::settings::NexoraSettings, shortcut: &str) -> bool {
    let mut settings = current.clone();
    settings.island_hotkey = shortcut.to_string();
    match crate::settings::update(app, settings) {
        Ok(stored) => {
            let _ = app.emit("settings-changed", &stored);
            true
        }
        Err(e) => {
            eprintln!("Nexora: failed to save island hotkey: {e}");
            false
        }
    }
}

/// Tauri command: change the island hotkey.
///
/// Validates the shortcut, re-registers it, and persists it only
/// when registration succeeds. Returns `false` — and keeps the old
/// hotkey registered and stored — when the shortcut does not parse
/// or the OS refuses it.
#[tauri::command]
pub fn set_island_hotkey(app: AppHandle, shortcut: String) -> bool {
    if Shortcut::from_str(&shortcut).is_err() {
        return false;
    }
    let current = crate::settings::load(&app);
    // Same shortcut under a different spelling (the parser is
    // case-insensitive): compare the parsed key, not the string.
    let same = Shortcut::from_str(&current.island_hotkey)
        .ok()
        .zip(Shortcut::from_str(&shortcut).ok())
        .is_some_and(|(old, new)| old.mods == new.mods && old.key == new.key);
    if !same {
        // Swap: drop the old key first (best effort — it may never
        // have been registered). If the new registration fails, put
        // the old key back and report false; the stored setting is
        // untouched.
        let _ = app
            .global_shortcut()
            .unregister(current.island_hotkey.as_str());
        if register(&app, shortcut.as_str()).is_err() {
            let _ = register(&app, current.island_hotkey.as_str());
            return false;
        }
    } else if !app.global_shortcut().is_registered(shortcut.as_str()) {
        // Same shortcut, not currently registered: register it.
        // A failure here changes nothing, so just report false.
        if register(&app, shortcut.as_str()).is_err() {
            return false;
        }
    }
    // Registration succeeded (or the key was already registered);
    // persist. On a write failure, roll the registration back so a
    // `false` return always means the old key is still in place.
    if store(&app, &current, &shortcut) {
        true
    } else {
        let _ = app.global_shortcut().unregister(shortcut.as_str());
        let _ = register(&app, current.island_hotkey.as_str());
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::{Code, Modifiers};

    #[test]
    fn hotkey_constant_parses_to_alt_space() {
        let shortcut = Shortcut::from_str(ISLAND_HOTKEY).expect("hotkey parses");
        assert_eq!(shortcut.mods, Modifiers::ALT);
        assert_eq!(shortcut.key, Code::Space);
    }

    #[test]
    fn hotkey_parsing_is_case_insensitive() {
        let lower = Shortcut::from_str("alt+space").expect("lowercase parses");
        let upper = Shortcut::from_str("Alt+Space").expect("mixed case parses");
        assert_eq!(lower.mods, upper.mods);
        assert_eq!(lower.key, upper.key);
    }

    #[test]
    fn shortcut_validation_accepts_keys_and_rejects_garbage() {
        assert!(is_valid_shortcut("Alt+Space"));
        assert!(is_valid_shortcut("Ctrl+Shift+N"));
        assert!(is_valid_shortcut("F9"));
        assert!(!is_valid_shortcut("banana"));
        assert!(!is_valid_shortcut(""));
        assert!(!is_valid_shortcut("Ctrl+Alt"));
        assert!(!is_valid_shortcut("Ctrl+Shift+C+A"));
    }
}
