//! Taskbar replacement content for the island's compact bar: the open
//! windows, the pinned apps, click to activate / minimize, and the Start
//! button.
//!
//! This is the backend of the taskbar zone in the compact bar (design
//! artifact `nexora-ui-ux-design`): it answers what the real taskbar
//! used to show, now that Nexora hides it. The frontend polls
//! [`list_windows`] and [`list_pinned`], draws the icons, and calls
//! [`activate_window`] / [`open_start_menu`] on clicks.
//!
//! The public functions are plain (non-`#[tauri::command]`) on purpose:
//! `ipc.rs` belongs to EAgent5, so he wires the commands (names and
//! signatures are listed in the branch report). Only the Windows
//! implementation talks to WinAPI (crate `windows`, target-gated in
//! `Cargo.toml`). On any other OS every function is a no-op that returns
//! an empty list / `false`, so `cargo check` and `cargo test` keep
//! passing off Windows.
//!
//! Icons are returned as base64 RGBA pixels (square, 32 px, top-down,
//! `icon_size` 0 and an empty string when no icon could be read) instead
//! of PNG, so no image-encoder dependency is needed: the frontend draws
//! the pixels on a canvas. The icon comes from the window's / pin target's
//! executable (first icon, `ExtractIconExW`), drawn into a 32-bit DIB.
//!
//! Known gaps (see the branch report): pinned UWP / Store apps cannot be
//! resolved to a file path (their links carry an AppUserModelId, not an
//! exe), so they are skipped; UWP windows report the ApplicationFrameHost
//! icon as "no icon" instead of a misleading shared one.

use serde::Serialize;

/// Edge of the square icon returned for windows and pins, in pixels.
pub const ICON_SIZE: u32 = 32;

/// One open window, as the compact bar shows it.
#[derive(Debug, Clone, Serialize)]
pub struct TaskbarWindow {
    /// Window handle as a number; the frontend passes it back to
    /// [`activate_window`].
    pub id: i64,
    pub title: String,
    pub minimized: bool,
    /// Whether this window is the current foreground window.
    pub active: bool,
    /// Square icon edge in pixels (0 when no icon).
    pub icon_size: u32,
    /// Icon pixels as base64 RGBA, row-major top-down ("" when no icon).
    pub icon_rgba_base64: String,
}

/// One app pinned to the user's taskbar.
#[derive(Debug, Clone, Serialize)]
pub struct PinnedApp {
    pub name: String,
    /// Resolved target path; "" when the link could not be resolved
    /// (such entries are skipped, so this is never "" in practice).
    pub target: String,
    /// Square icon edge in pixels (0 when no icon).
    pub icon_size: u32,
    /// Icon pixels as base64 RGBA, row-major top-down ("" when no icon).
    pub icon_rgba_base64: String,
}

/// What a click on a taskbar window should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleAction {
    /// Bring the window to the front (restoring it first if minimized).
    Activate,
    /// Minimize the window (it is already the foreground window).
    Minimize,
}

/// Raw window properties the Alt-Tab filter decides on. The Windows side
/// fills this in from live WinAPI calls; the decision itself is pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowProps {
    pub visible: bool,
    pub has_title: bool,
    /// Has the `WS_EX_TOOLWINDOW` style.
    pub is_tool_window: bool,
    /// Has the `WS_EX_APPWINDOW` style (overrides tool-window exclusion).
    pub is_app_window: bool,
    /// Has an owner window (`GetWindow(GW_OWNER)` succeeds).
    pub has_owner: bool,
    /// Cloaked by DWM (UWP / Store apps that are not really on screen).
    pub cloaked: bool,
    /// This window is the Nexora island itself.
    pub is_nexora_island: bool,
}

/// Pure decision: does this window belong in the taskbar list?
///
/// This is the Alt-Tab rule (see Raymond Chen, "How does Alt-Tab decide
/// which windows to show?"): a window lists when it is visible, titled,
/// not cloaked, not a tool window (unless it asked for an app-window
/// style), and has no owner. The island itself is always excluded.
pub fn should_list_window(props: WindowProps) -> bool {
    props.visible
        && props.has_title
        && !props.cloaked
        && !props.is_nexora_island
        && (!props.is_tool_window || props.is_app_window)
        && (!props.has_owner || props.is_app_window)
}

/// Pure decision: what does a click on this window do?
///
/// A click on the foreground window minimizes it (standard taskbar
/// behaviour); a click on any other window activates it.
pub fn toggle_action(is_foreground: bool) -> ToggleAction {
    if is_foreground {
        ToggleAction::Minimize
    } else {
        ToggleAction::Activate
    }
}

