use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TaskbarStatus {
    pub hidden: bool,
    pub mode: String,
}

pub fn status() -> TaskbarStatus {
    TaskbarStatus {
        hidden: false,
        mode: "island-shell".into(),
    }
}
