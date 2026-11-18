use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use sysinfo::System;

#[derive(Debug, Clone, Serialize)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub memory_percent: f32,
}

/// One shared `System` for the process lifetime.
///
/// `sysinfo` computes CPU usage from the delta between two refreshes, so a
/// fresh `System` on every call would always report 0%. Keeping one instance
/// means every poll after the first measures the real interval since the
/// previous poll (the island UI polls, so the interval is the poll period).
fn system() -> &'static Mutex<System> {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    SYSTEM.get_or_init(|| Mutex::new(System::new_all()))
}

pub fn collect() -> SystemStats {
    static FIRST_SAMPLE: AtomicBool = AtomicBool::new(true);

    let mut sys = system().lock().unwrap_or_else(|err| err.into_inner());
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    // First call in this process: there is no previous CPU sample yet, so
    // take a second sample after a short interval instead of reporting 0%.
    if FIRST_SAMPLE.swap(false, Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
        sys.refresh_cpu_usage();
        sys.refresh_memory();
    }

    let total_memory = sys.total_memory();
    let memory_percent = if total_memory == 0 {
        0.0
    } else {
        sys.used_memory() as f32 / total_memory as f32 * 100.0
    };

    SystemStats {
        cpu_percent: sys.global_cpu_usage().clamp(0.0, 100.0),
        memory_percent: memory_percent.clamp(0.0, 100.0),
    }
}

#[cfg(test)]
mod tests {
    use super::collect;

    #[test]
    fn collect_returns_percentages_in_range() {
        let stats = collect();
        assert!((0.0..=100.0).contains(&stats.cpu_percent));
        assert!((0.0..=100.0).contains(&stats.memory_percent));
    }
}
