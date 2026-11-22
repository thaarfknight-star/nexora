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
    version: "0.2.14",
    date: "2026-11-22",
    title: {
      en: "Security & QA Hardening Release",
      fa: "نسخه مقاوم‌سازی امنیتی و پایداری (نتایج ممیزی QA)",
    },
    features: {
      en: [
        "Added opt-in Online Translation privacy setting (off by default — local transcription only until user consents).",
        "Added Windows single-instance Named Mutex guard, click-through heartbeat safety net, and DPI ScaleFactorChanged re-anchoring.",
        "Separated minimal display-only capability for the subtitle window and restricted privileged commands to main/settings.",
      ],
      fa: [
        "افزودن تنظیم حریم خصوصی «ترجمه آنلاین» (پیش‌فرض خاموش — فقط رونویسی محلی تا زمان رضایت کاربر).",
        "افزودن قفل تک‌نمونه ویندوز (Named Mutex)، شبکه ایمنی Heartbeat برای Click-Through و تنظیم خودکار موقعیت هنگام تغییر DPI.",
        "جداسازی مجوزهای پنجره زیرنویس و محدود کردن دستورات حساس به پنجره‌های main و settings.",
      ],
    },
    fixes: {
      en: [
        "Hardened launch_pinned_app with strict allowlist validation, configured CSP/devCsp, added fail-closed SHA-256 & ZIP-bomb limits, and wrapped extern callbacks in catch_unwind.",
        "Ensured clean shutdown restores taskbar first, stops Whisper, and unregisters global shortcuts; clamped numeric settings and removed unused i18n keys.",
      ],
      fa: [
        "ایمن‌سازی launch_pinned_app با لیست سفید، فعال‌سازی CSP، افزودن تایید SHA-256 و محدودیت ضد ZIP-Bomb و محافظت catch_unwind در کالبک‌های unsafe.",
        "خروج تمیز با بستن Whisper و آزادسازی شورتکات جهانی، محدودسازی بازه تنظیمات عددی و به‌روزرسانی مستندات.",
      ],
    },
  },
  {
    version: "0.2.13",
    date: "2026-11-22",
    title: {
      en: "What's New Version Sync Fix & Local Engine Cleanup",
      fa: "اصلاح پنجره تغییرات نسخه جدید و پاکسازی مدل محلی قبلی",
    },
    features: {
      en: [
        "Removed the temporary Qwen2.5 + llama-server offline translator card to prepare for Nexora's own lightweight Nexora-MT v1 engine.",
        "Kept clean engine settings gating so un-downloaded engine controls stay hidden until installation finishes.",
      ],
      fa: [
        "حذف کامل مدل موقت Qwen2.5 و llama-server برای آماده‌سازی بستر مدل اختصاصی و سبک Nexora-MT v1.",
        "حفظ مخفی‌سازی تنظیمات موتورهای دانلودنشده تا پیش از تکمیل نصب.",
      ],
    },
    fixes: {
      en: [
        "Fixed the What's New modal always showing v0.2.5 by properly populating all release entries from v0.2.6 through v0.2.13 at the top of RELEASE_NOTES.",
      ],
      fa: [
        "رفع کامل مشکل ثابت ماندن پنجره تغییرات روی نسخه 0.2.5 با به‌روزرسانی صحیح لیست یادداشت‌های انتشار تا نسخه 0.2.13.",
      ],
    },
  },
  {
    version: "0.2.12",
    date: "2026-11-22",
    title: {
      en: "Official Tauri v2 Signed Update Engine",
      fa: "بازنویسی موتور آپدیت با پلاگین رسمی Tauri v2 (معماری ReChat)",
    },
    features: {
      en: [
        "Adopted the official signed Tauri v2 @tauri-apps/plugin-updater flow with minisign verification and NSIS currentUser passive install.",
        "Added an automatic startup update check toggle and confirmation dialog with release notes.",
      ],
      fa: [
        "بازنویسی کامل موتور آپدیت با پلاگین رسمی امضاشده Tauri v2 و نصب passive بدون نیاز به ادمین (currentUser).",
        "افزودن تیک بررسی خودکار آپدیت هنگام اجرا و پنجره تایید همراه با متن تغییرات.",
      ],
    },
    fixes: {
      en: [
        "Fixed in-app updates failing to apply in v0.2.10 and ensured Live Translate shuts down cleanly before updating.",
      ],
      fa: [
        "رفع خطای نصب نشدن آپدیت در نسخه 0.2.10 و بستن تمیز موتور زیرنویس پیش از شروع نصب.",
      ],
    },
  },
  {
    version: "0.2.10",
    date: "2026-11-22",
    title: {
      en: "Multi-Provider Rate-Safe Live Translation",
      fa: "ترجمه زنده پایدار با ۳ سرویس‌دهنده پشتیبان",
    },
    features: {
      en: [
        "Added a 3-provider translation fallback chain (Google gtx → Google clients5 → MyMemory) with browser User-Agent and rate-limit pacing.",
      ],
      fa: [
        "افزودن زنجیره ۳ سرویس‌دهنده ترجمه پشتیبان خودکار برای جلوگیری از قطع شدن ترجمه.",
      ],
    },
    fixes: {
      en: [
        "Fixed Live Subtitles showing untranslated dialogue when rapid polling triggered HTTP 429 rate limits.",
      ],
      fa: [
        "رفع نمایش متن اصلی بدون ترجمه ناشی از محدودیت درخواست (HTTP 429).",
      ],
    },
  },
  {
    version: "0.2.9",
    date: "2026-11-21",
    title: {
      en: "Real-Time Word-by-Word Streaming Subtitles",
      fa: "ترجمه و نمایش زنده کلمه‌به‌کلمه زیرنویس",
    },
    features: {
      en: [
        "Switched Live Translate to rolling-window word-by-word streaming (250 ms snapshots, Latest-Only slots, and Whisper --audio-ctx 256).",
      ],
      fa: [
        "تغییر موتور زیرنویس به استریم کلمه‌به‌کلمه زنده با اسنپ‌شات ۲۵۰ میلی‌ثانیه‌ای و حذف صف تاخیر.",
      ],
    },
    fixes: {
      en: [
        "Eliminated waiting for the end of each sentence before displaying live subtitles.",
      ],
      fa: [
        "حذف انتظار تا پایان جمله پیش از نمایش زیرنویس زنده.",
      ],
    },
  },
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
