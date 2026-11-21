export interface ReleaseEntry {
  version: string;
  date: string;
  title: {
    en: string;
    fa: string;
  };
  features: {
    en: string[];
    fa: string[];
  };
  fixes: {
    en: string[];
    fa: string[];
  };
}

export const RELEASE_NOTES: readonly ReleaseEntry[] = [
  {
    version: "0.2.5",
    date: "2026-11-21",
    title: {
      en: "What's New & Release Notes Experience",
      fa: "پنجره تغییرات نسخه جدید و یادداشت‌های انتشار",
    },
    features: {
      en: [
        "Added a sleek post-update What's New modal that automatically showcases new features and bug fixes right after updating Nexora.",
        "Added a 'What's New' button in Settings → Updates so you can review release highlights anytime in English or Persian.",
        "Automated structured release notes publishing in CHANGELOG.md and GitHub Releases for every new version tag.",
      ],
      fa: [
        "افزودن پنجره شیشه‌ای «تغییرات نسخه جدید» که بلافاصله بعد از هر آپدیت به‌صورت خودکار باز می‌شود و ویژگی‌ها و رفع باگ‌ها را نمایش می‌دهد.",
        "افزودن دکمه «تغییرات این نسخه» در بخش Settings → Updates برای مشاهده تاریخچه تغییرات در هر زمان.",
        "انتشار خودکار لیست منظم تغییرات و باگ‌های رفع‌شده در CHANGELOG.md و هر ریلیز گیت‌هاب.",
      ],
    },
    fixes: {
      en: [
        "Fixed Start Live Subtitles UI freeze (Not Responding) by making the backend window creation asynchronous.",
        "Fixed Windows NSIS auto-updater exit timing so updates install cleanly without closing prematurely.",
        "Switched Live Translate downloads and ZIP extraction to native Rust (reqwest + zip) with smooth 0–100% progress and zero console flashes.",
      ],
      fa: [
        "رفع هنگ و Not Responding هنگام زدن دکمه شروع زیرنویس زنده با تبدیل دستور ساخت پنجره به async.",
        "اصلاح زمان خروج موتور آپدیت روی ویندوز برای نصب روان اینستالر NSIS.",
        "تبدیل کامل دانلود و اکسترکت موتور ترجمه به کد بومی Rust با نوار پیشرفت واقعی و بدون باز شدن پنجره CMD.",
      ],
    },
  },
  {
    version: "0.2.4",
    date: "2026-11-21",
    title: {
      en: "Live Translate Stability & Auto-Updater Polish",
      fa: "پایداری ترجمه زنده و بهینه‌سازی موتور آپدیت",
    },
    features: {
      en: [
        "Optional downloadable Live Translate engine in the new Settings → Engines hub with custom shortcuts and updates.",
        "Theme-adaptive SVG icons for Notifications, Settings, and Live Subtitles across all 7 themes.",
        "Merged Appearance themes into Island Style and added a Minimize button to the Settings window.",
      ],
      fa: [
        "موتور قابل دانلود ترجمه زنده در تب جدید Engines با قابلیت آپدیت و تعیین میانبر اختصاصی.",
        "آیکون‌های SVG هماهنگ با تم برای نوتیفیکیشن، تنظیمات و زیرنویس در هر ۷ تم.",
        "ادغام بخش Appearance در Island Style و افزودن دکمه Minimize به پنجره Settings.",
      ],
    },
    fixes: {
      en: [
        "Eliminated side clipping when scaling the Dynamic Island up to 115%.",
        "Disabled hover-at-top-edge auto-open so the island opens only via top handle click or hotkey.",
        "Eliminated startup default-theme flash by applying saved theme synchronously before first paint.",
      ],
      fa: [
        "رفع بریدگی لبه‌ها هنگام بزرگ کردن اندازه داینامیک آیلند تا ۱۱۵٪.",
        "غیرفعال شدن باز شدن ناخواسته با بردن موس به بالای صفحه (باز شدن فقط با کلیک یا شورتکات).",
        "حذف تاخیر و پرش تم پیش‌فرض هنگام اجرای برنامه.",
      ],
    },
  },
];

export function latestReleaseNote(version?: string | null): ReleaseEntry {
  if (version) {
    const clean = version.replace(/^v/i, "");
    const exact = RELEASE_NOTES.find((r) => r.version === clean);
    if (exact) return exact;
  }
  return RELEASE_NOTES[0];
}
