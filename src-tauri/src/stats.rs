use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{Networks, System};

#[derive(Debug, Clone, Serialize)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub memory_percent: f32,
    /// Kilobytes per second received (download), 1 KB = 1024 bytes.
    /// 0.0 on the first call in the process: there is no previous sample
    /// yet to measure a rate against.
    pub net_down_kbps: f32,
    /// Kilobytes per second transmitted (upload), 1 KB = 1024 bytes.
    /// 0.0 on the first call in the process, same as `net_down_kbps`.
    pub net_up_kbps: f32,
    /// GPU utilization percent, or `None` when no GPU reading is available
    /// on this machine (no NVIDIA GPU/driver). Serializes as `null`, and
    /// the island chip shows "—" instead of a made-up number.
    pub gpu_percent: Option<f32>,
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

/// Shared network interfaces plus the instant of the previous poll.
///
/// `NetworkData::received()` / `transmitted()` already report the bytes
/// since the last `refresh()`, so the rate only needs the elapsed wall
/// time between two polls.
struct NetState {
    networks: Networks,
    prev_instant: Option<Instant>,
}

fn networks() -> &'static Mutex<NetState> {
    static NETWORKS: OnceLock<Mutex<NetState>> = OnceLock::new();
    NETWORKS.get_or_init(|| {
        Mutex::new(NetState {
            networks: Networks::new_with_refreshed_list(),
            prev_instant: None,
        })
    })
}

/// Kilobytes per second for `bytes` transferred in `elapsed`
/// (1 KB = 1024 bytes).
fn rate_kbps(bytes: u64, elapsed: Duration) -> f32 {
    if elapsed.is_zero() {
        0.0
    } else {
        bytes as f32 / elapsed.as_secs_f32() / 1024.0
    }
}

/// (download KB/s, upload KB/s) across all interfaces except loopback.
fn network_rates() -> (f32, f32) {
    let mut state = networks().lock().unwrap_or_else(|err| err.into_inner());
    state.networks.refresh();
    let now = Instant::now();
    let (down_bytes, up_bytes) = state
        .networks
        .list()
        .iter()
        .filter(|(name, _)| name.as_str() != "lo" && name.as_str() != "lo0")
        .map(|(_, data)| (data.received(), data.transmitted()))
        .fold((0u64, 0u64), |sum, cur| (sum.0 + cur.0, sum.1 + cur.1));
    let Some(prev) = state.prev_instant.replace(now) else {
        return (0.0, 0.0);
    };
    let elapsed = now.duration_since(prev);
    (rate_kbps(down_bytes, elapsed), rate_kbps(up_bytes, elapsed))
}

/// GPU utilization percent, or `None` when it cannot be read.
///
/// Windows only, via NVML (the NVIDIA driver). Every step fails cleanly
/// without an NVIDIA GPU/driver, so this returns `None` there and the
/// chip shows "—".
///
/// WMI perf counters (`Win32_PerfFormattedData_GPUPerformanceCounters_
/// GPUEngine`) were investigated first and rejected: they need ~150 lines
/// of unsafe COM/WMI plumbing, the per-engine instance names vary by
/// vendor (the `engtype_3D` instances have to be grouped by LUID and
/// summed, and the sum can exceed 100%), and the path cannot be
/// runtime-verified on this VM or on CI (no GPU). NVML gives one
/// driver-authoritative number and degrades to `None` on its own.
#[cfg(target_os = "windows")]
fn gpu_percent() -> Option<f32> {
    let nvml = nvml_wrapper::Nvml::init().ok()?;
    let device = nvml.device_by_index(0).ok()?;
    let rates = device.utilization_rates().ok()?;
    Some((rates.gpu as f32).clamp(0.0, 100.0))
}

#[cfg(not(target_os = "windows"))]
fn gpu_percent() -> Option<f32> {
    None
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
    let (net_down_kbps, net_up_kbps) = network_rates();

    SystemStats {
        cpu_percent: sys.global_cpu_usage().clamp(0.0, 100.0),
        memory_percent: memory_percent.clamp(0.0, 100.0),
        net_down_kbps,
        net_up_kbps,
        gpu_percent: gpu_percent(),
    }
}

#[cfg(test)]
mod tests {
    use super::{collect, rate_kbps};
    use std::time::Duration;

    #[test]
    fn collect_returns_percentages_in_range() {
        let stats = collect();
        assert!((0.0..=100.0).contains(&stats.cpu_percent));
        assert!((0.0..=100.0).contains(&stats.memory_percent));
        assert!(stats.net_down_kbps >= 0.0);
        assert!(stats.net_up_kbps >= 0.0);
        assert!(stats.gpu_percent.is_none_or(|v| (0.0..=100.0).contains(&v)));
    }

    #[test]
    fn rate_kbps_divides_by_elapsed_seconds() {
        assert_eq!(rate_kbps(10_240, Duration::from_secs(1)), 10.0);
        assert_eq!(rate_kbps(10_240, Duration::from_millis(500)), 20.0);
        assert_eq!(rate_kbps(0, Duration::from_secs(1)), 0.0);
        assert_eq!(rate_kbps(10_240, Duration::ZERO), 0.0);
    }
}
