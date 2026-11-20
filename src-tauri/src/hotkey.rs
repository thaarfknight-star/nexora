//! Global hotkey for the island: Alt+Space (Taha's choice).
//!
//! The key lives behind one constant ([`ISLAND_HOTKEY`]) so the next
//! step — making it configurable in settings — only touches this
//! module. The settings field and its UI belong to TAgent5 / EAgent2,
//! not to this branch.
//!
//! Registration is backend-only, through the plugin's Rust manager
//! (`GlobalShortcutExt::on_shortcut`). That path calls the OS hotkey
//! manager directly and never goes through a Tauri command, so no
//! capability entry is granted: giving the webview
//! `global-shortcut:allow-register` would only let the frontend
//! register arbitrary global shortcuts, which is the opposite of
//! scoping. The frontend never calls the plugin; it only listens for
//! [`HOTKEY_EVENT`].
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

use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// The island hotkey, as one accelerator string. The plugin parses
/// `alt` + `space` (see the unit test).
pub const ISLAND_HOTKEY: &str = "alt+space";

/// Event emitted to the frontend when the hotkey is pressed.
pub const HOTKEY_EVENT: &str = "island-hotkey";

/// Register the island hotkey with its toggle handler. Call once
/// from the `setup` hook in `lib.rs`, after the plugin itself is
/// registered. Returns an error when the OS refuses the registration
/// (e.g. another app already owns Alt+Space); the caller logs it and
/// keeps running — the island still works through the hot zone.
pub fn register_island_hotkey(app: &AppHandle) -> Result<(), tauri_plugin_global_shortcut::Error> {
    app.global_shortcut()
        .on_shortcut(ISLAND_HOTKEY, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = app.emit(HOTKEY_EVENT, ());
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    #[test]
    fn hotkey_constant_parses_to_alt_space() {
        let shortcut = Shortcut::from_str(ISLAND_HOTKEY).expect("hotkey parses");
        assert_eq!(shortcut.mods, Modifiers::ALT);
        assert_eq!(shortcut.key, Code::Space);
    }
}
