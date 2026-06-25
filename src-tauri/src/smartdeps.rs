// SPDX-License-Identifier: GPL-3.0-or-later
//! SMART tooling detection & guided install.
//!
//! Reads which disk types are present and which CLI tools (`nvme`, `smartctl`)
//! are installed, then maps the missing ones to the host's package manager so
//! the UI can offer a one-click, privileged install of exactly what this PC
//! needs — `nvme-cli` for NVMe drives, `smartmontools` for SATA/SAS.

use crate::health;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize, Clone, Debug)]
pub struct SmartDepsStatus {
    /// An NVMe drive is present (so `nvme-cli` is relevant).
    pub nvme_needed: bool,
    pub nvme_installed: bool,
    /// A SATA/SAS (sd*/hd*) drive is present (so `smartmontools` is relevant).
    pub sata_needed: bool,
    pub smartctl_installed: bool,
    /// Detected package manager key (`apt`/`dnf`/`pacman`/`zypper`), if any.
    pub manager: Option<String>,
    /// Packages still to install for full SMART coverage on this machine.
    pub missing: Vec<String>,
    /// True when something is missing AND a supported manager was detected.
    pub can_install: bool,
}

/// Look for an executable across PATH plus the usual sbin dirs (GUI sessions
/// often drop `/usr/sbin` from PATH, where `nvme`/`smartctl` live).
fn has_binary(bin: &str) -> bool {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for extra in [
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
    ] {
        let pb = PathBuf::from(extra);
        if !dirs.contains(&pb) {
            dirs.push(pb);
        }
    }
    dirs.iter().any(|d| d.join(bin).is_file())
}

/// Detect the system package manager by its binary (most reliable across
/// derivatives — an Ubuntu flavour still has `apt-get`).
pub fn detect_manager() -> Option<String> {
    for (bin, key) in [
        ("apt-get", "apt"),
        ("dnf", "dnf"),
        ("pacman", "pacman"),
        ("zypper", "zypper"),
    ] {
        if has_binary(bin) {
            return Some(key.to_string());
        }
    }
    None
}

/// What SMART tooling this machine needs vs. what it already has.
pub fn status() -> SmartDepsStatus {
    let disks = health::disks();
    let nvme_needed = disks.iter().any(|d| d.device.starts_with("nvme"));
    let sata_needed = disks
        .iter()
        .any(|d| d.device.starts_with("sd") || d.device.starts_with("hd"));

    let nvme_installed = has_binary("nvme");
    let smartctl_installed = has_binary("smartctl");
    let manager = detect_manager();

    let mut missing = Vec::new();
    if nvme_needed && !nvme_installed {
        missing.push("nvme-cli".to_string());
    }
    if sata_needed && !smartctl_installed {
        missing.push("smartmontools".to_string());
    }

    let can_install = manager.is_some() && !missing.is_empty();
    SmartDepsStatus {
        nvme_needed,
        nvme_installed,
        sata_needed,
        smartctl_installed,
        manager,
        missing,
        can_install,
    }
}

/// The packages this machine is missing (re-derived server-side; the UI never
/// dictates what gets installed).
pub fn missing_packages() -> Vec<String> {
    status().missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_binary_finds_sh() {
        assert!(has_binary("sh"));
    }

    #[test]
    fn has_binary_rejects_nonsense() {
        assert!(!has_binary("definitely-not-a-real-binary-xyz"));
    }
}
