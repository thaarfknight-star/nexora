# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Album cover art in `MediaInfo.cover_art` (`media.rs`): the session thumbnail via `GetThumbnailAsync`, returned as a `data:` URI ready for the media card `<img>`; empty when the session has no thumbnail.
- Source app icon in `MediaInfo.source_icon` (`media.rs`): the playing app's package logo via its GSMTC AUMID, returned as a `data:` URI ready for the media card `<img>`; empty when the app has no package or no logo file.

## [0.1.0] - 2026-11-18

First public release. Nexora is a floating Dynamic Island shell for Windows 10/11
(Tauri v2 + Rust + React) with safe taskbar replacement, a Watchdog, real GSMTC
media controls, live system stats, persistent settings, a system tray, six themes,
official brand assets, and a signed auto-update engine.

### Added

- **Dynamic Island window** (`island.rs`): floating pill pinned top-center of the
  primary display; compact 420×120 px expanding to 560×340 px with an animated
  resize (~180 ms); always-on-top, transparent, frameless, draggable; starts
  compact and top-centered on launch.
- **Tabbed island UI** (`App.tsx`): expanded island with Media, Stats, and
  Settings tabs; every backend command reachable from the UI; stats poll every
  3 seconds while expanded.
- **Taskbar hide/restore** (`taskbar.rs`): real hide/restore of `Shell_TrayWnd`
  and secondary taskbars via the Win32 API; live hidden/visible status;
  idempotent `restore()` for exit and crash; `set_taskbar_hidden` command and
  `auto_hide_taskbar` setting to hide the taskbar at startup.
- **Watchdog** (`watchdog.rs`): standalone process (`--nexora-watchdog <pid>`)
  that monitors `explorer.exe` every 2 seconds and restores the taskbar the
  moment Nexora dies; the tray Quit action calls the same restore path.
  Test guide in `docs/WATCHDOG_TEST.md`.
- **System stats** (`stats.rs`): real CPU and memory via `sysinfo` (shared
  `System` behind `OnceLock`, first sample after a 200 ms warm-up, values
  clamped to 0–100).
- **Settings persistence** (`settings.rs`): settings stored in the app data
  directory with atomic writes (temp file + rename) and fallback to defaults
  on a corrupt file; theme, always-on-top, and auto-hide taskbar.
- **System tray** (`tray.rs`): real tray icon with Show/Hide and Quit menu;
  left-click toggles the island; Quit restores the taskbar before exiting.
- **GSMTC media controls** (`media.rs`): live now-playing (title, artist,
  album, source app, playback status) plus play/pause, next, and previous
  through the Windows Global System Media Transport Controls session manager;
  a fresh session per query; "No active media" without panic when nothing is
  playing; non-Windows fallback.
- **Media buttons** (`App.tsx`): Prev / Play-Pause / Next wired to the four
  media transport commands, with a second row showing album, source app, and
  playback status.
- **Six themes** (`themes.ts`, `index.css`): Obsidian (default), Liquid Glass,
  Aurora, Sunset, Midnight, and Forest; swatch picker in the Settings tab;
  selection persisted.
- **Official brand assets**: bundle icons, header emblem (28 px), and Settings
  wordmark from the Nexora brand kit; tray uses the 32×32 brand icon.
- **Auto-update engine** (`updater.rs`, `tauri-plugin-updater`): in-app update
  check, download, and install from the rolling `update-feed` release;
  `update-progress` events; signed installers (`.exe` and `.msi`, each with a
  `.sig` sidecar) verified before install.
- **IPC wiring** (`ipc.rs`, `lib.rs`): all commands registered — island
  expand/collapse, taskbar hide/restore and status, settings load/save, stats,
  tray status, media transport, update check/download/install; single setup
  closure wiring island, Watchdog, tray, and updater.
- **CI and release pipeline**: `CI` workflow (fmt, clippy with `-D warnings`,
  tests, frontend build) and `Build & Release` workflow publishing signed
  Windows installers and the update feed.
- **Dual licensing** under MIT and Apache-2.0 (`LICENSE-MIT`,
  `LICENSE-APACHE`, `COPYRIGHT`) and third-party notices
  (`THIRD_PARTY_LICENSES.md`).
- **Repository documentation** (`README.md`, `CHANGELOG.md`,
  `docs/WATCHDOG_TEST.md`).

[Unreleased]: https://github.com/thaarfknight-star/nexora/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/thaarfknight-star/nexora/releases/tag/v0.1.0
