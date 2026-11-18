# Watchdog kill-app test (Windows only)

Acceptance for `feat/watchdog`: the taskbar must come back when Nexora dies,
not only when it quits cleanly. Run this on a real Windows build after the
taskbar branch (`feat/taskbar-hide-restore`) is merged, because hiding the
taskbar is that branch's job — the watchdog only restores it.

## Setup

1. Install Nexora 0.1.0 (or run `cargo tauri dev`).
2. Hide the taskbar from the island (taskbar branch feature).
3. Confirm the taskbar is hidden.

## Test 1 — kill (the safety-critical case)

1. Open Task Manager, find `nexora.exe`, **End task** (do not quit from the
   island or tray).
2. Expected: the taskbar reappears within about 1 second.
3. If it does not, the watchdog child did not run: check that only one
   `nexora.exe` process was running before the kill (the second one is the
   detached watchdog, no window, same binary, started with
   `--nexora-watchdog <pid>`).

## Test 2 — clean quit

1. Hide the taskbar again, then quit Nexora from the tray / island.
2. Expected: the taskbar reappears immediately (synchronous restore on exit,
   before the watchdog child is stopped).

## Test 3 — Explorer restart

1. With Nexora running, kill `explorer.exe` from Task Manager, then start it
   again (File > Run new task > `explorer.exe`).
2. Expected: Nexora keeps running, the taskbar comes back visible (Explorer
   creates it visible), and `get_watchdog_status` shows `explorer_restarts`
   increased by 1.

## Notes

- Showing the taskbar twice is harmless, so a clean quit followed by the
  watchdog firing is not a bug — the child is stopped on clean exit.
- The watchdog child holds no window and no tray icon; it exits by itself
  once the parent is gone. If Nexora is force-killed while Explorer is also
  down, the child retries showing the taskbar for about 1 second and exits;
  Explorer creates a visible taskbar when it restarts.
- `cargo test` covers the watchdog logic that can run without Windows
  (argument parsing, dead-parent exit, idempotent start/stop). The real
  Win32 paths are only exercised by the tests above on Windows.
