import { check as checkPluginUpdate } from "@tauri-apps/plugin-updater";
import { latestReleaseNote } from "./releaseNotes";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import emblem from "./assets/emblem.svg";
import wordmark from "./assets/wordmark.png";
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
import {
  nextVisible,
  shouldArmAutoClose,
  type IslandGuards,
  type IslandVisible,
} from "./islandStateMachine";

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
  /** Island hotkey, e.g. "Alt+Space"; changed with set_island_hotkey. */
  island_hotkey: string;
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
  online_translation?: boolean;
  translate_provider?: string;
  translate_source_lang?: string;
  translate_target_lang?: string;
  whisper_model?: string;
  /** Custom CSS injected inside the floating subtitle overlay window. */
  subtitle_custom_css?: string;
  /** Saved named custom CSS presets for the subtitle overlay. */
  subtitle_css_presets?: { name: string; css: string }[];
  /** Currently selected subtitle CSS preset name. */
  active_subtitle_css_preset?: string;
  /** Percent style: "with" (24%) or "without" (24). */
  stats_percent_style: string;
  /** UI language: "en" (default) or "fa". */
  language: string;
  /** Dynamic Island customization toggles (default true). */
  island_show_start?: boolean;
  island_show_apps?: boolean;
  island_show_stats?: boolean;
  island_show_gpu?: boolean;
  island_show_media?: boolean;
  island_show_clock?: boolean;
  island_show_notifications?: boolean;
  island_show_engine_local?: boolean;
  island_show_engine_online?: boolean;
  island_show_engine_dubbing?: boolean;
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

/** One open window for the compact bar's taskbar zone. Icons are
 * raw RGBA pixels as base64 (32x32), not PNG data-URLs. */
interface TaskbarWindow {
  id: number;
  title: string;
  minimized: boolean;
  active: boolean;
  icon_size: number;
  icon_rgba_base64: string;
  in_tray?: boolean;
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
  available_version?: string | null;
  notes?: string | null;
}

interface UpdateProgressPayload {
  downloaded: number;
  total: number | null;
}

type IslandTab = "apps" | "media" | "stats" | "engines" | "notifications" | "quick";

const STORAGE_KEY = "nexora-theme";
const SIZE_STORAGE_KEY = "nexora-island-size";
const GLOW_STORAGE_KEY = "nexora-island-glow";

function readIslandScale(): number {
  try {
    const raw = window.localStorage.getItem(SIZE_STORAGE_KEY);
    const val = raw ? Number(raw) : 100;
    if (Number.isFinite(val)) return Math.min(115, Math.max(75, Math.round(val)));
  } catch {
    // ignore
  }
  return 100;
}

function readIslandGlow(): number {
  try {
    const raw = window.localStorage.getItem(GLOW_STORAGE_KEY);
    const val = raw ? Number(raw) : 45;
    if (Number.isFinite(val)) return Math.min(100, Math.max(0, Math.round(val)));
  } catch {
    // ignore
  }
  return 45;
}

function applyVisualCssVars(scalePct: number, glowPct: number) {
  document.documentElement.style.setProperty("--island-scale", String(scalePct / 100));
  document.documentElement.style.setProperty("--island-glow", String(glowPct / 100));
}

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
function SvgStart() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
      <rect x="1.5" y="1.5" width="5.5" height="5.5" rx="1.2" />
      <rect x="9" y="1.5" width="5.5" height="5.5" rx="1.2" />
      <rect x="1.5" y="9" width="5.5" height="5.5" rx="1.2" />
      <rect x="9" y="9" width="5.5" height="5.5" rx="1.2" />
    </svg>
  );
}

function SvgPlay({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M8 5.14v13.72a1 1 0 0 0 1.52.85l11.02-6.86a1 1 0 0 0 0-1.7L9.52 4.29A1 1 0 0 0 8 5.14z" />
    </svg>
  );
}

function SvgPause({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <rect x="6" y="4.5" width="4.5" height="15" rx="1.4" />
      <rect x="13.5" y="4.5" width="4.5" height="15" rx="1.4" />
    </svg>
  );
}

function SvgStop({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <rect x="5" y="5" width="14" height="14" rx="2.5" />
    </svg>
  );
}

function SvgPrev({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <rect x="5" y="5" width="2.5" height="14" rx="1" />
      <path d="M18.2 5.4a1 1 0 0 1 1.3.95v11.3a1 1 0 0 1-1.56.83l-8.4-5.65a1 1 0 0 1 0-1.66l8.4-5.65c.08-.05.17-.09.26-.12z" />
    </svg>
  );
}

function SvgNext({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <rect x="16.5" y="5" width="2.5" height="14" rx="1" />
      <path d="M5.8 5.4a1 1 0 0 0-1.3.95v11.3a1 1 0 0 0 1.56.83l8.4-5.65a1 1 0 0 0 0-1.66l-8.4-5.65a1 1 0 0 0-.26-.12z" />
    </svg>
  );
}

function SvgRefresh({ size = 14 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.1"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 12a9 9 0 1 1-2.64-6.36" />
      <polyline points="21 3 21 9 15 9" />
    </svg>
  );
}

function SvgBell({ size = 16 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.9"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9" />
      <path d="M13.73 21a2 2 0 0 1-3.46 0" />
    </svg>
  );
}

