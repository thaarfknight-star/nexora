import { useEffect, useMemo, useRef, useState } from "react";
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
import {
  NOTIFICATION_KIND_LABELS,
  THEME_DESCRIPTIONS,
  applyLanguageToDocument,
  normalizeLanguage,
  t,
  tf,
  type Language,
} from "./i18n";
import { jalaliDate } from "./jalali";

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
  /** UI language: "en" (default) or "fa". */
  language: string;
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

/** One open window for the compact bar's taskbar zone. Icons are
 * raw RGBA pixels as base64 (32x32), not PNG data-URLs. */
interface TaskbarWindow {
  id: number;
  title: string;
  minimized: boolean;
  active: boolean;
  icon_size: number;
  icon_rgba_base64: string;
}

/** One app pinned to the user's taskbar. Same icon format. */
interface PinnedApp {
  name: string;
  target: string;
  icon_size: number;
  icon_rgba_base64: string;
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

function onOff(value: boolean, lang: Language): string {
  return t(lang, value ? "on" : "off");
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

// Both halves of the island clock: Gregorian first, then Jalali.
// The Settings date format applies to both — short is day + month,
// full adds the weekday and the year on both sides.
function clockDateParts(date: Date, format: string): {
  gregorian: string;
  jalali: string;
} {
  return {
    gregorian: clockDate(date, format),
    jalali: jalaliDate(date, format),
  };
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

function relativeTime(value: number | string, now: Date, lang: Language): string {
  const date = notificationDate(value);
  if (!date) {
    return typeof value === "string" ? value : "—";
  }
  const seconds = Math.max(
    0,
    Math.floor((now.getTime() - date.getTime()) / 1000),
  );
  if (seconds < 60) return t(lang, "relativeJustNow");
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return tf(lang, "relativeMin", { n: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return tf(lang, "relativeHour", { n: hours });
  return tf(lang, "relativeDay", { n: Math.floor(hours / 24) });
}

// Display labels for the notification kinds the backend emits
// (update found, taskbar restored). Any other kind falls back to
// its own name, capitalized, so grouping never depends on the
// backend's exact vocabulary.
function notificationKindLabel(kind: string, lang: Language): string {
  return (
    NOTIFICATION_KIND_LABELS[lang][kind] ??
    (kind ? kind.charAt(0).toUpperCase() + kind.slice(1) : t(lang, "kindOther"))
  );
}

/** Decode a base64 raw-RGBA icon into pixels, or null when the
 * backend sent no icon (icon_size 0) or the data is unusable. */
function decodeRgbaIcon(
  rgbaBase64: string,
  size: number,
): Uint8ClampedArray | null {
  if (!rgbaBase64 || size <= 0) {
    return null;
  }
  try {
    const bytes = Uint8Array.from(atob(rgbaBase64), (c) => c.charCodeAt(0));
    if (bytes.length !== size * size * 4) {
      return null;
    }
    return new Uint8ClampedArray(bytes);
  } catch {
    return null;
  }
}

/**
 * A taskbar icon from raw RGBA pixels: drawn on a canvas (the
 * backend sends pixels, not PNG). No icon data renders a placeholder
 * square with the title's first letter — never a fake app icon.
 */
function RgbaIcon({
  rgbaBase64,
  size,
  title,
}: {
  rgbaBase64: string;
  size: number;
  title: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const pixels = useMemo(
    () => decodeRgbaIcon(rgbaBase64, size),
    [rgbaBase64, size],
  );
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !pixels) {
      return;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    ctx.putImageData(new ImageData(pixels, size, size), 0, 0);
  }, [pixels, size]);
  if (!pixels) {
    return (
      <span
        className="taskbar-icon-placeholder"
        title={title}
        aria-label={title}
      >
        {title ? title.charAt(0) : "—"}
      </span>
    );
  }
  return (
    <canvas
      ref={canvasRef}
      width={size}
      height={size}
      className="taskbar-icon"
      title={title}
      aria-label={title}
    />
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
  // Compact bar's taskbar zone: open windows + pinned apps.
  const [taskbarWindows, setTaskbarWindows] = useState<TaskbarWindow[]>([]);
  const [pinnedApps, setPinnedApps] = useState<PinnedApp[]>([]);
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
  // UI language, from settings ("en" default). The whole island —
  // strings and reading direction — follows it.
  const [language, setLanguage] = useState<Language>("en");

  useEffect(() => {
    applyLanguageToDocument(language);
  }, [language]);

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

  const refreshTaskbar = () => {
    invoke<TaskbarWindow[]>("get_taskbar_windows")
      .then(setTaskbarWindows)
      .catch(() => setTaskbarWindows([]));
    invoke<PinnedApp[]>("get_pinned_apps")
      .then(setPinnedApps)
      .catch(() => setPinnedApps([]));
  };

  const activateTaskbarWindow = (id: number) => {
    // After the click — accepted or not — re-read the list so the
    // active/minimized marks never disagree with the backend.
    invoke<boolean>("activate_taskbar_window", { id })
      .then(() => refreshTaskbar())
      .catch(() => refreshTaskbar());
  };

  const openStartMenu = () => {
    invoke<boolean>("open_start_menu").catch(() => {});
  };

  const refreshAll = () => {
    refreshStats();
    refreshMedia();
    refreshNotifications();
    refreshTaskbar();
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
        setLanguage(normalizeLanguage(loaded.language));
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
      setLanguage(normalizeLanguage(event.payload.language));
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
    // The compact bar shows live stats, media and taskbar icons, so
    // the poll runs whenever the island is visible — not only when
    // the panel is expanded. Hidden stays silent (click-through).
    if (visible === "hidden") {
      return;
    }
    refreshAll();
    const id = window.setInterval(
      () => {
        refreshStats();
        refreshMedia();
        refreshNotifications();
        refreshTaskbar();
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
  const clockParts = clockDateParts(now, displaySettings.dateFormat);
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
        label: notificationKindLabel(item.kind, language),
        items: [item],
      });
    }
  }
  const tabs: { id: IslandTab; label: string }[] = [
    { id: "media", label: t(language, "tabMedia") },
    { id: "stats", label: t(language, "tabStats") },
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
            title={
              visible === "expanded"
                ? t(language, "collapse")
                : t(language, "expand")
            }
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
          <span className="island-divider" aria-hidden="true" />
          <div
            className="taskbar-zone"
            role="group"
            aria-label={t(language, "taskbarLabel")}
          >
            <button
              type="button"
              className="taskbar-start"
              onClick={openStartMenu}
              title={t(language, "startMenu")}
              aria-label={t(language, "startMenu")}
            >
              ⊞
            </button>
            {pinnedApps.map((app) => (
              <span
                key={app.target}
                className="taskbar-app pinned"
                title={app.name}
              >
                <RgbaIcon
                  rgbaBase64={app.icon_rgba_base64}
                  size={app.icon_size}
                  title={app.name}
                />
              </span>
            ))}
            {taskbarWindows.map((win) => (
              <button
                key={win.id}
                type="button"
                className={`taskbar-app${win.active ? " active" : ""}${
                  win.minimized ? " minimized" : ""
                }`}
                onClick={() => activateTaskbarWindow(win.id)}
                title={win.title}
                aria-label={win.title}
              >
                <RgbaIcon
                  rgbaBase64={win.icon_rgba_base64}
                  size={win.icon_size}
                  title={win.title}
                />
              </button>
            ))}
          </div>
          <span className="island-divider" aria-hidden="true" />
          <div className="compact-stats">
            <span
              className="compact-stat"
              title={
                stats
                  ? tf(language, "cpuUsageValue", {
                      v: stats.cpu_percent.toFixed(1),
                    })
                  : t(language, "cpuUsageEmpty")
              }
            >
              <span className="compact-stat-label">CPU</span>
              {stats
                ? formatPercent(
                    stats.cpu_percent,
                    displaySettings.percentStyle,
                  )
                : "—"}
            </span>
            <span
              className="compact-stat"
              title={
                stats
                  ? tf(language, "ramUsageValue", {
                      v: stats.memory_percent.toFixed(1),
                    })
                  : t(language, "ramUsageEmpty")
              }
            >
              <span className="compact-stat-label">RAM</span>
              {stats
                ? formatPercent(
                    stats.memory_percent,
                    displaySettings.percentStyle,
                  )
                : "—"}
            </span>
          </div>
          {media && (
            <>
              <span className="island-divider" aria-hidden="true" />
              <div className="compact-media">
                {media.cover_art && (
                  <img
                    className="compact-media-cover"
                    src={media.cover_art}
                    alt=""
                  />
                )}
                <button
                  type="button"
                  className="compact-media-btn"
                  onClick={() =>
                    sendMediaCommand(
                      media.playing ? "media_pause" : "media_play",
                    )
                  }
                  title={
                    media.playing
                      ? t(language, "mediaPause")
                      : t(language, "mediaPlay")
                  }
                  aria-label={
                    media.playing
                      ? t(language, "mediaPause")
                      : t(language, "mediaPlay")
                  }
                >
                  {media.playing ? "⏸" : "▶"}
                </button>
              </div>
            </>
          )}
          <span className="island-divider" aria-hidden="true" />
          <button
            type="button"
            className="island-clock"
            onClick={toggleExpand}
            title={
              visible === "expanded"
                ? t(language, "collapse")
                : t(language, "expand")
            }
            aria-label={
              visible === "expanded"
                ? t(language, "collapse")
                : t(language, "expand")
            }
          >
            <div className="island-clock-time">
              {clockTime(
                now,
                displaySettings.clock24h,
                displaySettings.showSeconds,
              )}
            </div>
            {visible === "expanded" && (
              <div className="island-clock-date">
                <span dir="ltr">{clockParts.gregorian}</span>
                {" · "}
                <span dir="rtl">{clockParts.jalali}</span>
              </div>
            )}
          </button>
          <button
            type="button"
            className="bell-btn"
            onClick={toggleNotifications}
            title={t(language, "notifications")}
            aria-label={t(language, "notifications")}
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
            title={t(language, "openSettings")}
            aria-label={t(language, "openSettings")}
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
                  ? t(language, "keepOpenOn")
                  : t(language, "keepOpenOff")
              }
              aria-pressed={keepOpen}
            >
              {keepOpen ? t(language, "pinned") : t(language, "keepOpen")}
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
                aria-label={t(language, "notifications")}
              >
                <div className="notifications-header">
                  <span className="notifications-title">
                    {t(language, "notifications")}
                  </span>
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
                      {density === "compact"
                        ? t(language, "densityComfortable")
                        : t(language, "densityCompact")}
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={markAllNotificationsRead}
                    >
                      {t(language, "markAllRead")}
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={clearNotifications}
                    >
                      {t(language, "clearAll")}
                    </button>
                  </span>
                </div>
                {notifications.length === 0 ? (
                  <div className="notifications-empty">
                    {t(language, "noNotifications")}
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
                            aria-label={tf(language, "clearKind", {
                              kind: group.label,
                            })}
                          >
                            {t(language, "clearGroup")}
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
                                {relativeTime(item.timestamp, now, language)}
                              </span>
                              <span className="notification-clock">
                                {notificationTime(item.timestamp)}
                              </span>
                            </span>
                            <button
                              type="button"
                              className="notification-dismiss"
                              onClick={() => dismissNotification(item.id)}
                              aria-label={t(language, "dismissNotification")}
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
              aria-label={t(language, "islandPanels")}
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
                      alt={media.album || t(language, "albumCover")}
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
                        : t(language, "noMedia")
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
                      {t(language, "mediaPrev")}
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
                      {media?.playing
                        ? t(language, "mediaPause")
                        : t(language, "mediaPlay")}
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={() => sendMediaCommand("media_next")}
                    >
                      {t(language, "mediaNext")}
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={refreshMedia}
                    >
                      {t(language, "mediaRefresh")}
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
                        ? tf(language, "cpuUsageValue", {
                            v: stats.cpu_percent.toFixed(1),
                          })
                        : t(language, "cpuUsageEmpty")
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
                        ? tf(language, "ramUsageValue", {
                            v: stats.memory_percent.toFixed(1),
                          })
                        : t(language, "ramUsageEmpty")
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
                        ? tf(language, "gpuUsageValue", {
                            v: stats.gpu_percent.toFixed(1),
                          })
                        : t(language, "gpuUsageEmpty")
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
                        ? tf(language, "uploadValue", {
                            v: stats.net_up_kbps.toFixed(2),
                          })
                        : t(language, "uploadEmpty")
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
                        ? tf(language, "downloadValue", {
                            v: stats.net_down_kbps.toFixed(2),
                          })
                        : t(language, "downloadEmpty")
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
                    {t(language, "taskbarLabel")}:{" "}
                    {taskbar
                      ? t(
                          language,
                          taskbar.hidden ? "taskbarHidden" : "taskbarShown",
                        )
                      : "—"}
                  </span>
                  <span>
                    {t(language, "watchdogLabel")}:{" "}
                    {watchdog
                      ? t(language, watchdog.healthy ? "healthy" : "unhealthy")
                      : "—"}
                  </span>
                </div>
                <div className="panel-line">
                  <span>
                    {t(language, "explorerLabel")}:{" "}
                    {watchdog
                      ? t(
                          language,
                          watchdog.explorer_ok ? "explorerOk" : "explorerDown",
                        )
                      : "—"}
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
  // UI language, derived from the settings object (backend `language`
  // field, "en" default). The whole window — strings and reading
  // direction — follows it.
  const language = normalizeLanguage(settings?.language);

  useEffect(() => {
    applyLanguageToDocument(language);
  }, [language]);

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

  const cycleLanguage = () => {
    // The switch persists through set_settings, so settings-changed
    // carries it to the island at once (rule 18).
    if (!settings) {
      return;
    }
    saveSettings({
      ...settings,
      language: language === "en" ? "fa" : "en",
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
    // unit, percent with %, English.
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
        language: "en",
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
      <h1 className="settings-title">{t(language, "settingsTitle")}</h1>
      <h2 className="settings-section-title">
        {t(language, "sectionLanguage")}
      </h2>
      <p className="settings-section-desc">
        {t(language, "languageDesc")}
      </p>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "sectionLanguage")}</span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={cycleLanguage}
          disabled={!settings}
        >
          {settings
            ? t(
                language,
                language === "fa" ? "languagePersian" : "languageEnglish",
              )
            : "—"}
        </button>
      </div>
      <h2 className="settings-section-title">
        {t(language, "sectionAppearance")}
      </h2>
      <p className="settings-section-desc">
        {t(language, "appearanceDesc")}
      </p>
      <div
        className="theme-cards"
        role="group"
        aria-label={t(language, "themeGroupLabel")}
      >
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
              {THEME_DESCRIPTIONS[language][candidate.id]}
            </span>
          </button>
        ))}
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "autoHideTaskbar")}</span>
          <span className="settings-row-desc">
            {t(language, "autoHideTaskbarDesc")}
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => toggleSetting("auto_hide_taskbar")}
        >
          {settings ? onOff(settings.auto_hide_taskbar, language) : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "alwaysOnTop")}</span>
          <span className="settings-row-desc">
            {t(language, "alwaysOnTopDesc")}
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => toggleSetting("always_on_top")}
        >
          {settings ? onOff(settings.always_on_top, language) : "—"}
        </button>
      </div>
      <h2 className="settings-section-title">
        {t(language, "sectionIslandStyle")}
      </h2>
      <p className="settings-section-desc">
        {t(language, "islandStyleDesc")}
      </p>
      <div className="settings-slider-row">
        <div className="settings-row-text">
          <span>{t(language, "islandBlur")}</span>
          <span className="settings-row-desc">
            {settings?.island_blur_px != null
              ? tf(language, "overridePx", { n: settings.island_blur_px })
              : tf(language, "themeDefaultPx", { n: themeBlurPx() })}
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
            aria-label={t(language, "blurAria")}
            onChange={(event) =>
              setStyleField(
                "island_blur_px",
                clampStylePx(Number(event.target.value)),
              )
            }
          />
          <span className="settings-slider-value">
            {settings?.island_blur_px != null
              ? tf(language, "valuePx", { n: settings.island_blur_px })
              : t(language, "defaultLabel")}
          </span>
          <button
            type="button"
            className="mini-btn"
            disabled={!settings || settings?.island_blur_px == null}
            onClick={() => setStyleField("island_blur_px", null)}
          >
            {t(language, "defaultLabel")}
          </button>
        </div>
      </div>
      <div className="settings-slider-row">
        <div className="settings-row-text">
          <span>{t(language, "cornerRadius")}</span>
          <span className="settings-row-desc">
            {settings?.island_radius_px != null
              ? tf(language, "overridePx", { n: settings.island_radius_px })
              : tf(language, "themeDefaultPx", {
                  n: ISLAND_RADIUS_DEFAULT_PX,
                })}
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
            aria-label={t(language, "radiusAria")}
            onChange={(event) =>
              setStyleField(
                "island_radius_px",
                clampStylePx(Number(event.target.value)),
              )
            }
          />
          <span className="settings-slider-value">
            {settings?.island_radius_px != null
              ? tf(language, "valuePx", { n: settings.island_radius_px })
              : t(language, "defaultLabel")}
          </span>
          <button
            type="button"
            className="mini-btn"
            disabled={!settings || settings?.island_radius_px == null}
            onClick={() => setStyleField("island_radius_px", null)}
          >
            {t(language, "defaultLabel")}
          </button>
        </div>
      </div>
      <h2 className="settings-section-title">{t(language, "sectionClock")}</h2>
      <p className="settings-section-desc">{t(language, "clockDesc")}</p>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "clock24h")}</span>
          <span className="settings-row-desc">
            {t(language, "clock24hDesc")}
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => toggleSetting("clock_24h")}
        >
          {settings ? onOff(settings.clock_24h, language) : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "showSeconds")}</span>
          <span className="settings-row-desc">
            {t(language, "showSecondsDesc")}
          </span>
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => toggleSetting("show_seconds")}
        >
          {settings ? onOff(settings.show_seconds, language) : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "dateFormat")}</span>
          <span className="settings-row-desc">
            {t(language, "dateFormatDesc")}
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
              ? t(language, "dateFull")
              : t(language, "dateShort")
            : "—"}
        </button>
      </div>
      <h2 className="settings-section-title">
        {t(language, "sectionPerformance")}
      </h2>
      <p className="settings-section-desc">
        {t(language, "performanceDesc")}
      </p>
      <div className="settings-slider-row">
        <div className="settings-row-text">
          <span>{t(language, "refreshInterval")}</span>
          <span className="settings-row-desc">
            {t(language, "refreshIntervalDesc")}
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
            aria-label={t(language, "refreshIntervalAria")}
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
            {settings
              ? tf(language, "valueSecs", {
                  n: clampPollSecs(settings.poll_interval_secs),
                })
              : "—"}
          </span>
        </div>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "speedUnit")}</span>
          <span className="settings-row-desc">
            {t(language, "speedUnitDesc")}
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
              ? t(language, "speedAuto")
              : settings.stats_speed_unit === "kb"
                ? "KB/s"
                : "MB/s"
            : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "percentStyle")}</span>
          <span className="settings-row-desc">
            {t(language, "percentStyleDesc")}
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
              ? t(language, "percentWith")
              : t(language, "percentWithout")
            : "—"}
        </button>
      </div>
      <div className="settings-row">
        <span>
          {updateInfo
            ? updateInfo.update_available
              ? t(language, "updateAvailable")
              : t(language, "noUpdateAvailable")
            : "—"}
        </span>
        <button type="button" className="mini-btn" onClick={checkForUpdates}>
          {t(language, "checkForUpdates")}
        </button>
      </div>
      <button
        type="button"
        className="mini-btn settings-reset"
        onClick={resetToDefaults}
      >
        {t(language, "resetToDefaults")}
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
