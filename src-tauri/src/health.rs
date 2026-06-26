// SPDX-License-Identifier: GPL-3.0-or-later
//! Disk health & I/O telemetry.
//!
//! Linux: capacity/model/throughput come from `/sys` and `/proc/diskstats`
//! (no privileges). macOS: enumerated via `diskutil` (throughput is not exposed
//! without IOKit, so it reads 0 there). SMART is read by the privileged helper
//! (`smartctl`/`nvme`), since it needs root on both platforms.

use serde::Serialize;

/// One physical disk's static profile plus a snapshot of its cumulative
/// read/write byte counters (the UI diffs counters over time for throughput).
#[derive(Serialize, Clone, Debug)]
pub struct DiskInfo {
    pub device: String,
    pub model: Option<String>,
    pub size_bytes: u64,
    pub rotational: bool,
    /// Cumulative bytes read since boot (0 on macOS — not exposed by CLI).
    pub read_bytes: u64,
    /// Cumulative bytes written since boot (0 on macOS).
    pub write_bytes: u64,
}

/// Host uptime in seconds.
#[cfg(target_os = "linux")]
pub fn host_uptime_secs() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_string))
        .and_then(|x| x.parse::<f64>().ok())
        .map(|f| f as u64)
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
pub fn host_uptime_secs() -> u64 {
    sysinfo::System::uptime()
}

#[cfg(target_os = "macos")]
pub fn host_uptime_secs() -> u64 {
    sysinfo::System::uptime()
}

// ---------------------------------------------------------------------------
// Linux: /proc + /sys
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
mod platform {
    use super::DiskInfo;
    use std::fs;

    const SECTOR: u64 = 512;

    /// True for whole physical disks; false for partitions and virtual devices.
    fn is_physical_disk(name: &str) -> bool {
        if name.starts_with("loop")
            || name.starts_with("ram")
            || name.starts_with("dm-")
            || name.starts_with("sr")
            || name.starts_with("zram")
            || name.starts_with("fd")
            || name.starts_with("md")
        {
            return false;
        }
        if name.starts_with("sd") || name.starts_with("vd") || name.starts_with("hd") {
            return !name
                .chars()
                .last()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(true);
        }
        if name.starts_with("nvme") || name.starts_with("mmcblk") {
            return !name.contains('p');
        }
        false
    }

    fn read_u64(path: &str) -> Option<u64> {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
    }

    fn disk_model(device: &str) -> Option<String> {
        fs::read_to_string(format!("/sys/block/{device}/device/model"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn disks() -> Vec<DiskInfo> {
        let content = fs::read_to_string("/proc/diskstats").unwrap_or_default();
        let mut out = Vec::new();
        for line in content.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 14 {
                continue;
            }
            let name = f[2];
            if !is_physical_disk(name) {
                continue;
            }
            let read_sectors: u64 = f[5].parse().unwrap_or(0);
            let write_sectors: u64 = f[9].parse().unwrap_or(0);
            let size_bytes = read_u64(&format!("/sys/block/{name}/size")).unwrap_or(0) * SECTOR;
            let rotational = read_u64(&format!("/sys/block/{name}/queue/rotational")) == Some(1);
            out.push(DiskInfo {
                device: name.to_string(),
                model: disk_model(name),
                size_bytes,
                rotational,
                read_bytes: read_sectors * SECTOR,
                write_bytes: write_sectors * SECTOR,
            });
        }
        out
    }
}

// ---------------------------------------------------------------------------
// macOS: diskutil
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod platform {
    use super::DiskInfo;
    use std::process::Command;

    /// Whole physical disks (`disk0`, `disk1`…). Synthesized APFS containers and
    /// disk-image mounts are skipped.
    fn physical_identifiers() -> Vec<String> {
        let Ok(out) = Command::new("diskutil").arg("list").output() else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut ids = Vec::new();
        for line in text.lines() {
            // e.g. "/dev/disk0 (internal, physical):"
            if let Some(rest) = line.strip_prefix("/dev/") {
                if rest.contains("physical") {
                    if let Some(id) = rest.split_whitespace().next() {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        ids
    }

    fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(|v| v.trim_start_matches(':').trim())
    }

    fn info(id: &str) -> Option<DiskInfo> {
        let out = Command::new("diskutil").args(["info", id]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);

        let model = field(&text, "Device / Media Name")
            .map(str::to_string)
            .filter(|s| !s.is_empty());

        // "Disk Size: 1.0 TB (1000000000000 Bytes) (...)" → 1000000000000
        let size_bytes = field(&text, "Disk Size")
            .and_then(|v| v.split('(').nth(1))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0);

        let rotational = field(&text, "Solid State")
            .map(|v| !v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false);

        Some(DiskInfo {
            device: id.to_string(),
            model,
            size_bytes,
            rotational,
            read_bytes: 0,
            write_bytes: 0,
        })
    }

    pub fn disks() -> Vec<DiskInfo> {
        physical_identifiers()
            .iter()
            .filter_map(|id| info(id))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Windows: sysinfo (model/rotational not exposed; throughput deferred = 0).
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
mod platform {
    use super::DiskInfo;
    use sysinfo::Disks;

    pub fn disks() -> Vec<DiskInfo> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for disk in Disks::new_with_refreshed_list().iter() {
            // sysinfo lists volumes; dedupe by device name, keep physical-ish.
            let name = disk.name().to_string_lossy().into_owned();
            let device = if name.is_empty() {
                disk.mount_point().to_string_lossy().into_owned()
            } else {
                name
            };
            if !seen.insert(device.clone()) {
                continue;
            }
            out.push(DiskInfo {
                device,
                model: None,
                size_bytes: disk.total_space(),
                rotational: false,
                read_bytes: 0,
                write_bytes: 0,
            });
        }
        out
    }
}

/// Snapshot of every physical disk: profile + cumulative I/O counters.
pub fn disks() -> Vec<DiskInfo> {
    platform::disks()
}

/// Names of every physical disk (passed to the helper for a single SMART read).
pub fn disk_names() -> Vec<String> {
    disks().into_iter().map(|d| d.device).collect()
}
