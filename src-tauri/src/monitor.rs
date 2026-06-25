// SPDX-License-Identifier: GPL-3.0-or-later
//! Background low-disk-space watcher.
//!
//! Polls free space on real mounts; when any drops below the configured
//! threshold it raises the window, emits a `low-space` event for the in-app
//! banner, and fires a desktop notification. Debounced: a mount only re-alerts
//! after it has recovered above the threshold.

use crate::settings;
use serde::Serialize;
use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;
use sysinfo::Disks;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone)]
pub struct LowSpaceAlert {
    pub mount: String,
    pub free_percent: f64,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

const POLL: Duration = Duration::from_secs(300);

/// Spawn the watcher on its own OS thread (cheap blocking sleeps; `AppHandle`
/// is `Send + Sync`, so emitting events and raising windows is fine here).
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut alerted: HashSet<String> = HashSet::new();
        loop {
            let cfg = settings::load();
            if cfg.monitor_enabled {
                let threshold = cfg.monitor_threshold as f64;
                let mut still = HashSet::new();
                for disk in Disks::new_with_refreshed_list().iter() {
                    let total = disk.total_space();
                    if total == 0 {
                        continue;
                    }
                    let available = disk.available_space();
                    let pct = available as f64 / total as f64 * 100.0;
                    if pct < threshold {
                        let mount = disk.mount_point().to_string_lossy().into_owned();
                        still.insert(mount.clone());
                        if !alerted.contains(&mount) {
                            raise_and_alert(
                                &app,
                                LowSpaceAlert {
                                    mount,
                                    free_percent: pct,
                                    free_bytes: available,
                                    total_bytes: total,
                                },
                            );
                        }
                    }
                }
                alerted = still;
            }
            std::thread::sleep(POLL);
        }
    });
}

fn raise_and_alert(app: &AppHandle, alert: LowSpaceAlert) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
    let _ = app.emit("low-space", alert.clone());
    let body = format!(
        "{} : {:.0}% libre. Pensez à libérer de l'espace.",
        alert.mount, alert.free_percent
    );

    #[cfg(not(target_os = "macos"))]
    let _ = Command::new("notify-send")
        .arg("--app-name=FreeYourDisk")
        .arg("--urgency=critical")
        .arg("FreeYourDisk")
        .arg(&body)
        .status();

    #[cfg(target_os = "macos")]
    {
        let safe = body.replace('"', "");
        let _ = Command::new("osascript")
            .args([
                "-e",
                &format!("display notification \"{safe}\" with title \"FreeYourDisk\""),
            ])
            .status();
    }
}
