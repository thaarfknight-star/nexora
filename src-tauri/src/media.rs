//! Live media information and transport controls through the Windows
//! Global System Media Transport Controls (GSMTC) session manager.
//!
//! Design notes:
//! - On Windows the session manager is requested fresh on every call.
//!   GSMTC sessions come and go as apps start/stop playback, so caching a
//!   stale session would report dead media; a fresh request is cheap
//!   compared to a wrong answer.
//! - The WinRT runtime is initialised best-effort (multithreaded
//!   apartment) on the calling thread; if it is already initialised the
//!   call is a harmless no-op and the error is ignored.
//! - No media session at all (or any WinRT failure) is not an error: the
//!   caller gets the "no active media" value, exactly like the old stub.
//! - On non-Windows targets the module keeps the same public API and
//!   always reports "no active media"; the `windows` crate is only a
//!   Windows dependency (see `Cargo.toml`).
//! - Album cover art comes from the session thumbnail
//!   (`GetThumbnailAsync`): the bytes are returned as a `data:` URI in
//!   `MediaInfo.cover_art` (empty when the session has no thumbnail). The
//!   frontend uses it directly as the `<img>` source.
//! - The source app's icon comes from the session's AUMID: the package
//!   family name (the part before `!`) is matched against the installed
//!   packages and the package logo file is read from the install folder.
//!   Anything missing means the empty string, never a substitute icon.
//! - Transport commands are separate play and pause (DEVagent's confirmed
//!   names): `media_play`, `media_pause`, `media_next`, `media_previous`.
//!   This module exposes the matching `play` / `pause` / `next` /
//!   `previous` functions; the IPC share registers them in `ipc.rs`.

#[cfg(any(target_os = "windows", test))]
use base64::Engine;
use serde::Serialize;

#[cfg(target_os = "windows")]
use windows::{
    core::{AgileReference, Result as WinResult, HSTRING},
    ApplicationModel::Package,
    Foundation::Uri,
    Management::Deployment::PackageManager,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionMediaProperties,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    },
    Storage::PathIO,
    Storage::Streams::DataReader,
    Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED},
};

/// What is currently playing, as reported by the system media session.
/// `cover_art` is a `data:` URI with the album cover art, ready to use as an
/// `<img>` source; empty when the session has no thumbnail.
#[derive(Debug, Clone, Serialize)]
pub struct MediaInfo {
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source_app: String,
    pub source_icon: String,
    pub playback_status: String,
    pub cover_art: String,
}

impl Default for MediaInfo {
    fn default() -> Self {
        Self {
            playing: false,
            title: "No active media".into(),
            artist: "Nexora".into(),
            album: String::new(),
            source_app: String::new(),
            source_icon: String::new(),
            playback_status: "none".into(),
            cover_art: String::new(),
        }
    }
}

/// The current track, live from the GSMTC session. Falls back to the
/// "no active media" value when nothing is playing.
#[cfg(target_os = "windows")]
pub async fn now_playing() -> MediaInfo {
    let Ok(session) = current_session().await else {
        return MediaInfo::default();
    };
    let status = session
        .GetPlaybackInfo()
        .and_then(|info| info.PlaybackStatus())
        .unwrap_or(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped);
    let (title, artist, album, cover_art) = match session.TryGetMediaPropertiesAsync() {
        Ok(operation) => match operation.await {
            Ok(properties) => (
                hstring_or(properties.Title(), "Unknown title"),
                hstring_or(properties.Artist(), "Unknown artist"),
                hstring_or(properties.AlbumTitle(), ""),
                read_cover(&properties).await,
            ),
            Err(_) => (
                "Unknown title".into(),
                "Unknown artist".into(),
                String::new(),
                String::new(),
            ),
        },
        Err(_) => (
            "Unknown title".into(),
            "Unknown artist".into(),
            String::new(),
            String::new(),
        ),
    };
    let source_app = hstring_or(session.SourceAppUserModelId(), "");
    let source_icon = read_source_icon(&source_app).await;
    MediaInfo {
        playing: is_playing(status),
        title,
        artist,
        album,
        cover_art,
        source_app,
        source_icon,
        playback_status: status_label(status).to_string(),
    }
}

