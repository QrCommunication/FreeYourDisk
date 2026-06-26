// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri commands exposed to the UI.
//!
//! The scan/preview/execute commands do heavy, blocking filesystem work, so
//! they run on a blocking thread pool via `spawn_blocking` — otherwise they
//! would block the main thread and freeze the UI on large home directories.

use crate::services::make_service;
use crate::state::AppState;
use crate::{applications, execute, filetypes, health, settings, smartdeps, snapshot, taskmgr};
use core_ipc::{
    DeletionPlan, ExecutionReport, InstallReport, MountUsage, ScanResult, ServiceId, SmartInfo,
};
use serde::Serialize;
use tauri::State;

/// A scan plus its incremental diff against the previous snapshot.
#[derive(Serialize)]
pub struct ScanResponse {
    pub result: ScanResult,
    /// True if this is the first scan of the service (no prior snapshot).
    pub first_scan: bool,
    /// Ids of items that are new or changed since the previous scan.
    pub new_ids: Vec<String>,
}

/// Host uptime + every physical disk's profile and cumulative I/O counters.
#[derive(Serialize)]
pub struct HealthOverview {
    pub uptime_secs: u64,
    pub disks: Vec<health::DiskInfo>,
}

/// Read-only scan for a service (off the main thread). Caches the result and
/// records a snapshot so the next scan can surface what changed.
#[tauri::command]
pub async fn scan(service: ServiceId, state: State<'_, AppState>) -> Result<ScanResponse, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let result = tauri::async_runtime::spawn_blocking(move || make_service(service, &cfg).scan())
        .await
        .map_err(|e| e.to_string())?;
    state
        .cache
        .lock()
        .map_err(|e| e.to_string())?
        .insert(service, result.clone());
    let (first_scan, new_ids) = snapshot::diff_and_record(service, &result.items);
    core_scan::cache::save(&settings::config_dir().join("dir-cache.json"));
    Ok(ScanResponse {
        result,
        first_scan,
        new_ids,
    })
}

/// Dry-run: build a deletion plan from the selected item ids (off the main thread).
#[tauri::command]
pub async fn preview(
    service: ServiceId,
    selection: Vec<String>,
    state: State<'_, AppState>,
) -> Result<DeletionPlan, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    tauri::async_runtime::spawn_blocking(move || make_service(service, &cfg).preview(&selection))
        .await
        .map_err(|e| e.to_string())
}

