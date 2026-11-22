# Changelog

## [0.2.14] - 2026-11-22

### What'''s New
- **Opt-In Online Translation Privacy Toggle**: Added `online_translation` (default `false`) with a first-time consent prompt so speech stays 100% local unless the user explicitly opts in.
- **Single-Instance Guard & Click-Through Safety Net**: Added Windows Named Mutex single-instance protection, a 15 s frontend heartbeat click-through fallback, and `ScaleFactorChanged` re-anchoring.
- **Least-Privilege Subtitle Capability**: Split `capabilities/subtitle.json` from `default.json` and enforced `window.label()` checks on privileged commands.

### Bug Fixes
- Hardened `launch_pinned_app` against arbitrary execution via normalized `list_pinned()` allowlist validation.
- Enabled strict `csp` and `devCsp` in `tauri.conf.json`.
- Added fail-closed `sha2` SHA-256 verification (with `TODO` digest constants), atomic `.part` downloads, ZIP-bomb extraction caps, and 3-attempt ephemeral port retry for `whisper-server.exe`.
- Removed `bootstrap.py` auto-push to `main` and `git tag -f` / `push -f` from CI; wrapped Win32 `EnumWindows` callbacks in `catch_unwind`; clamped numeric settings; updated `README.md` and removed unused `offlineEngine*` i18n keys.

## [0.2.13] - 2026-11-22

### What's New
- **Local Engine Cleanup**: Removed the temporary `Qwen2.5` + `llama-server` offline translator card and backend hooks to keep Nexora lean while preparing for the custom `Nexora-MT v1` native engine.

### Bug Fixes
- **Fixed What's New Modal Stuck on `v0.2.5`**: Updated `RELEASE_NOTES` (`readonly ReleaseEntry[]`) so the post-update What's New modal always reflects the active installed version (`v0.2.13` and prior releases).

## [0.2.12] - 2026-11-22

### What's New
- **Official Tauri v2 Update Engine (ReChat Architecture)**: Rewrote Nexora's updater around `@tauri-apps/plugin-updater` with minisign signature verification (`plugins.updater.pubkey`), NSIS `installMode: "currentUser"` passive install (no UAC), live `Started/Progress/Finished` progress bar, and automatic startup update check with confirmation dialog.
- **Auto-Update Startup Toggle**: Added a toggle in **Settings → Updates** (`nexora-auto-update`, enabled by default) to control silent startup update checks.
- **CI Feed Publishing Alignment**: Updated `.github/scripts/publish.py` to stamp UTC `pub_date` and also attach `update-feed.json` to each versioned tag release.

### Bug Fixes
- Fixed updates failing to apply in `v0.2.10` by replacing the custom PowerShell handoff with official `tauri-plugin-updater` `downloadAndInstall` and cleanly shutting down Live Translate / Whisper / Llama helpers before installer launch.

## [0.2.11] - 2026-11-22

### What's New
- **100% Offline Local AI Translator Engine**: Download `llama-server.exe` (`llama.cpp` `b12627`) + `Qwen2.5-0.5B-Instruct` (`Q4_K_M`, ~468 MB) right inside **Settings → Engines** for fast, 100% offline Persian & multilingual live subtitle translation with zero internet and zero rate limits.
- **Engine Settings Gating**: Configuration options for every engine (Live Translate & Offline Translator) stay completely hidden until that engine finishes downloading and installing.

### Bug Fixes
- Fixed Live Translate displaying "Installed" and exposing controls while a model download was still in progress.
- Ensured the detached PowerShell updater cleanly stops `llama-server` alongside `whisper-server` before applying updates.

## [0.2.10] - 2026-11-22

### What's New
- **Pure Non-Interactive PowerShell Update Handoff**: Replaced `cmd.exe` + `timeout.exe` with kernel-level PowerShell `Wait-Process`, `Stop-Process`, `Start-Process -Wait`, and automatic relaunch — zero `Ctrl+C` prompts or hangs.
- **3-Provider Live Translation Fallback Chain**: Routes translations through Google `gtx` → Google `clients5` → MyMemory with a browser `User-Agent` and smart 500 ms rate-limit pacing so live word-by-word translations never stall on HTTP 429.

### Bug Fixes
- Fixed Live Translate displaying only untranslated original dialogue when rapid word-by-word updates triggered HTTP 429 rate-limiting on Google Translate.
- Fixed the detached Windows updater stalling until `Ctrl+C` was pressed.

## [0.2.9] - 2026-11-21

### What's New
- **True Word-by-Word Incremental Live Subtitles**: Replaced sentence-end chunking with a continuous rolling-window streamer that snapshots active speech every **250 ms** (starting at 300 ms), updating both the original text and translation live in place as each word is spoken.
- **Latest-Only Shared Slots (Zero Queue Backlog)**: Replaced FIFO channels with `Condvar`-backed Latest-Only slots for both Whisper and Translation so Nexora always renders the latest spoken words immediately.
- **6× Faster Whisper Encoder (`--audio-ctx 256`)**: Cuts `ggml-tiny.bin` inference time to ~35–60 ms on CPU while smoothly rolling continuous speech windows without hard resets.

### Bug Fixes
- Fixed Live Translate waiting for the end of each sentence before translating and displaying subtitles.

## [0.2.8] - 2026-11-21