/// The current track. On non-Windows targets there is no GSMTC session,
/// so this always reports "no active media".
#[cfg(not(target_os = "windows"))]
pub async fn now_playing() -> MediaInfo {
    MediaInfo::default()
}

/// Start playback on the current session (IPC command `media_play`).
/// Returns `true` only when a live session accepted the command.
#[cfg(target_os = "windows")]
pub async fn play() -> bool {
    try_transport(|session| session.TryPlayAsync()).await
}

/// Start playback. On non-Windows targets there is no session.
#[cfg(not(target_os = "windows"))]
pub async fn play() -> bool {
    false
}

/// Pause playback on the current session (IPC command `media_pause`).
/// Returns `true` only when a live session accepted the command.
#[cfg(target_os = "windows")]
pub async fn pause() -> bool {
    try_transport(|session| session.TryPauseAsync()).await
}

/// Pause playback. On non-Windows targets there is no session.
#[cfg(not(target_os = "windows"))]
pub async fn pause() -> bool {
    false
}

/// Skip to the next track (IPC command `media_next`). Returns `true`
/// only when a live session accepted the command.
#[cfg(target_os = "windows")]
pub async fn next() -> bool {
    try_transport(|session| session.TrySkipNextAsync()).await
}

/// Skip to the next track. On non-Windows targets there is no session.
#[cfg(not(target_os = "windows"))]
pub async fn next() -> bool {
    false
}

/// Skip to the previous track (IPC command `media_previous`). Returns
/// `true` only when a live session accepted the command.
#[cfg(target_os = "windows")]
pub async fn previous() -> bool {
    try_transport(|session| session.TrySkipPreviousAsync()).await
}

/// Skip to the previous track. On non-Windows targets there is no session.
#[cfg(not(target_os = "windows"))]
pub async fn previous() -> bool {
    false
}

/// Stop playback on the current session (IPC command `media_stop`). Returns
/// `true` only when a live session accepted the command.
#[cfg(target_os = "windows")]
pub async fn stop() -> bool {
    try_transport(|session| session.TryStopAsync()).await
}

/// Stop playback. On non-Windows targets there is no session.
#[cfg(not(target_os = "windows"))]
pub async fn stop() -> bool {
    false
}

#[cfg(target_os = "windows")]
async fn current_session() -> WinResult<GlobalSystemMediaTransportControlsSession> {
    ensure_winrt();
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.await?;
    manager.GetCurrentSession()
}

/// Run one transport command against the current session. Returns `true`
/// only when a live session accepted the command.
#[cfg(target_os = "windows")]
async fn try_transport<F>(operation: F) -> bool
where
    F: FnOnce(
        &GlobalSystemMediaTransportControlsSession,
    ) -> WinResult<windows_future::IAsyncOperation<bool>>,
{
    let Ok(session) = current_session().await else {
        return false;
    };
    match operation(&session) {
        Ok(operation) => operation.await.unwrap_or(false),
        Err(_) => false,
    }
}

