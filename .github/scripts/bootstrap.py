#!/usr/bin/env python3
from pathlib import Path
import json
import struct
import zlib
ROOT = Path(__file__).resolve().parents[2]


def write_if_missing(rel_path: str, content: str) -> None:
    target = ROOT / rel_path
    if target.exists():
        return
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content.lstrip("\n"), encoding="utf-8")
    print(f"Created {rel_path}")
def png_chunk(tag: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


def make_png(width: int, height: int) -> bytes:
    rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            nx = (x / max(width - 1, 1)) * 2 - 1
            ny = (y / max(height - 1, 1)) * 2 - 1
            r2 = nx * nx + ny * ny
            if r2 < 0.72:
                r = int(20 + 60 * (1 - r2))
                g = int(140 + 90 * max(0.0, 1 - r2 * 1.4))
                b = int(230 + 25 * (1 - r2))
                a = 255
            else:
                r, g, b, a = 15, 23, 42, 255
            row.extend((r, g, b, a))
        rows.append(bytes(row))
    raw = b"".join(rows)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", zlib.compress(raw, 9))
        + png_chunk(b"IEND", b"")
    )


def make_ico(png_32: bytes) -> bytes:
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", 32, 32, 0, 0, 1, 32, len(png_32), 22)
    return header + entry + png_32


def make_icns(png_128: bytes) -> bytes:
    block = b"ic07" + struct.pack(">I", len(png_128) + 8) + png_128
    return b"icns" + struct.pack(">I", len(block) + 8) + block


def generate_icons() -> None:
    icons_dir = ROOT / "src-tauri" / "icons"
    icons_dir.mkdir(parents=True, exist_ok=True)
    png_32 = make_png(32, 32)
    png_128 = make_png(128, 128)
    png_256 = make_png(256, 256)
    targets = {
        "32x32.png": png_32,
        "128x128.png": png_128,
        "128x128@2x.png": png_256,
        "icon.ico": make_ico(png_32),
        "icon.icns": make_icns(png_128),
    }
    for name, data in targets.items():
        path = icons_dir / name
        if not path.exists():
            path.write_bytes(data)
            print(f"Generated src-tauri/icons/{name}")