### What's New
- **3-Stage Decoupled Real-Time Streaming Subtitles**: Separated WASAPI capture, Whisper inference (`-ac 512` for ~3× faster encoder), and Google Translate (with in-memory phrase cache) onto three independent threads so continuous speech flows with near-zero delay and no pauses.
- **160 ms Overlap Tail & Rapid 550–1,150 ms Chunks**: Eliminates boundary word clipping and cuts waiting time before subtitles appear.
- **Custom Native Rust Update Engine (Rewritten from Scratch)**: Streams the per-user NSIS `.exe` installer with live 0–100% progress, validates the binary header, and hands off to a hidden detached Windows apply script that waits for `nexora.exe` and Whisper to exit 100% before running `setup.exe /P`, then relaunches Nexora cleanly.

### Bug Fixes
- Permanently fixed the `Failed to kill Nexora. Please close it first then try again` NSIS abort during in-app updates.
- Eliminated remaining subtitle pauses and boundary word drops during continuous speech.

## [0.2.7] - 2026-11-21

### What's New
- **Warm Resident `whisper-server.exe` in RAM**: Keeps the `ggml-tiny.bin` model loaded in memory and sends in-memory WAV chunks over localhost HTTP (with automatic fast `whisper-cli.exe` fallback), eliminating per-sentence model reload latency.
- **2-Thread Pipelined Capture & Inference**: Separates WASAPI loopback audio capture from Whisper + translation onto concurrent threads so audio is never dropped while translating.
- **Smart VAD Pause Endpointing**: Flushes speech chunks immediately on natural 240 ms pauses (850 ms min, 2,000 ms cap) instead of waiting a fixed 3.2 seconds.
- **Persistent Keep-Alive Translation Client**: Reuses pooled HTTP/TLS connections to Google Translate (`OnceLock<reqwest::blocking::Client>` + `tcp_nodelay`).

### Bug Fixes
- Fixed multi-second live subtitle lag caused by serial capture, repeated 75 MB model cold-starts, and per-chunk TLS handshakes.
- Added bounded non-blocking channel backpressure (`try_send`) so subtitle latency never accumulates during long videos.

## [0.2.6] - 2026-11-21

### What's New
- **Pure Internal Audio Capture (No Microphone)**: Replaced PowerShell/WinMM microphone recording with native Windows WASAPI loopback — external microphone audio is never captured.
- **Two Audio Source Modes**: Choose between **System audio (all apps)** and **Specific window / app** (Windows process-tree loopback) in **Settings → Engines → Live Translate**.
- **Target Window / App Picker**: Lists currently open applications with a **Refresh** button so you can lock live subtitles onto a specific browser, video player, or game.
- **Silence Gating**: Skips Whisper transcription when captured audio is silent, reducing CPU usage and preventing silence hallucinations.

### Bug Fixes
- Fixed Live Subtitles not picking up YouTube or desktop playback audio (previously captured default microphone input instead of WASAPI render loopback).
- Removed periodic PowerShell process spawning during live audio capture for smoother streaming and instant Stop response.
- Adopted ReChat's native `@tauri-apps/plugin-updater` engine and NSIS `installMode: "currentUser"` + passive updater config (with clean `cleanup_before_exit` + `process::exit(0)` in `on_before_exit`), fixing the Windows `Failed to kill Nexora` installer abort during in-app updates.

## [0.2.5] - 2026-11-21

### What's New
- Added an automatic post-update **What's New** glassmorphic showcase modal that opens after updating Nexora to highlight new features and bug fixes.
- Added a **What's New** button inside **Settings → Updates** to review release highlights anytime in English or Persian.
- Automated structured release notes publishing from `CHANGELOG.md` to every GitHub Release and `update-feed.json`.

### Bug Fixes
- Fixed `Not Responding` UI freeze on **Start Live Subtitles** by running lazy subtitle window creation in an `async` Tauri command.
- Fixed Windows NSIS auto-updater exit timing so updates install cleanly without exiting prematurely.
- Replaced external `curl`/`powershell` downloads in Live Translate with native Rust `reqwest` streaming and `zip` extraction (smooth 0–100% progress, zero console flashes).

## [0.2.4] - 2026-11-21

### What's New
- Added the modular **Engines** hub in Settings with optional downloadable **Live Translate** speech-to-subtitle engine, version badge, update button, and custom shortcut recorder.
- Replaced static Bell and Gear PNGs with theme-adaptive SVG icons that match all 7 themes.
- Merged **Appearance** themes into **Island Style**, added Island size (`75%–115%`) and Outer glow (`0%–100% / Off`) sliders, and added a **Minimize** button to Settings.

### Bug Fixes
- Fixed left/right pill clipping at `115%` scale by widening the island canvas to `880px`.
- Disabled hover-to-open at the top screen edge so the island opens only on top handle click or global shortcut.
- Fixed startup default-theme flash by applying saved theme synchronously in `index.html`.
- Fixed Stats tab null-dereference crash (`gpu_percent`, `net_up_kbps`, `net_down_kbps`) and Settings close button (`core:window:allow-hide`).

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Album cover art in `MediaInfo.cover_art` (`media.rs`): the session thumbnail via `GetThumbnailAsync`, returned as a `data:` URI ready for the media card `<img>`; empty when the session has no thumbnail.
- Source app icon in `MediaInfo.source_icon` (`media.rs`): the playing app's package logo via its GSMTC AUMID, returned as a `data:` URI ready for the media card `<img>`; empty when the app has no package or no logo file.

### Fixed
- Media commands no longer fail to compile on Windows: the GSMTC thumbnail stream and the icon file buffer are WinRT interfaces that are not `Send`, so they are wrapped in `AgileReference` the moment they come out of an await instead of living across one (`media.rs`); a compile-time test now pins that every async media future is `Send`.

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