/// Album cover art as a `data:` URI (`data:<mime>;base64,...`), or the
/// empty string when the session has no thumbnail or any step fails.
/// Thumbnails over 1 MiB are skipped: the island only shows a small card.
#[cfg(target_os = "windows")]
async fn read_cover(
    properties: &GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> String {
    // `IRandomAccessStreamReference` is not Send, so it never becomes a
    // local of this async fn: it is created and consumed inside this
    // closure, which only returns the (Send) read operation.
    let Some(operation) = (|| {
        let thumbnail = properties.Thumbnail().ok()?;
        thumbnail.OpenReadAsync().ok()
    })() else {
        return String::new();
    };
    // `IRandomAccessStreamWithContentType` is not Send either, so the
    // stream is wrapped in an agile reference the moment it comes out
    // of the await; the raw interface never becomes a local here.
    // (Moving or dropping the raw interface before an await does NOT
    // satisfy the compiler — it stays in the future's state.)
    let Ok(stream) = operation
        .await
        .map(|s| AgileReference::new(&s))
        .and_then(|r| r)
    else {
        return String::new();
    };
    // All stream work happens inside this closure: it resolves the
    // agile reference, reads the metadata, and starts the load. Only
    // Send values come back out.
    let Some((content_type, size, reader, loaded_op)) = (|| {
        let stream = stream.resolve().ok()?;
        let content_type = hstring_or(stream.ContentType(), "image/jpeg");
        let size = stream.Size().ok()?;
        if size == 0 || size > 1_048_576 {
            return None;
        }
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        let op = reader.LoadAsync(size as u32).ok()?;
        Some((content_type, size, reader, op))
    })() else {
        return String::new();
    };
    let Ok(loaded) = loaded_op.await else {
        return String::new();
    };
    if loaded as u64 != size {
        return String::new();
    }
    let mut bytes = vec![0u8; size as usize];
    if reader.ReadBytes(&mut bytes).is_err() {
        return String::new();
    }
    cover_data_uri(&bytes, &content_type)
}

/// Build the `data:` URI the frontend uses as the `<img>` source for the
/// album cover. Empty bytes mean "no cover" and stay the empty string;
/// an empty content type falls back to JPEG, the format GSMTC thumbnails
/// almost always use.
#[cfg(any(target_os = "windows", test))]
fn cover_data_uri(bytes: &[u8], content_type: &str) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mime = if content_type.is_empty() {
        "image/jpeg"
    } else {
        content_type
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

#[cfg(target_os = "windows")]
fn ensure_winrt() {
    // Best effort: already initialised (or initialised differently) is
    // fine for our calls, so the result is intentionally ignored.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}

#[cfg(target_os = "windows")]
fn hstring_or(value: WinResult<HSTRING>, fallback: &str) -> String {
    match value {
        Ok(text) => {
            let text = text.to_string();
            if text.is_empty() {
                fallback.to_string()
            } else {
                text
            }
        }
        Err(_) => fallback.to_string(),
    }
}

/// Plain `if` chain on purpose: the WinRT status enum may gain variants,
/// so no `match` arm list is asserted here.
#[cfg(target_os = "windows")]
fn status_label(status: GlobalSystemMediaTransportControlsSessionPlaybackStatus) -> &'static str {
    use GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;
    if status == Status::Playing {
        "playing"
    } else if status == Status::Paused {
        "paused"
    } else if status == Status::Stopped {
        "stopped"
    } else if status == Status::Closed {
        "closed"
    } else if status == Status::Changing {
        "changing"
    } else {
        "unknown"
    }
}

#[cfg(target_os = "windows")]
fn is_playing(status: GlobalSystemMediaTransportControlsSessionPlaybackStatus) -> bool {
    status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
}

/// The source app's icon, as a `data:` URI the frontend uses as the
/// `<img>` source in the media card. The icon comes from the session's
/// AUMID: the package family name (the part before `!`) is matched
/// against the installed packages, and the package logo file is read
/// from the package's install folder. Anything missing — no AUMID, no
/// package, no logo file — means the empty string, never a substitute
/// icon. The logo is capped at 1 MiB, like the cover art.
#[cfg(target_os = "windows")]
async fn read_source_icon(aumid: &str) -> String {
    let Some(family) = aumid.split('!').next().filter(|f| !f.is_empty()) else {
        return String::new();
    };
    let Some(package) = find_package(family) else {
        return String::new();
    };
    let Ok(uri) = package.Logo() else {
        return String::new();
    };
    let Ok(path) = logo_file_path(&package, &uri) else {
        return String::new();
    };
    for candidate in logo_candidates(&path) {
        if let Some(bytes) = read_file_bytes(&candidate).await {
            return icon_data_uri(&bytes, &candidate);
        }
    }
    String::new()
}

/// The installed package whose family name matches, if any. Win32 apps
/// installed the classic way have no package, so "none" is normal.
#[cfg(target_os = "windows")]
fn find_package(family: &str) -> Option<Package> {
    let packages = PackageManager::new().ok()?.FindPackages().ok()?;
    let iterator = packages.First().ok()?;
    while iterator.HasCurrent().unwrap_or(false) {
        if let Ok(package) = iterator.Current() {
            let matches = package
                .Id()
                .and_then(|id| id.FamilyName())
                .map(|name| name == family)
                .unwrap_or(false);
            if matches {
                return Some(package);
            }
        }
        if !iterator.MoveNext().unwrap_or(false) {
            break;
        }
    }
    None
}

/// The logo file's absolute path. A `file:` URI is already a path; an
/// `ms-appx:` URI is resolved against the package's install folder
/// (the caller is unpackaged, so the URI scheme cannot resolve it).
#[cfg(target_os = "windows")]
fn logo_file_path(package: &Package, uri: &Uri) -> WinResult<String> {
    let scheme = uri.SchemeName()?.to_string();
    let logo_path = uri.Path()?.to_string();
    if scheme.eq_ignore_ascii_case("file") {
        return Ok(logo_path);
    }
    let folder = package.InstalledLocation()?.Path()?.to_string();
    Ok(format!(
        "{}\\{}",
        folder,
        logo_path.trim_start_matches('/').replace('/', "\\")
    ))
}

/// Logo file candidates: the exact path first, then the scale variants
/// AppX packages store on disk (`StoreLogo.scale-200.png` and friends).
/// Only image extensions get variants; anything else stays exact-only.
#[cfg(any(target_os = "windows", test))]
fn logo_candidates(path: &str) -> Vec<String> {
    let mut out = vec![path.to_string()];
    let lower = path.to_ascii_lowercase();
    if !(lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")) {
        return out;
    }
    if let Some(dot) = path.rfind('.') {
        let (base, ext) = path.split_at(dot);
        for scale in [
            "scale-100",
            "scale-125",
            "scale-150",
            "scale-200",
            "scale-400",
        ] {
            out.push(format!("{base}.{scale}{ext}"));
        }
    }
    out
}

/// The file's bytes, or `None` on any failure. Empty files and anything
/// over 1 MiB are rejected, like the cover art.
#[cfg(target_os = "windows")]
async fn read_file_bytes(path: &str) -> Option<Vec<u8>> {
    // `IBuffer` is not Send, so it is wrapped in an agile reference
    // the moment it comes out of the await; the raw interface never
    // becomes a local of this async fn.
    let buffer = PathIO::ReadBufferAsync(&HSTRING::from(path))
        .ok()?
        .await
        .map(|b| AgileReference::new(&b))
        .and_then(|r| r)
        .ok()?;
    let len = (|| {
        let buffer = buffer.resolve().ok()?;
        buffer.Length().ok().map(|l| l as usize)
    })()
    .filter(|&l| l != 0 && l <= 1_048_576)?;
    // The reader and the load operation are built inside this closure
    // from the resolved buffer; only Send values come back out.
    let (reader, loaded_op) = (|| {
        let buffer = buffer.resolve().ok()?;
        let reader = DataReader::FromBuffer(&buffer).ok()?;
        let op = reader.LoadAsync(len as u32).ok()?;
        Some((reader, op))
    })()?;
    let loaded = loaded_op.await.ok()?;
    if loaded as usize != len {
        return None;
    }
    let mut bytes = vec![0u8; len];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

/// Build the `data:` URI the frontend uses as the `<img>` source for the
/// source app's icon. Empty bytes mean "no icon" and stay the empty
/// string; the MIME type comes from the file extension and defaults to
/// PNG, the format package logos almost always use.
#[cfg(any(target_os = "windows", test))]
fn icon_data_uri(bytes: &[u8], path: &str) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let lower = path.to_ascii_lowercase();
    let mime = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "image/png"
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn playback_status_labels_cover_all_variants() {
        use GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;
        assert_eq!(status_label(Status::Playing), "playing");
        assert_eq!(status_label(Status::Paused), "paused");
        assert_eq!(status_label(Status::Stopped), "stopped");
        assert_eq!(status_label(Status::Closed), "closed");
        assert_eq!(status_label(Status::Changing), "changing");
        assert!(is_playing(Status::Playing));
        assert!(!is_playing(Status::Paused));
        assert!(!is_playing(Status::Stopped));
        assert!(!is_playing(Status::Closed));
        assert!(!is_playing(Status::Changing));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn media_futures_are_send() {
        // Every async media function is awaited by a `#[tauri::command]`,
        // which requires the future to be Send. This pins that down at
        // compile time, so a non-Send capture fails here instead of
        // only in CI.
        fn assert_send<T: Send>(_: T) {}
        assert_send(now_playing());
        assert_send(play());
        assert_send(pause());
        assert_send(next());
        assert_send(previous());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn now_playing_returns_a_struct_without_panicking() {
        let info = tauri::async_runtime::block_on(now_playing());
        assert!(!info.title.is_empty());
        assert!(matches!(
            info.playback_status.as_str(),
            "playing" | "paused" | "stopped" | "closed" | "changing" | "none"
        ));
        assert!(info.cover_art.is_empty() || info.cover_art.starts_with("data:image/"));
        assert!(info.source_icon.is_empty() || info.source_icon.starts_with("data:image/"));
    }

    #[test]
    fn cover_data_uri_builds_a_usable_img_source() {
        assert_eq!(cover_data_uri(&[], "image/jpeg"), "");
        assert_eq!(
            cover_data_uri(b"hi", "image/jpeg"),
            "data:image/jpeg;base64,aGk="
        );
        assert_eq!(cover_data_uri(b"hi", ""), "data:image/jpeg;base64,aGk=");
        assert_eq!(
            cover_data_uri(&[0x89, 0x50], "image/png"),
            "data:image/png;base64,iVA="
        );
    }

    #[test]
    fn icon_data_uri_builds_a_usable_img_source() {
        assert_eq!(icon_data_uri(&[], "logo.png"), "");
        assert_eq!(
            icon_data_uri(b"hi", "logo.png"),
            "data:image/png;base64,aGk="
        );
        assert_eq!(
            icon_data_uri(b"hi", "logo.jpg"),
            "data:image/jpeg;base64,aGk="
        );
        assert_eq!(
            icon_data_uri(b"hi", "logo.unknown"),
            "data:image/png;base64,aGk="
        );
    }

    #[test]
    fn logo_candidates_try_the_exact_path_first_then_scales() {
        assert_eq!(
            logo_candidates("C:\\pkg\\StoreLogo.png"),
            vec![
                "C:\\pkg\\StoreLogo.png",
                "C:\\pkg\\StoreLogo.scale-100.png",
                "C:\\pkg\\StoreLogo.scale-125.png",
                "C:\\pkg\\StoreLogo.scale-150.png",
                "C:\\pkg\\StoreLogo.scale-200.png",
                "C:\\pkg\\StoreLogo.scale-400.png",
            ]
        );
        assert_eq!(
            logo_candidates("C:\\pkg\\readme.txt"),
            vec!["C:\\pkg\\readme.txt"]
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn fallback_reports_no_media() {
        let info = tauri::async_runtime::block_on(now_playing());
        assert!(!info.playing);
        assert_eq!(info.title, "No active media");
        assert_eq!(info.artist, "Nexora");
        assert_eq!(info.playback_status, "none");
        assert_eq!(info.cover_art, "");
        assert_eq!(info.source_icon, "");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn fallback_transport_controls_report_no_session() {
        assert!(!tauri::async_runtime::block_on(play()));
        assert!(!tauri::async_runtime::block_on(pause()));
        assert!(!tauri::async_runtime::block_on(next()));
        assert!(!tauri::async_runtime::block_on(previous()));
    }
}
