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
    #[cfg(not(target_os = "windows"))]
    let extra = [
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
    ];
    #[cfg(target_os = "windows")]
    let extra = [
        "C:\\Program Files\\smartmontools\\bin",
        "C:\\Program Files (x86)\\smartmontools\\bin",
    ];
    for dir in extra {
        let pb = PathBuf::from(dir);
        if !dirs.contains(&pb) {
            dirs.push(pb);
        }
    }
    // On Windows, executables carry a .exe suffix.
    #[cfg(target_os = "windows")]
    let found = dirs.iter().any(|d| d.join(format!("{bin}.exe")).is_file());
    #[cfg(not(target_os = "windows"))]
    let found = dirs.iter().any(|d| d.join(bin).is_file());
    found
}

/// Detect the system package manager by its binary (most reliable across
/// derivatives — an Ubuntu flavour still has `apt-get`).
#[cfg(target_os = "linux")]
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

/// Windows uses winget (App Installer, present on Win10 1809+/Win11).
#[cfg(target_os = "windows")]
pub fn detect_manager() -> Option<String> {
    if has_binary("winget") {
        Some("winget".to_string())
    } else {
        None
    }
}

/// macOS uses Homebrew. (Apple Silicon installs it under /opt/homebrew/bin.)
#[cfg(target_os = "macos")]
pub fn detect_manager() -> Option<String> {
    if has_binary("brew") || std::path::Path::new("/opt/homebrew/bin/brew").is_file() {
        Some("brew".to_string())
    } else {
        None
    }
}

/// What SMART tooling this machine needs vs. what it already has.
pub fn status() -> SmartDepsStatus {
    let disks = health::disks();
    // On macOS `smartctl` covers NVMe too (no separate nvme-cli), and disks are
    // `diskN`, so we just key off "any disk present → smartmontools".
    #[cfg(target_os = "macos")]
    let (nvme_needed, sata_needed) = (false, !disks.is_empty());
    #[cfg(target_os = "linux")]
    let (nvme_needed, sata_needed) = (
        disks.iter().any(|d| d.device.starts_with("nvme")),
        disks
            .iter()
            .any(|d| d.device.starts_with("sd") || d.device.starts_with("hd")),
    );
    // On Windows, smartmontools covers NVMe natively — no separate nvme-cli needed.
    #[cfg(target_os = "windows")]
    let (nvme_needed, sata_needed) = (false, !disks.is_empty());

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

/// macOS: install via Homebrew as the current user (Homebrew refuses to run as
/// root, so there is no privilege escalation here).
#[cfg(target_os = "macos")]
pub fn brew_install(packages: &[String]) -> core_ipc::InstallReport {
    let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap_or("brew");
    let mut cmd = std::process::Command::new(brew);
    cmd.arg("install");
    for pkg in packages {
        cmd.arg(pkg);
    }
    match cmd.output() {
        Ok(out) if out.status.success() => core_ipc::InstallReport {
            success: true,
            message: format!("Installed: {}", packages.join(", ")),
        },
        Ok(out) => core_ipc::InstallReport {
            success: false,
            message: String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("brew install failed")
                .to_string(),
        },
        Err(err) => core_ipc::InstallReport {
            success: false,
            message: format!("failed to run brew: {err}"),
        },
    }
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