/// File stem of a path (file name without its extension), using plain
/// string operations so it stays testable off Windows.
fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(0) => name,
        Some(i) => &name[..i],
        None => name,
    }
}

/// Display name for a pinned app: the link description first, then the
/// target's file stem, then the link file's own stem.
pub fn pin_display_name(description: &str, target: Option<&str>, link_stem: &str) -> String {
    let desc = description.trim();
    if !desc.is_empty() {
        return desc.to_string();
    }
    if let Some(t) = target {
        let stem = file_stem(t);
        if !stem.is_empty() {
            return stem.to_string();
        }
    }
    link_stem.to_string()
}

/// All open windows, in Z-order (frontmost first), filtered by the
/// Alt-Tab rule. Empty off Windows.
pub fn list_windows() -> Vec<TaskbarWindow> {
    #[cfg(target_os = "windows")]
    {
        win::list_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// All apps pinned to the user's taskbar, in file-name order. Empty off
/// Windows. Links that cannot be resolved to a file path (notably UWP /
/// Store pins) are skipped.
pub fn list_pinned() -> Vec<PinnedApp> {
    #[cfg(target_os = "windows")]
    {
        win::list_pinned()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Click on a taskbar window: minimize it when it is the foreground
/// window, otherwise bring it to the front (restoring it first when
/// minimized). Returns whether the window ended up in the requested
/// state. `false` off Windows and for a dead handle.
pub fn activate_window(id: i64) -> bool {
    #[cfg(target_os = "windows")]
    {
        win::activate_window(id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        false
    }
}

/// Press the Windows key, which opens (or closes) the Start menu.
/// `false` off Windows and when the synthetic input was not delivered.
pub fn open_start_menu() -> bool {
    #[cfg(target_os = "windows")]
    {
        win::open_start_menu()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Launch a pinned app by its resolved target path — the same `target`
/// a `PinnedApp` carries. `true` means the launch was invoked;
/// `false` means it was not (empty path, or the launch failed).
/// `false` off Windows. Opens nothing else and logs nothing.
/// Normalizes a Windows path for case-insensitive allowlist comparison.
pub fn normalize_target_path(raw: &str) -> String {
    raw.trim().replace('/', "\\").to_lowercase()
}

/// Returns `true` if `target` uses a URL scheme (e.g. `http:`, `file:`,
/// `javascript:`) or UNC path (`\\server\share`).
pub fn has_url_scheme_or_unc(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        return true;
    }
    if let Some(colon) = trimmed.find(':') {
        if colon >= 2 {
            let prefix = &trimmed[..colon];
            if prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
            {
                return true;
            }
        }
    }
    false
}

/// Validates that `target` exactly matches (case-insensitively, normalized)
/// one of the pinned app targets returned by `list_pinned()`.
pub fn is_allowed_pinned_target(target: &str, allowed_targets: &[String]) -> bool {
    let norm = normalize_target_path(target);
    if norm.is_empty() {
        return false;
    }
    if allowed_targets
        .iter()
        .any(|allowed| normalize_target_path(allowed) == norm)
    {
        return true;
    }
    let _ = has_url_scheme_or_unc(target);
    false
}

pub fn launch_pinned_app(target: &str) -> bool {
    let allowed: Vec<String> = list_pinned().into_iter().map(|p| p.target).collect();
    if !is_allowed_pinned_target(target, &allowed) {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        win::launch_pinned_app(target)
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use base64::Engine as _;
    use std::ffi::c_void;
    use windows::core::{Interface as _, BOOL, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC, HGDIOBJ, RGBQUAD,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VK_LWIN,
    };
    use windows::Win32::UI::Shell::{
        ExtractIconExW, FOLDERID_RoamingAppData, IShellLinkW, SHGetKnownFolderPath, ShellExecuteW,
        ShellLink, KF_FLAG_DEFAULT, SLR_NOSEARCH, SLR_NOUPDATE, SLR_NO_UI,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, DrawIconEx, EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
        ShowWindow, DI_NORMAL, GWL_EXSTYLE, GW_OWNER, SW_MINIMIZE, SW_RESTORE, SW_SHOWNORMAL,
    };

    /// One enumerated window before filtering.
    struct RawWindow {
        hwnd: HWND,
        title: String,
        minimized: bool,
        active: bool,
        exe_path: String,
        props: WindowProps,
    }

    /// Null-terminated UTF-16 for a WinAPI string parameter. The Vec must
    /// outlive the call.
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Read a null-terminated UTF-16 string out of a `PWSTR` without
    /// taking ownership (the caller frees it).
    fn pwstr_to_string(p: PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        unsafe {
            let mut len = 0usize;
            while *p.0.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p.0, len))
        }
    }

    /// Read a null-terminated UTF-16 buffer filled by a WinAPI call.
    fn buf_to_string(buf: &[u16], len: usize) -> String {
        let len = len.min(buf.len());
        let end = buf[..len].iter().position(|&c| c == 0).unwrap_or(len);
        String::from_utf16_lossy(&buf[..end])
    }

    /// Whether `DwmGetWindowAttribute(DWMWA_CLOAKED)` says this window is
    /// cloaked. Any failure reads as "not cloaked" (older Windows has no
    /// cloaking; the filter then falls back to visibility alone).
    fn is_cloaked(hwnd: HWND) -> bool {
        let mut cloaked: u32 = 0;
        let ok = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut c_void,
                std::mem::size_of::<u32>() as u32,
            )
        }
        .is_ok();
        ok && cloaked != 0
    }

    /// `EnumWindows` callback: collect every top-level window's raw
    /// properties. `lparam` carries a `*mut Vec<RawWindow>`; enumeration
    /// is synchronous, so the pointer stays valid.
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<RawWindow>) };
        let visible = is_window_visible(hwnd);
        let mut title_buf = [0u16; 512];
        let title_len = unsafe { GetWindowTextW(hwnd, &mut title_buf) };
        let title = buf_to_string(&title_buf, title_len.max(0) as usize);
        let exstyle = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        let props = WindowProps {
            visible,
            has_title: !title.trim().is_empty(),
            is_tool_window: exstyle & 0x80 != 0, // WS_EX_TOOLWINDOW
            is_app_window: exstyle & 0x40000 != 0, // WS_EX_APPWINDOW
            has_owner: unsafe { GetWindow(hwnd, GW_OWNER) }
                .map(|h| !h.0.is_null())
                .unwrap_or(false),
            cloaked: is_cloaked(hwnd),
            is_nexora_island: hwnd.0 as isize == crate::taskbar::island_window(),
        };
        let exe_path = process_exe_path(hwnd);
        out.push(RawWindow {
            hwnd,
            title,
            minimized: unsafe { IsIconic(hwnd) }.as_bool(),
            active: unsafe { GetForegroundWindow() } == hwnd,
            exe_path,
            props,
        });
        BOOL(1)
    }

    /// Resolves the full executable path for any top-level window across
    /// processes using `QueryFullProcessImageNameW` (unlike
    /// `GetWindowModuleFileNameW`, which only works within the caller process).
    fn process_exe_path(hwnd: HWND) -> String {
        let mut pid: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if pid == 0 {
            return String::new();
        }
        let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            Ok(h) => h,
            Err(_) => return String::new(),
        };
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let ok = unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut size,
            )
        }
        .is_ok();
        let _ = unsafe { CloseHandle(handle) };
        if ok && size > 0 {
            buf_to_string(&buf, size as usize)
        } else {
            String::new()
        }
    }

    fn is_window_visible(hwnd: HWND) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        unsafe { IsWindowVisible(hwnd) }.as_bool()
    }

    pub fn list_windows() -> Vec<TaskbarWindow> {
        let mut raw: Vec<RawWindow> = Vec::new();
        let lparam = LPARAM(&mut raw as *mut Vec<RawWindow> as isize);
        // SAFETY: `enum_proc` only pushes into `raw`, which outlives the
        // synchronous enumeration.
        let _ = unsafe { EnumWindows(Some(enum_proc), lparam) };
        raw.into_iter()
            .filter(|w| should_list_window(w.props))
            .map(|w| {
                let (icon_size, icon_rgba_base64) = icon_for_window(&w.exe_path);
                TaskbarWindow {
                    id: w.hwnd.0 as i64,
                    title: w.title,
                    minimized: w.minimized,
                    active: w.active,
                    icon_size,
                    icon_rgba_base64,
                }
            })
            .collect()
    }

    /// Icon for a window's executable. UWP windows report
    /// ApplicationFrameHost.exe, which would give every Store app the
    /// same icon, so they get no icon instead.
    fn icon_for_window(exe_path: &str) -> (u32, String) {
        if exe_path.is_empty() {
            return (0, String::new());
        }
        let file = exe_path.rsplit(['\\', '/']).next().unwrap_or("");
        if file.eq_ignore_ascii_case("ApplicationFrameHost.exe") {
            return (0, String::new());
        }
        icon_rgba_for_file(exe_path, 0)
    }

    /// First icon of `path` at `index`, as (size, base64 RGBA).
    /// (0, "") when the file has no usable icon.
    fn icon_rgba_for_file(path: &str, index: i32) -> (u32, String) {
        let wide_path = wide(path);
        let mut large = windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut());
        // SAFETY: `wide_path` outlives the call; `large` receives the
        // icon handle, owned by us afterwards.
        let count = unsafe {
            ExtractIconExW(
                PCWSTR::from_raw(wide_path.as_ptr()),
                index,
                Some(&mut large as *mut _),
                None,
                1,
            )
        };
        if count == 0 || large.0.is_null() {
            return (0, String::new());
        }
        let out = hicon_to_rgba(large, ICON_SIZE);
        // SAFETY: `large` came from `ExtractIconExW` in this call.
        unsafe {
            let _ = DestroyIcon(large);
        }
        out
    }

    /// Draw an HICON into a 32-bit top-down DIB and return (size, base64
    /// RGBA). Icons without an alpha channel come back fully opaque.
    fn hicon_to_rgba(
        hicon: windows::Win32::UI::WindowsAndMessaging::HICON,
        size: u32,
    ) -> (u32, String) {
        unsafe {
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size as i32,
                    // Negative height: top-down rows, matching RGBA order.
                    biHeight: -(size as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [RGBQUAD {
                    rgbBlue: 0,
                    rgbGreen: 0,
                    rgbRed: 0,
                    rgbReserved: 0,
                }],
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                return (0, String::new());
            };
            let dc: HDC = CreateCompatibleDC(None);
            if dc.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                return (0, String::new());
            }
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            let drawn = DrawIconEx(
                dc,
                0,
                0,
                hicon,
                size as i32,
                size as i32,
                0,
                None,
                DI_NORMAL,
            )
            .is_ok();
            let _ = SelectObject(dc, old);
            let _ = DeleteDC(dc);
            if !drawn || bits.is_null() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                return (0, String::new());
            }
            let len = (size * size * 4) as usize;
            let src = std::slice::from_raw_parts(bits as *const u8, len);
            // DIB pixels are BGRA; canvas RGBA wants the channels swapped.
            let mut rgba = Vec::with_capacity(len);
            let mut any_alpha = false;
            for px in src.as_chunks::<4>().0 {
                if px[3] != 0 {
                    any_alpha = true;
                }
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
            // Free the DIB section only AFTER reading `bits` to prevent UAF crash.
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            if !any_alpha {
                // No alpha channel in this icon: treat it as opaque, or
                // the frontend would draw it fully transparent.
                for px in rgba.as_chunks_mut::<4>().0 {
                    px[3] = 255;
                }
            }
            (
                size,
                base64::engine::general_purpose::STANDARD.encode(&rgba),
            )
        }
    }

    /// RAII COM apartment for this thread. Only uninitializes when this
    /// guard actually initialized COM (a thread that already had COM up
    /// under another model is left alone).
    struct ComGuard {
        initialized: bool,
    }

    impl ComGuard {
        fn init() -> Self {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            // S_OK (0) / S_FALSE (1): we own the init. RPC_E_CHANGED_MODE
            // means COM is already up under another model; proceed
            // best-effort without uninitializing.
            ComGuard {
                initialized: hr.0 == 0 || hr.0 == 1,
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.initialized {
                unsafe { CoUninitialize() };
            }
        }
    }

    /// The user's pinned-taskbar folder:
    /// `%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar`.
    fn pinned_folder() -> Option<std::path::PathBuf> {
        // SAFETY: `SHGetKnownFolderPath` allocates the string; freed below.
        let path = unsafe { SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None) }
            .ok()?;
        let appdata = pwstr_to_string(path);
        unsafe { CoTaskMemFree(Some(path.0 as *const c_void)) };
        if appdata.is_empty() {
            return None;
        }
        Some(
            std::path::Path::new(&appdata)
                .join("Microsoft")
                .join("Internet Explorer")
                .join("Quick Launch")
                .join("User Pinned")
                .join("TaskBar"),
        )
    }

    /// Resolve one .lnk through `IShellLinkW`: description, target path,
    /// and the icon location the link itself names (if any).
    fn resolve_link(path: &std::path::Path) -> Option<(String, String, String, i32)> {
        let wide_path = wide(&path.to_string_lossy());
        // SAFETY: `wide_path` outlives the call; the COM object is
        // created and used only inside this function.
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        unsafe { persist.Load(PCWSTR::from_raw(wide_path.as_ptr()), STGM_READ) }.ok()?;
        unsafe {
            link.Resolve(
                HWND(std::ptr::null_mut()),
                (SLR_NO_UI.0 | SLR_NOUPDATE.0 | SLR_NOSEARCH.0) as u32,
            )
        }
        .ok()?;
        let mut target_buf = [0u16; 260];
        unsafe { link.GetPath(&mut target_buf, std::ptr::null_mut(), 0) }.ok()?;
        let target = buf_to_string(&target_buf, 260);
        let mut desc_buf = [0u16; 260];
        let description = unsafe { link.GetDescription(&mut desc_buf) }
            .ok()
            .map(|_| buf_to_string(&desc_buf, 260))
            .unwrap_or_default();
        let mut icon_buf = [0u16; 260];
        let mut icon_index: i32 = 0;
        let (icon_path, icon_index) =
            match unsafe { link.GetIconLocation(&mut icon_buf, &mut icon_index) } {
                Ok(()) => (buf_to_string(&icon_buf, 260), icon_index),
                Err(_) => (String::new(), 0),
            };
        Some((description, target, icon_path, icon_index))
    }

    pub fn list_pinned() -> Vec<PinnedApp> {
        let _com = ComGuard::init();
        let Some(folder) = pinned_folder() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&folder) else {
            return Vec::new();
        };
        let mut links: Vec<std::path::PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x.eq_ignore_ascii_case("lnk"))
                    .unwrap_or(false)
            })
            .collect();
        links.sort();
        let mut out = Vec::new();
        for link_path in links {
            let link_stem = link_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let Some((description, target, icon_path, icon_index)) = resolve_link(&link_path)
            else {
                continue;
            };
            if target.is_empty() {
                // UWP / Store pins carry an AppUserModelId instead of a
                // file path; resolving and launching them needs the
                // package manager, out of scope for this branch.
                continue;
            }
            let name = pin_display_name(&description, Some(&target), &link_stem);
            let (icon_size, icon_rgba_base64) = if !icon_path.is_empty() {
                let icon = icon_rgba_for_file(&icon_path, icon_index);
                if icon.0 == 0 {
                    icon_rgba_for_file(&target, 0)
                } else {
                    icon
                }
            } else {
                icon_rgba_for_file(&target, 0)
            };
            out.push(PinnedApp {
                name,
                target,
                icon_size,
                icon_rgba_base64,
            });
        }
        out
    }

    pub fn activate_window(id: i64) -> bool {
        let hwnd = HWND(id as *mut c_void);
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return false;
        }
        let is_foreground = unsafe { GetForegroundWindow() } == hwnd;
        match toggle_action(is_foreground) {
            ToggleAction::Minimize => {
                // SAFETY: `hwnd` was validated with `IsWindow` above.
                // The return value only reports the previous visibility
                // state; the live check below is the result.
                let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
                unsafe { IsIconic(hwnd) }.as_bool()
            }
            ToggleAction::Activate => {
                // SAFETY: as above.
                let was_minimized = unsafe { IsIconic(hwnd) }.as_bool();
                if was_minimized {
                    // Return value is the previous state, not success;
                    // the live check below is the result.
                    let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
                }
                let foreground = unsafe { SetForegroundWindow(hwnd) }.as_bool();
                foreground || (was_minimized && !unsafe { IsIconic(hwnd) }.as_bool())
            }
        }
    }

    /// One synthetic Windows-key event (press or release).
    fn win_key_input(key_up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    wScan: 0,
                    dwFlags: if key_up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn open_start_menu() -> bool {
        // Press and release the Windows key; the shell opens (or closes)
        // the Start menu itself. No window handle is involved.
        let inputs = [win_key_input(false), win_key_input(true)];
        // SAFETY: `inputs` is a plain two-element array of INPUT structs.
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        sent == inputs.len() as u32
    }

    pub fn launch_pinned_app(target: &str) -> bool {
        let wide_target = wide(target);
        let wide_open = wide("open");
        // SAFETY: both wide buffers outlive the call; no owner window,
        // no parameters, no working directory. `ShellExecuteW` returns
        // an HINSTANCE whose value is above 32 on success — at or
        // below 32 is an error code, never a handle.
        let instance: HINSTANCE = unsafe {
            ShellExecuteW(
                None,
                PCWSTR::from_raw(wide_open.as_ptr()),
                PCWSTR::from_raw(wide_target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        instance.0 as isize > 32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normal_props() -> WindowProps {
        WindowProps {
            visible: true,
            has_title: true,
            is_tool_window: false,
            is_app_window: false,
            has_owner: false,
            cloaked: false,
            is_nexora_island: false,
        }
    }

    #[test]
    fn alt_tab_filter_lists_a_normal_window() {
        assert!(should_list_window(normal_props()));
    }

    #[test]
    fn alt_tab_filter_rejects_each_bad_property() {
        let mut p = normal_props();
        p.visible = false;
        assert!(!should_list_window(p));
        let mut p = normal_props();
        p.has_title = false;
        assert!(!should_list_window(p));
        let mut p = normal_props();
        p.cloaked = true;
        assert!(!should_list_window(p));
        let mut p = normal_props();
        p.is_nexora_island = true;
        assert!(!should_list_window(p));
        let mut p = normal_props();
        p.has_owner = true;
        assert!(!should_list_window(p));
    }

    #[test]
    fn alt_tab_filter_tool_window_needs_app_window_style() {
        let mut p = normal_props();
        p.is_tool_window = true;
        assert!(!should_list_window(p));
        p.is_app_window = true;
        assert!(should_list_window(p));
    }

    #[test]
    fn click_foreground_minimizes_and_click_background_activates() {
        assert_eq!(toggle_action(true), ToggleAction::Minimize);
        assert_eq!(toggle_action(false), ToggleAction::Activate);
    }

    #[test]
    fn pin_display_name_prefers_description_then_target_then_link() {
        assert_eq!(
            pin_display_name("Google Chrome", Some("C:\\x\\chrome.exe"), "chrome"),
            "Google Chrome"
        );
        assert_eq!(
            pin_display_name(
                "",
                Some("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"),
                "chrome"
            ),
            "chrome"
        );
        assert_eq!(
            pin_display_name("   ", Some("C:\\x\\code.exe"), "vscode"),
            "code"
        );
        assert_eq!(pin_display_name("", None, "notepad"), "notepad");
    }

    #[test]
    fn file_stem_strips_directory_and_extension() {
        assert_eq!(file_stem("C:\\a\\b\\chrome.exe"), "chrome");
        assert_eq!(file_stem("my.app.exe"), "my.app");
        assert_eq!(file_stem("noext"), "noext");
        assert_eq!(file_stem("C:/a/b/notepad.exe"), "notepad");
    }

    #[test]
    fn launch_empty_target_is_false() {
        assert!(!launch_pinned_app(""));
    }

    #[test]
    fn launch_nonexistent_target_is_false() {
        assert!(!launch_pinned_app("C:\\does\\not\\exist\\app.exe"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_is_a_noop() {
        assert!(list_windows().is_empty());
        assert!(list_pinned().is_empty());
        assert!(!activate_window(12345));
        assert!(!open_start_menu());
        assert!(!launch_pinned_app("C:\\Windows\\notepad.exe"));
    }

    #[test]
    fn pinned_target_allowlist_validates_allowed_and_rejects_unsafe() {
        let allowed = vec![
            r"C:\Program Files\App\app.exe".to_string(),
            r"\\fileserver\apps\pinned.exe".to_string(),
        ];
        assert!(is_allowed_pinned_target(
            r"c:/program files/app/app.exe",
            &allowed
        ));
        assert!(is_allowed_pinned_target(
            r"\\fileserver\apps\pinned.exe",
            &allowed
        ));
        assert!(!is_allowed_pinned_target("", &allowed));
        assert!(!is_allowed_pinned_target("   ", &allowed));
        assert!(!is_allowed_pinned_target(
            r"C:\Windows\System32\cmd.exe",
            &allowed
        ));
        assert!(!is_allowed_pinned_target(
            r"\\evil\share\run.exe",
            &allowed
        ));
        assert!(!is_allowed_pinned_target("http://example.com", &allowed));
        assert!(!is_allowed_pinned_target(
            "file:///C:/Windows/calc.exe",
            &allowed
        ));
        assert!(!is_allowed_pinned_target("javascript:alert(1)", &allowed));
        assert!(has_url_scheme_or_unc("http://example.com"));
        assert!(has_url_scheme_or_unc("javascript:alert(1)"));
        assert!(has_url_scheme_or_unc(r"\\evil\share\a.exe"));
        assert!(!has_url_scheme_or_unc(r"C:\Program Files\App\app.exe"));
    }
}
