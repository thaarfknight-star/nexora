use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub memory_percent: f32,
}

pub fn collect() -> SystemStats {
    SystemStats {
        cpu_percent: 4.2,
        memory_percent: 38.0,
    }
}
