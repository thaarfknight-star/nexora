use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub update_available: bool,
}

pub fn check() -> UpdateInfo {
    UpdateInfo {
        current_version: env!("CARGO_PKG_VERSION").into(),
        update_available: false,
    }
}
