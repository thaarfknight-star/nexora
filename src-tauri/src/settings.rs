use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct NexoraSettings {
    pub auto_hide_taskbar: bool,
    pub always_on_top: bool,
    pub theme: String,
}

pub fn defaults() -> NexoraSettings {
    NexoraSettings {
        auto_hide_taskbar: false,
        always_on_top: true,
        theme: "obsidian".into(),
    }
}
