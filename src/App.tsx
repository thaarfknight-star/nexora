import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import emblem from "./assets/emblem.svg";
import wordmark from "./assets/wordmark.png";
import gear from "./assets/gear.png";
import bell from "./assets/bell.png";
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
  /** Sent by the stats backend when it reports them; "—" until then. */
  gpu_percent?: number;
  net_up_kbps?: number;
  net_down_kbps?: number;
}

interface MediaInfo {
  playing: boolean;
  title: string;
  artist: string;
  album: string;
  source_app: string;
  playback_status: string;
  /** Album cover art (data URI) when the media backend provides it. */
  cover_art?: string;
  /** Source-app icon (base64 PNG) when the media backend provides it. */
  source_icon?: string;
}

interface TaskbarStatus {
  hidden: boolean;
  mode: string;
}

interface WatchdogStatus {
  healthy: boolean;
  explorer_ok: boolean;
}

interface NotificationItem {
  id: string;
  title: string;
  body: string;
  kind: string;
  /** Unix timestamp (seconds or milliseconds) or an ISO string. */
  timestamp: number | string;
}

interface UpdateInfo {
  current_version: string;
  update_available: boolean;
}

type IslandTab = "media" | "stats";

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

function clockTime(date: Date): string {
  return date.toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function clockDate(date: Date): string {
  return date.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
  });
}

