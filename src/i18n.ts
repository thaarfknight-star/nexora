/**
 * English / Persian strings for the whole app (island, panel, Settings).
 *
 * The backend owns the `language` field in `NexoraSettings` ("en" by
 * default, normalized to en/fa); the frontend reads it from
 * `get_settings` and the `settings-changed` event. When Persian is
 * active the whole UI is RTL (`dir="rtl"` on <html>); the CSS uses
 * logical properties where the reading direction matters.
 *
 * Theme names stay English (proper nouns); theme descriptions are
 * translated here. Backend-provided values (media title/artist,
 * source app, playback status, the "Nexora Island" title) are never
 * translated. Numbers and units (KB/s, MB/s, %) stay Latin.
 */
import type { ThemeId } from "./themes";

export type Language = "en" | "fa";

export function normalizeLanguage(
  value: string | null | undefined,
): Language {
  return value === "fa" ? "fa" : "en";
}

/** Apply the language to the document: reading direction + lang. */
export function applyLanguageToDocument(lang: Language): void {
  document.documentElement.dir = lang === "fa" ? "rtl" : "ltr";
  document.documentElement.lang = lang;
}

const EN = {
  tabMedia: "Media",
  tabStats: "Stats",
  tabQuick: "Quick settings",
  onboardingWelcomeTitle: "Welcome to Nexora",
  onboardingWelcomeText:
    "A small island at the top of your screen replaces your taskbar — your apps, music, stats and clock, all in one place.",
  onboardingGetStarted: "Get started →",
  onboardingTaskbarTitle: "Your taskbar, replaced — safely",
  onboardingTaskbarText:
    "Nexora hides the Windows taskbar and shows your pinned & open apps in the island instead, so you never lose a way to switch apps.",
  onboardingTaskbarSafe:
    "🛡️ Your taskbar always comes back when Nexora closes — and there's a “Restore taskbar now” button in Settings → Taskbar.",
  onboardingContinue: "Continue →",
  onboardingLookTitle: "Pick your look",
  onboardingLookText:
    "You can change this any time in Settings → Appearance.",
  onboardingStart: "Start using Nexora ✓",
  quickTheme: "Theme",
  notifications: "Notifications",
  openSettings: "Open Settings",
  closeWindow: "Close window",
  expand: "Expand",
  collapse: "Collapse",
  keepOpen: "Keep open",
  pinned: "Pinned ✓",
  keepOpenOn: "Keep open is on — auto-close is off",
  keepOpenOff: "Keep open — stop auto-close",
  densityComfortable: "Comfortable",
  densityCompact: "Compact",
  markAllRead: "Mark all read",
  clearAll: "Clear all",
  clearGroup: "Clear",
  clearKind: "Clear {kind} notifications",
  noNotifications: "No notifications",
  dismissNotification: "Dismiss notification",
  islandPanels: "Island panels",
  noMedia: "No media playing",
  albumCover: "Album cover",
  mediaPrev: "Prev",
  mediaNext: "Next",
  mediaPlay: "Play",
  mediaPause: "Pause",
  mediaRefresh: "Refresh",
  cpuUsageValue: "CPU usage: {v}%",
  cpuUsageEmpty: "CPU usage: —",
  ramUsageValue: "RAM usage: {v}%",
  ramUsageEmpty: "RAM usage: —",
  gpuUsageValue: "GPU usage: {v}%",
  gpuUsageEmpty: "GPU usage: —",
  uploadValue: "Upload: {v} KB/s",
  uploadEmpty: "Upload: —",
  downloadValue: "Download: {v} KB/s",
  downloadEmpty: "Download: —",
  taskbarLabel: "Taskbar",
  taskbarHidden: "hidden",
  taskbarShown: "shown",
  watchdogLabel: "Watchdog",
  healthy: "healthy",
  unhealthy: "unhealthy",
  explorerLabel: "Explorer",
  explorerOk: "ok",
  explorerDown: "down",
  relativeJustNow: "just now",
  relativeMin: "{n} min ago",
  relativeHour: "{n} h ago",
  relativeDay: "{n} d ago",
  kindOther: "Other",
  on: "On",
  off: "Off",
  settingsTitle: "Settings",
  sectionAppearance: "Appearance",
  appearanceDesc:
    "Themes apply live to the island — hover a card to preview it here, click to keep it.",
  themeGroupLabel: "Nexora theme",
  autoHideTaskbar: "Auto-hide taskbar",
  autoHideTaskbarDesc: "Hide the Windows taskbar while Nexora runs.",
  alwaysOnTop: "Always on top",
  alwaysOnTopDesc: "Keep the island above other windows.",
  restoreTaskbar: "Restore taskbar",
  restoreTaskbarDesc:
    "Bring the Windows taskbar back right now, without closing Nexora.",
  restoreTaskbarBtn: "↩ Restore taskbar now",
  sectionIslandStyle: "Island style",
  islandStyleDesc:
    "Blur and corner radius override the theme defaults — set them back to Default to follow the theme again.",
  islandBlur: "Island blur",
  cornerRadius: "Corner radius",
  overridePx: "Override: {n} px.",
  themeDefaultPx: "Theme default: {n} px.",
  islandHotkey: "Island hotkey",
  islandHotkeyDesc: "Press to show or hide the island.",
  changeHotkey: "Change",
  hotkeyListening: "Press a key…",
  hotkeyCancelHint: "Press Esc to cancel.",
  hotkeyError: "Couldn't register that key — try another one.",
  valuePx: "{n} px",
  defaultLabel: "Default",
  blurAria: "Island blur in pixels",
  radiusAria: "Island corner radius in pixels",
  sectionClock: "Clock",
  clockDesc: "How the island clock shows the time and date.",
  clock24h: "24-hour clock",
  clock24hDesc: "Show the time as 21:47 instead of 9:47 PM.",
  showSeconds: "Show seconds",
  showSecondsDesc: "Show seconds in the island clock.",
  dateFormat: "Date format",
  dateFormatDesc: "Short shows Nov 19 · ۲۸ آبان; Full shows Thu, Nov 19, 2026 · پنجشنبه، ۲۸ آبان ۱۴۰۵.",
  dateShort: "Short",
  dateFull: "Full",
  sectionPerformance: "Performance",
  performanceDesc: "How often stats and media refresh, and how the numbers look.",
  refreshInterval: "Refresh interval",
  refreshIntervalDesc: "How often stats and media refresh.",
  refreshIntervalAria: "Refresh interval in seconds",
  valueSecs: "{n} s",
  speedUnit: "Speed unit",
  speedUnitDesc: "Auto switches to MB/s above 1024 KB/s.",
  speedAuto: "Auto",
  percentStyle: "Percent style",
  percentStyleDesc: "Show or hide the % sign on stat chips.",
  percentWith: "With %",
  percentWithout: "Without %",
  updateAvailable: "Update available",
  noUpdateAvailable: "No update available",
  checkForUpdates: "Check for updates",
  resetToDefaults: "Reset to defaults",
  sectionTaskbar: "Taskbar",
  taskbarDesc: "How the Windows taskbar behaves while Nexora runs.",
  sectionUpdates: "Updates",
  updatesDesc: "Check whether a newer Nexora is available.",
  sectionLanguage: "Language",
  languageDesc: "The language of the whole app — island, panel, and Settings.",
  languageEnglish: "English",
  languagePersian: "فارسی",
  startMenu: "Start menu",
} as const;

