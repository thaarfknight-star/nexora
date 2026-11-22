# Nexora

[![CI](https://github.com/thaarfknight-star/nexora/actions/workflows/ci.yml/badge.svg)](https://github.com/thaarfknight-star/nexora/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/thaarfknight-star/nexora)](https://github.com/thaarfknight-star/nexora/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)

**Nexora** is a floating Dynamic Island shell for **Windows 10 and Windows 11**, built with **Tauri v2**, **Rust**, and **React**. It provides a sleek, always-on-top floating Dynamic Island companion (with taskbar hiding currently disabled by default for maximum desktop safety) that expands on demand — showing live media, real system stats, and quick settings — while a dedicated Watchdog process guarantees the taskbar always comes back.

> **Status:** v0.2.14 is the first public release. The full v0.2.14 scope is implemented: island, taskbar replacement, Watchdog, GSMTC media, system stats, settings, tray, themes, brand assets, and the auto-update engine.

## Features

### Dynamic Island

- A smooth floating pill, pinned top-center of the primary display — **420×120 px** compact, expanding to **560×340 px** with an animated resize (~180 ms).
- Click the island to expand it into tabbed panels: **Media**, **Stats**, and **Settings**.
- Frameless, transparent, and always-on-top — it stays out of the way until you need it.
- On launch the island starts compact and top-centered; expanding it is a real animated resize, not a label swap.

### Safe taskbar replacement

- Hides the real Windows taskbar (`Shell_TrayWnd` plus secondary taskbars) through the Win32 API, with live hidden/visible status.
- **Quitting Nexora always restores the taskbar first** — including when the app crashes.
- Optional **auto-hide on startup**: enable `auto_hide_taskbar` in Settings and the taskbar is hidden as soon as Nexora starts.

### Watchdog

- A separate Watchdog process watches `explorer.exe` and restores the taskbar the moment Nexora dies — so a crash can never leave you without a taskbar.
- The tray's **Quit** action calls the same restore path as the Watchdog, so there is exactly one restore implementation.

### GSMTC media controls

- Native integration with the Windows **Global System Media Transport Controls**: live now-playing (title, artist, album, source app, playback status) plus **play/pause, next, and previous**.
- A fresh media session is opened for every query, and "No active media" is shown gracefully — never a panic — when nothing is playing.
- The Media tab shows track metadata and the source app, and its buttons drive the real transport commands.

### System monitoring

- Real **CPU and memory** readings from `sysinfo`, refreshed every 3 seconds while the island is expanded.
- Readings are clamped to 0–100% and the first CPU sample is taken after a short warm-up so it reflects real load, not a spike.

### Themes

Seven built-in themes, switchable from the separate Settings window and persisted across restarts:

| Theme | Style |
| --- | --- |
| **Neon Glass** (default) | Deep violet glass with a neon purple-blue glow — the brand-sheet look |
| **Obsidian** | Dark, neutral — the classic Nexora look |
| **Liquid Glass** | Signature frosted-glass aesthetic |
| **Aurora** | Cool green-blue glow |
| **Sunset** | Warm orange-pink gradient |
| **Midnight** | Deep blue night |
| **Forest** | Muted greens |

### System tray

- A real tray icon (the official Nexora mark) with a **Show / Hide** and **Quit** menu; left-click toggles the island.
- Quit restores the taskbar before the app exits.

### Auto-update engine

- Built-in updater powered by `tauri-plugin-updater`, fed by the rolling `update-feed` release.
- Check for updates from the Settings window; downloads are verified against the release signature before install.
- Every release ships signed installers (`.exe` and `.msi`, each with a `.sig` sidecar).

## Installation

**Requirements:** Windows 10 or Windows 11 (64-bit). No extra runtime is needed — WebView2 ships with Windows 11 and is installed on demand on Windows 10.

1. Download the latest installer from the [**Releases**](https://github.com/thaarfknight-star/nexora/releases) page:
   - `Nexora_0.2.14_x64-setup.exe` — NSIS installer (recommended)
   - `Nexora_0.2.14_x64_en-US.msi` — MSI installer
2. Run the installer and launch **Nexora** from the Start menu.
3. The island appears top-center. Click it to expand; right-click the tray icon for Show/Hide and Quit.

Updates arrive automatically: Nexora checks the update feed and installs signed releases in place.

## Usage

- **Expand / collapse:** click the island.
- **Media tab:** see what's playing and control playback (previous, play/pause, next).
- **Stats tab:** live CPU/RAM plus taskbar, Watchdog, and Explorer status.
- **Settings window:** pick a theme, toggle auto-hide taskbar, check for updates, and see the app version.
- **Tray:** left-click toggles the island; the menu offers Show/Hide and Quit (Quit restores the taskbar).

## Architecture

Tauri v2 app: a React frontend (`src/`) talking to a Rust backend (`src-tauri/src/`) over typed IPC commands.

| Module | Responsibility |
| --- | --- |
| `island.rs` | Window sizing, centering, and animated expand/collapse |
| `taskbar.rs` | Hide/restore the Windows taskbar via Win32 |
| `watchdog.rs` | Standalone process: restores the taskbar if Nexora dies; monitors `explorer.exe` |
| `media.rs` | GSMTC now-playing and transport controls |
| `stats.rs` | Real CPU/RAM via `sysinfo` |
| `settings.rs` | Settings persisted atomically to the app data directory |
| `tray.rs` | Tray icon, menu, and toggle |
| `updater.rs` | Auto-update check, download, and install |
| `ipc.rs` | All Tauri commands exposed to the frontend |
| `lib.rs` | App setup: single setup closure wiring island, Watchdog, tray, and updater |

Frontend: `src/App.tsx` (tabbed island UI), `src/themes.ts` (7 themes), `src/index.css` (theme variables).

## Development

**Prerequisites:** Node.js 18+, Rust (stable), and the [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) for Windows.

```bash
git clone https://github.com/thaarfknight-star/nexora.git
cd nexora
npm install

npm run tauri dev     # run the app in development mode
npm run build         # build the frontend (tsc + vite)
npm run tauri build   # build the Windows installers
```

Rust checks run in CI on every push: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. The Windows build is the gate — Win32/GSMTC code only compiles on `windows-latest`.

## Out of scope for v0.2.14

- Notification center
- Extra widgets
- Multi-monitor support (primary display only)
- Auto-start on login
- Linux / macOS builds

## Contributing

Issues and pull requests are welcome. Please keep changes focused, run `cargo fmt` and `npm run build` before opening a PR, and write commit messages in the repo's short English style (e.g. `feat(media): …`, `fix(ci): …`).

## License

Nexora is dual-licensed under either:

- **MIT License** ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)

at your option (`MIT OR Apache-2.0`).

See [COPYRIGHT](COPYRIGHT) for full copyright and licensing details, and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for third-party notices.