function notificationDate(value: number | string): Date | null {
  const date =
    typeof value === "number"
      ? new Date(value > 1e11 ? value : value * 1000)
      : new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

function notificationTime(value: number | string): string {
  const date = notificationDate(value);
  if (!date) {
    return typeof value === "string" ? value : "—";
  }
  return date.toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function relativeTime(value: number | string, now: Date): string {
  const date = notificationDate(value);
  if (!date) {
    return typeof value === "string" ? value : "—";
  }
  const seconds = Math.max(
    0,
    Math.floor((now.getTime() - date.getTime()) / 1000),
  );
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}

// Display labels for the notification kinds the backend emits
// (update found, taskbar restored). Any other kind falls back to
// its own name, capitalized, so grouping never depends on the
// backend's exact vocabulary.
const NOTIFICATION_KIND_LABELS: Record<string, string> = {
  update: "Updates",
  taskbar: "Taskbar",
};

function notificationKindLabel(kind: string): string {
  return (
    NOTIFICATION_KIND_LABELS[kind] ??
    (kind ? kind.charAt(0).toUpperCase() + kind.slice(1) : "Other")
  );
}

function IslandApp() {
  const [state, setState] = useState<IslandState>({
    expanded: false,
    mode: "compact",
    title: "Nexora Island",
  });
  const [theme, setTheme] = useState<ThemeId>("neon-glass");
  const [tab, setTab] = useState<IslandTab>("media");
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [taskbar, setTaskbar] = useState<TaskbarStatus | null>(null);
  const [watchdog, setWatchdog] = useState<WatchdogStatus | null>(null);
  const [now, setNow] = useState(() => new Date());
  const [notifications, setNotifications] = useState<NotificationItem[]>([]);
  const [showNotifications, setShowNotifications] = useState(false);
  const [seenIds, setSeenIds] = useState<string[]>([]);
  const [density, setDensity] = useState<"comfortable" | "compact">(
    "comfortable",
  );

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

  type MediaCommand =
    | "media_play"
    | "media_pause"
    | "media_next"
    | "media_previous";

  const sendMediaCommand = (command: MediaCommand) => {
    // After every command — accepted or not — re-read the live state
    // so the display never disagrees with the backend.
    invoke<boolean>(command)
      .then(() => refreshMedia())
      .catch(() => refreshMedia());
  };

  const refreshNotifications = () => {
    // The backend lands with EAgent5's feat/notifications; until then
    // the invoke fails and the center stays empty — never fake items.
    invoke<NotificationItem[]>("get_notifications")
      .then(setNotifications)
      .catch(() => setNotifications([]));
  };

  const refreshAll = () => {
    refreshStats();
    refreshMedia();
    refreshNotifications();
    invoke<TaskbarStatus>("get_taskbar_status")
      .then(setTaskbar)
      .catch(() => {});
    invoke<WatchdogStatus>("get_watchdog_status")
      .then(setWatchdog)
      .catch(() => {});
  };

  useEffect(() => {
    // Saved choice wins; the backend default is the fallback.
    const saved = readSavedTheme();
    invoke<NexoraSettings>("get_settings")
      .then((loaded) => applyTheme(themeOrDefault(saved ?? loaded.theme)))
      .catch(() => applyTheme(themeOrDefault(saved)));
    invoke<IslandState>("get_island_state")
      .then(setState)
      .catch(() => {});
    // The Settings window persists a new theme with set_settings, which
    // emits settings-changed — apply it here at once, no restart.
    let unlisten: (() => void) | undefined;
    listen<NexoraSettings>("settings-changed", (event) => {
      applyTheme(themeOrDefault(event.payload.theme));
    })
      .then((stop) => {
        unlisten = stop;
      })
      .catch(() => {});
    // New notifications arrive as notifications-changed; re-read the
    // list so the badge stays live. Absent backend, nothing fires.
    let unlistenNotifications: (() => void) | undefined;
    listen("notifications-changed", () => {
      refreshNotifications();
    })
      .then((stop) => {
        unlistenNotifications = stop;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
      unlistenNotifications?.();
    };
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
      refreshNotifications();
    }, 3000);
    const clockId = window.setInterval(() => setNow(new Date()), 1000);
    return () => {
      window.clearInterval(id);
      window.clearInterval(clockId);
    };
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

  const openSettings = () => {
    // The command lives on this branch (EAgent5); before it merges to
    // main the invoke fails silently and the island stays as-is.
    invoke("open_settings").catch(() => {});
  };

  const toggleNotifications = () => {
    if (!state.expanded) {
      // The center only renders expanded — open the island first.
      toggle();
    }
    if (!showNotifications) {
      refreshNotifications();
      setSeenIds(notifications.map((item) => item.id));
    }
    setShowNotifications((prev) => !prev);
  };

  const dismissNotification = (id: string) => {
    invoke("dismiss_notification", { id })
      .then(() => refreshNotifications())
      .catch(() => refreshNotifications());
  };

  const clearNotifications = () => {
    invoke("clear_notifications")
      .then(() => refreshNotifications())
      .catch(() => refreshNotifications());
  };

  const markAllNotificationsRead = () => {
    setSeenIds(notifications.map((item) => item.id));
  };

  const clearNotificationGroup = (kind: string) => {
    // Per-group clear reuses the single-item dismiss command.
    const ids = notifications
      .filter((item) => item.kind === kind)
      .map((item) => item.id);
    Promise.allSettled(
      ids.map((id) => invoke("dismiss_notification", { id })),
    ).then(() => refreshNotifications());
  };

  const currentTheme = themeById(theme);
  const unreadCount = notifications.filter(
    (item) => !seenIds.includes(item.id),
  ).length;
  // Groups keep first-seen kind order; items keep backend order.
  const notificationGroups: {
    kind: string;
    label: string;
    items: NotificationItem[];
  }[] = [];
  for (const item of notifications) {
    const existing = notificationGroups.find(
      (group) => group.kind === item.kind,
    );
    if (existing) {
      existing.items.push(item);
    } else {
      notificationGroups.push({
        kind: item.kind,
        label: notificationKindLabel(item.kind),
        items: [item],
      });
    }
  }
  const tabs: { id: IslandTab; label: string }[] = [
    { id: "media", label: "Media" },
    { id: "stats", label: "Stats" },
  ];

  return (
    <main className="island-shell" data-tauri-drag-region>
      <section
        className={`island-pill${state.expanded ? " expanded" : ""}`}
        data-tauri-drag-region
      >
        <div className="island-row" data-tauri-drag-region>
          <div className="island-brand">
            <img
              className="island-emblem"
              src={emblem}
              alt="Nexora"
              width={28}
              height={28}
            />
            <div>
              <div className="island-title">{state.title}</div>
              <div className="island-sub">
                Mode: {state.mode} · {currentTheme.name}
              </div>
            </div>
          </div>
          {state.expanded && (
            <div className="island-clock">
              <div className="island-clock-time">{clockTime(now)}</div>
              <div className="island-clock-date">{clockDate(now)}</div>
            </div>
          )}
          <button
            type="button"
            className="bell-btn"
            onClick={toggleNotifications}
            title="Notifications"
            aria-label="Notifications"
            aria-expanded={showNotifications}
          >
            <img src={bell} alt="" width={28} height={28} />
            {unreadCount > 0 && (
              <span className="bell-badge">
                {unreadCount > 9 ? "9+" : unreadCount}
              </span>
            )}
          </button>
          <button
            type="button"
            className="gear-btn"
            onClick={openSettings}
            title="Open Settings"
            aria-label="Open Settings"
          >
            <img src={gear} alt="" width={28} height={28} />
          </button>
          <button className="island-btn" onClick={toggle} type="button">
            {state.expanded ? "Collapse" : "Expand"}
          </button>
        </div>
        {state.expanded && (
          <>
            {showNotifications ? (
              <div
                className={`notifications-panel${
                  density === "compact" ? " compact" : ""
                }`}
                role="region"
                aria-label="Notifications"
              >
                <div className="notifications-header">
                  <span className="notifications-title">Notifications</span>
                  <span className="notifications-actions">
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={() =>
                        setDensity(
                          density === "compact"
                            ? "comfortable"
                            : "compact",
                        )
                      }
                    >
                      {density === "compact" ? "Comfortable" : "Compact"}
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={markAllNotificationsRead}
                    >
                      Mark all read
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={clearNotifications}
                    >
                      Clear all
                    </button>
                  </span>
                </div>
                {notifications.length === 0 ? (
                  <div className="notifications-empty">
                    No notifications
                  </div>
                ) : (
                  <div className="notifications-list">
                    {notificationGroups.map((group) => (
                      <div
                        key={group.kind}
                        className="notification-group"
                      >
                        <div className="notification-group-header">
                          <span className="notification-group-label">
                            {group.label}
                          </span>
                          <span className="notification-group-count">
                            {group.items.length}
                          </span>
                          <button
                            type="button"
                            className="notification-group-clear"
                            onClick={() =>
                              clearNotificationGroup(group.kind)
                            }
                            aria-label={`Clear ${group.label} notifications`}
                          >
                            Clear
                          </button>
                        </div>
                        {group.items.map((item) => (
                          <div key={item.id} className="notification-item">
                            <div className="notification-text">
                              <span className="notification-title">
                                {item.title}
                              </span>
                              {item.body && (
                                <span className="notification-body">
                                  {item.body}
                                </span>
                              )}
                            </div>
                            <span className="notification-time">
                              <span>
                                {relativeTime(item.timestamp, now)}
                              </span>
                              <span className="notification-clock">
                                {notificationTime(item.timestamp)}
                              </span>
                            </span>
                            <button
                              type="button"
                              className="notification-dismiss"
                              onClick={() => dismissNotification(item.id)}
                              aria-label="Dismiss notification"
                            >
                              ×
                            </button>
                          </div>
                        ))}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ) : (
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
                <div className="media-card">
                  {media?.cover_art ? (
                    <img
                      className="media-cover"
                      src={media.cover_art}
                      alt={media.album || "Album cover"}
                    />
                  ) : (
                    <span className="media-cover-placeholder">
                      <img src={emblem} alt="" />
                    </span>
                  )}
                  <div
                    className="media-info"
                    title={
                      media
                        ? `${media.artist} — ${media.title}${
                            media.album ? ` (${media.album})` : ""
                          }`
                        : "No media playing"
                    }
                  >
                    <span className="media-track">
                      {media ? media.title : "—"}
                    </span>
                    <span className="media-artist">
                      {media ? media.artist : "—"}
                      {media?.album ? ` — ${media.album}` : ""}
                    </span>
                  </div>
                  {media?.source_icon && (
                    <img
                      className="media-source"
                      src={media.source_icon}
                      alt={media.source_app}
                    />
                  )}
                </div>
                <div className="panel-line">
                  <span className="panel-controls">
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={() => sendMediaCommand("media_previous")}
                    >
                      Prev
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={() =>
                        sendMediaCommand(
                          media?.playing ? "media_pause" : "media_play",
                        )
                      }
                    >
                      {media?.playing ? "Pause" : "Play"}
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={() => sendMediaCommand("media_next")}
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
                  <span>
                    {media?.source_app ? media.source_app : "—"} ·{" "}
                    {media ? media.playback_status : "—"}
                  </span>
                </div>
              </div>
            )}
            {tab === "stats" && (
              <div className="island-panel" role="tabpanel">
                <div className="stat-chips">
                  <div
                    className="stat-chip"
                    title={
                      stats
                        ? `CPU usage: ${stats.cpu_percent.toFixed(1)}%`
                        : "CPU usage: —"
                    }
                  >
                    <span className="stat-chip-label">CPU</span>
                    <span className="stat-chip-value">
                      {stats ? `${stats.cpu_percent.toFixed(0)}%` : "—"}
                    </span>
                  </div>
                  <div
                    className="stat-chip"
                    title={
                      stats
                        ? `RAM usage: ${stats.memory_percent.toFixed(1)}%`
                        : "RAM usage: —"
                    }
                  >
                    <span className="stat-chip-label">RAM</span>
                    <span className="stat-chip-value">
                      {stats ? `${stats.memory_percent.toFixed(0)}%` : "—"}
                    </span>
                  </div>
                  <div
                    className="stat-chip"
                    title={
                      stats?.gpu_percent !== undefined
                        ? `GPU usage: ${stats.gpu_percent.toFixed(1)}%`
                        : "GPU usage: —"
                    }
                  >
                    <span className="stat-chip-label">GPU</span>
                    <span className="stat-chip-value">
                      {stats?.gpu_percent !== undefined
                        ? `${stats.gpu_percent.toFixed(0)}%`
                        : "—"}
                    </span>
                  </div>
                </div>
                <div className="net-speed">
                  <span
                    title={
                      stats?.net_up_kbps !== undefined
                        ? `Upload: ${stats.net_up_kbps.toFixed(2)} KB/s`
                        : "Upload: —"
                    }
                  >
                    ↑{" "}
                    {stats?.net_up_kbps !== undefined
                      ? `${stats.net_up_kbps.toFixed(1)} KB/s`
                      : "—"}
                  </span>
                  <span
                    title={
                      stats?.net_down_kbps !== undefined
                        ? `Download: ${stats.net_down_kbps.toFixed(2)} KB/s`
                        : "Download: —"
                    }
                  >
                    ↓{" "}
                    {stats?.net_down_kbps !== undefined
                      ? `${stats.net_down_kbps.toFixed(1)} KB/s`
                      : "—"}
                  </span>
                </div>
                <div className="panel-line">
                  <span>
                    Taskbar: {taskbar ? (taskbar.hidden ? "hidden" : "shown") : "—"}
                  </span>
                  <span>
                    Watchdog:{" "}
                    {watchdog ? (watchdog.healthy ? "healthy" : "unhealthy") : "—"}
                  </span>
                </div>
                <div className="panel-line">
                  <span>
                    Explorer:{" "}
                    {watchdog ? (watchdog.explorer_ok ? "ok" : "down") : "—"}
                  </span>
                </div>
              </div>
            )}
              </>
            )}
          </>
        )}
      </section>
    </main>
  );
}

function SettingsView() {
  const [settings, setSettings] = useState<NexoraSettings | null>(null);
  const [theme, setTheme] = useState<ThemeId>("neon-glass");
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);

  const applyTheme = (id: ThemeId) => {
    setTheme(id);
    document.documentElement.dataset.theme = id;
    saveTheme(id);
  };

  const saveSettings = (next: NexoraSettings) => {
    invoke<NexoraSettings>("set_settings", { settings: next })
      .then(setSettings)
      .catch(() => {});
  };

  const pickTheme = (id: ThemeId) => {
    applyTheme(id);
    // Persist through the backend so set_settings emits
    // settings-changed and the island applies the theme at once.
    if (settings) {
      saveSettings({ ...settings, theme: id });
    }
  };

  const toggleSetting = (key: "auto_hide_taskbar" | "always_on_top") => {
    if (!settings) {
      return;
    }
    saveSettings({ ...settings, [key]: !settings[key] });
  };

  const previewTheme = (id: ThemeId | null) => {
    // Hover preview only: local to this window, never persisted and
    // never sent to the island — set_settings stays out of it.
    document.documentElement.dataset.theme = id ?? theme;
  };

  const resetToDefaults = () => {
    // Backend defaults (settings.rs defaults()): theme neon-glass,
    // auto-hide off, always-on-top on.
    applyTheme("neon-glass");
    if (settings) {
      saveSettings({
        ...settings,
        theme: "neon-glass",
        auto_hide_taskbar: false,
        always_on_top: true,
      });
    }
  };

  const checkForUpdates = () => {
    invoke<UpdateInfo>("check_updates")
      .then(setUpdateInfo)
      .catch(() => {});
  };

  useEffect(() => {
    const saved = readSavedTheme();
    invoke<NexoraSettings>("get_settings")
      .then((loaded) => {
        setSettings(loaded);
        applyTheme(themeOrDefault(saved ?? loaded.theme));
      })
      .catch(() => applyTheme(themeOrDefault(saved)));
    invoke<UpdateInfo>("check_updates")
      .then(setUpdateInfo)
      .catch(() => {});
    // Runs once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <main className="settings-shell">
      <img className="settings-wordmark" src={wordmark} alt="Nexora" />
      <h1 className="settings-title">Settings</h1>
      <h2 className="settings-section-title">Appearance</h2>
      <p className="settings-section-desc">
        Themes apply live to the island — hover a card to preview it
        here, click to keep it.
      </p>
      <div className="theme-cards" role="group" aria-label="Nexora theme">
        {THEMES.map((candidate) => (
          <button
            key={candidate.id}
            type="button"
            className={`theme-card${
              candidate.id === theme ? " selected" : ""
            }`}
            data-theme={candidate.id}
            aria-label={candidate.name}
            aria-pressed={candidate.id === theme}
            onClick={() => pickTheme(candidate.id)}
            onMouseEnter={() => previewTheme(candidate.id)}
            onMouseLeave={() => previewTheme(null)}
            onFocus={() => previewTheme(candidate.id)}
            onBlur={() => previewTheme(null)}
          >
            <span className="theme-card-preview" aria-hidden="true">
              <span className="theme-card-dot" />
              <span className="theme-card-bar" />
              <span className="theme-card-bar short" />
            </span>
            <span className="theme-card-name">
              {candidate.name}
              {candidate.id === theme ? " ✓" : ""}
            </span>
            <span className="theme-card-desc">
              {candidate.description}
            </span>
          </button>
        ))}
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>Auto-hide taskbar</span>
          <span className="settings-row-desc">
            Hide the Windows taskbar while Nexora runs.
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => toggleSetting("auto_hide_taskbar")}
        >
          {settings ? onOff(settings.auto_hide_taskbar) : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>Always on top</span>
          <span className="settings-row-desc">
            Keep the island above other windows.
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => toggleSetting("always_on_top")}
        >
          {settings ? onOff(settings.always_on_top) : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span>
          {updateInfo
            ? updateInfo.update_available
              ? "Update available"
              : "No update available"
            : "—"}
        </span>
        <button type="button" className="mini-btn" onClick={checkForUpdates}>
          Check for updates
        </button>
      </div>
      <button
        type="button"
        className="mini-btn settings-reset"
        onClick={resetToDefaults}
      >
        Reset to defaults
      </button>
      <div className="settings-version">
        Nexora v{updateInfo ? updateInfo.current_version : "—"}
      </div>
    </main>
  );
}

export function App() {
  const [windowLabel, setWindowLabel] = useState<string | null>(null);

  useEffect(() => {
    // The settings window is opened by the backend with label "settings"
    // (EAgent5); every other window renders the island.
    try {
      setWindowLabel(getCurrentWindow().label);
    } catch {
      setWindowLabel("main");
    }
    // Runs once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (windowLabel === null) {
    return null;
  }
  return windowLabel === "settings" ? <SettingsView /> : <IslandApp />;
}

export default App;
