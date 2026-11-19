import { useEffect, useRef, useState } from "react";
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
  width: number;
  height: number;
  content_x: number;
  content_y: number;
  content_width: number;
  content_height: number;
  compact_height: number;
  handle_width: number;
  handle_height: number;
}

interface NexoraSettings {
  auto_hide_taskbar: boolean;
  always_on_top: boolean;
  theme: string;
  /** Island blur override in px; null means the theme default. */
  island_blur_px: number | null;
  /** Island corner-radius override in px; null means the theme default. */
  island_radius_px: number | null;
  /** Clock in 24-hour format (`false` is 12-hour). */
  clock_24h: boolean;
  /** Clock shows seconds. */
  show_seconds: boolean;
  /** Date format: "short" (Nov 16) or "full" (Sun, Nov 16, 2026). */
  date_format: string;
  /** Stats/media refresh interval in seconds. */
  poll_interval_secs: number;
  /** Speed unit: "auto" (MB/s above 1024 KB/s), "kb" or "mb". */
  stats_speed_unit: string;
  /** Percent style: "with" (24%) or "without" (24). */
  stats_percent_style: string;
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

function clockTime(date: Date, use24h: boolean, showSeconds: boolean): string {
  return date.toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    ...(showSeconds ? { second: "2-digit" } : {}),
    hour12: !use24h,
  });
}

function clockDate(date: Date, format: string): string {
  if (format === "full") {
    return date.toLocaleDateString("en-US", {
      weekday: "short",
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  }
  return date.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
  });
}

// Refresh interval the poll slider offers (seconds); a stored value
// outside it is clamped, never faster than 1s.
function clampPollSecs(value: number): number {
  return Math.min(5, Math.max(1, Math.round(value)));
}

function formatSpeed(kbps: number, unit: string): string {
  if (unit === "mb") return `${(kbps / 1024).toFixed(1)} MB/s`;
  if (unit === "kb") return `${kbps.toFixed(1)} KB/s`;
  return kbps >= 1024
    ? `${(kbps / 1024).toFixed(1)} MB/s`
    : `${kbps.toFixed(1)} KB/s`;
}

