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

use core_ipc::{DeletionPlan, Destination, ExecutionReport, ItemError, SmartInfo};
use core_trash::{delete_permanent, to_trash, validate, Zones};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// Root-owned zones this helper is ever allowed to touch. Hard-coded — never
/// received from the (semi-trusted) caller.
const ROOT_ZONES: &[&str] = &["/tmp", "/var/tmp"];

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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s == "smart").unwrap_or(false) {
        return run_smart(&args[2..]);
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
    let paths: Vec<PathBuf> = plan.items.iter().map(|item| item.path.clone()).collect();

    // Pre-validate every path; refuse the whole batch on any escape.
    let refusals: Vec<ItemError> = paths
        .iter()
        .filter_map(|path| match validate(path, &zones) {
            Ok(_) => None,
            Err(err) => Some(ItemError {
                path: path.clone(),
                message: err.to_string(),
            }),
        })
        .collect();

    if !refusals.is_empty() {
        write_report(&ExecutionReport {
            freed_bytes: 0,
            deleted_count: 0,
            errors: refusals,
        });
        return ExitCode::from(3);
    }

    let report = match plan.destination {
        Destination::Trash => to_trash(&paths, &zones),
        Destination::Permanent => delete_permanent(&paths, &zones),
    };
    write_report(&report);
    ExitCode::SUCCESS
}
