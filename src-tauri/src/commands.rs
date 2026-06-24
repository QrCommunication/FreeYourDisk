// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri commands exposed to the UI. Thin wrappers over the services and the
//! execution router; the heavy logic lives in `core-*` and `execute`.

use crate::execute;
use crate::services::make_service;
use crate::state::AppState;
use core_ipc::{DeletionPlan, ExecutionReport, MountUsage, ScanResult, ServiceId};
use tauri::State;

/// Read-only scan for a service. Result is cached for the session.
#[tauri::command]
pub fn scan(service: ServiceId, state: State<AppState>) -> ScanResult {
    let result = {
        let cfg = state.config.lock().expect("config lock");
        make_service(service, &cfg).scan()
    };
    state
        .cache
        .lock()
        .expect("cache lock")
        .insert(service, result.clone());
    result
}

/// Dry-run: build a deletion plan from the selected item ids.
#[tauri::command]
pub fn preview(service: ServiceId, selection: Vec<String>, state: State<AppState>) -> DeletionPlan {
    let cfg = state.config.lock().expect("config lock");
    make_service(service, &cfg).preview(&selection)
}

/// Execute a confirmed plan (user items in-process, root items via pkexec).
#[tauri::command]
pub fn execute(plan: DeletionPlan, state: State<AppState>) -> ExecutionReport {
    let home = state.config.lock().expect("config lock").home.clone();
    execute::execute_plan(&plan, &home, execute::pkexec_helper)
}

/// Per-mount disk usage for the dashboard donut.
#[tauri::command]
pub fn disk_usage() -> Vec<MountUsage> {
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
}