/// Execute a confirmed plan (off the main thread; root items via pkexec).
#[tauri::command]
pub async fn execute(
    plan: DeletionPlan,
    state: State<'_, AppState>,
) -> Result<ExecutionReport, String> {
    let home = state.config.lock().map_err(|e| e.to_string())?.home.clone();
    tauri::async_runtime::spawn_blocking(move || {
        execute::execute_plan(&plan, &home, execute::pkexec_helper)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Per-mount disk usage for the dashboard donut. Off the main thread — probing
/// mounts can be slow when network/stale mounts are present.
#[tauri::command]
pub async fn disk_usage() -> Result<Vec<MountUsage>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        use sysinfo::Disks;
        let disks = Disks::new_with_refreshed_list();
        disks
            .iter()
            .map(|disk| {
                let total = disk.total_space();
                let available = disk.available_space();
                MountUsage {
                    mount: disk.mount_point().to_string_lossy().into_owned(),
                    total,
                    used: total.saturating_sub(available),
                }
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// Whether the weekly cleanup timer is enabled (systemd on Linux, launchd on macOS).
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn schedule_enabled() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-enabled", "freeyourdisk.timer"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Enable or disable (and start/stop) the weekly cleanup timer.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn set_schedule(enabled: bool) -> Result<bool, String> {
    let action = if enabled { "enable" } else { "disable" };
    let out = std::process::Command::new("systemctl")
        .args(["--user", action, "--now", "freeyourdisk.timer"])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(enabled)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// macOS: a LaunchAgent that runs the headless temp/cache cleanup weekly.
#[cfg(target_os = "macos")]
fn cleanup_plist_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/LaunchAgents/com.qrcommunication.freeyourdisk.cleanup.plist")
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn schedule_enabled() -> bool {
    cleanup_plist_path().is_file()
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn set_schedule(enabled: bool) -> Result<bool, String> {
    let path = cleanup_plist_path();
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\
             <key>Label</key><string>com.qrcommunication.freeyourdisk.cleanup</string>\
             <key>ProgramArguments</key><array>\
             <string>{}</string><string>--headless</string><string>--service=temp</string><string>--apply</string>\
             </array>\
             <key>StartCalendarInterval</key><dict><key>Weekday</key><integer>0</integer><key>Hour</key><integer>3</integer></dict>\
             </dict></plist>\n",
            exe.display()
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, plist).map_err(|e| e.to_string())?;
        let _ = std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&path)
            .output();
        Ok(true)
    } else {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&path)
            .output();
        let _ = std::fs::remove_file(&path);
        Ok(false)
    }
}

/// Host uptime + per-disk capacity, model and cumulative I/O (no privileges).
#[tauri::command]
pub async fn health_overview() -> Result<HealthOverview, String> {
    tauri::async_runtime::spawn_blocking(|| HealthOverview {
        uptime_secs: health::host_uptime_secs(),
        disks: health::disks(),
    })
    .await
    .map_err(|e| e.to_string())
}

/// Disk-usage breakdown by file type across the user's home (off the main
/// thread — it walks every file).
#[tauri::command]
pub async fn file_types(state: State<'_, AppState>) -> Result<Vec<filetypes::TypeBucket>, String> {
    let root = state.config.lock().map_err(|e| e.to_string())?.home.clone();
    let result = tauri::async_runtime::spawn_blocking(move || filetypes::scan_types(&root))
        .await
        .map_err(|e| e.to_string())?;
    core_scan::cache::save(&settings::config_dir().join("dir-cache.json"));
    Ok(result)
}

/// Total bytes under the user's home directory (mtime-cached). Used to split the
/// home-usage breakdown from the real system (OS) footprint on the dashboard.
#[tauri::command]
pub async fn home_total(state: State<'_, AppState>) -> Result<u64, String> {
    let home = state.config.lock().map_err(|e| e.to_string())?.home.clone();
    let total =
        tauri::async_runtime::spawn_blocking(move || core_scan::cache::cached_dir_total(&home))
            .await
            .map_err(|e| e.to_string())?;
    core_scan::cache::save(&settings::config_dir().join("dir-cache.json"));
    Ok(total)
}

/// Measured OS footprint (real system size), as opposed to the `used − home`
/// residual which wrongly absorbs reserved blocks.
///
/// unix: delegates to `du` (hardlink-dedup, true blocks, single-fs `-x`).
/// Windows: `du` is absent, so we sum the system roots with the internal
/// mtime-cached walker (same engine as `home_total`). Unreadable subtrees
/// (ACL-locked) are skipped by the walker — an approximation, like `du`.
#[tauri::command]
pub async fn system_total() -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(measure_system)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
fn measure_system() -> u64 {
    #[cfg(target_os = "linux")]
    const ROOTS: &[&str] = &[
        "/usr",
        "/var",
        "/opt",
        "/boot",
        "/srv",
        "/root",
        "/swapfile",
    ];
    #[cfg(target_os = "macos")]
    const ROOTS: &[&str] = &[
        "/System",
        "/Library",
        "/usr",
        "/private/var",
        "/opt",
        "/Applications",
    ];

    let present: Vec<&str> = ROOTS
        .iter()
        .copied()
        .filter(|p| std::path::Path::new(p).exists())
        .collect();
    if present.is_empty() {
        return 0;
    }

    // GNU du reports bytes (`--block-size=1`); BSD du (macOS) only does
    // 1024-byte blocks (`-k`), so scale there.
    #[cfg(target_os = "linux")]
    let (du_args, mult): (&[&str], u64) = (&["-scx", "--block-size=1"], 1);
    #[cfg(target_os = "macos")]
    let (du_args, mult): (&[&str], u64) = (&["-scxk"], 1024);

    let Ok(out) = std::process::Command::new("du")
        .args(du_args)
        .args(&present)
        .output()
    else {
        return 0;
    };
    // The last line is "<n>\ttotal".
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .and_then(|line| line.split_whitespace().next())
        .and_then(|n| n.parse::<u64>().ok())
        .map(|v| v * mult)
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn measure_system() -> u64 {
    // No `du` on Windows: sum the real system roots with the internal walker.
    // Derive the roots from the environment so non-C: installs are counted
    // (mirrors the %WINDIR% pattern in temp.rs); fall back to the C:\ defaults.
    fn env_root(var: &str, fallback: &str) -> std::path::PathBuf {
        std::env::var_os(var)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(fallback))
    }
    let roots = [
        env_root("SystemRoot", "C:\\Windows"),
        env_root("ProgramFiles", "C:\\Program Files"),
        env_root("ProgramFiles(x86)", "C:\\Program Files (x86)"),
        env_root("ProgramData", "C:\\ProgramData"),
    ];
    let total: u64 = roots
        .iter()
        .filter(|p| p.exists())
        .map(|p| core_scan::cache::cached_dir_total(p))
        .sum();
    core_scan::cache::save(&settings::config_dir().join("dir-cache.json"));
    total
}

/// SMART for every physical disk, via one privileged (pkexec) helper call.
#[tauri::command]
pub async fn disk_smart() -> Result<Vec<SmartInfo>, String> {
    tauri::async_runtime::spawn_blocking(|| execute::pkexec_smart(&health::disk_names()))
        .await
        .map_err(|e| e.to_string())
}

/// What SMART tooling this PC needs vs. has (drives the install prompt).
#[tauri::command]
pub fn smart_deps_status() -> smartdeps::SmartDepsStatus {
    smartdeps::status()
}

/// Install the missing SMART tools (nvme-cli / smartmontools) for this PC via a
/// single privileged helper call. The manager and package set are re-derived
/// server-side — the UI cannot influence what gets installed.
#[tauri::command]
pub async fn install_smart_deps() -> Result<InstallReport, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let packages = smartdeps::missing_packages();
        if packages.is_empty() {
            return InstallReport {
                success: true,
                message: "already installed".to_string(),
            };
        }
        // macOS: Homebrew, user-level (no privilege escalation).
        #[cfg(target_os = "macos")]
        {
            smartdeps::brew_install(&packages)
        }
        // Linux: privileged package manager via the pkexec helper.
        #[cfg(not(target_os = "macos"))]
        {
            let Some(manager) = smartdeps::detect_manager() else {
                return InstallReport {
                    success: false,
                    message: "no supported package manager found".to_string(),
                };
            };
            execute::pkexec_install_deps(&manager, &packages)
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Load the last home-scan results (JSON), shown instantly on app open while a
/// fresh scan refreshes in the background.
#[tauri::command]
pub fn home_cache_load() -> Option<String> {
    std::fs::read_to_string(settings::config_dir().join("home-cache.json")).ok()
}

/// Persist the latest home-scan results for instant display next launch.
#[tauri::command]
pub fn home_cache_save(data: String) -> Result<(), String> {
    let dir = settings::config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("home-cache.json"), data).map_err(|e| e.to_string())
}

/// Installed applications (apt/flatpak/snap/AppImage), largest first.
#[tauri::command]
pub async fn list_applications() -> Result<Vec<applications::AppEntry>, String> {
    tauri::async_runtime::spawn_blocking(applications::list)
        .await
        .map_err(|e| e.to_string())
}

/// Ids of applications with a newer version available (best-effort).
#[tauri::command]
pub async fn app_updates() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(applications::updates)
        .await
        .map_err(|e| e.to_string())
}

/// Batch-uninstall the given application ids.
#[tauri::command]
pub async fn uninstall_apps(ids: Vec<String>) -> Result<applications::AppActionReport, String> {
    tauri::async_runtime::spawn_blocking(move || applications::uninstall(&ids))
        .await
        .map_err(|e| e.to_string())
}

/// Batch-update the given application ids.
#[tauri::command]
pub async fn update_apps(ids: Vec<String>) -> Result<applications::AppActionReport, String> {
    tauri::async_runtime::spawn_blocking(move || applications::update(&ids))
        .await
        .map_err(|e| e.to_string())
}

/// Live memory/swap/load telemetry for the task-manager graph (cheap, polled).
#[tauri::command]
pub fn mem_stats() -> taskmgr::MemStats {
    taskmgr::mem_stats()
}

/// Full process inventory (off the main thread — refreshing all processes is
/// non-trivial), largest memory first.
#[tauri::command]
pub async fn process_list() -> Result<Vec<taskmgr::ProcInfo>, String> {
    tauri::async_runtime::spawn_blocking(taskmgr::process_list)
        .await
        .map_err(|e| e.to_string())
}

/// Terminate (SIGTERM) or force-kill (SIGKILL) a process.
#[tauri::command]
pub fn kill_process(pid: u32, force: bool) -> bool {
    taskmgr::kill_process(pid, force)
}

/// Restart a process (capture its command line, kill it, relaunch).
#[tauri::command]
pub async fn restart_process(pid: u32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || taskmgr::restart_process(pid))
        .await
        .map_err(|e| e.to_string())?
}

/// Emergency: force-kill the largest non-critical memory consumer.
#[tauri::command]
pub async fn panic_kill() -> Result<Option<taskmgr::ProcInfo>, String> {
    tauri::async_runtime::spawn_blocking(taskmgr::panic_kill)
        .await
        .map_err(|e| e.to_string())
}

/// The app version, sourced from Cargo.toml (single source of truth). Served as
/// an app command so it never depends on the core `app:version` ACL permission.
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Read the persisted user settings.
#[tauri::command]
pub fn get_settings() -> settings::Settings {
    settings::load()
}

/// Persist user settings (also applies autostart + re-registers the hotkey).
#[tauri::command]
pub fn set_settings(
    app: tauri::AppHandle,
    settings: settings::Settings,
) -> Result<settings::Settings, String> {
    crate::settings::save(&settings)?;
    crate::shortcut::register(&app, &settings.shortcut);
    Ok(settings)
}
