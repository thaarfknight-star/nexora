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
//! - Transport commands are separate play and pause (DEVagent's confirmed
//!   names): `media_play`, `media_pause`, `media_next`, `media_previous`.
//!   This module exposes the matching `play` / `pause` / `next` /
//!   `previous` functions; the IPC share registers them in `ipc.rs`.

#[cfg(any(target_os = "windows", test))]
use base64::Engine;
use serde::Serialize;

#[cfg(target_os = "windows")]
use windows::{
    core::{Result as WinResult, HSTRING},
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionMediaProperties,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    },
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
    MediaInfo {
        playing: is_playing(status),
        title,
        artist,
        album,
        cover_art,
        source_app: hstring_or(session.SourceAppUserModelId(), ""),
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
    let Ok(thumbnail) = properties.Thumbnail() else {
        return String::new();
    };
    let Ok(operation) = thumbnail.OpenReadAsync() else {
        return String::new();
    };
    let Ok(stream) = operation.await else {
        return String::new();
    };
    let content_type = hstring_or(stream.ContentType(), "image/jpeg");
    let Ok(size) = stream.Size() else {
        return String::new();
    };
    if size == 0 || size > 1_048_576 {
        return String::new();
    }
    let Ok(reader) = DataReader::CreateDataReader(&stream) else {
        return String::new();
    };
    let Ok(loaded) = reader.LoadAsync(size as u32) else {
        return String::new();
    };
    let Ok(loaded) = loaded.await else {
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
    fn now_playing_returns_a_struct_without_panicking() {
        let info = tauri::async_runtime::block_on(now_playing());
        assert!(!info.title.is_empty());
        assert!(matches!(
            info.playback_status.as_str(),
            "playing" | "paused" | "stopped" | "closed" | "changing" | "none"
        ));
        assert!(info.cover_art.is_empty() || info.cover_art.starts_with("data:image/"));
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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn fallback_reports_no_media() {
        let info = tauri::async_runtime::block_on(now_playing());
        assert!(!info.playing);
        assert_eq!(info.title, "No active media");
        assert_eq!(info.artist, "Nexora");
        assert_eq!(info.playback_status, "none");
        assert_eq!(info.cover_art, "");
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