function SvgGear({ size = 16 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.9"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3.2" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function SvgSubtitle({ size = 15 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.9"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="2.5" y="4.5" width="19" height="15" rx="3" />
      <path d="M6.5 13h4" />
      <path d="M13 13h4.5" />
      <path d="M6.5 16h7" />
    </svg>
  );
}

function SvgCpu({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <rect x="9.5" y="9.5" width="5" height="5" rx="0.8" />
      <path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" />
    </svg>
  );
}

function SvgRam({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="2" y="6" width="20" height="10" rx="2" />
      <path d="M6 10v2M10 10v2M14 10v2M18 10v2" />
      <path d="M6 16v3M10 16v3M14 16v3M18 16v3" />
    </svg>
  );
}

function SvgGpu({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="2" y="5" width="20" height="12" rx="2" />
      <circle cx="8.5" cy="11" r="2.7" />
      <circle cx="16" cy="11" r="2.7" />
      <path d="M5 17v3M9 17v2M13 17v2" />
    </svg>
  );
}

function SvgGlobe({ size = 15 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" />
    </svg>
  );
}

function SvgDubbing({ size = 15 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 10a7 7 0 0 0 14 0M12 17v4M8 21h8" />
    </svg>
  );
}

function SvgChevronLeft({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M15 18l-6-6 6-6" />
    </svg>
  );
}

function SvgChevronRight({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 18l6-6-6-6" />
    </svg>
  );
}

interface AudioAppTarget {
  pid: number;
  title: string;
  exe_name: string;
}

interface TranslateEngineStatus {
  installed: boolean;
  downloading: boolean;
  running: boolean;
  model_size_mb: number;
  source_lang: string;
  target_lang: string;
  capture_mode: string;
  online_translation?: boolean;
  translate_provider?: string;
  whisper_model?: string;
  local_model_installed?: boolean;
  target_pid: number | null;
  target_app_name: string | null;
  model_path: string;
  last_error: string | null;
}

interface SubtitlePayload {
  original: string;
  translated: string;
  target_lang: string;
  active: boolean;
  translation_status?: "ok" | "pending" | "failed" | "offline" | string;
}

interface DubbingStatus {
  piper_installed: boolean;
  voice_installed: boolean;
  installed_voices: string[];
  ready: boolean;
  downloading: boolean;
  running: boolean;
  voice_id: string;
  capture_mode: string;
  target_pid: number | null;
  target_app_name: string | null;
  duck_percent: number;
  speech_rate_pct: number;
  auto_gender_voice: boolean;
  female_voice_id: string;
  last_detected_gender: string;
  male_clone_f0_hz: number | null;
  female_clone_f0_hz: number | null;
  last_spoken_fa: string;
  last_error: string | null;
}

interface ApiKeyStatus {
  google_cloud: boolean;
  gemini: boolean;
}

interface TestProviderResult {
  ok: boolean;
  translated: string | null;
  latency_ms: number;
  error: string | null;
}

interface DownloadProgressPayload {
  percent: number;
  stage: string;
}

const RgbaIcon = memo(function RgbaIcon({
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
});

function IslandApp() {
  const [state, setState] = useState<IslandState>({
    expanded: false,
    mode: "compact",
    title: "Nexora Island",
    // Backend constants (island.rs) until get_island_state answers.
    width: 880,
    height: 440,
    content_x: 40,
    content_y: 40,
    content_width: 800,
    content_height: 360,
    compact_height: 64,
    handle_width: 120,
    handle_height: 4,
  });
  const [theme, setTheme] = useState<ThemeId>(() => themeOrDefault(readSavedTheme()));
  useEffect(() => {
    const syncVisuals = () => {
      applyVisualCssVars(readIslandScale(), readIslandGlow());
    };
    syncVisuals();
    const onStorage = (ev: StorageEvent) => {
      if (ev.key === SIZE_STORAGE_KEY || ev.key === GLOW_STORAGE_KEY) {
        syncVisuals();
      }
    };
    window.addEventListener("storage", onStorage);
    const pollVisual = window.setInterval(syncVisuals, 1500);
    return () => {
      window.removeEventListener("storage", onStorage);
      window.clearInterval(pollVisual);
    };
  }, []);
  const [tab, setTab] = useState<IslandTab>("media");
  // First-run onboarding (design artifact): three pages shown in the
  // expanded panel instead of the tabs, until the user finishes page
  // 3. Completion lives in localStorage only — no settings field.
  const [onboardingPage, setOnboardingPage] = useState<number | null>(null);
  // Ref mirror for the once-registered listeners (Esc, hot zone):
  // they only ever see the first render's closure.
  const onboardingRef = useRef<number | null>(null);
  // Full settings object, for the Quick settings tab: every change
  // goes through set_settings (rule 18), never local storage.
  const [settings, setSettings] = useState<NexoraSettings | null>(null);
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [now, setNow] = useState(() => new Date());
  // Island visibility state machine (design artifact): the window
  // never resizes — the pill animates inside the fixed canvas. The
  // transition rules live in islandStateMachine.ts (tested); the
  // handlers below only ask it for the next state.
  // Delay after the pointer leaves the island before it steps one
  // level down (expanded -> compact -> hidden).
  const AUTO_CLOSE_DELAY_MS = 5000;
  // Hidden-state handle hit area, in logical pixels (DEVagent spec):
  // the visible strip stays 120x4 (backend handle_width/height),
  // the clickable area around it is ~160x12 at the top edge.
  const HANDLE_HIT_WIDTH = 160;
  const HANDLE_HIT_HEIGHT = 12;
  // Launch hint: ~6 s in COMPACT after every launch, with a hint
  // line; then it collapses (never during onboarding or hover).
  const LAUNCH_HINT_MS = 6000;
  const LAUNCH_HINT_HEIGHT = 28;
  const [visible, setVisibleState] = useState<IslandVisible>("hidden");
  // Stage height duration: open ~320 ms, close ~220 ms (design
  // artifact). Set per transition; the CSS default is the close one.
  const [stageMs, setStageMs] = useState(220);
  // Easing follows the motion sheet: open is the spring
  // cubic-bezier(.34,1.4,.64,1), close is cubic-bezier(.4,0,1,1).
  const [stageEasing, setStageEasing] = useState(
    "cubic-bezier(0.34, 1.4, 0.64, 1)",
  );
  const [keepOpen, setKeepOpen] = useState(false);
  const visibleRef = useRef<IslandVisible>("hidden");
  const keepOpenRef = useRef(false);
  const hoveringRef = useRef(false);
  const lastInteractionAtRef = useRef(Date.now());
  const autoCloseTimer = useRef<number | undefined>(undefined);
  // Launch hint (item 3): true while the ~6 s COMPACT intro runs.
  const [launchHint, setLaunchHint] = useState(false);
  const launchHintRef = useRef(false);
  const launchHintTimer = useRef<number | undefined>(undefined);
  // "Island shown" signal (item 4): true once the island has been
  // shown at least once (compact or expanded) in this launch.
  const islandShownRef = useRef(false);
  // Taskbar content rendered at least once (both backend lists
  // fetched successfully); gates the "island shown" signal.
  const [taskbarContentLoaded, setTaskbarContentLoaded] = useState(false);
  const taskbarHideSignaledRef = useRef(false);

  const clearAutoCloseTimer = () => {
    if (autoCloseTimer.current !== undefined) {
      window.clearTimeout(autoCloseTimer.current);
      autoCloseTimer.current = undefined;
    }
  };

  // Guards for the state machine, read from refs (the listeners only
  // ever see the first render's closure).
  const machineGuards = (): IslandGuards => ({
    onboardingOpen: onboardingRef.current !== null,
    pinned: keepOpenRef.current,
    hovering: hoveringRef.current,
    launchHintActive: launchHintRef.current,
  });

  // Diagnostic visibility log (item 5, TAgent3's command): every
  // visibility transition with its reason. Silent until the command
  // lands — the invoke just fails and is swallowed.
  const logVisibilityTransition = (
    from: IslandVisible,
    to: IslandVisible,
    reason: string,
  ) => {
    invoke("log_visibility_transition", { from, to, reason }).catch(() => {});
  };

  const showIsland = (next: IslandVisible, reason: string) => {
    const from = visibleRef.current;
    // A no-op transition changes nothing and logs nothing.
    if (next === from) {
      return;
    }
    const opening =
      (from === "hidden" && next === "compact") ||
      (from === "compact" && next === "expanded");
    setStageMs(opening ? 320 : 220);
    setStageEasing(
      opening
        ? "cubic-bezier(0.34, 1.4, 0.64, 1)"
        : "cubic-bezier(0.4, 0, 1, 1)",
    );
    visibleRef.current = next;
    setVisibleState(next);
    if (next === "compact" || next === "expanded") {
      islandShownRef.current = true;
    }
    logVisibilityTransition(from, next, reason);
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
    if (next === "hidden") {
      clearAutoCloseTimer();
    } else if (shouldArmAutoClose(machineGuards())) {
      armAutoClose();
    }
  };

  const armAutoClose = () => {
    clearAutoCloseTimer();
    // The guards live in the state machine (one source of truth):
    // onboarding, pin, hover, and the launch hint all block arming,
    // so the island never vanishes mid-read.
    if (!shouldArmAutoClose(machineGuards())) {
      return;
    }
    autoCloseTimer.current = window.setTimeout(() => {
      const current = visibleRef.current;
      if (current === "hidden") {
        return;
      }
      const next = nextVisible(
        current,
        "auto-close-timeout",
        machineGuards(),
      );
      showIsland(next, "auto-close-timeout");
    }, AUTO_CLOSE_DELAY_MS);
  };

  const [notifications, setNotifications] = useState<NotificationItem[]>([]);
  const [liveSubActive, setLiveSubActive] = useState(false);
  const [islandTranslateStatus, setIslandTranslateStatus] =
    useState<TranslateEngineStatus | null>(null);
  const [, setTranslateInstalled] = useState(false);

  useEffect(() => {
    invoke<TranslateEngineStatus>("get_translate_status")
      .then((st) => {
        setIslandTranslateStatus(st);
        setTranslateInstalled(st.installed);
        setLiveSubActive(st.running);
      })
      .catch(() => {});
    let unlisten: (() => void) | undefined;
    listen<TranslateEngineStatus>("translate-status-changed", (ev) => {
      setIslandTranslateStatus(ev.payload);
      setTranslateInstalled(ev.payload.installed);
      setLiveSubActive(ev.payload.running);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, []);

  const [islandDubbingStatus, setIslandDubbingStatus] =
    useState<DubbingStatus | null>(null);

  useEffect(() => {
    invoke<DubbingStatus>("get_dubbing_status")
      .then(setIslandDubbingStatus)
      .catch(() => {});
    let unlisten: (() => void) | undefined;
    listen<DubbingStatus>("dubbing-status-changed", (ev) => {
      setIslandDubbingStatus(ev.payload);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    const onBlur = () => {
      hoveringRef.current = false;
      if (visibleRef.current !== "hidden") {
        armAutoClose();
      }
    };
    const onDocLeave = () => {
      hoveringRef.current = false;
      if (visibleRef.current !== "hidden") {
        armAutoClose();
      }
    };
    const idleWatchdog = window.setInterval(() => {
      if (visibleRef.current === "hidden") return;
      if (!shouldArmAutoClose(machineGuards())) return;
      if (
        autoCloseTimer.current === undefined &&
        Date.now() - lastInteractionAtRef.current >= AUTO_CLOSE_DELAY_MS
      ) {
        const next = nextVisible(
          visibleRef.current,
          "auto-close-timeout",
          machineGuards(),
        );
        showIsland(next, "auto-close-timeout");
      }
    }, 1500);
    window.addEventListener("blur", onBlur);
    document.addEventListener("mouseleave", onDocLeave);
    return () => {
      unlisten?.();
      window.clearInterval(idleWatchdog);
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("mouseleave", onDocLeave);
    };
  }, []);

  const [activeTranslateMode, setActiveTranslateMode] = useState<"local" | "online" | null>(null);
  const taskbarTrackRef = useRef<HTMLDivElement | null>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);

  const updateCarouselArrows = useCallback(() => {
    const el = taskbarTrackRef.current;
    if (!el) {
      setCanScrollLeft(false);
      setCanScrollRight(false);
      return;
    }
    setCanScrollLeft(el.scrollLeft > 2);
    setCanScrollRight(el.scrollLeft + el.clientWidth < el.scrollWidth - 2);
  }, []);

  const scrollAppsCarousel = (direction: -1 | 1) => {
    const el = taskbarTrackRef.current;
    if (!el) return;
    el.scrollBy({ left: direction * 136, behavior: "smooth" });
  };

  const toggleEngineTranslate = (mode: "local" | "online") => {
    if (liveSubActive && activeTranslateMode === mode) {
      invoke<TranslateEngineStatus>("stop_live_translate")
        .then((st) => {
          setLiveSubActive(st.running);
          if (!st.running) setActiveTranslateMode(null);
        })
        .catch(() => {
          setLiveSubActive(false);
          setActiveTranslateMode(null);
        });
      return;
    }
    invoke<TranslateEngineStatus>("get_translate_status")
      .then((st) => {
        if (!st.installed) {
          openSettings();
          return;
        }
        const savedMode = window.localStorage.getItem("nexora-translate-capture-mode") || st.capture_mode || "system";
        const savedPidRaw = window.localStorage.getItem("nexora-translate-target-pid");
        const savedPid = savedPidRaw ? Number(savedPidRaw) : st.target_pid;
        const savedAppName = window.localStorage.getItem("nexora-translate-target-app") || st.target_app_name;
        const provider =
          mode === "local"
            ? st.local_model_installed
              ? "local"
              : "none"
            : st.translate_provider === "google_cloud" || st.translate_provider === "gemini"
              ? st.translate_provider
              : "google_free";
        invoke<TranslateEngineStatus>("start_live_translate", {
          sourceLang: st.source_lang || "en",
          targetLang: st.target_lang || "fa",
          captureMode: savedMode,
          onlineTranslation: mode === "online",
          translateProvider: provider,
          targetPid: savedMode === "app" && savedPid ? savedPid : null,
          targetAppName: savedMode === "app" ? savedAppName : null,
        })
          .then((next) => {
            setLiveSubActive(next.running);
            setActiveTranslateMode(next.running ? mode : null);
          })
          .catch(() => {});
      })
      .catch(() => openSettings());
  };

  const toggleLiveDubbingQuick = () => {
    if (islandDubbingStatus?.running) {
      invoke<DubbingStatus>("stop_live_dubbing")
        .then(setIslandDubbingStatus)
        .catch(() => {});
      return;
    }
    invoke<DubbingStatus>("get_dubbing_status")
      .then((st) => {
        if (!st.ready) {
          openSettings();
          return;
        }
        const savedVoice = window.localStorage.getItem("nexora-dubbing-voice") || st.voice_id || "fa_IR-amir-medium";
        const savedDuck = Number(window.localStorage.getItem("nexora-dubbing-duck") || st.duck_percent || 80);
        const savedRate = Number(window.localStorage.getItem("nexora-dubbing-rate") || st.speech_rate_pct || 100);
        const savedAutoGender = window.localStorage.getItem("nexora-dubbing-auto-gender") === "1" || st.auto_gender_voice;
        const savedFemaleVoice = window.localStorage.getItem("nexora-dubbing-female-voice") || st.female_voice_id || "auto_female";
        const savedMode = window.localStorage.getItem("nexora-dubbing-capture-mode") || st.capture_mode || "system";
        const savedPidRaw = window.localStorage.getItem("nexora-dubbing-target-pid");
        const savedPid = savedPidRaw ? Number(savedPidRaw) : st.target_pid;
        const savedAppName = window.localStorage.getItem("nexora-dubbing-target-app") || st.target_app_name;
        invoke<DubbingStatus>("start_live_dubbing", {
          voiceId: savedVoice,
          duckPercent: savedDuck,
          speechRatePct: savedRate,
          autoGenderVoice: savedAutoGender,
          femaleVoiceId: savedFemaleVoice,
          captureMode: savedMode,
          targetPid: savedMode === "app" && savedPid ? savedPid : null,
          targetAppName: savedMode === "app" ? savedAppName : null,
        })
          .then(setIslandDubbingStatus)
          .catch(() => {});
      })
      .catch(() => openSettings());
  };
  const [seenIds, setSeenIds] = useState<string[]>([]);
  // Compact bar's taskbar zone: open windows (Taskbar + System Tray).
  const [taskbarWindows, setTaskbarWindows] = useState<TaskbarWindow[]>([]);
  useEffect(() => {
    const id = window.requestAnimationFrame(updateCarouselArrows);
    return () => window.cancelAnimationFrame(id);
  }, [taskbarWindows, visible, updateCarouselArrows]);
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
    | "media_stop"
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
    // Both lists fetched in the same round count as "content
    // rendered" for the island-shown signal (item 4). A failed fetch
    // clears its list but does not count.
    let windowsOk = false;
    let pinnedOk = false;
    const maybeLoaded = () => {
      if (windowsOk && pinnedOk) {
        setTaskbarContentLoaded(true);
      }
    };
    invoke<TaskbarWindow[]>("get_taskbar_windows")
      .then((windows) => {
        setTaskbarWindows(windows);
        windowsOk = true;
        maybeLoaded();
      })
      .catch(() => setTaskbarWindows([]));
    invoke<PinnedApp[]>("get_pinned_apps")
      .then((apps) => {
        setPinnedApps(apps);
        pinnedOk = true;
        maybeLoaded();
      })
      .catch(() => setPinnedApps([]));
  };

  const activateTaskbarWindow = (id: number) => {
    // After the click — accepted or not — re-read the list so the
    // active/minimized marks never disagree with the backend.
    invoke<boolean>("activate_taskbar_window", { id })
      .then(() => refreshTaskbar())
      .catch(() => refreshTaskbar());
  };

  const launchPinnedApp = (target: string) => {
    // After the launch — accepted or not — re-read the lists so the
    // newly opened window shows up in the taskbar zone.
    invoke<boolean>("launch_pinned_app", { target })
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
  };

  useEffect(() => {
    // Saved choice wins; the backend default is the fallback.
    const saved = readSavedTheme();
    invoke<NexoraSettings>("get_settings")
      .then((loaded) => {
        setSettings(loaded);
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
    window.setInterval(() => {
      invoke("island_heartbeat").catch(() => {});
    }, 5000);
    // Silent startup update check via official @tauri-apps/plugin-updater (ReChat flow)
    if (window.localStorage.getItem("nexora-auto-update") !== "0") {
      void (async () => {
        try {
          const update = await checkPluginUpdate();
          if (!update) return;
          const notes = update.body ? "\n\n" + update.body : "";
          const promptText = t(window.localStorage.getItem("nexora-language") === "fa" ? "fa" : "en", "updateConfirmPrompt").replace("{v}", update.version) + notes;
          if (!window.confirm(promptText)) return;
          try {
            window.localStorage.setItem("nexora-show-whats-new", "1");
          } catch {
            // ignore
          }
          await invoke("prepare_for_update_exit").catch(() => {});
          await update.downloadAndInstall();
        } catch {
          // silent on background startup check
        }
      })();
    }
    invoke<UpdateInfo>("check_updates")
      .then((info) => {
        try {
          const cur = info.current_version;
          const prev = window.localStorage.getItem("nexora-last-seen-version");
          if (prev && prev !== cur) {
            window.localStorage.setItem("nexora-show-whats-new", cur);
            window.localStorage.setItem("nexora-last-seen-version", cur);
            openSettings();
          } else if (!prev && cur) {
            window.localStorage.setItem("nexora-last-seen-version", cur);
          }
        } catch {
          // ignore
        }
      })
      .catch(() => {});
    // The Settings window persists a new theme with set_settings, which
    // emits settings-changed — apply it here at once, no restart.
    let unlisten: (() => void) | undefined;
    listen<NexoraSettings>("settings-changed", (event) => {
      setSettings(event.payload);
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
      // Hovering the top edge no longer auto-opens the island;
      // users open it explicitly via clicking the top handle or hotkey.
      if (event.payload.in_hot_zone && visibleRef.current !== "hidden") {
        lastInteractionAtRef.current = Date.now();
        clearAutoCloseTimer();
      } else if (
        !event.payload.in_hot_zone &&
        visibleRef.current !== "hidden" &&
        !hoveringRef.current
      ) {
        armAutoClose();
      }
    })
      .then((stop) => {
        unlistenHotZone = stop;
      })
      .catch(() => {});
    // Global hotkey (backend, Alt+Space): toggles the island through
    // the same state machine — hidden -> compact, compact / expanded
    // -> hidden. Unlike Esc it also closes a pinned island: it is the
    // explicit toggle. It never dismisses the onboarding mid-read
    // (same guard as Esc).
    let unlistenHotkey: (() => void) | undefined;
    listen("island-hotkey", () => {
      showIsland(
        nextVisible(visibleRef.current, "hotkey", machineGuards()),
        "hotkey",
      );
    })
      .then((stop) => {
        unlistenHotkey = stop;
      })
      .catch(() => {});
    // Esc steps one level down, unless the island is pinned open.
    // The guards live in the state machine (same guard as the
    // hotkey, plus the pin).
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      showIsland(
        nextVisible(visibleRef.current, "esc", machineGuards()),
        "esc",
      );
    };
    window.addEventListener("keydown", onKeyDown);
    // The island starts hidden: the fixed canvas lets clicks pass
    // through until the hot zone wakes it.
    invoke("set_island_click_through", { ignore: true }).catch(() => {});
    // First run: the onboarding has not been finished — open the
    // island expanded on the welcome page. This runs after the
    // click-through line above, so the island receives clicks.
    if (window.localStorage.getItem("nexora-onboarding-done") !== "1") {
      onboardingRef.current = 0;
      setOnboardingPage(0);
      showIsland("expanded", "onboarding-open");
    } else {
      // Every launch: present the island in COMPACT for ~6 s with a
      // hint line, then collapse — never during onboarding (above)
      // or while the cursor is on the island (guarded at fire time).
      // Asked of the machine like every other handler; the else
      // branch already guarantees a non-onboarding launch, so the
      // result is the same "compact".
      showIsland(
        nextVisible("hidden", "launch-start", machineGuards()),
        "launch-start",
      );
      launchHintRef.current = true;
      setLaunchHint(true);
      launchHintTimer.current = window.setTimeout(() => {
        launchHintRef.current = false;
        setLaunchHint(false);
        if (visibleRef.current !== "hidden" && !hoveringRef.current) {
          armAutoClose();
        }
      }, LAUNCH_HINT_MS);
    }
    return () => {
      clearAutoCloseTimer();
      if (launchHintTimer.current !== undefined) {
        window.clearTimeout(launchHintTimer.current);
        launchHintTimer.current = undefined;
      }
      window.removeEventListener("keydown", onKeyDown);
      unlisten?.();
      unlistenNotifications?.();
      unlistenHotZone?.();
      unlistenHotkey?.();
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
    const pollMs = clampPollSecs(displaySettings.pollIntervalSecs) * 1000;
    const id = window.setInterval(() => {
      if (document.hidden) return;
      refreshStats();
      refreshMedia();
      if (visibleRef.current === "expanded") {
        refreshNotifications();
      }
    }, pollMs);
    const appsId = window.setInterval(() => {
      if (document.hidden) return;
      refreshTaskbar();
    }, Math.max(4000, pollMs));
    const clockMs = displaySettings.showSeconds ? 1000 : 15000;
    const clockId = window.setInterval(() => {
      if (!document.hidden) setNow(new Date());
    }, clockMs);
    return () => {
      window.clearInterval(id);
      window.clearInterval(appsId);
      window.clearInterval(clockId);
    };
    // Refresh helpers only use stable setters and invoke.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, displaySettings.pollIntervalSecs]);

  // "Island shown" signal (item 4, TAgent1's command): once the
  // island has been shown at least once and the taskbar content has
  // rendered in the compact bar, tell Rust — exactly once per
  // launch. The backend decides (first call wins; hides the taskbar
  // only when auto_hide_taskbar is on) and logs the outcome to
  // island-anchor.log. Silent until TAgent1's branch
  // (fix/defer-taskbar-hide) merges.
  useEffect(() => {
    if (
      taskbarHideSignaledRef.current ||
      !islandShownRef.current ||
      !taskbarContentLoaded ||
      visible === "hidden"
    ) {
      return;
    }
    taskbarHideSignaledRef.current = true;
    // Taskbar hiding disabled: Nexora runs in floating companion mode.
  }, [visible, taskbarContentLoaded]);

  // Clicking the island itself toggles expand / collapse; the bell
  // and gear buttons are siblings, so their clicks do not toggle.
  // The toggle has no guards (see islandStateMachine.ts): it is the
  // deliberate collapse, and the only manual way to hide the island
  // while the onboarding is open.
  const toggleExpand = () => {
    showIsland(
      nextVisible(visible, "toggle-expand", machineGuards()),
      "toggle-expand",
    );
  };

  // Clicking the empty canvas around the pill steps one level down.
  // The onboarding guard lives in the state machine ("shell-click"):
  // empty-space clicks never dismiss the onboarding mid-read.
  const onShellClick = (event: React.MouseEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) {
      return;
    }
    showIsland(
      nextVisible(visibleRef.current, "shell-click", machineGuards()),
      "shell-click",
    );
  };

  const onShellMouseEnter = () => {
    hoveringRef.current = true;
    lastInteractionAtRef.current = Date.now();
    clearAutoCloseTimer();
  };

  const onShellMouseLeave = () => {
    hoveringRef.current = false;
    lastInteractionAtRef.current = Date.now();
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

  const saveSettings = (next: NexoraSettings) => {
    invoke<NexoraSettings>("set_settings", { settings: next })
      .then(setSettings)
      .catch(() => {});
  };

  const toggleQuickSetting = (
    key: "auto_hide_taskbar" | "always_on_top" | "clock_24h",
  ) => {
    if (!settings) {
      return;
    }
    saveSettings({ ...settings, [key]: !settings[key] });
    // The clock reads displaySettings, not the settings object —
    // flip it at once so the toggle does not wait for the round-trip.
    if (key === "clock_24h") {
      setDisplaySettings((prev) => ({
        ...prev,
        clock24h: !prev.clock24h,
      }));
    }
  };

  const cycleQuickTheme = () => {
    if (!settings) {
      return;
    }
    const currentIndex = THEMES.findIndex(
      (candidate) => candidate.id === theme,
    );
    const next = THEMES[(currentIndex + 1) % THEMES.length];
    applyTheme(next.id);
    saveSettings({ ...settings, theme: next.id });
  };

  // Onboarding navigation. The four theme cards on page 3 are the
  // design's picks, in its order; Neon Glass reads as preselected
  // because it is the default theme (theme state starts there).
  const ONBOARDING_THEMES: ThemeId[] = [
    "neon-glass",
    "obsidian",
    "aurora",
    "liquid-glass",
  ];

  const goOnboardingPage = (next: number) => {
    onboardingRef.current = next;
    setOnboardingPage(next);
  };

  const pickOnboardingTheme = (id: ThemeId) => {
    if (!settings) {
      return;
    }
    applyTheme(id);
    saveSettings({ ...settings, theme: id });
  };

  const finishOnboarding = () => {
    try {
      window.localStorage.setItem("nexora-onboarding-done", "1");
    } catch {
      // Storage blocked: the onboarding just shows once more.
    }
    onboardingRef.current = null;
    setOnboardingPage(null);
  };

  // Selecting a tab; opening Notifications also marks everything
  // seen and re-reads the list, like the bell used to do.
  const selectTab = (next: IslandTab) => {
    setTab(next);
    if (next === "notifications") {
      refreshNotifications();
      setSeenIds(notifications.map((item) => item.id));
    }
  };

  // The bell opens the Notifications tab (or back to Media when it
  // is already open); the center only renders expanded.
  const openNotificationsTab = () => {
    showIsland(
      nextVisible(visible, "bell", machineGuards()),
      "notifications-bell",
    );
    selectTab(tab === "notifications" ? "media" : "notifications");
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
  const hasDownloadedEngine = Boolean(
    islandTranslateStatus?.installed ||
      islandTranslateStatus?.local_model_installed ||
      islandDubbingStatus?.ready,
  );
  const tabs: { id: IslandTab; label: string }[] = [
    { id: "apps", label: t(language, "tabApps") },
    { id: "media", label: t(language, "tabMedia") },
    { id: "stats", label: t(language, "tabStats") },
    ...(hasDownloadedEngine ? [{ id: "engines" as IslandTab, label: t(language, "tabEngines") }] : []),
    { id: "notifications", label: t(language, "notifications") },
    { id: "quick", label: t(language, "tabQuick") },
  ];

  return (
    <main
      className="island-shell"
      onPointerDown={() => { if (!hoveringRef.current && visibleRef.current !== "hidden") armAutoClose(); }}
      onClick={onShellClick}
    >
      <div
        className={`island-stage ${visible}`}
        onMouseEnter={onShellMouseEnter}
        onMouseLeave={onShellMouseLeave}
        onMouseMove={() => { lastInteractionAtRef.current = Date.now(); }}
        style={{
          transitionDuration: `${stageMs}ms`,
          transitionTimingFunction: stageEasing,
          // Geometry comes from the backend (island.rs), one source of truth.
          left: Math.max(0, Math.round((state.width - Math.min(680, state.content_width)) / 2)),
          // Hidden: the handle sits exactly at y=0 (top edge) and
          // slides down to content_y when the island opens (item 1).
          top: visible === "hidden" ? 0 : state.content_y,
          width: Math.min(680, state.content_width),
          height:
            visible === "hidden"
              ? HANDLE_HIT_HEIGHT
              : visible === "compact"
                ? state.compact_height +
                  (launchHint ? LAUNCH_HINT_HEIGHT : 0)
                : state.content_height,
        }}
      >
        <button
          type="button"
          className="island-handle"
          aria-hidden={visible !== "hidden"}
          tabIndex={visible === "hidden" ? 0 : -1}
          aria-label={t(language, "expand")}
          onClick={() =>
            showIsland(
              nextVisible(
                visibleRef.current,
                "handle-click",
                machineGuards(),
              ),
              "handle-click",
            )
          }
          style={{
            width: HANDLE_HIT_WIDTH,
            height: HANDLE_HIT_HEIGHT,
          }}
        >
          <span
            className="island-handle-strip"
            aria-hidden="true"
            style={{
              width: state.handle_width,
              height: state.handle_height,
            }}
          />
        </button>
        <section
          className={`island-pill${visible === "expanded" ? " expanded" : ""}${
            visible === "compact" && launchHint ? " launch-hint" : ""
          }`}
        style={{
          transitionDuration: `${stageMs}ms`,
          transitionTimingFunction: stageEasing,
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
            title={`Nexora — ${
              visible === "expanded"
                ? t(language, "collapse")
                : t(language, "expand")
            }`}
          >
            <img
              className="island-emblem"
              src={emblem}
              alt="Nexora"
              width={28}
              height={28}
            />
          </div>
          {((settings?.island_show_start ?? true) ||
            (settings?.island_show_apps ?? true)) && (
            <>
              <span className="island-divider" aria-hidden="true" />
              <div
                className="taskbar-zone"
                role="group"
                aria-label={t(language, "taskbarLabel")}
              >
                {(settings?.island_show_start ?? true) && (
                  <button
                    type="button"
                    className="taskbar-start"
                    onClick={openStartMenu}
                    title={t(language, "startMenu")}
                    aria-label={t(language, "startMenu")}
                  >
                    <SvgStart />
                  </button>
                )}
                {(settings?.island_show_apps ?? true) && (
                  <div className="taskbar-carousel">
                    {(canScrollLeft || canScrollRight) && (
                      <button
                        type="button"
                        className="carousel-arrow"
                        disabled={!canScrollLeft}
                        onClick={() => scrollAppsCarousel(-1)}
                        title={t(language, "carouselPrev")}
                        aria-label={t(language, "carouselPrev")}
                      >
                        <SvgChevronLeft size={13} />
                      </button>
                    )}
                    <div
                      ref={taskbarTrackRef}
                      className="taskbar-track"
                      onScroll={updateCarouselArrows}
                      onWheel={(e) => {
                        const el = taskbarTrackRef.current;
                        if (!el || el.scrollWidth <= el.clientWidth) return;
                        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
                          e.preventDefault();
                          el.scrollLeft += e.deltaY;
                          updateCarouselArrows();
                        }
                      }}
                    >
                      {taskbarWindows.map((win) => (
                        <button
                          key={win.id}
                          type="button"
                          className={`taskbar-app${win.active ? " active" : ""}${
                            win.minimized ? " minimized" : ""
                          }${win.in_tray ? " in-tray" : ""}`}
                          onClick={() => activateTaskbarWindow(win.id)}
                          title={
                            win.in_tray
                              ? `${win.title} (${t(language, "trayAppBadge")})`
                              : win.title
                          }
                          aria-label={
                            win.in_tray
                              ? `${win.title} (${t(language, "trayAppBadge")})`
                              : win.title
                          }
                        >
                          <RgbaIcon
                            rgbaBase64={win.icon_rgba_base64}
                            size={win.icon_size}
                            title={win.title}
                          />
                        </button>
                      ))}
                    </div>
                    {(canScrollLeft || canScrollRight) && (
                      <button
                        type="button"
                        className="carousel-arrow"
                        disabled={!canScrollRight}
                        onClick={() => scrollAppsCarousel(1)}
                        title={t(language, "carouselNext")}
                        aria-label={t(language, "carouselNext")}
                      >
                        <SvgChevronRight size={13} />
                      </button>
                    )}
                  </div>
                )}
              </div>
            </>
          )}
          {(settings?.island_show_stats ?? true) && (
            <>
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
                  <span className="stat-icon">
                    <SvgCpu size={13} />
                  </span>
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
                  <span className="stat-icon">
                    <SvgRam size={13} />
                  </span>
                  {stats
                    ? formatPercent(
                        stats.memory_percent,
                        displaySettings.percentStyle,
                      )
                    : "—"}
                </span>
                {(settings?.island_show_gpu ?? true) && (
                  <span
                    className="compact-stat"
                    title={
                      stats?.gpu_percent != null
                        ? tf(language, "gpuUsageValue", {
                            v: stats.gpu_percent.toFixed(1),
                          })
                        : t(language, "gpuUsageEmpty")
                    }
                  >
                    <span className="stat-icon">
                      <SvgGpu size={13} />
                    </span>
                    {stats?.gpu_percent != null
                      ? formatPercent(
                          stats.gpu_percent,
                          displaySettings.percentStyle,
                        )
                      : "—"}
                  </span>
                )}
              </div>
            </>
          )}
          {media && (settings?.island_show_media ?? true) && (
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
                  {media.playing ? <SvgPause size={12} /> : <SvgPlay size={12} />}
                </button>
                <button
                  type="button"
                  className="compact-media-btn"
                  onClick={() => sendMediaCommand("media_stop")}
                  title={t(language, "mediaStop")}
                  aria-label={t(language, "mediaStop")}
                >
                  <SvgStop size={10} />
                </button>
              </div>
            </>
          )}
          {(settings?.island_show_clock ?? true) && (
            <>
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
            </>
          )}
          {(settings?.island_show_engine_local ?? true) && (
            <button
              type="button"
              className={`theme-icon-btn engine-quick-btn${
                liveSubActive && activeTranslateMode !== "online" ? " active" : ""
              }`}
              onClick={() => toggleEngineTranslate("local")}
              title={t(language, "engineLocalTooltip")}
              aria-label={t(language, "engineLocalTooltip")}
            >
              <SvgSubtitle size={15} />
            </button>
          )}
          {(settings?.island_show_engine_online ?? true) && (
            <button
              type="button"
              className={`theme-icon-btn engine-quick-btn${
                liveSubActive && activeTranslateMode === "online"
                  ? " active active-online"
                  : ""
              }`}
              onClick={() => toggleEngineTranslate("online")}
              title={t(language, "engineOnlineTooltip")}
              aria-label={t(language, "engineOnlineTooltip")}
            >
              <SvgGlobe size={15} />
            </button>
          )}
          {(settings?.island_show_engine_dubbing ?? true) && (
            <button
              type="button"
              className={`theme-icon-btn engine-quick-btn${
                islandDubbingStatus?.running ? " active active-dubbing" : ""
              }`}
              onClick={toggleLiveDubbingQuick}
              title={t(language, "engineDubbingTooltip")}
              aria-label={t(language, "engineDubbingTooltip")}
            >
              <SvgDubbing size={15} />
            </button>
          )}
          {(settings?.island_show_notifications ?? true) && (
            <button
              type="button"
              className="bell-btn theme-icon-btn"
              onClick={openNotificationsTab}
              title={t(language, "notifications")}
              aria-label={t(language, "notifications")}
              aria-expanded={visible === "expanded" && tab === "notifications"}
            >
              <SvgBell size={15} />
              {unreadCount > 0 && (
                <span className="bell-badge">
                  {unreadCount > 9 ? "9+" : unreadCount}
                </span>
              )}
            </button>
          )}
          <button
            type="button"
            className="gear-btn theme-icon-btn"
            onClick={openSettings}
            title={t(language, "openSettings")}
            aria-label={t(language, "openSettings")}
          >
            <SvgGear size={15} />
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
        {visible === "compact" && launchHint && (
          <div className="island-launch-hint" aria-live="polite">
            {tf(language, "launchHint", {
              hotkey: settings?.island_hotkey ?? "Alt+Space",
            })}
          </div>
        )}
        {visible === "expanded" && (
          <>
            {onboardingPage !== null ? (
              <div className="onboarding-panel">
                {onboardingPage === 0 && (
                  <>
                    <img
                      className="onboarding-emblem"
                      src={emblem}
                      alt="Nexora"
                      width={52}
                      height={52}
                    />
                    <h2 className="onboarding-title">
                      {t(language, "onboardingWelcomeTitle")}
                    </h2>
                    <p className="onboarding-text">
                      {t(language, "onboardingWelcomeText")}
                    </p>
                    <button
                      type="button"
                      className="mini-btn onboarding-btn"
                      onClick={() => goOnboardingPage(1)}
                    >
                      {t(language, "onboardingGetStarted")}
                    </button>
                  </>
                )}
                {onboardingPage === 1 && (
                  <>
                    <h2 className="onboarding-title">
                      {t(language, "onboardingTaskbarTitle")}
                    </h2>
                    <p className="onboarding-text">
                      {t(language, "onboardingTaskbarText")}
                    </p>
                    <div className="onboarding-callout">
                      {t(language, "onboardingTaskbarSafe")}
                    </div>
                    <button
                      type="button"
                      className="mini-btn onboarding-btn"
                      onClick={() => goOnboardingPage(2)}
                    >
                      {t(language, "onboardingContinue")}
                    </button>
                  </>
                )}
                {onboardingPage === 2 && (
                  <>
                    <h2 className="onboarding-title">
                      {t(language, "onboardingLookTitle")}
                    </h2>
                    <div className="theme-cards onboarding-themes">
                      {ONBOARDING_THEMES.map((candidate) => {
                        const definition = themeById(candidate);
                        return (
                          <button
                            key={candidate}
                            type="button"
                            className={`theme-card${
                              candidate === theme ? " selected" : ""
                            }`}
                            data-theme={candidate}
                            aria-label={definition.name}
                            aria-pressed={candidate === theme}
                            onClick={() => pickOnboardingTheme(candidate)}
                            disabled={!settings}
                          >
                            <span
                              className="theme-card-preview"
                              aria-hidden="true"
                            >
                              <span className="theme-card-dot" />
                              <span className="theme-card-bar" />
                              <span className="theme-card-bar short" />
                            </span>
                            <span className="theme-card-name">
                              {definition.name}
                              {candidate === theme ? " ✓" : ""}
                            </span>
                          </button>
                        );
                      })}
                    </div>
                    <p className="onboarding-text">
                      {t(language, "onboardingLookText")}
                    </p>
                    <button
                      type="button"
                      className="mini-btn onboarding-btn"
                      onClick={finishOnboarding}
                    >
                      {t(language, "onboardingStart")}
                    </button>
                  </>
                )}
                <div className="onboarding-dots" aria-hidden="true">
                  {[0, 1, 2].map((index) => (
                    <i
                      key={index}
                      className={`onboarding-dot${
                        onboardingPage === index ? " on" : ""
                      }`}
                    />
                  ))}
                </div>
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
                  onClick={() => selectTab(candidate.id)}
                >
                  {candidate.label}
                </button>
              ))}
            </div>
            {tab === "notifications" && (
              <div
                className={`notifications-panel${
                  density === "compact" ? " compact" : ""
                }`}
                role="tabpanel"
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
                          <div
                            key={item.id}
                            className={`notification-item${item.kind === "updates" ? " clickable" : ""}`}
                            onClick={
                              item.kind === "updates"
                                ? () => {
                                    try {
                                      window.localStorage.setItem("nexora-settings-nav", "updates");
                                    } catch {
                                      // ignore
                                    }
                                    openSettings();
                                  }
                                : undefined
                            }
                          >
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
            )}
            {tab === "apps" && (
              <div className="island-panel apps-panel" role="tabpanel">
                <div className="apps-section-head">
                  <span>{t(language, "runningAppsTitle")} ({taskbarWindows.length})</span>
                  {pinnedApps.length > 0 && (
                    <div className="pinned-strip">
                      {pinnedApps.slice(0, 4).map((app) => (
                        <button
                          key={app.target}
                          type="button"
                          className="pinned-chip"
                          onClick={() => launchPinnedApp(app.target)}
                          title={app.name}
                        >
                          <RgbaIcon
                            rgbaBase64={app.icon_rgba_base64}
                            size={app.icon_size}
                            title={app.name}
                          />
                          <span className="pinned-chip-name">{app.name}</span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
                {taskbarWindows.length === 0 ? (
                  <div className="apps-empty">{t(language, "noRunningApps")}</div>
                ) : (
                  <div className="apps-list">
                    {taskbarWindows.map((win) => (
                      <button
                        key={win.id}
                        type="button"
                        className={`app-list-item${win.active ? " active" : ""}${
                          win.minimized ? " minimized" : ""
                        }`}
                        onClick={() => activateTaskbarWindow(win.id)}
                        title={win.title}
                      >
                        <RgbaIcon
                          rgbaBase64={win.icon_rgba_base64}
                          size={win.icon_size}
                          title={win.title}
                        />
                        <span className="app-list-title">{win.title}</span>
                        <span
                          className={`app-status-dot${win.active ? " active" : ""}`}
                          aria-hidden="true"
                        />
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}
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
                  <span className="panel-controls media-icon-controls">
                    <button
                      type="button"
                      className="media-icon-btn"
                      onClick={() => sendMediaCommand("media_previous")}
                      title={t(language, "mediaPrev")}
                      aria-label={t(language, "mediaPrev")}
                    >
                      <SvgPrev size={15} />
                    </button>
                    <button
                      type="button"
                      className="media-icon-btn primary"
                      onClick={() =>
                        sendMediaCommand(
                          media?.playing ? "media_pause" : "media_play",
                        )
                      }
                      title={
                        media?.playing
                          ? t(language, "mediaPause")
                          : t(language, "mediaPlay")
                      }
                      aria-label={
                        media?.playing
                          ? t(language, "mediaPause")
                          : t(language, "mediaPlay")
                      }
                    >
                      {media?.playing ? <SvgPause size={16} /> : <SvgPlay size={16} />}
                    </button>
                    <button
                      type="button"
                      className="media-icon-btn"
                      onClick={() => sendMediaCommand("media_stop")}
                      title={t(language, "mediaStop")}
                      aria-label={t(language, "mediaStop")}
                    >
                      <SvgStop size={13} />
                    </button>
                    <button
                      type="button"
                      className="media-icon-btn"
                      onClick={() => sendMediaCommand("media_next")}
                      title={t(language, "mediaNext")}
                      aria-label={t(language, "mediaNext")}
                    >
                      <SvgNext size={15} />
                    </button>
                    <button
                      type="button"
                      className="media-icon-btn"
                      onClick={refreshMedia}
                      title={t(language, "mediaRefresh")}
                      aria-label={t(language, "mediaRefresh")}
                    >
                      <SvgRefresh size={14} />
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
                    <span className="stat-chip-head"><span className="stat-icon"><SvgCpu size={14} /></span><span className="stat-chip-label">CPU</span></span>
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
                    <span className="stat-chip-head"><span className="stat-icon"><SvgRam size={14} /></span><span className="stat-chip-label">RAM</span></span>
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
                      stats?.gpu_percent != null
                        ? tf(language, "gpuUsageValue", {
                            v: stats.gpu_percent.toFixed(1),
                          })
                        : t(language, "gpuUsageEmpty")
                    }
                  >
                    <span className="stat-chip-head"><span className="stat-icon"><SvgGpu size={14} /></span><span className="stat-chip-label">GPU</span></span>
                    <span className="stat-chip-value">
                      {stats?.gpu_percent != null
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
                      stats?.net_up_kbps != null
                        ? tf(language, "uploadValue", {
                            v: stats.net_up_kbps.toFixed(2),
                          })
                        : t(language, "uploadEmpty")
                    }
                  >
                    ↑{" "}
                    {stats?.net_up_kbps != null
                      ? formatSpeed(
                          stats.net_up_kbps,
                          displaySettings.speedUnit,
                        )
                      : "—"}
                  </span>
                  <span
                    title={
                      stats?.net_down_kbps != null
                        ? tf(language, "downloadValue", {
                            v: stats.net_down_kbps.toFixed(2),
                          })
                        : t(language, "downloadEmpty")
                    }
                  >
                    ↓{" "}
                    {stats?.net_down_kbps != null
                      ? formatSpeed(
                          stats.net_down_kbps,
                          displaySettings.speedUnit,
                        )
                      : "—"}
                  </span>
                </div>

              </div>
            )}
            {tab === "engines" && hasDownloadedEngine && (
              <div className="island-panel" role="tabpanel">
                {islandTranslateStatus?.local_model_installed && islandTranslateStatus?.installed && (
                  <div className="panel-line">
                    <span className="panel-title">{t(language, "localEngineTitle")}</span>
                    <div className="panel-controls">
                      <button
                        type="button"
                        className={`mini-btn${islandTranslateStatus.running && islandTranslateStatus.translate_provider === "local" ? " danger-btn" : " primary-btn"}`}
                        onClick={() => {
                          if (islandTranslateStatus.running && islandTranslateStatus.translate_provider === "local") {
                            invoke<TranslateEngineStatus>("stop_live_translate")
                              .then((st) => {
                                setIslandTranslateStatus(st);
                                setLiveSubActive(st.running);
                              })
                              .catch(() => {});
                          } else {
                            const savedMode = window.localStorage.getItem("nexora-translate-capture-mode") || "system";
                            const savedPidRaw = window.localStorage.getItem("nexora-translate-target-pid");
                            const savedPid = savedPidRaw ? Number(savedPidRaw) : islandTranslateStatus.target_pid;
                            const savedAppName = window.localStorage.getItem("nexora-translate-target-app") || islandTranslateStatus.target_app_name;
                            invoke<TranslateEngineStatus>("start_live_translate", {
                              sourceLang: "en",
                              targetLang: "fa",
                              captureMode: savedMode,
                              onlineTranslation: false,
                              translateProvider: "local",
                              targetPid: savedMode === "app" && savedPid ? savedPid : null,
                              targetAppName: savedMode === "app" ? savedAppName : null,
                            })
                              .then((st) => {
                                setIslandTranslateStatus(st);
                                setLiveSubActive(st.running);
                              })
                              .catch(() => {});
                          }
                        }}
                      >
                        {islandTranslateStatus.running && islandTranslateStatus.translate_provider === "local"
                          ? t(language, "stopLocalTranslateBtn")
                          : t(language, "startLocalTranslateBtn")}
                      </button>
                    </div>
                  </div>
                )}
                {islandTranslateStatus?.installed && (
                  <div className="panel-line">
                    <span className="panel-title">{t(language, "onlineEngineTitle")}</span>
                    <div className="panel-controls">
                      <button
                        type="button"
                        className={`mini-btn${islandTranslateStatus.running && islandTranslateStatus.translate_provider !== "local" ? " danger-btn" : " primary-btn"}`}
                        onClick={() => {
                          if (islandTranslateStatus.running && islandTranslateStatus.translate_provider !== "local") {
                            invoke<TranslateEngineStatus>("stop_live_translate")
                              .then((st) => {
                                setIslandTranslateStatus(st);
                                setLiveSubActive(st.running);
                              })
                              .catch(() => {});
                          } else {
                            const savedMode = window.localStorage.getItem("nexora-translate-capture-mode") || "system";
                            const savedPidRaw = window.localStorage.getItem("nexora-translate-target-pid");
                            const savedPid = savedPidRaw ? Number(savedPidRaw) : islandTranslateStatus.target_pid;
                            const savedAppName = window.localStorage.getItem("nexora-translate-target-app") || islandTranslateStatus.target_app_name;
                            const onlineProv =
                              islandTranslateStatus.translate_provider &&
                              islandTranslateStatus.translate_provider !== "local" &&
                              islandTranslateStatus.translate_provider !== "none"
                                ? islandTranslateStatus.translate_provider
                                : "google_free";
                            invoke<TranslateEngineStatus>("start_live_translate", {
                              sourceLang: islandTranslateStatus.source_lang || "en",
                              targetLang: islandTranslateStatus.target_lang || "fa",
                              captureMode: savedMode,
                              onlineTranslation: true,
                              translateProvider: onlineProv,
                              targetPid: savedMode === "app" && savedPid ? savedPid : null,
                              targetAppName: savedMode === "app" ? savedAppName : null,
                            })
                              .then((st) => {
                                setIslandTranslateStatus(st);
                                setLiveSubActive(st.running);
                              })
                              .catch(() => {});
                          }
                        }}
                      >
                        {islandTranslateStatus.running && islandTranslateStatus.translate_provider !== "local"
                          ? t(language, "stopOnlineTranslateBtn")
                          : t(language, "startOnlineTranslateBtn")}
                      </button>
                    </div>
                  </div>
                )}
                {islandDubbingStatus?.ready && (
                  <div className="panel-line">
                    <span className="panel-title">{t(language, "dubbingEngineTitle")}</span>
                    <div className="panel-controls">
                      <button
                        type="button"
                        className={`mini-btn${islandDubbingStatus.running ? " danger-btn" : " primary-btn"}`}
                        onClick={() => {
                          if (islandDubbingStatus.running) {
                            invoke<DubbingStatus>("stop_live_dubbing")
                              .then(setIslandDubbingStatus)
                              .catch(() => {});
                          } else {
                            const savedMode = window.localStorage.getItem("nexora-translate-capture-mode") || "system";
                            const savedPidRaw = window.localStorage.getItem("nexora-translate-target-pid");
                            const savedPid = savedPidRaw ? Number(savedPidRaw) : islandDubbingStatus.target_pid;
                            const savedAppName = window.localStorage.getItem("nexora-translate-target-app") || islandDubbingStatus.target_app_name;
                            invoke<DubbingStatus>("start_live_dubbing", {
                              captureMode: savedMode,
                              targetPid: savedMode === "app" && savedPid ? savedPid : null,
                              targetAppName: savedMode === "app" ? savedAppName : null,
                              voiceId: islandDubbingStatus.voice_id,
                              duckPercent: islandDubbingStatus.duck_percent,
                              speechRatePct: islandDubbingStatus.speech_rate_pct,
                              autoGenderVoice: islandDubbingStatus.auto_gender_voice,
                              femaleVoiceId: islandDubbingStatus.female_voice_id,
                            })
                              .then(setIslandDubbingStatus)
                              .catch(() => {});
                          }
                        }}
                      >
                        {islandDubbingStatus.running
                          ? t(language, "stopDubbingBtn")
                          : t(language, "startDubbingBtn")}
                      </button>
                    </div>
                  </div>
                )}
                <div className="quick-actions">
                  <button type="button" className="mini-btn" onClick={openSettings}>
                    {t(language, "openEngineSettingsBtn")}
                  </button>
                </div>
              </div>
            )}
            {tab === "quick" && (
              <div className="island-panel" role="tabpanel">
                <div className="settings-row">
                  <span className="settings-row-text">
                    <span>{t(language, "quickTheme")}</span>
                    <span className="settings-row-desc">
                      {THEME_DESCRIPTIONS[language][theme]}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="mini-btn"
                    onClick={cycleQuickTheme}
                    disabled={!settings}
                  >
                    {currentTheme.name}
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
                    onClick={() => toggleQuickSetting("always_on_top")}
                    disabled={!settings}
                  >
                    {settings ? onOff(settings.always_on_top, language) : "—"}
                  </button>
                </div>
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
                    onClick={() => toggleQuickSetting("clock_24h")}
                    disabled={!settings}
                  >
                    {settings ? onOff(settings.clock_24h, language) : "—"}
                  </button>
                </div>
                <div className="quick-actions">
                  <button
                    type="button"
                    className="mini-btn"
                    onClick={openSettings}
                  >
                    {t(language, "openSettings")}
                  </button>
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

// Build a global-shortcut string ("Ctrl+Shift+N", "Alt+Space") from
// a keydown event, or null when the press is not a capturable
// hotkey: a lone modifier, Escape (cancels the capture), an
// uncapturable key — or a bare letter, digit, Space or arrow, which
// as a global hotkey would fire on every keystroke. Bare F-keys are
// fine (nothing types F9). Modifier order is fixed Ctrl, Alt, Shift,
// Super, matching the backend's parseable form.
function shortcutFromKeyEvent(event: KeyboardEvent): string | null {
  const key = event.key;
  if (
    key === "Escape" ||
    key === "Control" ||
    key === "Alt" ||
    key === "Shift" ||
    key === "Meta"
  ) {
    return null;
  }
  const code = event.code;
  let main: string | null = null;
  if (code.startsWith("Key") && code.length === 4) {
    main = code.slice(3);
  } else if (code.startsWith("Digit") && code.length === 6) {
    main = code.slice(5);
  } else if (/^F([1-9]|1[0-2])$/.test(code)) {
    main = code;
  } else if (code === "Space") {
    main = "Space";
  } else if (
    code === "Enter" ||
    code === "Tab" ||
    code === "Backspace" ||
    code === "Delete" ||
    code === "Insert" ||
    code === "Home" ||
    code === "End" ||
    code === "PageUp" ||
    code === "PageDown" ||
    code.startsWith("Arrow")
  ) {
    main = code;
  }
  if (main === null) {
    return null;
  }
  const isFunctionKey = /^F([1-9]|1[0-2])$/.test(main);
  const mods: string[] = [];
  if (event.ctrlKey) mods.push("Ctrl");
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");
  if (event.metaKey) mods.push("Super");
  if (mods.length === 0 && !isFunctionKey) {
    return null;
  }
  return [...mods, main].join("+");
}

function SettingsView() {
  const [settings, setSettings] = useState<NexoraSettings | null>(null);
  const [theme, setTheme] = useState<ThemeId>(() => themeOrDefault(readSavedTheme()));
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [capturingHotkey, setCapturingHotkey] = useState(false);
  const [hotkeyError, setHotkeyError] = useState(false);
  // Window controls (frameless frame): the title bar is the drag
  // region and the × button closes this window. The close button
  // must stay OUTSIDE the drag region, or the drag eats its clicks.
  const [islandScalePct, setIslandScalePct] = useState<number>(() => readIslandScale());
  const [islandGlowPct, setIslandGlowPct] = useState<number>(() => readIslandGlow());
  const [translateStatus, setTranslateStatus] = useState<TranslateEngineStatus | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgressPayload | null>(null);
  const [srcLang, setSrcLang] = useState("auto");
  const [tgtLang, setTgtLang] = useState("fa");
  const [captureMode, setCaptureMode] = useState<"system" | "app">(() =>
    window.localStorage.getItem("nexora-translate-capture-mode") === "app" ? "app" : "system",
  );
  const [audioTargets, setAudioTargets] = useState<AudioAppTarget[]>([]);
  const [targetPid, setTargetPid] = useState<number | null>(() => {
    const raw = window.localStorage.getItem("nexora-translate-target-pid");
    return raw ? Number(raw) : null;
  });
  const [targetAppName, setTargetAppName] = useState<string>(
    () => window.localStorage.getItem("nexora-translate-target-app") || "",
  );
  const [testSubText, setTestSubText] = useState("Welcome to Nexora live subtitles!");

  const refreshAudioTargets = () => {
    invoke<AudioAppTarget[]>("list_audio_sources")
      .then((list) => {
        setAudioTargets(list);
        if (list.length > 0 && (!targetPid || !list.some((item) => item.pid === targetPid))) {
          setTargetPid(list[0].pid);
          setTargetAppName(list[0].title || list[0].exe_name);
          try {
            window.localStorage.setItem("nexora-translate-target-pid", String(list[0].pid));
            window.localStorage.setItem("nexora-translate-target-app", list[0].title || list[0].exe_name);
          } catch {
            // ignore
          }
        }
      })
      .catch(() => {});
  };

  useEffect(() => {
    refreshAudioTargets();
  }, []);

  useEffect(() => {
    invoke<TranslateEngineStatus>("get_translate_status")
      .then((st) => {
        setTranslateStatus(st);
        if (st.source_lang) setSrcLang(st.source_lang);
        if (st.target_lang) setTgtLang(st.target_lang);
      })
      .catch(() => {});
    let unlisten: (() => void) | undefined;
    listen<DownloadProgressPayload>("translate-download-progress", (ev) => {
      setDownloadProgress(ev.payload);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, []);

  const [apiKeyStatus, setApiKeyStatus] = useState<ApiKeyStatus>({
    google_cloud: false,
    gemini: false,
  });
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [apiKeyMsg, setApiKeyMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [testingProvider, setTestingProvider] = useState(false);

  const [openEngineCard, setOpenEngineCard] = useState<"local" | "online" | "dubbing" | null>(null);
  const [dubbingStatus, setDubbingStatus] = useState<DubbingStatus | null>(null);
  const [dubbingProgress, setDubbingProgress] = useState<DownloadProgressPayload | null>(null);
  const [dubVoiceId, setDubVoiceId] = useState("fa_IR-amir-medium");
  const [dubAutoGender, setDubAutoGender] = useState(true);
  const [dubFemaleVoiceId, setDubFemaleVoiceId] = useState("auto_female");
  const [dubDuckPercent, setDubDuckPercent] = useState(25);
  const [dubRatePct, setDubRatePct] = useState(105);
  const [dubTesting, setDubTesting] = useState(false);
  const [recordingCloneSlot, setRecordingCloneSlot] = useState<"male" | "female" | null>(null);
  const [subtitleCssDraft, setSubtitleCssDraft] = useState("");
  const subtitleCssTimerRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (settings && subtitleCssTimerRef.current === undefined) {
      setSubtitleCssDraft(settings.subtitle_custom_css ?? "");
    }
  }, [settings]);

  const saveSubtitleCssNow = (cssText: string) => {
    if (subtitleCssTimerRef.current !== undefined) {
      window.clearTimeout(subtitleCssTimerRef.current);
      subtitleCssTimerRef.current = undefined;
    }
    if (!settings) return;
    invoke<NexoraSettings>("set_settings", {
      settings: { ...settings, subtitle_custom_css: cssText },
    })
      .then(setSettings)
      .catch(() => {});
  };

  const handleSubtitleCssChange = (cssText: string) => {
    setSubtitleCssDraft(cssText);
    if (subtitleCssTimerRef.current !== undefined) {
      window.clearTimeout(subtitleCssTimerRef.current);
    }
    subtitleCssTimerRef.current = window.setTimeout(() => {
      subtitleCssTimerRef.current = undefined;
      saveSubtitleCssNow(cssText);
    }, 350);
  };

  const [presetNameInput, setPresetNameInput] = useState("");
  const cssPresets = settings?.subtitle_css_presets ?? [];
  const activeCssPreset = settings?.active_subtitle_css_preset ?? "";

  const handleSelectCssPreset = (presetName: string) => {
    if (!settings) return;
    if (!presetName) {
      setSubtitleCssDraft("");
      invoke<NexoraSettings>("set_settings", {
        settings: {
          ...settings,
          subtitle_custom_css: "",
          active_subtitle_css_preset: "",
        },
      })
        .then(setSettings)
        .catch(() => {});
      return;
    }
    const found = cssPresets.find((p) => p.name === presetName);
    if (!found) return;
    setSubtitleCssDraft(found.css);
    setPresetNameInput(found.name);
    invoke<NexoraSettings>("set_settings", {
      settings: {
        ...settings,
        subtitle_custom_css: found.css,
        active_subtitle_css_preset: found.name,
      },
    })
      .then(setSettings)
      .catch(() => {});
  };

  const handleSaveCssPreset = () => {
    if (!settings) return;
    const name = presetNameInput.trim();
    if (!name) return;
    const nextPresets = [
      ...cssPresets.filter((p) => p.name !== name),
      { name, css: subtitleCssDraft },
    ];
    invoke<NexoraSettings>("set_settings", {
      settings: {
        ...settings,
        subtitle_custom_css: subtitleCssDraft,
        subtitle_css_presets: nextPresets,
        active_subtitle_css_preset: name,
      },
    })
      .then(setSettings)
      .catch(() => {});
  };

  const handleDeleteCssPreset = () => {
    if (!settings || !activeCssPreset) return;
    const nextPresets = cssPresets.filter((p) => p.name !== activeCssPreset);
    setPresetNameInput("");
    invoke<NexoraSettings>("set_settings", {
      settings: {
        ...settings,
        subtitle_css_presets: nextPresets,
        active_subtitle_css_preset: "",
      },
    })
      .then(setSettings)
      .catch(() => {});
  };

  useEffect(() => {
    invoke<DubbingStatus>("get_dubbing_status")
      .then((st) => {
        setDubbingStatus(st);
        if (st.voice_id) setDubVoiceId(st.voice_id);
        setDubAutoGender(st.auto_gender_voice);
        if (st.female_voice_id) setDubFemaleVoiceId(st.female_voice_id);
        setDubDuckPercent(st.duck_percent);
        setDubRatePct(st.speech_rate_pct);
      })
      .catch(() => {});
    let unSt: (() => void) | undefined;
    let unPr: (() => void) | undefined;
    listen<DubbingStatus>("dubbing-status-changed", (ev) => {
      setDubbingStatus(ev.payload);
    }).then((fn) => { unSt = fn; }).catch(() => {});
    listen<DownloadProgressPayload>("dubbing-download-progress", (ev) => {
      setDubbingProgress(ev.payload);
    }).then((fn) => { unPr = fn; }).catch(() => {});
    return () => {
      unSt?.();
      unPr?.();
    };
  }, []);
  const [onlineProviderChoice, setOnlineProviderChoice] = useState<string>(() => {
    try {
      return window.localStorage.getItem("nexora-online-provider") || "google_free";
    } catch {
      return "google_free";
    }
  });
  const activeProvider = settings?.translate_provider ?? "none";
  const activeWhisperModel = settings?.whisper_model ?? "ggml-tiny.bin";

  const handleToggleEngineByMode = (mode: "local" | "online") => {
    const isRunningThisMode =
      Boolean(translateStatus?.running) &&
      (mode === "local"
        ? translateStatus?.translate_provider === "local" || translateStatus?.translate_provider === "none"
        : translateStatus?.translate_provider !== "local" && translateStatus?.translate_provider !== "none");
    if (isRunningThisMode) {
      invoke<TranslateEngineStatus>("stop_live_translate")
        .then(setTranslateStatus)
        .catch(() => {});
      return;
    }
    const chosenProvider =
      mode === "local"
        ? translateStatus?.local_model_installed
          ? "local"
          : "none"
        : onlineProviderChoice;
    if (mode === "online") {
      const consented = window.localStorage.getItem("nexora-cloud-translate-consent") === "1";
      if (!consented) {
        if (!window.confirm(t(language, "onlineTranslationConsent"))) {
          return;
        }
        try {
          window.localStorage.setItem("nexora-cloud-translate-consent", "1");
        } catch {
          // ignore
        }
      }
    }
    persistTranslateSettings({
      translate_provider: chosenProvider,
      online_translation: mode === "online",
    });
    invoke<TranslateEngineStatus>("start_live_translate", {
      sourceLang: mode === "local" ? "en" : srcLang,
      targetLang: mode === "local" ? "fa" : tgtLang,
      captureMode,
      onlineTranslation: mode === "online",
      translateProvider: chosenProvider,
      targetPid: captureMode === "app" ? targetPid : null,
      targetAppName: captureMode === "app" ? targetAppName || null : null,
    })
      .then(setTranslateStatus)
      .catch((err) => {
        setTranslateStatus((prev) =>
          prev ? { ...prev, running: false, last_error: String(err) } : prev,
        );
      });
  };

  const refreshApiKeyStatus = () => {
    invoke<ApiKeyStatus>("get_translate_api_key_status")
      .then(setApiKeyStatus)
      .catch(() => {});
  };

  useEffect(() => {
    refreshApiKeyStatus();
  }, []);

  const persistTranslateSettings = (patch: Partial<NexoraSettings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    invoke<NexoraSettings>("set_settings", { settings: next })
      .then((saved) => {
        setSettings(saved);
        invoke<TranslateEngineStatus>("get_translate_status")
          .then(setTranslateStatus)
          .catch(() => {});
      })
      .catch(() => {});
  };

  const handleSaveApiKey = (providerOverride?: string) => {
    const trimmed = apiKeyInput.trim();
    if (!trimmed) return;
    setApiKeyMsg(null);
    invoke<ApiKeyStatus>("set_translate_api_key", {
      provider: providerOverride ?? onlineProviderChoice,
      apiKey: trimmed,
    })
      .then((st) => {
        setApiKeyStatus(st);
        setApiKeyInput("");
        setApiKeyMsg({ ok: true, text: t(language, "apiKeySaved") });
      })
      .catch((err) => {
        setApiKeyMsg({ ok: false, text: String(err) });
      });
  };

  const handleDeleteApiKey = (providerOverride?: string) => {
    setApiKeyMsg(null);
    invoke<ApiKeyStatus>("delete_translate_api_key", {
      provider: providerOverride ?? onlineProviderChoice,
    })
      .then((st) => {
        setApiKeyStatus(st);
        setApiKeyInput("");
      })
      .catch((err) => {
        setApiKeyMsg({ ok: false, text: String(err) });
      });
  };

  const handleTestProvider = (providerOverride?: string) => {
    setTestingProvider(true);
    setApiKeyMsg(null);
    invoke<TestProviderResult>("test_translate_provider", {
      provider: providerOverride ?? activeProvider,
      targetLang: tgtLang,
    })
      .then((res) => {
        if (res.ok) {
          setApiKeyMsg({
            ok: true,
            text: tf(language, "testConnectionOk", {
              ms: res.latency_ms,
              text: res.translated ?? "",
            }),
          });
        } else {
          setApiKeyMsg({ ok: false, text: res.error ?? "Failed" });
        }
      })
      .catch((err) => {
        setApiKeyMsg({ ok: false, text: String(err) });
      })
      .finally(() => setTestingProvider(false));
  };

  const handleDownloadEngine = () => {
    setDownloadProgress({ percent: 5, stage: "Starting download..." });
    invoke<TranslateEngineStatus>("download_translate_engine", {
      whisperModel: activeWhisperModel,
    })
      .then((st) => {
        setTranslateStatus(st);
        setDownloadProgress(null);
      })
      .catch((err) => {
        setDownloadProgress(null);
        setTranslateStatus((prev) =>
          prev ? { ...prev, downloading: false, last_error: String(err) } : prev,
        );
      });
  };

  const handleDownloadLocalModel = () => {
    setDownloadProgress({ percent: 5, stage: "Downloading Nexora-MT v2..." });
    invoke<TranslateEngineStatus>("download_local_translate_model")
      .then((st) => {
        setTranslateStatus(st);
        setDownloadProgress(null);
      })
      .catch((err) => {
        setDownloadProgress(null);
        setTranslateStatus((prev) =>
          prev ? { ...prev, downloading: false, last_error: String(err) } : prev,
        );
      });
  };

  const handleRemoveLocalModel = () => {
    invoke<TranslateEngineStatus>("remove_local_translate_model")
      .then(setTranslateStatus)
      .catch(() => {});
  };

  const handleRemoveEngine = () => {
    invoke<TranslateEngineStatus>("remove_translate_engine")
      .then(setTranslateStatus)
      .catch(() => {});
  };

  const handleTestSubtitle = () => {
    invoke<SubtitlePayload>("test_subtitle_overlay", {
      text: testSubText,
      targetLang: tgtLang,
    }).catch(() => {});
  };

  const updateIslandScale = (pct: number) => {
    const clamped = Math.min(115, Math.max(75, Math.round(pct)));
    setIslandScalePct(clamped);
    try {
      window.localStorage.setItem(SIZE_STORAGE_KEY, String(clamped));
    } catch {
      // ignore
    }
    applyVisualCssVars(clamped, islandGlowPct);
  };

  const updateIslandGlow = (pct: number) => {
    const clamped = Math.min(100, Math.max(0, Math.round(pct)));
    setIslandGlowPct(clamped);
    try {
      window.localStorage.setItem(GLOW_STORAGE_KEY, String(clamped));
    } catch {
      // ignore
    }
    applyVisualCssVars(islandScalePct, clamped);
  };

  const [, setEngineHotkey] = useState<string>("Ctrl+Shift+T");
  const [capturingEngineHotkey, setCapturingEngineHotkey] = useState(false);

  useEffect(() => {
    if (!capturingEngineHotkey) return;
    const onKey = (ev: KeyboardEvent) => {
      ev.preventDefault();
      ev.stopPropagation();
      if (ev.key === "Escape") {
        setCapturingEngineHotkey(false);
        return;
      }
      if (["Control", "Shift", "Alt", "Meta"].includes(ev.key)) return;
      const mods: string[] = [];
      if (ev.ctrlKey) mods.push("Ctrl");
      if (ev.altKey) mods.push("Alt");
      if (ev.shiftKey) mods.push("Shift");
      const keyName = ev.key.length === 1 ? ev.key.toUpperCase() : ev.key;
      const combo = mods.length > 0 ? `${mods.join("+")}+${keyName}` : keyName;
      setEngineHotkey(combo);
      try {
        window.localStorage.setItem("nexora-engine-translate-hotkey", combo);
      } catch {
        // ignore
      }
      setCapturingEngineHotkey(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturingEngineHotkey]);

  const minimizeSettingsWindow = () => {
    getCurrentWindow().minimize().catch(() => undefined);
  };

  const closeSettingsWindow = () => {
    const win = getCurrentWindow();
    win.hide().catch(() => win.close().catch(() => undefined));
  };
  // The sidebar section on screen. The window is 900x620 with a
  // 7-section sidebar (design artifact); the title bar above it is
  // the window's drag region + close button (frameless frame from
  // EAgent5).
  type SettingsSection =
    | "language"
    | "islandStyle"
    | "clock"
    | "performance"
    | "liveTranslate"
    | "updates";
  const [section, setSection] = useState<SettingsSection>(() => {
    try {
      const target = window.localStorage.getItem("nexora-settings-nav");
      if (target === "updates" || target === "liveTranslate" || target === "islandStyle") {
        window.localStorage.removeItem("nexora-settings-nav");
        return target;
      }
    } catch {
      // ignore
    }
    return "language";
  });

  useEffect(() => {
    const consumeNav = () => {
      try {
        const target = window.localStorage.getItem("nexora-settings-nav");
        if (target === "updates" || target === "liveTranslate" || target === "islandStyle") {
          window.localStorage.removeItem("nexora-settings-nav");
          setSection(target);
        }
      } catch {
        // ignore
      }
    };
    window.addEventListener("focus", consumeNav);
    window.addEventListener("storage", consumeNav);
    return () => {
      window.removeEventListener("focus", consumeNav);
      window.removeEventListener("storage", consumeNav);
    };
  }, []);
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
      | "show_seconds"
      | "island_show_start"
      | "island_show_apps"
      | "island_show_stats"
      | "island_show_gpu"
      | "island_show_media"
      | "island_show_clock"
      | "island_show_notifications"
      | "island_show_engine_local"
      | "island_show_engine_online"
      | "island_show_engine_dubbing",
  ) => {
    if (!settings) {
      return;
    }
    const currentVal = key.startsWith("island_show_")
      ? (settings[key] ?? true)
      : Boolean(settings[key]);
    saveSettings({ ...settings, [key]: !currentVal });
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
        auto_hide_taskbar: false,
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
        island_show_start: true,
        island_show_apps: true,
        island_show_stats: true,
        island_show_gpu: true,
        island_show_media: true,
        island_show_clock: true,
        island_show_notifications: true,
        island_show_engine_local: true,
        island_show_engine_online: true,
        island_show_engine_dubbing: true,
      });
    }
  };

  const [autoUpdate, setAutoUpdate] = useState<boolean>(
    () => window.localStorage.getItem("nexora-auto-update") !== "0",
  );
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [updateProgressPct, setUpdateProgressPct] = useState<number | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [showWhatsNew, setShowWhatsNew] = useState<boolean>(() => {
    try {
      if (window.localStorage.getItem("nexora-show-whats-new")) {
        window.localStorage.removeItem("nexora-show-whats-new");
        return true;
      }
    } catch {
      // ignore
    }
    return false;
  });

  useEffect(() => {
    const checkWhatsNewFlag = () => {
      try {
        if (window.localStorage.getItem("nexora-show-whats-new")) {
          window.localStorage.removeItem("nexora-show-whats-new");
          setShowWhatsNew(true);
        }
      } catch {
        // ignore
      }
    };
    window.addEventListener("focus", checkWhatsNewFlag);
    window.addEventListener("storage", checkWhatsNewFlag);
    return () => {
      window.removeEventListener("focus", checkWhatsNewFlag);
      window.removeEventListener("storage", checkWhatsNewFlag);
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<UpdateProgressPayload>("update-progress", (ev) => {
      const { downloaded, total } = ev.payload;
      if (total && total > 0) {
        setUpdateProgressPct(Math.min(100, Math.round((downloaded / total) * 100)));
      }
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, []);

  const checkForUpdates = () => {
    setCheckingUpdate(true);
    setUpdateError(null);
    invoke<UpdateInfo>("check_updates")
      .then((info) => {
        setUpdateInfo(info);
      })
      .catch((err) => {
        setUpdateError(String(err));
      })
      .finally(() => {
        setCheckingUpdate(false);
      });
  };

  const installAvailableUpdate = () => {
    setInstallingUpdate(true);
    setUpdateProgressPct(0);
    setUpdateError(null);
    void (async () => {
      try {
        const update = await checkPluginUpdate();
        if (!update) {
          setInstallingUpdate(false);
          setUpdateProgressPct(null);
          return;
        }
        const notes = update.body ? "\n\n" + update.body : "";
        const promptText = t(language, "updateConfirmPrompt").replace("{v}", update.version) + notes;
        if (!window.confirm(promptText)) {
          setInstallingUpdate(false);
          setUpdateProgressPct(null);
          return;
        }
        try {
          window.localStorage.setItem("nexora-show-whats-new", "1");
        } catch {
          // ignore
        }
        await invoke("prepare_for_update_exit").catch(() => {});
        let downloaded = 0;
        let total = 0;
        await update.downloadAndInstall((ev) => {
          if (ev.event === "Started") {
            total = ev.data.contentLength ?? 0;
            setUpdateProgressPct(1);
          } else if (ev.event === "Progress") {
            downloaded += ev.data.chunkLength;
            if (total > 0) {
              setUpdateProgressPct(Math.min(99, Math.round((downloaded / total) * 100)));
            }
          } else if (ev.event === "Finished") {
            setUpdateProgressPct(100);
          }
        });
      } catch (err) {
        setUpdateError(String(err));
        setInstallingUpdate(false);
        setUpdateProgressPct(null);
      }
    })();
  };

  // The Restore-taskbar row (Taskbar section): bring the Windows
  // taskbar back right now. The result is the taskbar itself coming
  // back, so there is nothing to show here.


  // Change the island hotkey: the backend validates the shortcut,
  // re-registers it and persists it only on success, so a false
  // return means the old key is still in place — keep showing it
  // and say so. SettingsView loads its settings once, so apply a
  // success here too (the island picks it up from settings-changed).
  const submitHotkey = (shortcut: string) => {
    invoke<boolean>("set_island_hotkey", { shortcut })
      .then((ok) => {
        if (ok) {
          setSettings((prev) =>
            prev ? { ...prev, island_hotkey: shortcut } : prev,
          );
          setHotkeyError(false);
        } else {
          setHotkeyError(true);
        }
      })
      .catch(() => setHotkeyError(true));
  };

  // While capturing, the next keydown is the new hotkey: Escape
  // cancels, a lone modifier or an uncapturable key keeps listening.
  // The current hotkey is registered globally, so the OS swallows it
  // before the webview sees a keydown — but the backend still emits
  // island-hotkey, so treat that press as re-confirming the current
  // key instead of leaving the capture stuck.
  useEffect(() => {
    if (!capturingHotkey) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      if (event.repeat) {
        return;
      }
      if (event.key === "Escape") {
        setCapturingHotkey(false);
        return;
      }
      const shortcut = shortcutFromKeyEvent(event);
      if (shortcut === null) {
        return;
      }
      setCapturingHotkey(false);
      submitHotkey(shortcut);
    };
    window.addEventListener("keydown", onKeyDown);
    let cancelled = false;
    let unlistenHotkey: (() => void) | undefined;
    listen("island-hotkey", () => {
      if (!cancelled && settings) {
        setCapturingHotkey(false);
        submitHotkey(settings.island_hotkey);
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlistenHotkey = fn;
      }
    });
    return () => {
      cancelled = true;
      window.removeEventListener("keydown", onKeyDown);
      unlistenHotkey?.();
    };
  }, [capturingHotkey]);

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

    // Keep Settings theme & state live-synced whenever the Dynamic Island changes theme.
    let unlistenSettings: (() => void) | undefined;
    listen<NexoraSettings>("settings-changed", (event) => {
      setSettings(event.payload);
      applyTheme(themeOrDefault(event.payload.theme));
    })
      .then((fn) => {
        unlistenSettings = fn;
      })
      .catch(() => {});

    const syncThemeFromStorage = () => {
      const latest = readSavedTheme();
      if (latest) {
        applyTheme(themeOrDefault(latest));
      }
    };
    window.addEventListener("storage", syncThemeFromStorage);
    window.addEventListener("focus", syncThemeFromStorage);

    return () => {
      unlistenSettings?.();
      window.removeEventListener("storage", syncThemeFromStorage);
      window.removeEventListener("focus", syncThemeFromStorage);
    };
    // Runs once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const sections: { id: SettingsSection; label: string }[] = [
    { id: "language", label: t(language, "sectionLanguage") },
    { id: "islandStyle", label: t(language, "sectionIslandStyle") },
    { id: "clock", label: t(language, "sectionClock") },
    { id: "performance", label: t(language, "sectionPerformance") },
    { id: "liveTranslate", label: t(language, "sectionEngines") },
    { id: "updates", label: t(language, "sectionUpdates") },
  ];

  return (
    <main className="settings-shell">
      <div className="settings-titlebar">
        <div className="settings-titlebar-drag" data-tauri-drag-region>
          <span>◈ Nexora Settings</span>
        </div>
        <div className="settings-window-controls">
          <button
            type="button"
            className="settings-min-btn"
            onClick={minimizeSettingsWindow}
            title={t(language, "minimizeWindow")}
            aria-label={t(language, "minimizeWindow")}
          >
            ─
          </button>
          <button
            type="button"
            className="settings-close-btn"
            onClick={closeSettingsWindow}
            title={t(language, "closeWindow")}
            aria-label={t(language, "closeWindow")}
          >
            ✕
          </button>
        </div>
      </div>
      <div className="settings-body">
      <aside className="settings-sidebar">
        <img className="settings-wordmark" src={wordmark} alt="Nexora" />
        <h1 className="settings-title">{t(language, "settingsTitle")}</h1>
        <nav
          className="settings-nav"
          aria-label={t(language, "settingsTitle")}
        >
          {sections.map((candidate) => (
            <button
              key={candidate.id}
              type="button"
              className={`settings-nav-btn${
                section === candidate.id ? " active" : ""
              }`}
              aria-current={section === candidate.id}
              onClick={() => setSection(candidate.id)}
            >
              {candidate.label}
            </button>
          ))}
        </nav>
        <div className="settings-sidebar-footer">
          <button
            type="button"
            className="mini-btn settings-reset"
            onClick={resetToDefaults}
          >
            {t(language, "resetToDefaults")}
          </button>
        </div>
      </aside>
      <div className="settings-content">
      {section === "language" && (
        <>
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
        </>
      )}
      {section === "islandStyle" && (
        <>

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

      <div className="island-style-divider" />

      <h2 className="settings-section-title">
        {t(language, "sectionIslandStyle")}
      </h2>
      <p className="settings-section-desc">
        {t(language, "islandStyleDesc")}
      </p>
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
      <div className="settings-slider-row">
        <div className="settings-row-text">
          <span>{t(language, "islandSize")}</span>
          <span className="settings-row-desc">
            {t(language, "islandSizeDesc")}
          </span>
        </div>
        <div className="settings-slider-line">
          <input
            type="range"
            className="settings-slider"
            min={75}
            max={115}
            step={5}
            value={islandScalePct}
            aria-label={t(language, "islandSize")}
            onChange={(event) => updateIslandScale(Number(event.target.value))}
          />
          <span className="settings-slider-value">{islandScalePct}%</span>
          <button
            type="button"
            className="mini-btn"
            onClick={() => updateIslandScale(100)}
          >
            {t(language, "defaultLabel")}
          </button>
        </div>
      </div>
      <div className="settings-slider-row">
        <div className="settings-row-text">
          <span>{t(language, "islandGlow")}</span>
          <span className="settings-row-desc">
            {t(language, "islandGlowDesc")}
          </span>
        </div>
        <div className="settings-slider-line">
          <input
            type="range"
            className="settings-slider"
            min={0}
            max={100}
            step={5}
            value={islandGlowPct}
            aria-label={t(language, "islandGlow")}
            onChange={(event) => updateIslandGlow(Number(event.target.value))}
          />
          <span className="settings-slider-value">
            {islandGlowPct === 0 ? t(language, "glowOff") : `${islandGlowPct}%`}
          </span>
          <button
            type="button"
            className="mini-btn"
            onClick={() => updateIslandGlow(islandGlowPct === 0 ? 45 : 0)}
          >
            {islandGlowPct === 0 ? t(language, "defaultLabel") : t(language, "glowOff")}
          </button>
        </div>
      </div>
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
      <div className="settings-row">
        <span className="settings-row-text">
          <span>{t(language, "islandHotkey")}</span>
          <span className="settings-row-desc">
            {capturingHotkey
              ? t(language, "hotkeyCancelHint")
              : t(language, "islandHotkeyDesc")}
          </span>
        </span>
        <div className="settings-hotkey-control">
          <span className="settings-slider-value">
            {settings ? settings.island_hotkey : "—"}
          </span>
          <button
            type="button"
            className="mini-btn"
            disabled={!settings}
            onClick={() => {
              setHotkeyError(false);
              setCapturingHotkey((prev) => !prev);
            }}
          >
            {capturingHotkey
              ? t(language, "hotkeyListening")
              : t(language, "changeHotkey")}
          </button>
        </div>
      </div>
      {hotkeyError && (
        <p className="settings-error">{t(language, "hotkeyError")}</p>
      )}

      <div className="island-style-divider" />

      <h2 className="settings-section-title">
        {t(language, "sectionIslandWidgets")}
      </h2>
      <p className="settings-section-desc">
        {t(language, "islandWidgetsDesc")}
      </p>
      {(
        [
          ["island_show_start", "widgetStartBtn", "widgetStartBtnDesc"],
          ["island_show_apps", "widgetOpenApps", "widgetOpenAppsDesc"],
          ["island_show_stats", "widgetStats", "widgetStatsDesc"],
          ["island_show_gpu", "widgetGpuStat", "widgetGpuStatDesc"],
          ["island_show_media", "widgetMedia", "widgetMediaDesc"],
          ["island_show_clock", "widgetClock", "widgetClockDesc"],
          ["island_show_notifications", "widgetNotifications", "widgetNotificationsDesc"],
          ["island_show_engine_local", "widgetEngineLocal", "widgetEngineLocalDesc"],
          ["island_show_engine_online", "widgetEngineOnline", "widgetEngineOnlineDesc"],
          ["island_show_engine_dubbing", "widgetEngineDubbing", "widgetEngineDubbingDesc"],
        ] as const
      ).map(([key, labelKey, descKey]) => (
        <div className="settings-row" key={key}>
          <span className="settings-row-text">
            <span>{t(language, labelKey)}</span>
            <span className="settings-row-desc">{t(language, descKey)}</span>
          </span>
          <button
            type="button"
            className="mini-btn"
            disabled={!settings}
            onClick={() => toggleSetting(key)}
          >
            {settings ? onOff(settings[key] ?? true, language) : "—"}
          </button>
        </div>
      ))}
        </>
      )}
      {section === "clock" && (
        <>
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
        </>
      )}
      {section === "performance" && (
        <>
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
        </>
      )}
      {section === "liveTranslate" && (
        <>
      <h2 className="settings-section-title">
        {t(language, "sectionEngines")}
      </h2>
      <p className="settings-section-desc">
        {t(language, "enginesDesc")}
      </p>
      {downloadProgress && (
        <div className="translate-progress-box">
          <div className="translate-progress-label">
            <span>{downloadProgress.stage}</span>
            <span>{downloadProgress.percent}%</span>
          </div>
          <div className="translate-progress-track">
            <div
              className="translate-progress-fill"
              style={{ width: `${downloadProgress.percent}%` }}
            />
          </div>
        </div>
      )}
      {translateStatus?.last_error && (
        <div className="translate-error">{translateStatus.last_error}</div>
      )}
      <div className="engines-stack">
        <div className="engine-card">
          <button
            type="button"
            className="engine-card-toggle"
            aria-expanded={openEngineCard === "local"}
            onClick={() => setOpenEngineCard((c) => (c === "local" ? null : "local"))}
          >
            <div>
              <div className="engine-card-title">{t(language, "localEngineTitle")}</div>
              <div className="settings-row-desc">{t(language, "localEngineDesc")}</div>
            </div>
            <div className="engine-card-header-right">
              <span className="engine-version-badge">
                {translateStatus?.installed && translateStatus?.local_model_installed
                  ? t(language, "engineInstalled")
                  : "Offline"}
              </span>
              <span className={`engine-chevron${openEngineCard === "local" ? " open" : ""}`}>▾</span>
            </div>
          </button>
          {openEngineCard === "local" && (
            <div className="engine-card-body">
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "engineStatusLabel")}</span>
                  <span className="settings-row-desc">
                    {translateStatus?.installed
                      ? `${t(language, "engineInstalled")} (${translateStatus.model_size_mb} MB)`
                      : t(language, "engineNotInstalled")}
                  </span>
                </span>
                <div className="translate-btn-group">
                  {!translateStatus?.installed ? (
                    <button
                      type="button"
                      className="mini-btn primary-btn"
                      onClick={handleDownloadEngine}
                      disabled={translateStatus?.downloading || downloadProgress !== null}
                    >
                      {t(language, "downloadEngineBtn")}
                    </button>
                  ) : (
                    <>
                      <button
                        type="button"
                        className="mini-btn"
                        onClick={handleDownloadEngine}
                        disabled={translateStatus?.downloading || downloadProgress !== null}
                      >
                        {t(language, "updateEngineBtn")}
                      </button>
                      <button
                        type="button"
                        className="mini-btn danger-btn"
                        onClick={handleRemoveEngine}
                      >
                        {t(language, "removeEngineBtn")}
                      </button>
                    </>
                  )}
                </div>
              </div>
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "localModelLabel")}</span>
                  <span className="audio-mode-hint">
                    {translateStatus?.local_model_installed
                      ? t(language, "localModelInstalled")
                      : t(language, "localModelNotInstalled")}
                  </span>
                </span>
                <div className="translate-btn-group">
                  {!translateStatus?.local_model_installed ? (
                    <button
                      type="button"
                      className="mini-btn primary-btn"
                      onClick={handleDownloadLocalModel}
                      disabled={translateStatus?.downloading || downloadProgress !== null}
                    >
                      {t(language, "downloadLocalModelBtn")}
                    </button>
                  ) : (
                    <>
                      <button
                        type="button"
                        className="mini-btn"
                        onClick={handleDownloadLocalModel}
                        disabled={translateStatus?.downloading || downloadProgress !== null}
                      >
                        {t(language, "updateLocalModelBtn")}
                      </button>
                      <button
                        type="button"
                        className="mini-btn"
                        onClick={() => handleTestProvider("local")}
                        disabled={testingProvider}
                      >
                        {testingProvider
                          ? t(language, "testingConnectionBtn")
                          : t(language, "testConnectionBtn")}
                      </button>
                      <button
                        type="button"
                        className="mini-btn danger-btn"
                        onClick={handleRemoveLocalModel}
                      >
                        {t(language, "removeEngineBtn")}
                      </button>
                    </>
                  )}
                </div>
              </div>
              {apiKeyMsg && (
                <div className={apiKeyMsg.ok ? "translate-ok-box" : "translate-error"}>
                  {apiKeyMsg.text}
                </div>
              )}
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "whisperModelLabel")}</span>
                  <span className="audio-mode-hint">{t(language, "whisperModelDesc")}</span>
                </span>
                <select
                  className="translate-select"
                  value={activeWhisperModel}
                  onChange={(e) => persistTranslateSettings({ whisper_model: e.target.value })}
                >
                  <option value="ggml-tiny.en.bin">ggml-tiny.en (~74 MB, English fast)</option>
                  <option value="ggml-base.en.bin">ggml-base.en (~141 MB, English accurate)</option>
                  <option value="ggml-tiny.bin">ggml-tiny (~74 MB, Multilingual fast)</option>
                  <option value="ggml-large-v3-turbo-q5_0.bin">ggml-large-v3-turbo-q5_0 (~547 MB, Multilingual HQ)</option>
                </select>
              </div>
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "audioSourceLabel")}</span>
                  <span className="audio-mode-hint">{t(language, "audioSourceNoMicHint")}</span>
                </span>
                <div className="audio-mode-pills">
                  <button
                    type="button"
                    className={`audio-mode-pill${captureMode === "system" ? " active" : ""}`}
                    onClick={() => {
                      setCaptureMode("system");
                      try { window.localStorage.setItem("nexora-translate-capture-mode", "system"); } catch {}
                    }}
                  >
                    {t(language, "audioSourceSystem")}
                  </button>
                  <button
                    type="button"
                    className={`audio-mode-pill${captureMode === "app" ? " active" : ""}`}
                    onClick={() => {
                      setCaptureMode("app");
                      refreshAudioTargets();
                      try { window.localStorage.setItem("nexora-translate-capture-mode", "app"); } catch {}
                    }}
                  >
                    {t(language, "audioSourceApp")}
                  </button>
                </div>
              </div>
              {captureMode === "app" && (
                <div className="settings-row">
                  <span className="settings-row-text"><span>{t(language, "targetAppLabel")}</span></span>
                  <div className="translate-btn-group">
                    <select
                      className="translate-select"
                      value={targetPid ?? ""}
                      onChange={(e) => {
                        const pid = e.target.value ? Number(e.target.value) : null;
                        setTargetPid(pid);
                        const found = audioTargets.find((item) => item.pid === pid);
                        const label = found ? found.title || found.exe_name : "";
                        setTargetAppName(label);
                        try {
                          if (pid) window.localStorage.setItem("nexora-translate-target-pid", String(pid));
                          window.localStorage.setItem("nexora-translate-target-app", label);
                        } catch {}
                      }}
                    >
                      {audioTargets.length === 0 && (
                        <option value="">{t(language, "targetAppPlaceholder")}</option>
                      )}
                      {audioTargets.map((item) => (
                        <option key={item.pid} value={item.pid}>
                          {item.exe_name} — {item.title}
                        </option>
                      ))}
                    </select>
                    <button type="button" className="mini-btn" onClick={refreshAudioTargets}>
                      {t(language, "refreshAppsBtn")}
                    </button>
                  </div>
                </div>
              )}
              {translateStatus?.installed && (
                <div className="settings-row">
                  <span className="settings-row-text">
                    <span>{t(language, "localEngineTitle")}</span>
                  </span>
                  <button
                    type="button"
                    className={`mini-btn${translateStatus.running && (translateStatus.translate_provider === "local" || translateStatus.translate_provider === "none") ? " danger-btn" : " primary-btn"}`}
                    onClick={() => handleToggleEngineByMode("local")}
                  >
                    {translateStatus.running && (translateStatus.translate_provider === "local" || translateStatus.translate_provider === "none")
                      ? t(language, "stopLocalTranslateBtn")
                      : t(language, "startLocalTranslateBtn")}
                  </button>
                </div>
              )}
            </div>
          )}
        </div>

        <div className="engine-card">
          <button
            type="button"
            className="engine-card-toggle"
            aria-expanded={openEngineCard === "online"}
            onClick={() => setOpenEngineCard((c) => (c === "online" ? null : "online"))}
          >
            <div>
              <div className="engine-card-title">{t(language, "onlineEngineTitle")}</div>
              <div className="settings-row-desc">{t(language, "onlineEngineDesc")}</div>
            </div>
            <div className="engine-card-header-right">
              <span className="engine-version-badge">Cloud</span>
              <span className={`engine-chevron${openEngineCard === "online" ? " open" : ""}`}>▾</span>
            </div>
          </button>
          {openEngineCard === "online" && (
            <div className="engine-card-body">
              {!translateStatus?.installed && (
                <div className="settings-row">
                  <span className="settings-row-text">
                    <span>{t(language, "engineStatusLabel")}</span>
                    <span className="settings-row-desc">{t(language, "engineNotInstalled")}</span>
                  </span>
                  <button
                    type="button"
                    className="mini-btn primary-btn"
                    onClick={handleDownloadEngine}
                    disabled={translateStatus?.downloading || downloadProgress !== null}
                  >
                    {t(language, "downloadEngineBtn")}
                  </button>
                </div>
              )}
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "onlineTranslationLabel")}</span>
                  <span className="audio-mode-hint">{t(language, "onlineTranslationDesc")}</span>
                </span>
                <select
                  className="translate-select"
                  value={onlineProviderChoice}
                  onChange={(e) => {
                    const nextProv = e.target.value;
                    setOnlineProviderChoice(nextProv);
                    setApiKeyMsg(null);
                    try {
                      window.localStorage.setItem("nexora-online-provider", nextProv);
                    } catch {}
                    persistTranslateSettings({
                      translate_provider: nextProv,
                      online_translation: true,
                    });
                  }}
                >
                  <option value="google_free">{t(language, "providerGoogleFree")}</option>
                  <option value="google_cloud">{t(language, "providerGoogleCloud")}</option>
                  <option value="gemini">{t(language, "providerGemini")}</option>
                </select>
              </div>
              {onlineProviderChoice === "gemini" && (
                <div className="translate-notice-box">
                  {t(language, "geminiFreeTierNotice")}
                </div>
              )}
              {onlineProviderChoice === "google_free" && (
                <div className="settings-row">
                  <span className="settings-row-text">
                    <span>{t(language, "providerGoogleFree")}</span>
                  </span>
                  <button
                    type="button"
                    className="mini-btn"
                    onClick={() => handleTestProvider("google_free")}
                    disabled={testingProvider}
                  >
                    {testingProvider
                      ? t(language, "testingConnectionBtn")
                      : t(language, "testConnectionBtn")}
                  </button>
                </div>
              )}
              {(onlineProviderChoice === "google_cloud" || onlineProviderChoice === "gemini") && (
                <div className="settings-row">
                  <span className="settings-row-text">
                    <span>{t(language, "apiKeyLabel")}</span>
                    <span className="audio-mode-hint">
                      {(onlineProviderChoice === "google_cloud" ? apiKeyStatus.google_cloud : apiKeyStatus.gemini)
                        ? t(language, "apiKeySaved")
                        : t(language, "apiKeyMissing")}
                    </span>
                  </span>
                  <div className="translate-btn-group">
                    <input
                      type="password"
                      className="translate-input"
                      placeholder={t(language, "apiKeyPlaceholder")}
                      value={apiKeyInput}
                      onChange={(e) => setApiKeyInput(e.target.value)}
                      autoComplete="off"
                    />
                    <button
                      type="button"
                      className="mini-btn primary-btn"
                      onClick={() => handleSaveApiKey(onlineProviderChoice)}
                      disabled={!apiKeyInput.trim()}
                    >
                      {t(language, "saveApiKeyBtn")}
                    </button>
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={() => handleTestProvider(onlineProviderChoice)}
                      disabled={testingProvider}
                    >
                      {testingProvider
                        ? t(language, "testingConnectionBtn")
                        : t(language, "testConnectionBtn")}
                    </button>
                    {(onlineProviderChoice === "google_cloud" ? apiKeyStatus.google_cloud : apiKeyStatus.gemini) && (
                      <button
                        type="button"
                        className="mini-btn danger-btn"
                        onClick={() => handleDeleteApiKey(onlineProviderChoice)}
                      >
                        {t(language, "deleteApiKeyBtn")}
                      </button>
                    )}
                  </div>
                </div>
              )}
              {apiKeyMsg && (
                <div className={apiKeyMsg.ok ? "translate-ok-box" : "translate-error"}>
                  {apiKeyMsg.text}
                </div>
              )}
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "sourceLangLabel")}</span>
                  <span className="audio-mode-hint">{t(language, "sourceLangHint")}</span>
                </span>
                <select
                  className="translate-select"
                  value={srcLang}
                  onChange={(e) => {
                    setSrcLang(e.target.value);
                    persistTranslateSettings({ translate_source_lang: e.target.value });
                  }}
                >
                  <option value="en">English (recommended)</option>
                  <option value="fa">فارسی (Persian)</option>
                  <option value="ja">Japanese</option>
                  <option value="ru">Russian</option>
                  <option value="de">German</option>
                  <option value="fr">French</option>
                  <option value="auto">Auto Detect (slower)</option>
                </select>
              </div>
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "targetLangLabel")}</span>
                </span>
                <select
                  className="translate-select"
                  value={tgtLang}
                  onChange={(e) => {
                    setTgtLang(e.target.value);
                    persistTranslateSettings({ translate_target_lang: e.target.value });
                  }}
                >
                  <option value="fa">فارسی (Persian)</option>
                  <option value="en">English</option>
                  <option value="ar">العربية (Arabic)</option>
                  <option value="tr">Türkçe (Turkish)</option>
                </select>
              </div>
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "testSubtitleLabel")}</span>
                </span>
                <div className="translate-btn-group">
                  <input
                    type="text"
                    className="translate-input"
                    value={testSubText}
                    onChange={(e) => setTestSubText(e.target.value)}
                  />
                  <button type="button" className="mini-btn" onClick={handleTestSubtitle}>
                    {t(language, "testSubtitleBtn")}
                  </button>
                </div>
              </div>
              {translateStatus?.installed && (
                <div className="settings-row">
                  <span className="settings-row-text">
                    <span>{t(language, "onlineEngineTitle")}</span>
                  </span>
                  <button
                    type="button"
                    className={`mini-btn${translateStatus.running && translateStatus.translate_provider !== "local" && translateStatus.translate_provider !== "none" ? " danger-btn" : " primary-btn"}`}
                    onClick={() => handleToggleEngineByMode("online")}
                  >
                    {translateStatus.running && translateStatus.translate_provider !== "local" && translateStatus.translate_provider !== "none"
                      ? t(language, "stopOnlineTranslateBtn")
                      : t(language, "startOnlineTranslateBtn")}
                  </button>
                </div>
              )}
            </div>
          )}
        </div>

        <div className="engine-card">
          <button
            type="button"
            className="engine-card-toggle"
            aria-expanded={openEngineCard === "dubbing"}
            onClick={() => setOpenEngineCard((c) => (c === "dubbing" ? null : "dubbing"))}
          >
            <div>
              <div className="engine-card-title">{t(language, "dubbingEngineTitle")}</div>
              <div className="settings-row-desc">{t(language, "dubbingEngineDesc")}</div>
            </div>
            <span className="engine-card-chevron">{openEngineCard === "dubbing" ? "▾" : "▸"}</span>
          </button>
          {openEngineCard === "dubbing" && (
            <div className="engine-card-body">
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "dubbingVoiceLabel")}</span>
                  <span className="audio-mode-hint">
                    {dubbingStatus?.piper_installed && (dubVoiceId === "clone_male" ? (dubbingStatus.installed_voices?.length ?? 0) > 0 : dubbingStatus.installed_voices?.includes(dubVoiceId))
                      ? t(language, "dubbingInstalled")
                      : t(language, "dubbingNotInstalled")}
                  </span>
                </span>
                <select
                  className="translate-select"
                  value={dubVoiceId}
                  onChange={(e) => setDubVoiceId(e.target.value)}
                >
                  <option value="fa_IR-amir-medium">Amir (Persian Natural, 22 kHz)</option>
                  <option value="fa_IR-gyro-medium">Gyro (Persian Expressive, 22 kHz)</option>
                  {dubbingStatus?.male_clone_f0_hz && (
                    <option value="clone_male">{t(language, "voiceCloneMaleOption")}</option>
                  )}
                </select>
              </div>
              {!translateStatus?.installed && (
                <div className="audio-mode-hint">{t(language, "dubbingNeedsWhisper")}</div>
              )}
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "dubbingAutoGenderLabel")}</span>
                  <span className="audio-mode-hint">{t(language, "dubbingAutoGenderDesc")}</span>
                </span>
                <input
                  type="checkbox"
                  checked={dubAutoGender}
                  onChange={(e) => setDubAutoGender(e.target.checked)}
                />
              </div>
              {dubAutoGender && (
                <div className="settings-row">
                  <span>{t(language, "dubbingFemaleVoiceLabel")}</span>
                  <select
                    className="translate-select"
                    value={dubFemaleVoiceId}
                    onChange={(e) => setDubFemaleVoiceId(e.target.value)}
                  >
                    <option value="auto_female">{t(language, "dubbingFemaleAutoOption")}</option>
                    {dubbingStatus?.female_clone_f0_hz && (
                      <option value="clone_female">{t(language, "voiceCloneFemaleOption")}</option>
                    )}
                    <option value="fa_IR-gyro-medium">Gyro (Persian Expressive, 22 kHz)</option>
                    <option value="fa_IR-amir-medium">Amir (Persian Natural, 22 kHz)</option>
                  </select>
                </div>
              )}
              {dubbingStatus?.running && dubbingStatus.last_detected_gender !== "unknown" && (
                <div className="audio-mode-hint">
                  {t(language, "dubbingLastGenderLabel")}:{" "}
                  <strong>{dubbingStatus.last_detected_gender === "female" ? "Female / زن" : "Male / مرد"}</strong>
                </div>
              )}
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "audioSourceLabel")}</span>
                  <span className="audio-mode-hint">{t(language, "audioSourceNoMicHint")}</span>
                </span>
                <div className="audio-mode-pills">
                  <button
                    type="button"
                    className={`audio-mode-pill${captureMode === "system" ? " active" : ""}`}
                    onClick={() => {
                      setCaptureMode("system");
                      try { window.localStorage.setItem("nexora-translate-capture-mode", "system"); } catch {}
                    }}
                  >
                    {t(language, "audioSourceSystem")}
                  </button>
                  <button
                    type="button"
                    className={`audio-mode-pill${captureMode === "app" ? " active" : ""}`}
                    onClick={() => {
                      setCaptureMode("app");
                      refreshAudioTargets();
                      try { window.localStorage.setItem("nexora-translate-capture-mode", "app"); } catch {}
                    }}
                  >
                    {t(language, "audioSourceApp")}
                  </button>
                </div>
              </div>
              {captureMode === "app" && (
                <div className="settings-row">
                  <span className="settings-row-text"><span>{t(language, "targetAppLabel")}</span></span>
                  <div className="translate-btn-group">
                    <select
                      className="translate-select"
                      value={targetPid ?? ""}
                      onChange={(e) => {
                        const pid = e.target.value ? Number(e.target.value) : null;
                        setTargetPid(pid);
                        const found = audioTargets.find((item) => item.pid === pid);
                        const label = found ? found.title || found.exe_name : "";
                        setTargetAppName(label);
                        try {
                          if (pid) window.localStorage.setItem("nexora-translate-target-pid", String(pid));
                          window.localStorage.setItem("nexora-translate-target-app", label);
                        } catch {}
                      }}
                    >
                      {audioTargets.length === 0 && (
                        <option value="">{t(language, "targetAppPlaceholder")}</option>
                      )}
                      {audioTargets.map((item) => (
                        <option key={item.pid} value={item.pid}>
                          {item.exe_name} — {item.title}
                        </option>
                      ))}
                    </select>
                    <button type="button" className="mini-btn" onClick={refreshAudioTargets}>
                      {t(language, "refreshAppsBtn")}
                    </button>
                  </div>
                </div>
              )}
              <div className="settings-row">
                <span>{t(language, "dubbingDuckLabel")} ({dubDuckPercent}%)</span>
                <input
                  type="range"
                  min={5}
                  max={100}
                  step={5}
                  value={dubDuckPercent}
                  onChange={(e) => setDubDuckPercent(Number(e.target.value))}
                />
              </div>
              <div className="settings-row">
                <span>{t(language, "dubbingRateLabel")} ({(dubRatePct / 100).toFixed(2)}x)</span>
                <input
                  type="range"
                  min={75}
                  max={140}
                  step={5}
                  value={dubRatePct}
                  onChange={(e) => setDubRatePct(Number(e.target.value))}
                />
              </div>
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "voiceCloneTitle")}</span>
                  <span className="audio-mode-hint">{t(language, "voiceCloneDesc")}</span>
                </span>
              </div>
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "voiceCloneMaleLabel")}</span>
                  <span className="audio-mode-hint">
                    {dubbingStatus?.male_clone_f0_hz
                      ? `${t(language, "voiceCloneReadyHint")} (${Math.round(dubbingStatus.male_clone_f0_hz)} Hz)`
                      : t(language, "voiceCloneNotRecorded")}
                  </span>
                </span>
                <div className="translate-btn-group">
                  <button
                    type="button"
                    className="mini-btn primary-btn"
                    disabled={recordingCloneSlot !== null}
                    onClick={() => {
                      setRecordingCloneSlot("male");
                      invoke<DubbingStatus>("record_voice_clone_sample", { slot: "male", durationSecs: 6 })
                        .then((st) => {
                          setDubbingStatus(st);
                          setDubVoiceId("clone_male");
                        })
                        .catch(() => {})
                        .finally(() => setRecordingCloneSlot(null));
                    }}
                  >
                    {recordingCloneSlot === "male"
                      ? t(language, "voiceCloneRecordingBtn")
                      : t(language, "voiceCloneRecordBtn")}
                  </button>
                  {dubbingStatus?.male_clone_f0_hz && dubbingStatus.ready && (
                    <button
                      type="button"
                      className="mini-btn"
                      disabled={dubTesting}
                      onClick={() => {
                        setDubTesting(true);
                        invoke<string>("test_dubbing_voice", {
                          voiceId: "clone_male",
                          speechRatePct: dubRatePct,
                        }).finally(() => setDubTesting(false));
                      }}
                    >
                      {t(language, "testDubbingBtn")}
                    </button>
                  )}
                  {dubbingStatus?.male_clone_f0_hz && (
                    <button
                      type="button"
                      className="mini-btn danger-btn"
                      onClick={() => {
                        invoke<DubbingStatus>("delete_voice_clone_sample", { slot: "male" })
                          .then((st) => {
                            setDubbingStatus(st);
                            if (dubVoiceId === "clone_male") setDubVoiceId("fa_IR-amir-medium");
                          })
                          .catch(() => {});
                      }}
                    >
                      {t(language, "subtitleCssDeletePresetBtn")}
                    </button>
                  )}
                </div>
              </div>
              <div className="settings-row">
                <span className="settings-row-text">
                  <span>{t(language, "voiceCloneFemaleLabel")}</span>
                  <span className="audio-mode-hint">
                    {dubbingStatus?.female_clone_f0_hz
                      ? `${t(language, "voiceCloneReadyHint")} (${Math.round(dubbingStatus.female_clone_f0_hz)} Hz)`
                      : t(language, "voiceCloneNotRecorded")}
                  </span>
                </span>
                <div className="translate-btn-group">
                  <button
                    type="button"
                    className="mini-btn primary-btn"
                    disabled={recordingCloneSlot !== null}
                    onClick={() => {
                      setRecordingCloneSlot("female");
                      invoke<DubbingStatus>("record_voice_clone_sample", { slot: "female", durationSecs: 6 })
                        .then((st) => {
                          setDubbingStatus(st);
                          setDubFemaleVoiceId("clone_female");
                        })
                        .catch(() => {})
                        .finally(() => setRecordingCloneSlot(null));
                    }}
                  >
                    {recordingCloneSlot === "female"
                      ? t(language, "voiceCloneRecordingBtn")
                      : t(language, "voiceCloneRecordBtn")}
                  </button>
                  {dubbingStatus?.female_clone_f0_hz && dubbingStatus.ready && (
                    <button
                      type="button"
                      className="mini-btn"
                      disabled={dubTesting}
                      onClick={() => {
                        setDubTesting(true);
                        invoke<string>("test_dubbing_voice", {
                          voiceId: "clone_female",
                          speechRatePct: dubRatePct,
                        }).finally(() => setDubTesting(false));
                      }}
                    >
                      {t(language, "testDubbingBtn")}
                    </button>
                  )}
                  {dubbingStatus?.female_clone_f0_hz && (
                    <button
                      type="button"
                      className="mini-btn danger-btn"
                      onClick={() => {
                        invoke<DubbingStatus>("delete_voice_clone_sample", { slot: "female" })
                          .then((st) => {
                            setDubbingStatus(st);
                            if (dubFemaleVoiceId === "clone_female") setDubFemaleVoiceId("auto_female");
                          })
                          .catch(() => {});
                      }}
                    >
                      {t(language, "subtitleCssDeletePresetBtn")}
                    </button>
                  )}
                </div>
              </div>
              <div className="settings-row">
                <div className="translate-btn-group">
                  <button
                    type="button"
                    className="mini-btn primary-btn"
                    disabled={dubbingStatus?.downloading}
                    onClick={() => {
                      setDubbingProgress({ percent: 2, stage: "Starting..." });
                      invoke<DubbingStatus>("download_dubbing_engine", { voiceId: dubVoiceId })
                        .then(setDubbingStatus)
                        .catch(() => {});
                    }}
                  >
                    {t(language, "downloadDubbingBtn")}
                  </button>
                  {dubbingStatus?.ready && (dubVoiceId === "clone_male" || dubbingStatus.installed_voices?.includes(dubVoiceId)) && (
                    <>
                      <button
                        type="button"
                        className="mini-btn"
                        disabled={dubTesting}
                        onClick={() => {
                          setDubTesting(true);
                          invoke<string>("test_dubbing_voice", {
                            voiceId: dubVoiceId,
                            speechRatePct: dubRatePct,
                          })
                            .finally(() => setDubTesting(false));
                        }}
                      >
                        {t(language, "testDubbingBtn")}
                      </button>
                      <button
                        type="button"
                        className={`mini-btn${dubbingStatus.running ? " danger-btn" : " primary-btn"}`}
                        onClick={() => {
                          if (dubbingStatus.running) {
                            invoke<DubbingStatus>("stop_live_dubbing")
                              .then(setDubbingStatus)
                              .catch(() => {});
                          } else {
                            invoke<DubbingStatus>("start_live_dubbing", {
                              captureMode,
                              targetPid: captureMode === "app" ? targetPid : null,
                              targetAppName: captureMode === "app" ? targetAppName : null,
                              voiceId: dubVoiceId,
                              duckPercent: dubDuckPercent,
                              speechRatePct: dubRatePct,
                              autoGenderVoice: dubAutoGender,
                              femaleVoiceId: dubFemaleVoiceId,
                              translateProvider: settings?.translate_provider ?? "local",
                            })
                              .then(setDubbingStatus)
                              .catch(() => {});
                          }
                        }}
                      >
                        {dubbingStatus.running ? t(language, "stopDubbingBtn") : t(language, "startDubbingBtn")}
                      </button>
                    </>
                  )}
                  {(dubbingStatus?.piper_installed || dubbingStatus?.voice_installed) && (
                    <button
                      type="button"
                      className="mini-btn danger-btn"
                      onClick={() => {
                        invoke<DubbingStatus>("remove_dubbing_engine")
                          .then(setDubbingStatus)
                          .catch(() => {});
                      }}
                    >
                      {t(language, "removeDubbingBtn")}
                    </button>
                  )}
                </div>
              </div>
              {dubbingStatus?.downloading && dubbingProgress && (
                <div className="translate-progress-box">
                  <div className="translate-progress-label">
                    <span>{dubbingProgress.stage}</span>
                    <span>{dubbingProgress.percent}%</span>
                  </div>
                  <div className="translate-progress-track">
                    <div
                      className="translate-progress-fill"
                      style={{ width: `${dubbingProgress.percent}%` }}
                    />
                  </div>
                </div>
              )}
              {dubbingStatus?.last_error && (
                <div className="translate-error">{dubbingStatus.last_error}</div>
              )}
            </div>
          )}
        </div>

        <div className="engine-card">
          <div className="engine-card-body">
            <div className="settings-row">
              <span className="settings-row-text">
                <span>{t(language, "subtitleCustomCssLabel")}</span>
                <span className="audio-mode-hint">{t(language, "subtitleCustomCssDesc")}</span>
              </span>
              <div className="translate-btn-group">
                <button
                  type="button"
                  className="mini-btn"
                  onClick={() => {
                    setSubtitleCssDraft("");
                    saveSubtitleCssNow("");
                  }}
                >
                  {t(language, "subtitleCustomCssReset")}
                </button>
                <button
                  type="button"
                  className="mini-btn primary-btn"
                  onClick={() => {
                    saveSubtitleCssNow(subtitleCssDraft);
                    handleTestSubtitle();
                  }}
                >
                  {t(language, "testSubtitleBtn")}
                </button>
              </div>
            </div>
            <div className="settings-row">
              <span>{t(language, "subtitleCssPresetLabel")}</span>
              <div className="translate-btn-group">
                <select
                  className="translate-select"
                  value={activeCssPreset}
                  onChange={(e) => handleSelectCssPreset(e.target.value)}
                >
                  <option value="">{t(language, "subtitleCssPresetDefault")}</option>
                  {cssPresets.map((p) => (
                    <option key={p.name} value={p.name}>
                      {p.name}
                    </option>
                  ))}
                </select>
                {activeCssPreset && (
                  <button
                    type="button"
                    className="mini-btn danger-btn"
                    onClick={handleDeleteCssPreset}
                  >
                    {t(language, "subtitleCssDeletePresetBtn")}
                  </button>
                )}
              </div>
            </div>
            <textarea
              className="subtitle-css-textarea"
              value={subtitleCssDraft}
              placeholder={t(language, "subtitleCustomCssPlaceholder")}
              onChange={(e) => handleSubtitleCssChange(e.target.value)}
            />
            <div className="subtitle-preset-bar">
              <input
                type="text"
                className="subtitle-preset-input"
                value={presetNameInput}
                placeholder={t(language, "subtitleCssPresetNamePlaceholder")}
                onChange={(e) => setPresetNameInput(e.target.value)}
              />
              <button
                type="button"
                className="mini-btn primary-btn"
                disabled={!presetNameInput.trim()}
                onClick={handleSaveCssPreset}
              >
                {t(language, "subtitleCssSavePresetBtn")}
              </button>
            </div>
          </div>
        </div>
      </div>
        </>
      )}
      {section === "updates" && (
        <>
      <h2 className="settings-section-title">
        {t(language, "sectionUpdates")}
      </h2>
      <p className="settings-section-desc">
        {t(language, "updatesDesc")}
      </p>
      <div className="settings-row">
        <span>{t(language, "autoUpdateLabel")}</span>
        <input
          type="checkbox"
          checked={autoUpdate}
          onChange={(e) => {
            const on = e.target.checked;
            setAutoUpdate(on);
            try {
              window.localStorage.setItem("nexora-auto-update", on ? "1" : "0");
            } catch {
              // ignore
            }
          }}
        />
      </div>
      <div className="settings-row">
        <span>
          {updateInfo
            ? updateInfo.update_available
              ? `${t(language, "updateAvailable")}${
                  updateInfo.available_version ? ` (v${updateInfo.available_version})` : ""
                }`
              : t(language, "noUpdateAvailable")
            : "—"}
        </span>
        <div className="translate-btn-group">
          <button
            type="button"
            className="mini-btn"
            onClick={checkForUpdates}
            disabled={checkingUpdate || installingUpdate}
          >
            {checkingUpdate ? t(language, "checkingUpdates") : t(language, "checkForUpdates")}
          </button>
          {updateInfo?.update_available && (
            <button
              type="button"
              className="mini-btn primary-btn"
              onClick={installAvailableUpdate}
              disabled={installingUpdate}
            >
              {installingUpdate
                ? `${t(language, "installingUpdate")}${
                    updateProgressPct !== null ? ` ${updateProgressPct}%` : ""
                  }`
                : t(language, "downloadAndInstall")}
            </button>
          )}
        </div>
      </div>
      {installingUpdate && updateProgressPct !== null && (
        <div className="translate-progress-box">
          <div className="translate-progress-label">
            <span>{t(language, "installingUpdate")}</span>
            <span>{updateProgressPct}%</span>
          </div>
          <div className="translate-progress-track">
            <div
              className="translate-progress-fill"
              style={{ width: `${updateProgressPct}%` }}
            />
          </div>
        </div>
      )}
      {updateError && <div className="translate-error">{updateError}</div>}
      <div className="settings-row">
        <span className="settings-version">
          Nexora v{updateInfo ? updateInfo.current_version : "0.2.5"}
        </span>
        <button
          type="button"
          className="mini-btn"
          onClick={() => setShowWhatsNew(true)}
        >
          ✨ {t(language, "whatsNewBtn")}
        </button>
      </div>
        </>
      )}
      </div>
      </div>
      {showWhatsNew && (() => {
        const hasPendingUpdate = Boolean(
          updateInfo?.update_available && updateInfo.available_version,
        );
        const release = latestReleaseNote(updateInfo?.current_version);
        const remoteLines = (updateInfo?.notes ?? "")
          .split(/\r?\n/)
          .map((l) => l.trim())
          .filter((l) => l.length > 0 && !l.startsWith("#"));
        const cleanRemoteBullets = remoteLines
          .map((l) => l.replace(/^[-*•]\s+/, "").replace(/\*\*/g, ""))
          .filter(Boolean);
        return (
          <div
            className="whats-new-backdrop"
            onClick={() => setShowWhatsNew(false)}
          >
            <div
              className="whats-new-modal"
              dir={language === "fa" ? "rtl" : "ltr"}
              onClick={(e) => e.stopPropagation()}
            >
              <div className="whats-new-header">
                <div>
                  <div className="whats-new-kicker">
                    {hasPendingUpdate
                      ? t(language, "whatsNewAvailableTitle")
                      : t(language, "whatsNewTitle")}
                  </div>
                  <h3 className="whats-new-title">
                    {hasPendingUpdate
                      ? `Nexora v${updateInfo?.available_version}`
                      : release.title[language]}
                  </h3>
                </div>
                <span className="whats-new-badge">
                  v{hasPendingUpdate ? updateInfo?.available_version : release.version}
                </span>
              </div>
              <div className="whats-new-body">
                {hasPendingUpdate ? (
                  <div className="whats-new-group">
                    <div className="whats-new-group-title features">
                      ✦ {t(language, "whatsNewFeatures")}
                    </div>
                    <ul className="whats-new-list">
                      {(cleanRemoteBullets.length > 0
                        ? cleanRemoteBullets
                        : [`Nexora v${updateInfo?.available_version}`]
                      ).map((item, idx) => (
                        <li key={idx}>{item}</li>
                      ))}
                    </ul>
                  </div>
                ) : (
                  <>
                    <div className="whats-new-group">
                      <div className="whats-new-group-title features">
                        ✦ {t(language, "whatsNewFeatures")}
                      </div>
                      <ul className="whats-new-list">
                        {release.features[language].map((item, idx) => (
                          <li key={idx}>{item}</li>
                        ))}
                      </ul>
                    </div>
                    <div className="whats-new-group">
                      <div className="whats-new-group-title fixes">
                        ✓ {t(language, "whatsNewFixes")}
                      </div>
                      <ul className="whats-new-list">
                        {release.fixes[language].map((item, idx) => (
                          <li key={idx}>{item}</li>
                        ))}
                      </ul>
                    </div>
                  </>
                )}
              </div>
              <div className="whats-new-footer">
                <span className="whats-new-date">
                  {hasPendingUpdate
                    ? `v${updateInfo?.current_version} → v${updateInfo?.available_version}`
                    : release.date}
                </span>
                <div className="translate-btn-group">
                  {hasPendingUpdate && (
                    <button
                      type="button"
                      className="mini-btn primary-btn"
                      disabled={installingUpdate}
                      onClick={() => {
                        setShowWhatsNew(false);
                        installAvailableUpdate();
                      }}
                    >
                      {t(language, "downloadAndInstall")}
                    </button>
                  )}
                  <button
                    type="button"
                    className={`mini-btn${hasPendingUpdate ? "" : " primary-btn"}`}
                    onClick={() => setShowWhatsNew(false)}
                  >
                    {t(language, "whatsNewGotIt")}
                  </button>
                </div>
              </div>
            </div>
          </div>
        );
      })()}
    </main>
  );
}

function normSubWord(w: string): string {
  return w.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "").toLowerCase();
}

