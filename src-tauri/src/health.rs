// SPDX-License-Identifier: GPL-3.0-or-later
//! Disk health & I/O telemetry.
//!
//! Capacity, model and real-time read/write throughput come from `/sys` and
//! `/proc/diskstats` (no privileges). SMART (health, power-on hours, temp) is
//! read by the privileged helper, since `smartctl` needs root.

use serde::Serialize;
use std::fs;

/// One physical disk's static profile plus a snapshot of its cumulative
/// read/write byte counters (the UI diffs counters over time for throughput).
#[derive(Serialize, Clone, Debug)]
pub struct DiskInfo {
    pub device: String,
    pub model: Option<String>,
    pub size_bytes: u64,
    pub rotational: bool,
    /// Cumulative bytes read since boot.
    pub read_bytes: u64,
    /// Cumulative bytes written since boot.
    pub write_bytes: u64,
}

const SECTOR: u64 = 512;

/// Host uptime in seconds (from `/proc/uptime`).
pub fn host_uptime_secs() -> u64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_string))
        .and_then(|x| x.parse::<f64>().ok())
        .map(|f| f as u64)
        .unwrap_or(0)
}

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
        // sda = disk, sda1 = partition
        return !name
            .chars()
            .last()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(true);
    }
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        // nvme0n1 = disk, nvme0n1p1 = partition
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

/// Snapshot of every physical disk: profile + cumulative I/O counters.
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
        // /proc/diskstats fields (0-based): 5 = sectors read, 9 = sectors written.
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

/// Names of every physical disk (passed to the helper for a single SMART read).
pub fn disk_names() -> Vec<String> {
    disks().into_iter().map(|d| d.device).collect()
}
