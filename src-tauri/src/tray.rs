use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TrayStatus {
    pub active: bool,
    pub items: usize,
}

pub fn status() -> TrayStatus {
    TrayStatus {
        active: true,
        items: 0,
    }
}
