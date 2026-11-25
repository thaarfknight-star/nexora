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
    version: "0.2.24",
    date: "2026-11-25",
    title: {
      en: "Stacked 2-Line Subtitles, Saved CSS Presets, Dubbing Fix & Accurate Update Notes",
      fa: "زیرنویس دوخطی استریم/گیم، پریست‌های CSS سفارشی، رفع کرش دوبله و اصلاح پنجره تغییرات آپدیت",
    },
    features: {
      en: [
        "Redesigned live subtitles into a clean 2-tier stacked pill: previous line slides up into a dimmer top pill while the active line completes word-by-word below.",
        "Added named Custom Subtitle CSS Presets so you can save, switch, and delete your own subtitle styles anytime.",
      ],
      fa: [
        "بازطراحی زیرنویس به سبک دوخطی گیم/استریم: جمله قبلی به کپسول کم‌رنگ بالایی می‌رود و جمله جدید کلمه‌به‌کلمه در کپسول اصلی پایین کامل می‌شود.",
        "افزودن قابلیت ذخیره CSS سفارشی با نام دلخواه (پریست) برای انتخاب سریع یا حذف در هر زمان.",
      ],
    },
    fixes: {
      en: [
        "Fixed Piper TTS crashing on the Persian Amir voice by automatically sanitizing multi-codepoint phoneme keys in .onnx.json.",
        "Fixed the What's New modal showing the current/previous version when an update is available — it now shows the new version's actual changelog.",
      ],
      fa: [
        "رفع کامل کرش موتور دوبله روی صدای فارسی امیر (خطای aɪ is not a single codepoint) با پاک‌سازی خودکار فایل کانفیگ صوتی.",
        "رفع نمایش تغییرات نسخه قبلی/فعلی هنگام آپدیت؛ پنجره تغییرات حالا دقیقاً تغییرات نسخه جدید آماده نصب را نمایش می‌دهد.",
      ],
    },
  },
  {
    version: "0.2.23",
    date: "2026-11-25",
    title: {
      en: "Nexora-Voice v1 Fixes, Flicker-Free Subtitles & Reliable Island Auto-Collapse",
      fa: "بازنویسی و رفع باگ‌های موتور دوبله، زیرنویس کلمه‌به‌کلمه و جمع شدن داینامیک آیلند",
    },
    features: {
      en: [
        "Added dedicated System Audio / Specific Window selector and per-voice download status directly inside the Live Dubbing card.",
        "Added debounced live Custom Subtitle CSS editing with instant Apply & Preview.",
      ],
      fa: [
        "افزودن انتخابگر اختصاصی کل سیستم / پنجره خاص و وضعیت نصب جداگانه هر صدا داخل کارت موتور دوبله.",
        "بهینه‌سازی ادیتور زنده CSS سفارشی زیرنویس با ذخیره بهینه و پیش‌نمایش فوری.",
      ],
    },
    fixes: {
      en: [
        "Fixed Piper TTS execution on Windows (working directory + --espeak_data), added fallback system loopback, prevented sending English text to Persian TTS, and surfaced errors cleanly.",
        "Fixed word-by-word subtitles rewriting from word 1 on minor Whisper edits by using punctuation-insensitive overlap tracking and stable keys.",
        "Fixed Dynamic Island staying open after leaving the hot-zone or switching apps by adding hot-zone leave re-arming and an inactivity watchdog.",
        "Added backend 4-second silence clearing so old translations never flash after a pause.",
      ],
      fa: [
        "رفع خطای اجرای Piper در ویندوز (تنظیم مسیر کاری و espeak-ng-data)، جلوگیری از ارسال متن انگلیسی به صدای فارسی و نمایش خطاها.",
        "رفع بازنویسی از اول جمله‌ها در زیرنویس با تشخیص ادامه جمله و کلیدهای پایدار بدون چشمک زدن.",
        "رفع جمع نشدن داینامیک آیلند پس از خروج از لبه بالا یا سوییچ به برنامه‌های دیگر.",
        "پاک‌سازی کامل ترجمه قبلی در بک‌اند پس از ۴ ثانیه سکوت.",
      ],
    },
  },
  {
    version: "0.2.22",
    date: "2026-11-24",
    title: {
      en: "Nexora-Voice v1 Dubbing Foundation, Word-by-Word Subtitles & Custom CSS",
      fa: "پایه موتور دوبله Nexora-Voice v1، زیرنویس کلمه‌به‌کلمه و CSS سفارشی",
    },
    features: {
      en: [
        "Started Nexora-Voice v1: offline Persian live dubbing engine with Piper ONNX neural voices (Amir & Gyro), System/App audio capture, self-echo exclusion, and automatic ISimpleAudioVolume audio ducking.",
        "Upgraded live subtitles to reveal sentences smoothly word-by-word instead of rewriting from the start.",
        "Added 4-second silence auto-fade so subtitles disappear cleanly when playback pauses or ends.",
        "Added live Custom Subtitle CSS customization in Settings → Engines.",
      ],
      fa: [
        "شروع ساخت موتور دوبله آفلاین Nexora-Voice v1 با صداهای طبیعی فارسی (امیر و گیرو)، انتخاب پنجره یا کل سیستم، حذف اکوی خودکار و کاهش خودکار صدای اصلی.",
        "بهبود نمایش زیرنویس به‌صورت کلمه‌به‌کلمه و پیوسته به جای بازنویسی از ابتدا.",
        "محو شدن خودکار زیرنویس پس از ۴ ثانیه سکوت هنگام توقف یا پایان ویدیو.",
        "افزودن قابلیت شخصی‌سازی زنده ظاهر زیرنویس با CSS سفارشی در تنظیمات.",
      ],
    },
    fixes: {
      en: [
        "Fixed the Dynamic Island staying open when idle by scoping hover detection to the pill and stepping down from compact to hidden on timeout or focus loss.",
      ],
      fa: [
        "رفع مشکل جمع نشدن داینامیک آیلند در زمان بیکاری با محدود کردن هاور به خود کپسول و جمع شدن خودکار پس از خروج یا از دست دادن فوکوس.",
      ],
    },
  },
  {
    version: "0.2.21",
    date: "2026-11-24",
    title: {
      en: "Nexora-MT v2 Offline Model (~51 MB, Per-Row INT8)",
      fa: "انتشار مدل آفلاین Nexora-MT v2 (حدود ۵۱ مگابایت، Per-Row INT8)",
    },
    features: {
      en: [
        "Shipped the newly trained Nexora-MT v2 offline English → Persian subtitle model (55M params, 6 enc / 4 dec, 24k BPE, Per-Row INT8, ~50.8 MB) with SHA-256 verification.",
        "Added automatic cleanup of legacy nexora-mt-en-fa-v1.nxmt files when installing or removing v2.",
      ],
      fa: [
        "انتشار مدل جدید و آموزش‌دیده Nexora-MT v2 برای ترجمه آفلاین انگلیسی به فارسی (۵۵ میلیون پارامتر، کوانتایزیشن سطری Per-Row INT8، حدود ۵۱ مگابایت) با تایید SHA-256.",
        "پاک‌سازی خودکار فایل قدیمی nexora-mt-en-fa-v1.nxmt هنگام نصب یا حذف نسخه v2 برای آزادسازی فضای دیسک.",
      ],
    },
    fixes: {
      en: [
        "Patched RMSNorm to compute variance in FP32 inside the Colab training notebook to prevent FP16 overflow on T4 GPUs.",
      ],
      fa: [
        "اصلاح محاسبه واریانس RMSNorm به FP32 در نوت‌بوک آموزش Colab برای جلوگیری از خطای سرریز FP16 روی GPUهای T4.",
      ],
    },
  },
  {
    version: "0.2.20",
    date: "2026-11-22",
    title: {
      en: "Beam Search Decoding, .nxmt v2 (Per-Row INT8) Engine & One-Click Model Updates",
      fa: "دیکودینگ Beam Search، پشتیبانی موتور از .nxmt v2 (کوانتایزیشن سطری) و دکمه آپدیت مدل",
    },
    features: {
      en: [
        "Upgraded Nexora-MT pure-Rust decoder to Beam Search (beam=3) with length penalty and 3-gram repetition blocking for smoother Persian subtitles.",
        "Added native .nxmt v2 (Per-Row INT8) format support and the Nexora-MT v2 multi-corpus Colab training notebook.",
        "Added a real one-click Update Model button for the offline translation model with automatic hot-reload.",
      ],
      fa: [
        "ارتقای موتور بومی Nexora-MT به Beam Search (beam=3) با جریمه تکرار ۳-گرام برای ترجمه‌های روان‌تر و دستوری‌تر فارسی.",
        "افزودن پشتیبانی کامل از فرمت .nxmt v2 (کوانتایزیشن سطری Per-Row INT8) و نوت‌بوک آموزش Nexora-MT v2 در Colab.",
        "افزودن دکمه واقعی «آپدیت مدل» برای مدل ترجمه آفلاین با بارگذاری مجدد خودکار بدون نیاز به ریستارت برنامه.",
      ],
    },
    fixes: {
      en: [
        "Invalidated the resident Nexora-MT RAM cache immediately when updating or removing the offline model.",
      ],
      fa: [
        "پاک‌سازی خودکار کش حافظه مدل Nexora-MT بلافاصله پس از آپدیت یا حذف مدل آفلاین.",
      ],
    },
  },
  {
    version: "0.2.19",
    date: "2026-11-22",
    title: {
      en: "Real-Time Local Translation Fix, Separate Collapsible Engines, Island Engines Tab & 5s Auto-Collapse",
      fa: "ترجمه لحظه‌ای واقعی موتور محلی، جداسازی و بازوبسته‌شدن موتورها، تب Engines جزیره و جمع‌شدن ۵ ثانیه‌ای",
    },
    features: {
      en: [
        "Split Local Offline Translate and Online Live Translate into two independent collapsible cards in Settings → Engines.",
        "Added a conditional Engines tab to the Dynamic Island (shown only when at least one engine is downloaded).",
        "Restored Google Translate (Free, no API key) alongside Google Cloud Translation v2 and Gemini 2.5 Flash-Lite.",
        "Changed all 7 theme hover animations to play once smoothly instead of looping, and added 5-second inactivity auto-collapse.",
      ],
      fa: [
        "جداسازی کامل موتور ترجمه آفلاین محلی و موتور ترجمه آنلاین در دو کارت مستقل و قابل باز و بسته شدن در تنظیمات.",
        "افزودن تب «موتورها» به داینامیک آیلند (فقط زمانی که حداقل یک موتور دانلود شده باشد).",
        "بازگرداندن حالت Google Translate رایگان (بدون کلید API) در کنار Google Cloud و Gemini.",
        "اجرای نرم و یک‌باره انیمیشن‌های هاور هر ۷ تم به‌جای تکرار مداوم، و جمع‌شدن خودکار جزیره پس از ۵ ثانیه بدون کلیک یا هاور.",
      ],
    },
    fixes: {
      en: [
        "Fixed live subtitles getting stuck on 'Translating...' during continuous speech and parallelized Nexora-MT v1 CPU decoding for instant real-time output.",
      ],
      fa: [
        "رفع کامل گیر کردن زیرنویس محلی و آنلاین روی «در حال ترجمه...» در گفتار پیوسته و چندترد کردن موتور Nexora-MT v1 برای ترجمه در لحظه.",
      ],
    },
  },
  {
    version: "0.2.18",
    date: "2026-11-22",
    title: {
      en: "Nexora-MT v1 — Pure-Rust Offline English → Persian Subtitle Translator",
      fa: "موتور ترجمه آفلاین اختصاصی Nexora-MT v1 (انگلیسی به فارسی، بدون اینترنت)",
    },
    features: {
      en: [
        "Added native pure-Rust inference engine for Nexora-MT v1 (.nxmt) with ByteLevel BPE, INT8 quantization, and incremental KV-cache.",
        "Added one-click SHA-256 verified download, test, and remove controls for nexora-mt-en-fa-v1.nxmt (~27 MB) inside Settings → Engines.",
      ],
      fa: [
        "افزودن موتور بومی خالص Rust برای اجرای مدل اختصاصی Nexora-MT v1 با توکنایزر ByteLevel BPE، وزن‌های INT8 و KV-Cache پرسرعت روی CPU.",
        "افزودن دانلود یک‌کلیکی با تایید SHA-256، دکمه تست و حذف برای مدل آفلاین nexora-mt-en-fa-v1.nxmt (حدود ۲۷ مگابایت) در تنظیمات موتورها.",
      ],
    },
    fixes: {
      en: [
        "Activated the Local offline translation provider using the resident Nexora-MT v1 engine.",
      ],
      fa: [
        "فعال‌سازی کامل پروایدر ترجمه آفلاین محلی با بارگذاری مقیم مدل Nexora-MT v1 در حافظه.",
      ],
    },
  },
  {
    version: "0.2.17",
    date: "2026-11-22",
    title: {
      en: "Official Live Translate Architecture, Adaptive Whisper VAD & Bold Theme Animations",
      fa: "معماری رسمی ترجمه زنده، مدل‌های انتخابی Whisper، ذخیره امن کلید API و انیمیشن‌های متمایز تم‌ها",
    },
    features: {
      en: [
        "Upgraded all 7 Dynamic Island theme entrance and hover animations with bold, unmistakable per-theme motion and sheen/halo effects.",
        "Added official Google Cloud Translation v2 and Gemini 2.5 Flash-Lite providers with secure Windows Credential Manager key storage and Test Connection.",
        "Added selectable SHA-256-verified Whisper models (tiny.en, base.en, tiny, large-v3-turbo-q5_0), adaptive noise-floor VAD, and hallucination/repetition filtering.",
        "Added live partial subtitle rendering with stability gating, latest-wins cancellation, bounded cache, and explicit offline/failed status badges.",
      ],
      fa: [
        "بازطراحی انیمیشن‌های ظاهر شدن و هاور هر ۷ تم داینامیک آیلند با افکت‌های متمایز و کاملاً قابل مشاهده (شاین شیشه‌ای، هاله نئون، موج شفق قطبی و...).",
        "جایگزینی endpointهای غیررسمی با API رسمی Google Cloud Translation v2 و Gemini 2.5 Flash-Lite همراه با ذخیره امن کلید در Windows Credential Manager و دکمه تست اتصال.",
        "افزودن مدل‌های قابل انتخاب Whisper (tiny.en، base.en، tiny، large-v3-turbo-q5_0) با تایید SHA-256، دروازه VAD تطبیقی و فیلتر توهم و تکرار.",
        "نمایش فوری متن اصلی زیرنویس با پایدارسازی، لغو درخواست‌های قدیمی (latest-wins)، کش نرمال‌شده و نمایشگر واضح وضعیت آفلاین/خطا.",
      ],
    },
    fixes: {
      en: [
        "Removed unofficial Google/MyMemory endpoints and fake Chrome User-Agent headers.",
        "Stopped silently masking failed/offline translations as original text and fixed identical proper-noun outputs being treated as errors.",
      ],
      fa: [
        "حذف کامل endpointهای غیررسمی Google و MyMemory و هدرهای جعلی مرورگر.",
        "رفع نمایش بی‌صدای متن اصلی به‌جای ترجمه در زمان آفلاین یا خطا، و پذیرش صحیح ترجمه اسامی خاص یکسان با ورودی.",
      ],
    },
  },
  {
    version: "0.2.16",
    date: "2026-11-22",
    title: {
      en: "Live Theme Sync, Theme Animations & Verified Live Translate Digests",
      fa: "همگام‌سازی لحظه‌ای تم، انیمیشن‌های اختصاصی هر تم و فعال‌سازی مجدد دانلود موتور ترجمه",
    },
    features: {
      en: [
        "Added 7 signature theme-specific entrance and hover animations for the Dynamic Island (Liquid Glass, Neon Glass, Aurora, Sunset, Midnight, Forest, Obsidian).",
        "Added live bidirectional theme synchronization between the Dynamic Island and the Settings window.",
        "Pinned ggml-tiny.bin to its verified HuggingFace commit and configured real SHA-256 digests for whisper-bin-x64.zip and ggml-tiny.bin.",
      ],
      fa: [
        "افزودن ۷ انیمیشن اختصاصی برای ظاهر شدن و هاور داینامیک آیلند متناسب با هر تم (Liquid Glass، Neon Glass، Aurora، Sunset، Midnight، Forest و Obsidian).",
        "همگام‌سازی لحظه‌ای و دوطرفه تم بین داینامیک آیلند و پنجره Settings بدون نیاز به انتخاب دستی مجدد.",
        "ثبت هش‌های واقعی SHA-256 و commit پین‌شده برای موتور Live Translate جهت دانلود امن و بدون خطا.",
      ],
    },
    fixes: {
      en: [
        "Fixed Live Translate download refusing with unconfigured TODO SHA-256 digest.",
        "Fixed the Settings window staying on the old theme when switching themes from the Dynamic Island.",
      ],
      fa: [
        "رفع خطای SHA-256 digest is not configured هنگام دانلود موتور ترجمه زنده.",
        "رفع مشکل هماهنگ نشدن تم پنجره Settings هنگام تغییر تم از داخل داینامیک آیلند.",
      ],
    },
  },
  {
    version: "0.2.15",
    date: "2026-11-22",
    title: {
      en: "NSIS Pre-Install Hook & Deterministic Update Exit",
      fa: "رفع ریشه‌ای خطای Failed to kill Nexora در نصاب و آپدیتر",
    },
    features: {
      en: [
        "Added a verified NSIS_HOOK_PREINSTALL hook (src-tauri/windows/hooks.nsh) that runs before Tauri's CheckIfAppIsRunning to cleanly close any lingering nexora.exe and whisper-server.exe processes.",
        "Added a deterministic, diagnostics-logged pre-exit sequence (restore taskbar -> stop watchdog with 2 s timeout -> synchronously kill whisper-server -> exit).",
      ],
      fa: [
        "افزودن هوک رسمی NSIS_HOOK_PREINSTALL (فایل hooks.nsh) پیش از مرحله CheckIfAppIsRunning برای بستن تمیز هرگونه پروسه باقیمانده nexora.exe و whisper-server.exe.",
        "افزودن ترتیب خروج منظم و ثبت لاگ مرحله‌به‌مرحله در diagnostics.log (ابتدا بازیابی taskbar، سپس توقف watchdog با تایم‌اوت ۲ ثانیه و بستن هم‌زمان whisper-server).",
      ],
    },
    fixes: {
      en: [
        "Fixed 'Failed to kill Nexora. Please close it first then try again' during in-app updates and manual over-the-top NSIS installs.",
        "Ensured whisper-server.exe is killed synchronously on update exit and tray quit.",
      ],
      fa: [
        "رفع ریشه‌ای خطای «Failed to kill Nexora. Please close it first then try again» هنگام آپدیت داخل برنامه یا نصب مستقیم روی نسخه در حال اجرا.",
        "تضمین بسته شدن هم‌زمان و بدون جا ماندن whisper-server.exe هنگام آپدیت و خروج از Tray.",
      ],
    },
  },
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
