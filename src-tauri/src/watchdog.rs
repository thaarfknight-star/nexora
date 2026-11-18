use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WatchdogStatus {
    pub healthy: bool,
    pub explorer_ok: bool,
}

pub fn check_health() -> WatchdogStatus {
    WatchdogStatus {
        healthy: true,
        explorer_ok: true,
    }
}
