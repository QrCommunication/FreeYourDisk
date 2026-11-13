// SPDX-License-Identifier: GPL-3.0-or-later
//! Background low-disk-space watcher.
//!
//! Polls free space on the machine's own writable mounts; when one drops below
//! the configured threshold it emits a `low-space` event (the in-app modal keeps
//! it until the user opens the window) and fires a desktop notification. It
//! never raises or focuses the window: an alert from a background task must not
//! steal the user's foreground. Debounced: a mount only re-alerts after it has
//! recovered above the threshold.
//!
//! Removable and read-only mounts are ignored. They are not what FreeYourDisk
//! cleans, and they are typically "full by design" — e.g. the 73 MB read-only
//! vfat partition a locked Samsung T7 Touch exposes — or come and go with every
//! USB/dock reconnection, which resets the debounce and re-alerted endlessly.

use crate::settings;
use serde::Serialize;
use std::collections::HashSet;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;
use std::time::Duration;
use sysinfo::{Disk, Disks};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LowSpaceAlert {
    pub mount: String,
    pub free_percent: f64,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

const POLL: Duration = Duration::from_secs(300);

/// The facts the watch policy needs about one mount, decoupled from
/// `sysinfo::Disk` (which cannot be built in tests).
#[derive(Debug, Clone)]
struct MountSample {
    mount: String,
    total: u64,
    available: u64,
    removable: bool,
    read_only: bool,
}

impl From<&Disk> for MountSample {
    fn from(disk: &Disk) -> Self {
        Self {
            mount: disk.mount_point().to_string_lossy().into_owned(),
            total: disk.total_space(),
            available: disk.available_space(),
            // On Linux sysinfo flags every device exposed under
            // /dev/disk/by-id/usb-* — USB SSDs included, even when the kernel's
            // own `removable` attribute is 0.
            removable: disk.is_removable(),
            read_only: disk.is_read_only(),
        }
    }
}

/// Only the machine's own writable storage is worth watching: nothing can be
/// freed on a read-only mount, and removable media are outside the cleaner's
/// scope (and churn on every reconnection).
fn is_watchable(sample: &MountSample) -> bool {
    sample.total > 0 && !sample.removable && !sample.read_only
}

/// One polling round: returns the alerts to fire now and the new set of mounts
/// currently below the threshold (the debounce state for the next round).
fn evaluate(
    samples: &[MountSample],
    threshold_percent: f64,
    alerted: &HashSet<String>,
) -> (Vec<LowSpaceAlert>, HashSet<String>) {
    let mut alerts = Vec::new();
    let mut below = HashSet::new();
    for sample in samples.iter().filter(|s| is_watchable(s)) {
        let free_percent = sample.available as f64 / sample.total as f64 * 100.0;
        if free_percent >= threshold_percent {
            continue;
        }
        below.insert(sample.mount.clone());
        if !alerted.contains(&sample.mount) {
            alerts.push(LowSpaceAlert {
                mount: sample.mount.clone(),
                free_percent,
                free_bytes: sample.available,
                total_bytes: sample.total,
            });
        }
    }
    (alerts, below)
}

/// Spawn the watcher on its own OS thread (cheap blocking sleeps; `AppHandle`
/// is `Send + Sync`, so emitting events is fine here).
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut alerted: HashSet<String> = HashSet::new();
        loop {
            let cfg = settings::load();
            if cfg.monitor_enabled {
                let samples: Vec<MountSample> = Disks::new_with_refreshed_list()
                    .iter()
                    .map(MountSample::from)
                    .collect();
                let (alerts, below) = evaluate(&samples, cfg.monitor_threshold as f64, &alerted);
                for alert in alerts {
                    notify_low_space(&app, alert);
                }
                alerted = below;
            }
            std::thread::sleep(POLL);
        }
    });
}