type StringKey = keyof typeof EN;

const FA: Record<StringKey, string> = {
  tabMedia: "مدیا",
  tabStats: "آمار",
  tabQuick: "تنظیمات سریع",
  onboardingWelcomeTitle: "به Nexora خوش آمدید",
  onboardingWelcomeText:
    "جزیره‌ای کوچک در بالای صفحه‌ات جای تسک‌بار را می‌گیرد — اپ‌ها، موزیک، آمار و ساعت، همه در یک جا.",
  onboardingGetStarted: "شروع کن ←",
  onboardingTaskbarTitle: "تسک‌بارت، با خیال راحت جایگزین شد",
  onboardingTaskbarText:
    "Nexora تسک‌بار ویندوز را پنهان می‌کند و اپ‌های پین‌شده و باز را به‌جایش در جزیره نشان می‌دهد، پس هیچ‌وقت راه جابه‌جایی بین اپ‌ها را از دست نمی‌دهی.",
  onboardingTaskbarSafe:
    "🛡️ تسک‌بارت همیشه با بستن Nexora برمی‌گردد — و دکمه‌ی «بازگرداندن تسک‌بار» در بخش تسک‌بارِ تنظیمات هست.",
  onboardingContinue: "ادامه ←",
  onboardingLookTitle: "ظاهرت را انتخاب کن",
  onboardingLookText:
    "هر وقت خواستی، از بخش ظاهر در تنظیمات می‌توانی عوضش کنی.",
  onboardingStart: "شروع استفاده از Nexora ✓",
  quickTheme: "تم",
  notifications: "اعلان‌ها",
  openSettings: "باز کردن تنظیمات",
  closeWindow: "بستن پنجره",
  expand: "باز کردن",
  collapse: "بستن",
  keepOpen: "باز نگه دار",
  pinned: "پین‌شده ✓",
  keepOpenOn: "«باز نگه دار» روشن است — بستن خودکار خاموش است",
  keepOpenOff: "«باز نگه دار» — جلوی بستن خودکار را بگیر",
  densityComfortable: "راحت",
  densityCompact: "فشرده",
  markAllRead: "خواندن همه",
  clearAll: "پاک‌کردن همه",
  clearGroup: "پاک‌کردن",
  clearKind: "پاک‌کردن اعلان‌های {kind}",
  noNotifications: "اعلانی نیست",
  dismissNotification: "رد کردن اعلان",
  islandPanels: "پنل‌های جزیره",
  noMedia: "چیزی در حال پخش نیست",
  albumCover: "کاور آلبوم",
  mediaPrev: "قبلی",
  mediaNext: "بعدی",
  mediaPlay: "پخش",
  mediaPause: "توقف",
  mediaRefresh: "تازه‌سازی",
  cpuUsageValue: "مصرف CPU: {v}٪",
  cpuUsageEmpty: "مصرف CPU: —",
  ramUsageValue: "مصرف RAM: {v}٪",
  ramUsageEmpty: "مصرف RAM: —",
  gpuUsageValue: "مصرف GPU: {v}٪",
  gpuUsageEmpty: "مصرف GPU: —",
  uploadValue: "آپلود: {v} KB/s",
  uploadEmpty: "آپلود: —",
  downloadValue: "دانلود: {v} KB/s",
  downloadEmpty: "دانلود: —",
  taskbarLabel: "تسک‌بار",
  taskbarHidden: "مخفی",
  taskbarShown: "نمایان",
  watchdogLabel: "واچ‌داگ",
  healthy: "سالم",
  unhealthy: "ناسالم",
  explorerLabel: "اکسپلورر",
  explorerOk: "سالم",
  explorerDown: "خراب",
  relativeJustNow: "همین حالا",
  relativeMin: "{n} دقیقه پیش",
  relativeHour: "{n} ساعت پیش",
  relativeDay: "{n} روز پیش",
  kindOther: "دیگر",
  on: "روشن",
  off: "خاموش",
  settingsTitle: "تنظیمات",
  sectionAppearance: "ظاهر",
  appearanceDesc:
    "تم‌ها روی جزیره زنده اعمال می‌شوند — روی کارت هاور کن تا پیش‌نمایشش را اینجا ببینی، کلیک کن تا بماند.",
  themeGroupLabel: "تم Nexora",
  autoHideTaskbar: "مخفی‌کردن خودکار تسک‌بار",
  autoHideTaskbarDesc: "تسک‌بار ویندوز را در مدت اجرای Nexora مخفی کن.",
  alwaysOnTop: "همیشه روی همه",
  alwaysOnTopDesc: "جزیره را بالای بقیه‌ی پنجره‌ها نگه دار.",
  restoreTaskbar: "بازگرداندن تسک‌بار",
  restoreTaskbarDesc:
    "تسک‌بار ویندوز را همین حالا برگردان، بدون بستن Nexora.",
  restoreTaskbarBtn: "↩ بازگرداندن تسک‌بار",
  sectionIslandStyle: "سبک جزیره",
  islandStyleDesc:
    "بلور و گردی گوشه روی پیش‌فرض تم می‌نشینند — برای اینکه دوباره از تم پیروی کنند، آن‌ها را به «پیش‌فرض» برگردان.",
  islandBlur: "بلور جزیره",
  cornerRadius: "گردی گوشه",
  overridePx: "جایگزین: {n} پیکسل.",
  themeDefaultPx: "پیش‌فرض تم: {n} پیکسل.",
  islandHotkey: "هات‌کی جزیره",
  islandHotkeyDesc: "برای نشان‌دادن یا پنهان‌کردن جزیره فشار بده.",
  changeHotkey: "تغییر",
  hotkeyListening: "کلیدی را فشار بده…",
  hotkeyCancelHint: "برای لغو Esc را بزن.",
  hotkeyError: "این کلید ثبت نشد — یکی دیگر را امتحان کن.",
  valuePx: "{n} پیکسل",
  defaultLabel: "پیش‌فرض",
  blurAria: "بلور جزیره بر حسب پیکسل",
  radiusAria: "گردی گوشه‌ی جزیره بر حسب پیکسل",
  sectionClock: "ساعت",
  clockDesc: "ساعت جزیره زمان و تاریخ را چطور نشان بدهد.",
  clock24h: "ساعت ۲۴ ساعته",
  clock24hDesc: "زمان را به‌صورت 21:47 نشان بده به‌جای 9:47 عصر.",
  showSeconds: "نمایش ثانیه",
  showSecondsDesc: "ثانیه را در ساعت جزیره نشان بده.",
  dateFormat: "قالب تاریخ",
  dateFormatDesc: "کوتاه ۱۹ نوامبر · ۲۸ آبان را نشان می‌دهد؛ کامل پنجشنبه، ۱۹ نوامبر ۲۰۲۶ · پنجشنبه، ۲۸ آبان ۱۴۰۵.",
  dateShort: "کوتاه",
  dateFull: "کامل",
  sectionPerformance: "عملکرد",
  performanceDesc: "آمار و مدیا هر چند وقت تازه شوند و عددها چطور دیده شوند.",
  refreshInterval: "فاصله‌ی تازه‌سازی",
  refreshIntervalDesc: "آمار و مدیا هر چند وقت تازه شوند.",
  refreshIntervalAria: "فاصله‌ی تازه‌سازی بر حسب ثانیه",
  valueSecs: "{n} ثانیه",
  speedUnit: "واحد سرعت",
  speedUnitDesc: "خودکار بالای 1024 KB/s به MB/s می‌رود.",
  speedAuto: "خودکار",
  percentStyle: "سبک درصد",
  percentStyleDesc: "علامت ٪ را روی چیپ‌های آمار نشان بده یا مخفی کن.",
  percentWith: "با ٪",
  percentWithout: "بدون ٪",
  updateAvailable: "به‌روزرسانی موجود است",
  noUpdateAvailable: "به‌روزرسانی‌ای نیست",
  checkForUpdates: "بررسی به‌روزرسانی",
  resetToDefaults: "بازگردانی به پیش‌فرض‌ها",
  sectionTaskbar: "تسک‌بار",
  taskbarDesc: "رفتار تسک‌بار ویندوز در مدت اجرای Nexora.",
  sectionUpdates: "به‌روزرسانی‌ها",
  updatesDesc: "بررسی کن نسخه‌ی تازه‌تری از Nexora آمده است یا نه.",
  sectionLanguage: "زبان",
  languageDesc: "زبان کل برنامه — جزیره، پنل و تنظیمات.",
  languageEnglish: "English",
  languagePersian: "فارسی",
  startMenu: "منوی استارت",
};

