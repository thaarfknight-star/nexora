#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Watchdog mode: Nexora re-invokes itself as a detached helper that
    // waits for the parent process to exit, then restores the taskbar.
    // This must run before Tauri starts, and must never open the island.
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == nexora_lib::watchdog::WATCHDOG_ARG {
        let code = nexora_lib::watchdog::parse_watchdog_args(&args)
            .map(nexora_lib::watchdog::run_watchdog_process)
            .unwrap_or(2);
        std::process::exit(code);
    }
    nexora_lib::run();
}