FILES = {
    "src-tauri/build.rs": """
fn main() {
    tauri_build::build()
}
""",
    "src-tauri/capabilities/default.json": json.dumps(
        {
            "$schema": "../gen/schemas/desktop-schema.json",
            "identifier": "default",
            "description": "Default capability for Nexora",
            "windows": ["main"],
            "permissions": [
                "core:default",
                "core:window:allow-start-dragging",
                "core:window:allow-set-size",
                "core:window:allow-set-position",
                "core:window:allow-show",
                "core:window:allow-hide",
            ],
        },
        indent=2,
    )
    + "\n",
    "src-tauri/src/main.rs": """
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nexora_lib::run();
}
""",
    "src-tauri/src/lib.rs": """
pub mod ipc;
pub mod island;
pub mod media;
pub mod settings;
pub mod stats;
pub mod taskbar;
pub mod tray;
pub mod updater;
pub mod watchdog;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ipc::get_island_state,
            ipc::set_island_expanded,
            ipc::get_system_stats,
            ipc::get_now_playing,
            ipc::get_taskbar_status,
            ipc::get_watchdog_status,
            ipc::get_tray_status,
            ipc::get_settings,
            ipc::check_updates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Nexora Tauri application");
}
""",
    "src-tauri/src/island.rs": """
use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct IslandState {
    pub expanded: bool,
    pub mode: String,
    pub title: String,
}

static EXPANDED: Mutex<bool> = Mutex::new(false);

pub fn current_state() -> IslandState {
    let expanded = *EXPANDED.lock().unwrap_or_else(|e| e.into_inner());
    IslandState {
        expanded,
        mode: if expanded { "expanded".into() } else { "compact".into() },
        title: "Nexora Island".into(),
    }
}

pub fn set_expanded(expanded: bool) -> IslandState {
    if let Ok(mut guard) = EXPANDED.lock() {
        *guard = expanded;
    }
    current_state()
}
""",
    "src-tauri/src/taskbar.rs": """
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TaskbarStatus {
    pub hidden: bool,
    pub mode: String,
}

pub fn status() -> TaskbarStatus {
    TaskbarStatus {
        hidden: false,
        mode: "island-shell".into(),
    }
}
""",
    "src-tauri/src/watchdog.rs": """
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WatchdogStatus {
    pub healthy: bool,
    pub explorer_ok: bool,
}

pub fn check_health() -> WatchdogStatus {
    WatchdogStatus {
        healthy: true,
        explorer_ok: true,
    }
}
""",
    "src-tauri/src/media.rs": """
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MediaInfo {
    pub playing: bool,
    pub title: String,
    pub artist: String,
}

pub fn now_playing() -> MediaInfo {
    MediaInfo {
        playing: false,
        title: "No active media".into(),
        artist: "Nexora".into(),
    }
}
""",
    "src-tauri/src/stats.rs": """
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub memory_percent: f32,
}

pub fn collect() -> SystemStats {
    SystemStats {
        cpu_percent: 4.2,
        memory_percent: 38.0,
    }
}
""",
    "src-tauri/src/tray.rs": """
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TrayStatus {
    pub active: bool,
    pub items: usize,
}

pub fn status() -> TrayStatus {
    TrayStatus {
        active: true,
        items: 0,
    }
}
""",
    "src-tauri/src/settings.rs": """
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct NexoraSettings {
    pub auto_hide_taskbar: bool,
    pub always_on_top: bool,
    pub theme: String,
}

pub fn defaults() -> NexoraSettings {
    NexoraSettings {
        auto_hide_taskbar: false,
        always_on_top: true,
        theme: "obsidian".into(),
    }
}
""",
    "src-tauri/src/updater.rs": """
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub update_available: bool,
}

pub fn check() -> UpdateInfo {
    UpdateInfo {
        current_version: env!("CARGO_PKG_VERSION").into(),
        update_available: false,
    }
}
""",
    "src-tauri/src/ipc.rs": """
use crate::{island, media, settings, stats, taskbar, tray, updater, watchdog};

#[tauri::command]
pub fn get_island_state() -> island::IslandState {
    island::current_state()
}

#[tauri::command]
pub fn set_island_expanded(expanded: bool) -> island::IslandState {
    island::set_expanded(expanded)
}

#[tauri::command]
pub fn get_system_stats() -> stats::SystemStats {
    stats::collect()
}

#[tauri::command]
pub fn get_now_playing() -> media::MediaInfo {
    media::now_playing()
}

#[tauri::command]
pub fn get_taskbar_status() -> taskbar::TaskbarStatus {
    taskbar::status()
}

#[tauri::command]
pub fn get_watchdog_status() -> watchdog::WatchdogStatus {
    watchdog::check_health()
}

#[tauri::command]
pub fn get_tray_status() -> tray::TrayStatus {
    tray::status()
}

#[tauri::command]
pub fn get_settings() -> settings::NexoraSettings {
    settings::defaults()
}

#[tauri::command]
pub fn check_updates() -> updater::UpdateInfo {
    updater::check()
}
""",    "index.html": """
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Nexora</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
""",
    "vite.config.ts": """
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
""",
    "tsconfig.json": json.dumps(
        {
            "compilerOptions": {
                "target": "ES2020",
                "useDefineForClassFields": True,
                "lib": ["ES2020", "DOM", "DOM.Iterable"],
                "module": "ESNext",
                "skipLibCheck": True,
                "moduleResolution": "bundler",
                "allowImportingTsExtensions": True,
                "resolveJsonModule": True,
                "isolatedModules": True,
                "noEmit": True,
                "jsx": "react-jsx",
                "strict": True,
                "noUnusedLocals": True,
                "noUnusedParameters": True,
            },
            "include": ["src"],
        },
        indent=2,
    )
    + "\n",
    "tailwind.config.js": """
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {},
  },
  plugins: [],
};
""",
    "postcss.config.js": """
export default {
  plugins: {},
};
""",
    "src/vite-env.d.ts": """
/// <reference types="vite/client" />
""",
    "src/index.css": """
:root {
  color-scheme: dark;
  font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
}

* {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

html,
body,
#root {
  width: 100%;
  height: 100%;
  background: transparent;
  overflow: hidden;
  user-select: none;
}

.island-shell {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 8px;
}

.island-pill {
  width: 100%;
  height: 100%;
  border-radius: 28px;
  background: rgba(15, 23, 42, 0.92);
  border: 1px solid rgba(148, 163, 184, 0.25);
  color: #f8fafc;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 20px;
  backdrop-filter: blur(18px);
}

.island-brand {
  display: flex;
  align-items: center;
  gap: 10px;
}

.island-dot {
  width: 10px;
  height: 10px;
  border-radius: 9999px;
  background: #38bdf8;
}

.island-title {
  font-size: 14px;
  font-weight: 600;
}

.island-sub {
  font-size: 12px;
  color: #94a3b8;
}

.island-btn {
  background: rgba(56, 189, 248, 0.16);
  color: #7dd3fc;
  border: 1px solid rgba(56, 189, 248, 0.35);
  border-radius: 9999px;
  padding: 6px 14px;
  font-size: 12px;
  cursor: pointer;
}
""",
    "src/App.tsx": """
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface IslandState {
  expanded: boolean;
  mode: string;
  title: string;
}

export function App() {
  const [state, setState] = useState<IslandState>({
    expanded: false,
    mode: "compact",
    title: "Nexora Island",
  });

  useEffect(() => {
    invoke<IslandState>("get_island_state")
      .then(setState)
      .catch(() => {});
  }, []);

  const toggle = () => {
    const next = !state.expanded;
    invoke<IslandState>("set_island_expanded", { expanded: next })
      .then(setState)
      .catch(() =>
        setState((prev) => ({
          ...prev,
          expanded: next,
          mode: next ? "expanded" : "compact",
        }))
      );
  };

  return (
    <main className="island-shell" data-tauri-drag-region>
      <section className="island-pill" data-tauri-drag-region>
        <div className="island-brand">
          <span className="island-dot" />
          <div>
            <div className="island-title">{state.title}</div>
            <div className="island-sub">Mode: {state.mode}</div>
          </div>
        </div>
        <button className="island-btn" onClick={toggle} type="button">
          {state.expanded ? "Collapse" : "Expand"}
        </button>
      </section>
    </main>
  );
}

export default App;
""",
    "src/main.tsx": """
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
""",
    "scripts/generate-icons.mjs": """
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

const iconsDir = path.resolve("src-tauri/icons");
fs.mkdirSync(iconsDir, { recursive: true });

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) {
    c ^= b;
    for (let i = 0; i < 8; i++) c = (c >>> 1) ^ (c & 1 ? 0xedb88320 : 0);
  }
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const tag = Buffer.from(type);
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([tag, data])), 0);
  return Buffer.concat([len, tag, data, crc]);
}

function makePng(size) {
  const rows = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(1 + size * 4);
    for (let x = 0; x < size; x++) {
      const o = 1 + x * 4;
      row[o] = 56;
      row[o + 1] = 189;
      row[o + 2] = 248;
      row[o + 3] = 255;
    }
    rows.push(row);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(Buffer.concat(rows))),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

fs.writeFileSync(path.join(iconsDir, "32x32.png"), makePng(32));
fs.writeFileSync(path.join(iconsDir, "128x128.png"), makePng(128));
fs.writeFileSync(path.join(iconsDir, "128x128@2x.png"), makePng(256));
console.log("Generated PNG icons in src-tauri/icons");
""",
    "scripts/sync-version.mjs": """
import fs from "node:fs";

const pkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
const tauriConfPath = "src-tauri/tauri.conf.json";
const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, "utf8"));
tauriConf.version = pkg.version;
fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + "\\n");

const cargoPath = "src-tauri/Cargo.toml";
const cargo = fs.readFileSync(cargoPath, "utf8").replace(
  /^version\\s*=\\s*"[^"]+"/m,
  `version = "${pkg.version}"`
);
fs.writeFileSync(cargoPath, cargo);
console.log(`Synced version ${pkg.version}`);
""",
}


def main() -> None:
    for rel_path, content in FILES.items():
        write_if_missing(rel_path, content)
    generate_icons()


if __name__ == "__main__":
    main()