export function t(lang: Language, key: StringKey): string {
  return (lang === "fa" ? FA[key] : EN[key]) ?? EN[key];
}

/** Translate a string with `{name}` placeholders. */
export function tf(
  lang: Language,
  key: StringKey,
  replacements: Record<string, string | number>,
): string {
  let out = t(lang, key);
  for (const [name, value] of Object.entries(replacements)) {
    // split/join instead of replaceAll: the tsconfig lib predates ES2021.
    out = out.split(`{${name}}`).join(String(value));
  }
  return out;
}

/** Theme descriptions, per language (names stay English). */
export const THEME_DESCRIPTIONS: Record<Language, Record<ThemeId, string>> = {
  en: {
    obsidian: "Dark slate pill with a sky-blue accent.",
    "liquid-glass": "Translucent frosted glass with a soft white glow.",
    aurora: "Deep teal with an emerald accent.",
    sunset: "Warm ember dark with an orange accent.",
    midnight: "Deep violet night with a lavender accent.",
    forest: "Deep green with a fresh leaf accent.",
    "neon-glass":
      "Deep violet glass with a neon purple-blue glow — the brand-sheet look.",
  },
  fa: {
    obsidian: "قرص خاکستری تیره با اکسنت آبی آسمانی.",
    "liquid-glass": "شیشه‌ی مات نیمه‌شفاف با درخشش سفید ملایم.",
    aurora: "فیروزه‌ای عمیق با اکسنت زمردی.",
    sunset: "تیره‌ی گرم اخگری با اکسنت نارنجی.",
    midnight: "شب بنفش عمیق با اکسنت اسطوخودوسی.",
    forest: "سبز عمیق با اکسنت برگ تازه.",
    "neon-glass":
      "شیشه‌ی بنفش عمیق با درخشش نئون بنفش-آبی — ظاهر شیت برند.",
  },
};

/** Display labels for the notification kinds the backend emits. */
export const NOTIFICATION_KIND_LABELS: Record<
  Language,
  Record<string, string>
> = {
  en: {
    update: "Updates",
    taskbar: "Taskbar",
  },
  fa: {
    update: "به‌روزرسانی‌ها",
    taskbar: "تسک‌بار",
  },
};
