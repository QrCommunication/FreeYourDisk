// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri commands exposed to the UI.
//!
//! The scan/preview/execute commands do heavy, blocking filesystem work, so
//! they run on a blocking thread pool via `spawn_blocking` — otherwise they
//! would block the main thread and freeze the UI on large home directories.

use crate::services::make_service;
use crate::state::AppState;
use crate::{applications, execute, filetypes, health, settings, snapshot};
use core_ipc::{DeletionPlan, ExecutionReport, MountUsage, ScanResult, ServiceId, SmartInfo};
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

/// Whether the weekly cleanup systemd user timer is enabled.
#[tauri::command]
pub fn schedule_enabled() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-enabled", "freeyourdisk.timer"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Enable or disable (and start/stop) the weekly cleanup timer.
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

/// Measured OS footprint: the real system size (~68 GB), as opposed to the
/// `used − home` residual which wrongly absorbs ext4 reserved blocks.
///
/// Delegates to `du` for correctness: it deduplicates hardlinks, counts true
/// block usage and stays on one filesystem (`-x`) — replicating that in a hand
/// rolled walker is error-prone (snaps/Docker overlays/sparse files all skew
/// it). `/snap` is excluded: it mounts decompressed squashfs whose real on-disk
/// cost (the compressed images) already lives under `/var`.
#[tauri::command]
pub async fn system_total() -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(|| {
        const ROOTS: &[&str] = &[
            "/usr",
            "/var",
            "/opt",
            "/boot",
            "/srv",
            "/root",
            "/swapfile",
        ];
        let present: Vec<&str> = ROOTS
            .iter()
            .copied()
            .filter(|p| std::path::Path::new(p).exists())
            .collect();
        if present.is_empty() {
            return 0;
        }
        // `du -scx --block-size=1`: summary, grand total, one filesystem, bytes.
        let Ok(out) = std::process::Command::new("du")
            .args(["-scx", "--block-size=1"])
            .args(&present)
            .output()
        else {
            return 0;
        };
        // The last line is "<bytes>\ttotal".
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .last()
            .and_then(|line| line.split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
    })
    .await
    .map_err(|e| e.to_string())
}

/// SMART for every physical disk, via one privileged (pkexec) helper call.
#[tauri::command]
pub async fn disk_smart() -> Result<Vec<SmartInfo>, String> {
    tauri::async_runtime::spawn_blocking(|| execute::pkexec_smart(&health::disk_names()))
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

/// Read the persisted user settings.
#[tauri::command]
pub fn get_settings() -> settings::Settings {
    settings::load()
}

/// Persist user settings (also applies the autostart side effect).
#[tauri::command]
pub fn set_settings(settings: settings::Settings) -> Result<settings::Settings, String> {
    crate::settings::save(&settings)?;
    Ok(settings)
}
