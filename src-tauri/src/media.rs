use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MediaInfo {
    pub playing: bool,
    pub title: String,
    pub artist: String,
}

pub fn now_playing() -> MediaInfo {
    MediaInfo {
        playing: false,
        title: "No active media".into(),
        artist: "Nexora".into(),
    }
}
