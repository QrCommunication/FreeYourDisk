// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri commands exposed to the UI.
//!
//! The scan/preview/execute commands do heavy, blocking filesystem work, so
//! they run on a blocking thread pool via `spawn_blocking` — otherwise they
//! would block the main thread and freeze the UI on large home directories.

use crate::execute;
use crate::services::make_service;
use crate::state::AppState;
use core_ipc::{DeletionPlan, ExecutionReport, MountUsage, ScanResult, ServiceId};
use tauri::State;

/// Read-only scan for a service (off the main thread). Result is cached.
#[tauri::command]
pub async fn scan(service: ServiceId, state: State<'_, AppState>) -> Result<ScanResult, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let result = tauri::async_runtime::spawn_blocking(move || make_service(service, &cfg).scan())
        .await
        .map_err(|e| e.to_string())?;
    state
        .cache
        .lock()
        .map_err(|e| e.to_string())?
        .insert(service, result.clone());
    Ok(result)
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
