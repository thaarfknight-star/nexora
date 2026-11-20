use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
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

/// Evidence log file name, inside the app data directory — the same
/// pattern as `island::ANCHOR_LOG_FILE`.
pub const STATS_LOG_FILE: &str = "nexora-stats.log";

/// App data directory for `nexora-stats.log`, set once from `lib.rs`
/// setup. Until then (unit tests, direct `collect()` calls) logging is
/// silently skipped.
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Point the stats log at the app data directory. Called once from
/// `lib.rs` setup; a second call is ignored. Writes one marker line
/// so run boundaries are visible in the log.
pub fn set_log_dir(dir: PathBuf) {
    if LOG_DIR.set(dir.clone()).is_ok() {
        write_stats_log(&format!(
            "t={:.3} stats-log-started dir={}\n",
            now_secs(),
            dir.display()
        ));
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Append one line to `nexora-stats.log`. Failures are silent:
/// evidence must never break the island.
fn write_stats_log(line: &str) {
    let Some(dir) = LOG_DIR.get() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join(STATS_LOG_FILE);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
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

/// The NVML handle, initialized at most once per process — including a
/// failed init, which is cached as `None` and never retried.
/// `Nvml::init()` talks to the NVIDIA driver and must not run on every
/// poll: it is the prime suspect for the island stalling a few seconds
/// after launch (every poll re-ran it), so it runs once, lazily, on the
/// first poll that needs it. `Nvml` is `Send + Sync` (asserted by the
/// crate itself), so holding it in a `static Mutex` is sound.
#[cfg(all(target_os = "windows", feature = "gpu-nvml"))]
fn nvml_handle() -> &'static Mutex<Option<nvml_wrapper::Nvml>> {
    static NVML: OnceLock<Mutex<Option<nvml_wrapper::Nvml>>> = OnceLock::new();
    NVML.get_or_init(|| Mutex::new(nvml_wrapper::Nvml::init().ok()))
}

/// (GPU utilization percent, stage) — the stage names where a poll
/// stopped, for `nexora-stats.log`. `None` means the chip shows "—".
///
/// Windows only, via NVML (the NVIDIA driver). Every step fails cleanly
/// without an NVIDIA GPU/driver, so this returns `None` there.
///
/// WMI perf counters (`Win32_PerfFormattedData_GPUPerformanceCounters_
/// GPUEngine`) were investigated first and rejected: they need ~150 lines
/// of unsafe COM/WMI plumbing, the per-engine instance names vary by
/// vendor (the `engtype_3D` instances have to be grouped by LUID and
/// summed, and the sum can exceed 100%), and the path cannot be
/// runtime-verified on this VM or on CI (no GPU). NVML gives one
/// driver-authoritative number and degrades to `None` on its own.
///
/// Diagnostic builds (`--no-default-features`, see Cargo.toml) compile
/// this out entirely: the stage is `gpu-disabled-no-gpu-poll` and no
/// NVML code runs at all.
#[cfg(all(target_os = "windows", feature = "gpu-nvml"))]
fn gpu_percent() -> (Option<f32>, &'static str) {
    let guard = nvml_handle().lock().unwrap_or_else(|err| err.into_inner());
    let Some(nvml) = guard.as_ref() else {
        return (None, "nvml-init-failed");
    };
    let Ok(device) = nvml.device_by_index(0) else {
        return (None, "nvml-device-failed");
    };
    match device.utilization_rates() {
        Ok(rates) => (Some((rates.gpu as f32).clamp(0.0, 100.0)), "ok"),
        Err(_) => (None, "nvml-rates-failed"),
    }
}

#[cfg(not(all(target_os = "windows", feature = "gpu-nvml")))]
fn gpu_percent() -> (Option<f32>, &'static str) {
    #[cfg(feature = "gpu-nvml")]
    let stage = "gpu-unavailable-platform";
    #[cfg(not(feature = "gpu-nvml"))]
    let stage = "gpu-disabled-no-gpu-poll";
    (None, stage)
}

pub fn collect() -> SystemStats {
    static FIRST_SAMPLE: AtomicBool = AtomicBool::new(true);
    let poll_start = Instant::now();

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
    let (gpu, gpu_stage) = gpu_percent();

    let stats = SystemStats {
        cpu_percent: sys.global_cpu_usage().clamp(0.0, 100.0),
        memory_percent: memory_percent.clamp(0.0, 100.0),
        net_down_kbps,
        net_up_kbps,
        gpu_percent: gpu,
    };

    // Evidence: one line per completed poll. A poll that hangs mid-way
    // leaves no line, so a gap between timestamps (or a missing line
    // after the last one) shows exactly where the poll stopped. `poll_ms`
    // shows how long the poll itself took.
    write_stats_log(&format!(
        "t={:.3} cpu={:.1} mem={:.1} down={:.1} up={:.1} gpu={} gpu_stage={} poll_ms={}\n",
        now_secs(),
        stats.cpu_percent,
        stats.memory_percent,
        stats.net_down_kbps,
        stats.net_up_kbps,
        stats
            .gpu_percent
            .map_or("none".to_string(), |v| format!("{v:.1}")),
        gpu_stage,
        poll_start.elapsed().as_millis()
    ));

    stats
}

#[cfg(test)]
mod tests {
    use super::{collect, rate_kbps, set_log_dir};
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

    #[test]
    fn collect_logs_one_line_per_poll() {
        let dir = std::env::temp_dir().join("nexora-stats-log-test");
        let _ = std::fs::remove_dir_all(&dir);
        set_log_dir(dir.clone());
        collect();
        collect();
        let text = std::fs::read_to_string(dir.join("nexora-stats.log"))
            .expect("stats log file exists after two polls");
        assert!(
            text.contains("stats-log-started"),
            "log has a run marker line"
        );
        let poll_lines: Vec<&str> = text.lines().filter(|l| l.contains("poll_ms=")).collect();
        assert!(
            poll_lines.len() >= 2,
            "two polls wrote two lines, got {}: {text}",
            poll_lines.len()
        );
        for line in poll_lines {
            assert!(line.contains("cpu="), "poll line has cpu: {line}");
            // Every stage gpu_percent() can return: `ok` and the
            // `nvml-*-failed` stages happen on Windows (CI runners
            // have no NVIDIA GPU, so init fails there), while the
            // `gpu-*` stages are the off-Windows / no-feature
            // fallbacks. The old `gpu_stage=gpu-` check only passed
            // off-Windows and turned CI red (run 44733041079).
            const KNOWN_STAGES: [&str; 6] = [
                "ok",
                "nvml-init-failed",
                "nvml-device-failed",
                "nvml-rates-failed",
                "gpu-unavailable-platform",
                "gpu-disabled-no-gpu-poll",
            ];
            assert!(
                KNOWN_STAGES
                    .iter()
                    .any(|s| line.contains(&format!("gpu_stage={s}"))),
                "poll line has a known gpu stage: {line}"
            );
        }
    }
}