fn notify_low_space(app: &AppHandle, alert: LowSpaceAlert) {
    let _ = app.emit("low-space", alert.clone());
    let body = format!(
        "{} : {:.0}% libre. Pensez à libérer de l'espace.",
        alert.mount, alert.free_percent
    );

    #[cfg(target_os = "linux")]
    let _ = Command::new("notify-send")
        .arg("--app-name=FreeYourDisk")
        .arg("--urgency=critical")
        .arg("FreeYourDisk")
        .arg(&body)
        .status();

    #[cfg(target_os = "windows")]
    crate::toast::show("FreeYourDisk", &body);

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

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn sample(mount: &str, total: u64, available: u64) -> MountSample {
        MountSample {
            mount: mount.to_owned(),
            total,
            available,
            removable: false,
            read_only: false,
        }
    }

    #[test]
    fn locked_samsung_t7_partition_never_alerts() {
        // Real values observed 2026-11-12: 73 MB vfat, 1.3 MB free, USB + RO.
        let t7 = MountSample {
            removable: true,
            read_only: true,
            ..sample("/media/rony/T7 Touch", 73 * 1024 * 1024, 1_300_000)
        };
        let (alerts, below) = evaluate(&[t7], 5.0, &HashSet::new());
        assert!(alerts.is_empty());
        assert!(below.is_empty());
    }

    #[test]
    fn removable_writable_mount_is_ignored() {
        let usb_stick = MountSample {
            removable: true,
            ..sample("/media/usb", 16 * GIB, GIB / 10)
        };
        assert!(!is_watchable(&usb_stick));
        assert!(evaluate(&[usb_stick], 5.0, &HashSet::new()).0.is_empty());
    }

    #[test]
    fn read_only_internal_mount_is_ignored() {
        let squashfs = MountSample {
            read_only: true,
            ..sample("/snap/core/1", GIB, 0)
        };
        assert!(evaluate(&[squashfs], 5.0, &HashSet::new()).0.is_empty());
    }

    #[test]
    fn zero_sized_mount_is_ignored() {
        assert!(!is_watchable(&sample("/proc", 0, 0)));
    }

    #[test]
    fn internal_mount_below_threshold_alerts_with_its_figures() {
        let root = sample("/", 100 * GIB, 2 * GIB);
        let (alerts, below) = evaluate(&[root], 5.0, &HashSet::new());
        assert_eq!(
            alerts,
            vec![LowSpaceAlert {
                mount: "/".into(),
                free_percent: 2.0,
                free_bytes: 2 * GIB,
                total_bytes: 100 * GIB,
            }]
        );
        assert!(below.contains("/"));
    }

    #[test]
    fn mount_above_threshold_does_not_alert() {
        let root = sample("/", 100 * GIB, 50 * GIB);
        let (alerts, below) = evaluate(&[root], 5.0, &HashSet::new());
        assert!(alerts.is_empty());
        assert!(below.is_empty());
    }

    #[test]
    fn alert_fires_once_then_again_only_after_recovery() {
        let low = sample("/", 100 * GIB, GIB);
        let ok = sample("/", 100 * GIB, 40 * GIB);

        let (first, state) = evaluate(std::slice::from_ref(&low), 5.0, &HashSet::new());
        assert_eq!(first.len(), 1);

        let (second, state) = evaluate(std::slice::from_ref(&low), 5.0, &state);
        assert!(second.is_empty(), "still low: must stay debounced");

        let (recovered, state) = evaluate(&[ok], 5.0, &state);
        assert!(recovered.is_empty());
        assert!(state.is_empty());

        let (again, _) = evaluate(&[low], 5.0, &state);
        assert_eq!(again.len(), 1, "dropped low again after recovery");
    }

    #[test]
    fn excluded_mounts_do_not_hide_a_low_internal_mount() {
        let samples = [
            MountSample {
                removable: true,
                read_only: true,
                ..sample("/media/t7", 73 * 1024 * 1024, 0)
            },
            sample("/home", 500 * GIB, 10 * GIB),
        ];
        let (alerts, _) = evaluate(&samples, 5.0, &HashSet::new());
        let mounts: Vec<_> = alerts.iter().map(|a| a.mount.as_str()).collect();
        assert_eq!(mounts, ["/home"]);
    }
}
