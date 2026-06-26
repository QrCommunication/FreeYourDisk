// SPDX-License-Identifier: GPL-3.0-or-later
//! Minimal privileged helper for FreeYourDisk.
//!
//! Reads a `DeletionPlan` (JSON) on stdin, **re-validates** every path against
//! hard-coded root zones (never trusting the caller), executes, and writes an
//! `ExecutionReport` (JSON) on stdout.
//!
//! Security stance: all-or-nothing. If *any* path escapes the root zones the
//! entire batch is refused — a caller cannot smuggle a malicious path alongside
//! legitimate ones to obtain partial execution.
//!
//! Exit codes: 0 = success, 2 = invalid input, 3 = a path was refused.

use core_ipc::{DeletionPlan, ExecutionReport, InstallReport, SmartInfo};
use core_trash::Zones;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// Root-owned zones this helper is ever allowed to touch. Hard-coded — never
/// received from the (semi-trusted) caller.
const ROOT_ZONES: &[&str] = &["/tmp", "/var/tmp"];

/// The only packages this helper will ever install — hard-coded so a caller can
/// never smuggle an arbitrary package name into a root install.
const ALLOWED_PACKAGES: &[&str] = &["nvme-cli", "smartmontools"];

fn write_report(report: &ExecutionReport) {
    if let Ok(json) = serde_json::to_string(report) {
        let _ = std::io::stdout().write_all(json.as_bytes());
    }
}

/// `smartctl` is read-only; device names are validated to be plain alphanumeric
/// (no path separators / shell metacharacters) before they reach the command.
fn unavailable(device: &str) -> SmartInfo {
    SmartInfo {
        device: device.to_string(),
        available: false,
        passed: None,
        power_on_hours: None,
        temperature_c: None,
    }
}

/// Some nvme-cli versions emit numbers as JSON strings — accept both.
fn as_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}
fn as_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