function formatPercent(value: number, style: string): string {
  const text = value.toFixed(0);
  return style === "without" ? text : `${text}%`;
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
    // Backend constants (island.rs) until get_island_state answers.
    width: 640,
    height: 440,
    content_x: 40,
    content_y: 40,
    content_width: 560,
    content_height: 360,
    compact_height: 64,
    handle_width: 120,
    handle_height: 4,
  });
  const [theme, setTheme] = useState<ThemeId>("neon-glass");
  const [tab, setTab] = useState<IslandTab>("media");
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [taskbar, setTaskbar] = useState<TaskbarStatus | null>(null);
  const [watchdog, setWatchdog] = useState<WatchdogStatus | null>(null);
  const [now, setNow] = useState(() => new Date());
  // Island visibility state machine (design artifact): the window
  // never resizes — the pill animates inside the fixed canvas.
  type IslandVisible = "hidden" | "compact" | "expanded";
  // Delay after the pointer leaves the island before it steps one
  // level down (expanded -> compact -> hidden).
  const AUTO_CLOSE_DELAY_MS = 2500;
  const [visible, setVisibleState] = useState<IslandVisible>("hidden");
  // Stage height duration: open ~320 ms, close ~220 ms (design
  // artifact). Set per transition; the CSS default is the close one.
  const [stageMs, setStageMs] = useState(220);
  const [keepOpen, setKeepOpen] = useState(false);
  const visibleRef = useRef<IslandVisible>("hidden");
  const keepOpenRef = useRef(false);
  const hoveringRef = useRef(false);
  const autoCloseTimer = useRef<number | undefined>(undefined);

  const clearAutoCloseTimer = () => {
    if (autoCloseTimer.current !== undefined) {
      window.clearTimeout(autoCloseTimer.current);
      autoCloseTimer.current = undefined;
    }
  };

  const showIsland = (next: IslandVisible) => {
    const opening =
      (visibleRef.current === "hidden" && next === "compact") ||
      (visibleRef.current === "compact" && next === "expanded");
    setStageMs(opening ? 320 : 220);
    visibleRef.current = next;
    setVisibleState(next);
    // Keep the backend flag in sync: it drives the poll loop.
    invoke<IslandState>("set_island_expanded", {
      expanded: next === "expanded",
    })
      .then(setState)
      .catch(() => {});
    // Click-through policy: hidden -> ignore, visible -> receive.
    invoke("set_island_click_through", { ignore: next === "hidden" }).catch(
      () => {},
    );
  };

  const armAutoClose = () => {
    clearAutoCloseTimer();
    if (keepOpenRef.current || hoveringRef.current) {
      return;
    }
    autoCloseTimer.current = window.setTimeout(() => {
      const current = visibleRef.current;
      if (current === "expanded") {
        showIsland("compact");
        armAutoClose();
      } else if (current === "compact") {
        showIsland("hidden");
      }
    }, AUTO_CLOSE_DELAY_MS);
  };

  const [notifications, setNotifications] = useState<NotificationItem[]>([]);
  const [showNotifications, setShowNotifications] = useState(false);
  const [seenIds, setSeenIds] = useState<string[]>([]);
  const [density, setDensity] = useState<"comfortable" | "compact">(
    "comfortable",
  );
  // Island style overrides from settings; null means the theme default.
  const [islandStyle, setIslandStyle] = useState<{
    blur: number | null;
    radius: number | null;
  }>({ blur: null, radius: null });
  // Clock and performance display, from settings; the fallbacks are
  // the backend defaults() so the island is right even before the
  // first settings load.
  const [displaySettings, setDisplaySettings] = useState({
    clock24h: true,
    showSeconds: false,
    dateFormat: "short",
    pollIntervalSecs: 3,
    speedUnit: "auto",
    percentStyle: "with",
  });

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
      .then((loaded) => {
        applyTheme(themeOrDefault(saved ?? loaded.theme));
        setIslandStyle({
          blur: loaded.island_blur_px ?? null,
          radius: loaded.island_radius_px ?? null,
        });
        setDisplaySettings({
          clock24h: loaded.clock_24h ?? true,
          showSeconds: loaded.show_seconds ?? false,
          dateFormat: loaded.date_format ?? "short",
          pollIntervalSecs: clampPollSecs(loaded.poll_interval_secs ?? 3),
          speedUnit: loaded.stats_speed_unit ?? "auto",
          percentStyle: loaded.stats_percent_style ?? "with",
        });
      })
      .catch(() => applyTheme(themeOrDefault(saved)));
    invoke<IslandState>("get_island_state")
      .then(setState)
      .catch(() => {});
    // The Settings window persists a new theme with set_settings, which
    // emits settings-changed — apply it here at once, no restart.
    let unlisten: (() => void) | undefined;
    listen<NexoraSettings>("settings-changed", (event) => {
      applyTheme(themeOrDefault(event.payload.theme));
      setIslandStyle({
        blur: event.payload.island_blur_px ?? null,
        radius: event.payload.island_radius_px ?? null,
      });
      setDisplaySettings({
        clock24h: event.payload.clock_24h ?? true,
        showSeconds: event.payload.show_seconds ?? false,
        dateFormat: event.payload.date_format ?? "short",
        pollIntervalSecs: clampPollSecs(event.payload.poll_interval_secs ?? 3),
        speedUnit: event.payload.stats_speed_unit ?? "auto",
        percentStyle: event.payload.stats_percent_style ?? "with",
      });
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
    // Hot zone (backend watcher, 100 ms poll of the monitor top edge):
    // entering wakes the island from hidden; leaving arms the
    // auto-close timer. The watcher also fires once at startup.
    let unlistenHotZone: (() => void) | undefined;
    listen<{ in_hot_zone: boolean }>("island-hot-zone", (event) => {
      if (event.payload.in_hot_zone) {
        clearAutoCloseTimer();
        if (visibleRef.current === "hidden") {
          showIsland("compact");
        }
      } else if (visibleRef.current === "compact") {
        armAutoClose();
      }
    })
      .then((stop) => {
        unlistenHotZone = stop;
      })
      .catch(() => {});
    // Esc steps one level down, unless the island is pinned open.
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || keepOpenRef.current) {
        return;
      }
      if (visibleRef.current === "expanded") {
        showIsland("compact");
      } else if (visibleRef.current === "compact") {
        showIsland("hidden");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    // The island starts hidden: the fixed canvas lets clicks pass
    // through until the hot zone wakes it.
    invoke("set_island_click_through", { ignore: true }).catch(() => {});
    return () => {
      clearAutoCloseTimer();
      window.removeEventListener("keydown", onKeyDown);
      unlisten?.();
      unlistenNotifications?.();
      unlistenHotZone?.();
    };
    // Runs once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (visible !== "expanded") {
      return;
    }
    refreshAll();
    const id = window.setInterval(
      () => {
        refreshStats();
        refreshMedia();
        refreshNotifications();
      },
      clampPollSecs(displaySettings.pollIntervalSecs) * 1000,
    );
    const clockId = window.setInterval(() => setNow(new Date()), 1000);
    return () => {
      window.clearInterval(id);
      window.clearInterval(clockId);
    };
    // Refresh helpers only use stable setters and invoke.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, displaySettings.pollIntervalSecs]);

  // Clicking the island itself toggles expand / collapse; the bell
  // and gear buttons are siblings, so their clicks do not toggle.
  const toggleExpand = () => {
    if (visible === "compact") {
      showIsland("expanded");
    } else if (visible === "expanded") {
      showIsland("compact");
    }
  };

  // Clicking the empty canvas around the pill steps one level down.
  const onShellClick = (event: React.MouseEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) {
      return;
    }
    if (visible === "expanded") {
      showIsland("compact");
    } else if (visible === "compact") {
      showIsland("hidden");
    }
  };

  const onShellMouseEnter = () => {
    hoveringRef.current = true;
    clearAutoCloseTimer();
  };

  const onShellMouseLeave = () => {
    hoveringRef.current = false;
    if (visibleRef.current !== "hidden") {
      armAutoClose();
    }
  };

  const toggleKeepOpen = () => {
    const next = !keepOpen;
    keepOpenRef.current = next;
    setKeepOpen(next);
    if (next) {
      clearAutoCloseTimer();
    } else if (!hoveringRef.current && visibleRef.current !== "hidden") {
      armAutoClose();
    }
  };

  const openSettings = () => {
    // The command lives on this branch (EAgent5); before it merges to
    // main the invoke fails silently and the island stays as-is.
    invoke("open_settings").catch(() => {});
  };

  const toggleNotifications = () => {
    if (visible !== "expanded") {
      // The center only renders expanded — open the island first.
      showIsland("expanded");
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
    <main
      className="island-shell"
      onMouseEnter={onShellMouseEnter}
      onMouseLeave={onShellMouseLeave}
      onClick={onShellClick}
    >
      <div
        className={`island-stage ${visible}`}
        style={{
          transitionDuration: `${stageMs}ms`,
          // Geometry comes from the backend (island.rs), one source of truth.
          left: state.content_x,
          top: state.content_y,
          width: state.content_width,
          height:
            visible === "hidden"
              ? state.handle_height
              : visible === "compact"
                ? state.compact_height
                : state.content_height,
        }}
      >
        <div
          className="island-handle"
          aria-hidden="true"
          style={{
            width: state.handle_width,
            height: state.handle_height,
          }}
        />
        <section
          className={`island-pill${visible === "expanded" ? " expanded" : ""}`}
        style={{
          ...(islandStyle.blur !== null
            ? { backdropFilter: `blur(${islandStyle.blur}px)` }
            : {}),
          ...(islandStyle.radius !== null
            ? { borderRadius: `${islandStyle.radius}px` }
            : {}),
        }}
      >
        <div className="island-row">
          <div
            className="island-brand"
            onClick={toggleExpand}
            title={visible === "expanded" ? "Collapse" : "Expand"}
          >
            <img
              className="island-emblem"
              src={emblem}
              alt="Nexora"
              width={28}
              height={28}
            />
            <div>
              <div className="island-title">{state.title}</div>
              <div className="island-sub">{currentTheme.name}</div>
            </div>
          </div>
          {visible === "expanded" && (
            <div className="island-clock">
              <div className="island-clock-time">
                {clockTime(
                  now,
                  displaySettings.clock24h,
                  displaySettings.showSeconds,
                )}
              </div>
              <div className="island-clock-date">
                {clockDate(now, displaySettings.dateFormat)}
              </div>
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
          {visible === "expanded" && (
            <button
              type="button"
              className={`mini-btn${keepOpen ? " pinned" : ""}`}
              onClick={toggleKeepOpen}
              title={
                keepOpen
                  ? "Keep open is on — auto-close is off"
                  : "Keep open — stop auto-close"
              }
              aria-pressed={keepOpen}
            >
              {keepOpen ? "Pinned ✓" : "Keep open"}
            </button>
          )}
        </div>
        {visible === "expanded" && (
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
                      {stats
                        ? formatPercent(
                            stats.cpu_percent,
                            displaySettings.percentStyle,
                          )
                        : "—"}
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
                      {stats
                        ? formatPercent(
                            stats.memory_percent,
                            displaySettings.percentStyle,
                          )
                        : "—"}
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
                        ? formatPercent(
                            stats.gpu_percent,
                            displaySettings.percentStyle,
                          )
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
                      ? formatSpeed(
                          stats.net_up_kbps,
                          displaySettings.speedUnit,
                        )
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
                      ? formatSpeed(
                          stats.net_down_kbps,
                          displaySettings.speedUnit,
                        )
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
      </div>
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

  const toggleSetting = (
    key:
      | "auto_hide_taskbar"
      | "always_on_top"
      | "clock_24h"
      | "show_seconds",
  ) => {
    if (!settings) {
      return;
    }
    saveSettings({ ...settings, [key]: !settings[key] });
  };

  const cycleDateFormat = () => {
    if (!settings) {
      return;
    }
    saveSettings({
      ...settings,
      date_format: settings.date_format === "full" ? "short" : "full",
    });
  };

  const cycleSpeedUnit = () => {
    if (!settings) {
      return;
    }
    const next =
      settings.stats_speed_unit === "auto"
        ? "kb"
        : settings.stats_speed_unit === "kb"
          ? "mb"
          : "auto";
    saveSettings({ ...settings, stats_speed_unit: next });
  };

  const cyclePercentStyle = () => {
    if (!settings) {
      return;
    }
    saveSettings({
      ...settings,
      stats_percent_style:
        settings.stats_percent_style === "with" ? "without" : "with",
    });
  };

  // Slider range for the island style overrides (px). The backend
  // stores Option<u8>; the frontend clamps to this range so a stale
  // or out-of-range stored value never breaks the slider.
  const ISLAND_STYLE_MIN_PX = 0;
  const ISLAND_STYLE_MAX_PX = 32;
  const clampStylePx = (value: number) =>
    Math.min(
      ISLAND_STYLE_MAX_PX,
      Math.max(ISLAND_STYLE_MIN_PX, Math.round(value)),
    );

  // The theme's own blur, read live from the CSS variable so the
  // "theme default" label always matches the active theme.
  const themeBlurPx = () => {
    const raw = getComputedStyle(document.documentElement).getPropertyValue(
      "--island-blur",
    );
    const parsed = Number.parseInt(raw, 10);
    return Number.isNaN(parsed) ? 18 : parsed;
  };

  // The theme's own corner radius: the fixed 28px in .island-pill.
  const ISLAND_RADIUS_DEFAULT_PX = 28;

  const setStyleField = (
    key: "island_blur_px" | "island_radius_px",
    value: number | null,
  ) => {
    if (!settings) {
      return;
    }
    saveSettings({ ...settings, [key]: value });
  };

  const previewTheme = (id: ThemeId | null) => {
    // Hover preview only: local to this window, never persisted and
    // never sent to the island — set_settings stays out of it.
    document.documentElement.dataset.theme = id ?? theme;
  };

  const resetToDefaults = () => {
    // Backend defaults (settings.rs defaults()): theme neon-glass,
    // auto-hide on, always-on-top on, blur and radius unset (theme),
    // 24-hour clock, no seconds, short date, 3s poll, auto speed
    // unit, percent with %.
    applyTheme("neon-glass");
    if (settings) {
      saveSettings({
        ...settings,
        theme: "neon-glass",
        auto_hide_taskbar: true,
        always_on_top: true,
        island_blur_px: null,
        island_radius_px: null,
        clock_24h: true,
        show_seconds: false,
        date_format: "short",
        poll_interval_secs: 3,
        stats_speed_unit: "auto",
        stats_percent_style: "with",
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
      <h2 className="settings-section-title">Island style</h2>
      <p className="settings-section-desc">
        Blur and corner radius override the theme defaults — set them
        back to Default to follow the theme again.
      </p>
      <div className="settings-slider-row">
        <div className="settings-row-text">
          <span>Island blur</span>
          <span className="settings-row-desc">
            {settings?.island_blur_px != null
              ? `Override: ${settings.island_blur_px} px.`
              : `Theme default: ${themeBlurPx()} px.`}
          </span>
        </div>
        <div className="settings-slider-line">
          <input
            type="range"
            className="settings-slider"
            min={ISLAND_STYLE_MIN_PX}
            max={ISLAND_STYLE_MAX_PX}
            step={1}
            value={settings?.island_blur_px ?? themeBlurPx()}
            disabled={!settings}
            aria-label="Island blur in pixels"
            onChange={(event) =>
              setStyleField(
                "island_blur_px",
                clampStylePx(Number(event.target.value)),
              )
            }
          />
          <span className="settings-slider-value">
            {settings?.island_blur_px != null
              ? `${settings.island_blur_px} px`
              : "Default"}
          </span>
          <button
            type="button"
            className="mini-btn"
            disabled={!settings || settings?.island_blur_px == null}
            onClick={() => setStyleField("island_blur_px", null)}
          >
            Default
          </button>
        </div>
      </div>
      <div className="settings-slider-row">
        <div className="settings-row-text">
          <span>Corner radius</span>
          <span className="settings-row-desc">
            {settings?.island_radius_px != null
              ? `Override: ${settings.island_radius_px} px.`
              : `Theme default: ${ISLAND_RADIUS_DEFAULT_PX} px.`}
          </span>
        </div>
        <div className="settings-slider-line">
          <input
            type="range"
            className="settings-slider"
            min={ISLAND_STYLE_MIN_PX}
            max={ISLAND_STYLE_MAX_PX}
            step={1}
            value={settings?.island_radius_px ?? ISLAND_RADIUS_DEFAULT_PX}
            disabled={!settings}
            aria-label="Island corner radius in pixels"
            onChange={(event) =>
              setStyleField(
                "island_radius_px",
                clampStylePx(Number(event.target.value)),
              )
            }
          />
          <span className="settings-slider-value">
            {settings?.island_radius_px != null
              ? `${settings.island_radius_px} px`
              : "Default"}
          </span>
          <button
            type="button"
            className="mini-btn"
            disabled={!settings || settings?.island_radius_px == null}
            onClick={() => setStyleField("island_radius_px", null)}
          >
            Default
          </button>
        </div>
      </div>
      <h2 className="settings-section-title">Clock</h2>
      <p className="settings-section-desc">
        How the island clock shows the time and date.
      </p>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>24-hour clock</span>
          <span className="settings-row-desc">
            Show the time as 21:47 instead of 9:47 PM.
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => toggleSetting("clock_24h")}
        >
          {settings ? onOff(settings.clock_24h) : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>Show seconds</span>
          <span className="settings-row-desc">
            Show seconds in the island clock.
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => toggleSetting("show_seconds")}
        >
          {settings ? onOff(settings.show_seconds) : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>Date format</span>
          <span className="settings-row-desc">
            Short shows Nov 16; Full shows Sun, Nov 16, 2026.
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={cycleDateFormat}
          disabled={!settings}
        >
          {settings
            ? settings.date_format === "full"
              ? "Full"
              : "Short"
            : "—"}
        </button>
      </div>
      <h2 className="settings-section-title">Performance</h2>
      <p className="settings-section-desc">
        How often stats and media refresh, and how the numbers look.
      </p>
      <div className="settings-slider-row">
        <div className="settings-row-text">
          <span>Refresh interval</span>
          <span className="settings-row-desc">
            How often stats and media refresh.
          </span>
        </div>
        <div className="settings-slider-line">
          <input
            type="range"
            className="settings-slider"
            min={1}
            max={5}
            step={1}
            value={settings ? clampPollSecs(settings.poll_interval_secs) : 3}
            disabled={!settings}
            aria-label="Refresh interval in seconds"
            onChange={(event) =>
              settings &&
              saveSettings({
                ...settings,
                poll_interval_secs: clampPollSecs(
                  Number(event.target.value),
                ),
              })
            }
          />
          <span className="settings-slider-value">
            {settings ? `${clampPollSecs(settings.poll_interval_secs)} s` : "—"}
          </span>
        </div>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>Speed unit</span>
          <span className="settings-row-desc">
            Auto switches to MB/s above 1024 KB/s.
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={cycleSpeedUnit}
          disabled={!settings}
        >
          {settings
            ? settings.stats_speed_unit === "auto"
              ? "Auto"
              : settings.stats_speed_unit === "kb"
                ? "KB/s"
                : "MB/s"
            : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>Percent style</span>
          <span className="settings-row-desc">
            Show or hide the % sign on stat chips.
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={cyclePercentStyle}
          disabled={!settings}
        >
          {settings
            ? settings.stats_percent_style === "with"
              ? "With %"
              : "Without %"
            : "—"}
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