function commonWordPrefixLen(a: string[], b: string[]): number {
  const n = Math.min(a.length, b.length);
  let i = 0;
  while (i < n && normSubWord(a[i]) === normSubWord(b[i]) && normSubWord(a[i]) !== "") {
    i += 1;
  }
  return i;
}

function wordOverlapRatio(a: string[], b: string[]): number {
  if (a.length === 0 || b.length === 0) return 0;
  const setA = new Set(a.map(normSubWord).filter(Boolean));
  if (setA.size === 0) return 0;
  let hits = 0;
  for (const w of b) {
    const nw = normSubWord(w);
    if (nw && setA.has(nw)) hits += 1;
  }
  return hits / Math.max(1, Math.min(a.length, b.length));
}

function SubtitleOverlay() {
  const SUBTITLE_SILENCE_FADE_MS = 4000;
  const FADE_ANIM_MS = 450;

  const [payload, setPayload] = useState<SubtitlePayload>({
    original: "",
    translated: "",
    target_lang: "fa",
    active: false,
    translation_status: "ok",
  });
  const [fadingOut, setFadingOut] = useState(false);
  const [customCss, setCustomCss] = useState("");
  const [prevLine, setPrevLine] = useState("");
  const [activeWords, setActiveWords] = useState<string[]>([]);
  const [activeRevealed, setActiveRevealed] = useState(0);
  const [activeSettled, setActiveSettled] = useState(0);

  const prevWordsRef = useRef<string[]>([]);
  const activeRevealedRef = useRef(0);
  const lastActiveTextRef = useRef("");
  const fadingRef = useRef(false);
  const silenceTimerRef = useRef<number | undefined>(undefined);
  const hideTimerRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    invoke<NexoraSettings>("get_settings")
      .then((s) => setCustomCss(s.subtitle_custom_css ?? ""))
      .catch(() => {});
    let unlistenSet: (() => void) | undefined;
    listen<NexoraSettings>("settings-changed", (ev) => {
      setCustomCss(ev.payload.subtitle_custom_css ?? "");
    })
      .then((fn) => {
        unlistenSet = fn;
      })
      .catch(() => {});
    return () => {
      unlistenSet?.();
    };
  }, []);

  useEffect(() => {
    const clearTimers = () => {
      if (silenceTimerRef.current !== undefined) {
        window.clearTimeout(silenceTimerRef.current);
        silenceTimerRef.current = undefined;
      }
      if (hideTimerRef.current !== undefined) {
        window.clearTimeout(hideTimerRef.current);
        hideTimerRef.current = undefined;
      }
    };

    const resetOverlay = () => {
      lastActiveTextRef.current = "";
      prevWordsRef.current = [];
      activeRevealedRef.current = 0;
      setPrevLine("");
      setActiveWords([]);
      setActiveRevealed(0);
      setActiveSettled(0);
      fadingRef.current = false;
      setFadingOut(false);
      setPayload((cur) => ({ ...cur, original: "", translated: "", active: false }));
    };

    const triggerSmoothFadeOut = () => {
      clearTimers();
      if (!lastActiveTextRef.current) {
        resetOverlay();
        return;
      }
      fadingRef.current = true;
      setFadingOut(true);
      hideTimerRef.current = window.setTimeout(resetOverlay, FADE_ANIM_MS);
    };

    let unlisten: (() => void) | undefined;
    listen<SubtitlePayload>("subtitle-update", (ev) => {
      const next = ev.payload;
      if (!next.active || (!next.original && !next.translated)) {
        triggerSmoothFadeOut();
        return;
      }

      // Game/stream style: show clean translated line (or original only when no translation exists yet)
      const displayText = (next.translated || next.original).trim();
      if (!displayText) {
        return;
      }

      const nextWords = displayText.split(/\s+/);
      const prefix = commonWordPrefixLen(prevWordsRef.current, nextWords);
      const sameSentence =
        prevWordsRef.current.length > 0 &&
        (prefix > 0 || wordOverlapRatio(prevWordsRef.current, nextWords) >= 0.38);

      // When a genuinely new sentence starts, promote the previous finalized sentence
      // into the dimmer top history pill (.subtitle-prev), just like broadcast/game subtitles.
      if (!sameSentence && lastActiveTextRef.current && lastActiveTextRef.current !== displayText) {
        setPrevLine(lastActiveTextRef.current);
      }

      const settled = sameSentence
        ? Math.min(Math.max(activeRevealedRef.current, prefix, prevWordsRef.current.length), nextWords.length)
        : 0;
      const nextRev = sameSentence ? settled : Math.min(1, nextWords.length);

      prevWordsRef.current = nextWords;
      activeRevealedRef.current = nextRev;
      setActiveWords(nextWords);
      setActiveSettled(settled);
      setActiveRevealed(nextRev);
      setPayload(next);

      if (displayText !== lastActiveTextRef.current || fadingRef.current) {
        lastActiveTextRef.current = displayText;
        clearTimers();
        fadingRef.current = false;
        setFadingOut(false);
        silenceTimerRef.current = window.setTimeout(
          triggerSmoothFadeOut,
          SUBTITLE_SILENCE_FADE_MS,
        );
      }
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      clearTimers();
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (activeRevealed >= activeWords.length) {
      return;
    }
    const id = window.setInterval(() => {
      if (activeRevealedRef.current < activeWords.length) {
        const step = activeWords.length - activeRevealedRef.current > 6 ? 2 : 1;
        activeRevealedRef.current = Math.min(activeWords.length, activeRevealedRef.current + step);
        setActiveRevealed(activeRevealedRef.current);
      }
    }, 38);
    return () => window.clearInterval(id);
  }, [activeWords.length, activeRevealed]);

  if (!payload.active || activeWords.length === 0) {
    return customCss ? <style id="nexora-subtitle-custom-css">{customCss}</style> : null;
  }

  const isRtl = payload.target_lang === "fa" || payload.target_lang === "ar";
  const uiLang: Language = isRtl ? "fa" : "en";
  const status = payload.translation_status ?? "ok";
  const shownWords = activeWords.slice(0, activeRevealed);

  return (
    <div className="subtitle-overlay-root" dir={isRtl ? "rtl" : "ltr"}>
      {customCss && <style id="nexora-subtitle-custom-css">{customCss}</style>}
      <div className={`subtitle-stack${fadingOut ? " subtitle-stack--fading" : ""}`}>
        {prevLine && (
          <div key={prevLine} className="subtitle-prev">
            {prevLine}
          </div>
        )}
        <div className="subtitle-box">
          <div className="subtitle-translated">
            {shownWords.map((w, idx) => (
              <span
                key={`w-${idx}`}
                className={idx >= activeSettled ? "sub-word sub-word--new" : "sub-word"}
              >
                {idx > 0 ? " " : ""}
                {w}
              </span>
            ))}
          </div>
          {status === "offline" && (
            <div className="subtitle-status-badge offline">
              {t(uiLang, "subtitleStatusOffline")}
            </div>
          )}
          {status === "failed" && (
            <div className="subtitle-status-badge failed">
              {t(uiLang, "subtitleStatusFailed")}
            </div>
          )}
        </div>
      </div>
    </div>
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
  if (windowLabel === "settings") {
    return <SettingsView />;
  }
  if (windowLabel === "subtitle") {
    return <SubtitleOverlay />;
  }
  return <IslandApp />;
}

export default App;
