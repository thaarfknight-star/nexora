import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  THEMES,
  themeById,
  themeOrDefault,
  type ThemeId,
} from "./themes";

interface IslandState {
  expanded: boolean;
  mode: string;
  title: string;
}

interface NexoraSettings {
  auto_hide_taskbar: boolean;
  always_on_top: boolean;
  theme: string;
}

interface SystemStats {
  cpu_percent: number;
  memory_percent: number;
}

interface MediaInfo {
  playing: boolean;
  title: string;
  artist: string;
}

interface TaskbarStatus {
  hidden: boolean;
  mode: string;
}

interface WatchdogStatus {
  healthy: boolean;
  explorer_ok: boolean;
}

interface TrayStatus {
  active: boolean;
  items: number;
}

interface UpdateInfo {
  current_version: string;
  update_available: boolean;
}

type IslandTab = "media" | "stats" | "settings";

const STORAGE_KEY = "nexora-theme";

function readSavedTheme(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function saveTheme(id: ThemeId) {
  try {
    localStorage.setItem(STORAGE_KEY, id);
  } catch {
    // Storage unavailable (private mode, etc.) — the theme still applies
    // for this session.
  }
}

function onOff(value: boolean): string {
  return value ? "On" : "Off";
}

export function App() {
  const [state, setState] = useState<IslandState>({
    expanded: false,
    mode: "compact",
    title: "Nexora Island",
  });
  const [theme, setTheme] = useState<ThemeId>("obsidian");
  const [tab, setTab] = useState<IslandTab>("media");
  const [settings, setSettings] = useState<NexoraSettings | null>(null);
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [taskbar, setTaskbar] = useState<TaskbarStatus | null>(null);
  const [watchdog, setWatchdog] = useState<WatchdogStatus | null>(null);
  const [tray, setTray] = useState<TrayStatus | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);

  const applyTheme = (id: ThemeId) => {
    setTheme(id);
    document.documentElement.dataset.theme = id;
    saveTheme(id);
  };

  const refreshStats = () => {
    invoke<SystemStats>("get_system_stats")
      .then(setStats)
      .catch(() => {});
  };

  const refreshMedia = () => {
    invoke<MediaInfo>("get_now_playing")
      .then(setMedia)
      .catch(() => {});
  };

  const refreshAll = () => {
    refreshStats();
    refreshMedia();
    invoke<TaskbarStatus>("get_taskbar_status")
      .then(setTaskbar)
      .catch(() => {});
    invoke<WatchdogStatus>("get_watchdog_status")
      .then(setWatchdog)
      .catch(() => {});
    invoke<TrayStatus>("get_tray_status")
      .then(setTray)
      .catch(() => {});
    // Settings are shown read-only; the saved theme is never overridden
    // here (localStorage wins, see the mount effect).
    invoke<NexoraSettings>("get_settings")
      .then(setSettings)
      .catch(() => {});
    invoke<UpdateInfo>("check_updates")
      .then(setUpdateInfo)
      .catch(() => {});
  };

  useEffect(() => {
    // Saved choice wins; the backend default is the fallback.
    const saved = readSavedTheme();
    invoke<NexoraSettings>("get_settings")
      .then((loaded) => {
        setSettings(loaded);
        applyTheme(themeOrDefault(saved ?? loaded.theme));
      })
      .catch(() => applyTheme(themeOrDefault(saved)));
    invoke<IslandState>("get_island_state")
      .then(setState)
      .catch(() => {});
    // Runs once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!state.expanded) {
      return;
    }
    refreshAll();
    const id = window.setInterval(() => {
      refreshStats();
      refreshMedia();
    }, 3000);
    return () => window.clearInterval(id);
    // Refresh helpers only use stable setters and invoke.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.expanded]);

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

  const checkForUpdates = () => {
    invoke<UpdateInfo>("check_updates")
      .then(setUpdateInfo)
      .catch(() => {});
  };

  const currentTheme = themeById(theme);
  const tabs: { id: IslandTab; label: string }[] = [
    { id: "media", label: "Media" },
    { id: "stats", label: "Stats" },
    { id: "settings", label: "Settings" },
  ];

  return (
    <main className="island-shell" data-tauri-drag-region>
      <section
        className={`island-pill${state.expanded ? " expanded" : ""}`}
        data-tauri-drag-region
      >
        <div className="island-row" data-tauri-drag-region>
          <div className="island-brand">
            <span className="island-dot" />
            <div>
              <div className="island-title">{state.title}</div>
              <div className="island-sub">
                Mode: {state.mode} · {currentTheme.name}
              </div>
            </div>
          </div>
          <button className="island-btn" onClick={toggle} type="button">
            {state.expanded ? "Collapse" : "Expand"}
          </button>
        </div>
        {state.expanded && (
          <>
            <div
              className="island-tabs"
              role="tablist"
              aria-label="Island panels"
            >
              {tabs.map((candidate) => (
                <button
                  key={candidate.id}
                  type="button"
                  role="tab"
                  aria-selected={tab === candidate.id}
                  className={`island-tab${
                    tab === candidate.id ? " active" : ""
                  }`}
                  onClick={() => setTab(candidate.id)}
                >
                  {candidate.label}
                </button>
              ))}
            </div>
            {tab === "media" && (
              <div className="island-panel" role="tabpanel">
                <div className="panel-line">
                  <span className="panel-title">
                    {media ? `${media.title} — ${media.artist}` : "—"}
                  </span>
                  <span className="panel-controls">
                    <button
                      type="button"
                      className="mini-btn"
                      disabled
                      title="Playback controls arrive with the media engine"
                    >
                      Prev
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      disabled
                      title="Playback controls arrive with the media engine"
                    >
                      {media?.playing ? "Pause" : "Play"}
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      disabled
                      title="Playback controls arrive with the media engine"
                    >
                      Next
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={refreshMedia}
                    >
                      Refresh
                    </button>
                  </span>
                </div>
              </div>
            )}
            {tab === "stats" && (
              <div className="island-panel" role="tabpanel">
                <div className="panel-line">
                  <span>
                    CPU{" "}
                    {stats ? `${stats.cpu_percent.toFixed(1)}%` : "—"} ·
                    Memory{" "}
                    {stats ? `${stats.memory_percent.toFixed(1)}%` : "—"}
                  </span>
                  <span>
                    Taskbar: {taskbar ? (taskbar.hidden ? "hidden" : "shown") : "—"}
                  </span>
                </div>
                <div className="panel-line">
                  <span>
                    Watchdog:{" "}
                    {watchdog ? (watchdog.healthy ? "healthy" : "unhealthy") : "—"}
                  </span>
                  <span>
                    Explorer:{" "}
                    {watchdog ? (watchdog.explorer_ok ? "ok" : "down") : "—"}
                  </span>
                </div>
              </div>
            )}
            {tab === "settings" && (
              <div className="island-panel" role="tabpanel">
                <div
                  className="island-themes"
                  role="group"
                  aria-label="Island theme"
                >
                  <span className="theme-label">Theme:</span>
                  {THEMES.map((candidate) => (
                    <button
                      key={candidate.id}
                      type="button"
                      className={`theme-swatch${
                        candidate.id === theme ? " selected" : ""
                      }`}
                      style={{ background: candidate.swatch }}
                      title={`${candidate.name} — ${candidate.description}`}
                      aria-label={candidate.name}
                      aria-pressed={candidate.id === theme}
                      onClick={() => applyTheme(candidate.id)}
                    />
                  ))}
                </div>
                <div className="panel-line">
                  <span>
                    Always on top:{" "}
                    {settings ? onOff(settings.always_on_top) : "—"} ·
                    Auto-hide taskbar:{" "}
                    {settings ? onOff(settings.auto_hide_taskbar) : "—"}
                  </span>
                  <span>
                    Tray items: {tray ? tray.items : "—"} · v
                    {updateInfo ? updateInfo.current_version : "—"}
                  </span>
                </div>
                <div className="panel-line">
                  <span>
                    {updateInfo
                      ? updateInfo.update_available
                        ? "Update available"
                        : "No update available"
                      : "—"}
                  </span>
                  <button
                    type="button"
                    className="mini-btn"
                    onClick={checkForUpdates}
                  >
                    Check for updates
                  </button>
                </div>
              </div>
            )}
          </>
        )}
      </section>
    </main>
  );
}

export default App;