/// SMART for an NVMe device via `nvme smart-log` (the right tool for NVMe).
fn smart_nvme(device: &str) -> Option<SmartInfo> {
    let path = format!("/dev/{device}");
    let out = Command::new("nvme")
        .args(["smart-log", &path, "-o", "json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let json = serde_json::from_slice::<serde_json::Value>(&out.stdout).ok()?;
    let passed = json
        .get("critical_warning")
        .and_then(as_u64)
        .map(|w| w == 0);
    let power_on_hours = json.get("power_on_hours").and_then(as_u64);
    // nvme reports temperature in Kelvin.
    let temperature_c =
        json.get("temperature")
            .and_then(as_i64)
            .map(|k| if k > 200 { k - 273 } else { k });
    if passed.is_none() && power_on_hours.is_none() && temperature_c.is_none() {
        return None;
    }
    Some(SmartInfo {
        device: device.to_string(),
        available: true,
        passed,
        power_on_hours,
        temperature_c,
    })
}

/// SMART for a SATA/SAS device via `smartctl`.
fn smart_smartctl(device: &str) -> SmartInfo {
    let path = format!("/dev/{device}");
    let Ok(out) = Command::new("smartctl").args(["-a", "-j", &path]).output() else {
        return unavailable(device);
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return unavailable(device);
    };
    let passed = json
        .get("smart_status")
        .and_then(|s| s.get("passed"))
        .and_then(|v| v.as_bool());
    let power_on_hours = json
        .get("power_on_time")
        .and_then(|p| p.get("hours"))
        .and_then(as_u64);
    let temperature_c = json
        .get("temperature")
        .and_then(|t| t.get("current"))
        .and_then(as_i64);
    let available = passed.is_some() || power_on_hours.is_some() || temperature_c.is_some();
    SmartInfo {
        device: device.to_string(),
        available,
        passed,
        power_on_hours,
        temperature_c,
    }
}

fn smart_one(device: &str) -> SmartInfo {
    if device.is_empty() || !device.chars().all(|c| c.is_ascii_alphanumeric()) {
        return unavailable(device);
    }
    // NVMe: prefer nvme-cli; fall back to smartctl if nvme is unavailable.
    if device.starts_with("nvme") {
        if let Some(info) = smart_nvme(device) {
            return info;
        }
    }
    smart_smartctl(device)
}

/// `freeyourdisk-helper smart <dev>...` → JSON array of SmartInfo on stdout.
fn run_smart(devices: &[String]) -> ExitCode {
    let results: Vec<SmartInfo> = devices.iter().map(|d| smart_one(d)).collect();
    if let Ok(json) = serde_json::to_string(&results) {
        let _ = std::io::stdout().write_all(json.as_bytes());
    }
    ExitCode::SUCCESS
}

/// Build the install command(s) for a package manager. Each manager has a fixed
/// command template — only the (allowlisted) package names vary. Returns `None`
/// for an unknown manager.
fn install_steps(manager: &str, packages: &[&str]) -> Option<Vec<Command>> {
    let mut steps = Vec::new();
    let push_install = |program: &str, prefix: &[&str], steps: &mut Vec<Command>| {
        let mut cmd = Command::new(program);
        cmd.args(prefix).arg("--");
        for p in packages {
            cmd.arg(p);
        }
        cmd.env("DEBIAN_FRONTEND", "noninteractive");
        steps.push(cmd);
    };
    match manager {
        "apt" => {
            // Refresh first (a stale mirror would 404 the install); failure is
            // tolerated below.
            let mut update = Command::new("apt-get");
            update
                .args(["update", "-qq"])
                .env("DEBIAN_FRONTEND", "noninteractive");
            steps.push(update);
            push_install(
                "apt-get",
                &["install", "-y", "--no-install-recommends"],
                &mut steps,
            );
        }
        "dnf" => push_install("dnf", &["install", "-y"], &mut steps),
        "pacman" => push_install("pacman", &["-Sy", "--noconfirm", "--needed"], &mut steps),
        "zypper" => push_install("zypper", &["--non-interactive", "install"], &mut steps),
        _ => return None,
    }
    Some(steps)
}

/// `freeyourdisk-helper install-deps <manager> <pkg>...` — install SMART tools.
fn run_install_deps(args: &[String]) -> ExitCode {
    let emit = |success: bool, message: String| {
        if let Ok(json) = serde_json::to_string(&InstallReport { success, message }) {
            let _ = std::io::stdout().write_all(json.as_bytes());
        }
    };
    let Some(manager) = args.first() else {
        emit(false, "no package manager specified".into());
        return ExitCode::from(2);
    };
    let packages: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    if packages.is_empty() {
        emit(false, "no packages specified".into());
        return ExitCode::from(2);
    }
    // Hard allowlist: refuse anything outside the known SMART tools.
    if let Some(bad) = packages.iter().find(|p| !ALLOWED_PACKAGES.contains(p)) {
        emit(false, format!("package not allowed: {bad}"));
        return ExitCode::from(3);
    }
    let Some(steps) = install_steps(manager, &packages) else {
        emit(false, format!("unsupported package manager: {manager}"));
        return ExitCode::from(3);
    };
    for (i, mut step) in steps.into_iter().enumerate() {
        let is_apt_update = manager == "apt" && i == 0;
        match step.status() {
            Ok(s) if s.success() => {}
            Ok(_) if is_apt_update => { /* stale mirror — tolerate, install may still work */ }
            Ok(s) => {
                emit(
                    false,
                    format!("install failed (exit {})", s.code().unwrap_or(-1)),
                );
                return ExitCode::from(1);
            }
            Err(err) => {
                emit(false, format!("failed to run {manager}: {err}"));
                return ExitCode::from(1);
            }
        }
    }
    emit(true, format!("Installed: {}", packages.join(", ")));
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s == "smart").unwrap_or(false) {
        return run_smart(&args[2..]);
    }
    if args.get(1).map(|s| s == "install-deps").unwrap_or(false) {
        return run_install_deps(&args[2..]);
    }

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("freeyourdisk-helper: failed to read stdin");
        return ExitCode::from(2);
    }

    let plan: DeletionPlan = match serde_json::from_str(&input) {
        Ok(plan) => plan,
        Err(err) => {
            eprintln!("freeyourdisk-helper: invalid plan: {err}");
            return ExitCode::from(2);
        }
    };

    let zones = Zones(ROOT_ZONES.iter().map(PathBuf::from).collect());
    // Validation + execution is the shared single source of truth (also used by
    // the Windows elevated executor). All-or-nothing: any escape refuses the batch.
    match core_trash::execute_root_plan(&plan, &zones) {
        Ok(report) => {
            write_report(&report);
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            write_report(&refusal);
            ExitCode::from(3)
        }
    }
}
