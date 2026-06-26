// SPDX-License-Identifier: GPL-3.0-or-later
//! Headless mode for the optional systemd user timer.
//!
//! Runs a non-interactive cleanup of old files in `~/.cache` only — strictly
//! user-owned zones, never root. The timer never triggers privileged actions.

use core_services::{Service, TempRoot, TempService};
use core_trash::{to_trash, Zones};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Outcome of a headless run.
#[derive(Debug, PartialEq, Eq)]
pub struct HeadlessOutcome {
    pub considered: usize,
    pub freed_bytes: u64,
    pub deleted_count: usize,
    pub applied: bool,
}

/// Default age threshold for the scheduled cache cleanup.
const MIN_AGE_DAYS: u32 = 7;

/// Scan (and optionally trash) old files under `~/.cache`. User-only by
/// construction: the single root is the user's cache, marked non-root.
pub fn cache_cleanup(home: &Path, min_age_days: u32, apply: bool) -> HeadlessOutcome {
    let service = TempService {
        roots: vec![TempRoot {
            path: home.join(".cache"),
            requires_root: false,
        }],
        min_age_days,
    };
    let items = service.scan().items;
    let considered = items.len();

    if !apply {
        return HeadlessOutcome {
            considered,
            freed_bytes: 0,
            deleted_count: 0,
            applied: false,
        };
    }

    let zones = Zones(vec![home.to_path_buf()]);
    let paths: Vec<PathBuf> = items.iter().map(|item| item.path.clone()).collect();
    let report = to_trash(&paths, &zones);

    HeadlessOutcome {
        considered,
        freed_bytes: report.freed_bytes,
        deleted_count: report.deleted_count,
        applied: true,
    }
}

fn humanize(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut i = 0;
    while value >= 1024.0 && i < UNITS.len() - 1 {
        value /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[i])
    }
}

fn is_french() -> bool {
    std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LANG"))
        .or_else(|_| std::env::var("LANGUAGE"))
        .map(|lang| lang.to_lowercase().starts_with("fr"))
        .unwrap_or(false)
}

fn notify(freed_bytes: u64, count: usize) {
    let body = if is_french() {
        format!("{} libérés · {count} éléments", humanize(freed_bytes))
    } else {
        format!("{} freed · {count} items", humanize(freed_bytes))
    };
    let _ = Command::new("notify-send")
        .arg("--app-name=FreeYourDisk")
        .arg("FreeYourDisk")
        .arg(body)
        .status();
}

/// CLI entry point for `--headless`. Returns a process exit code.
pub fn run(args: &[String]) -> i32 {
    // Only the temp/cache service is supported in headless mode.
    let service = args
        .iter()
        .find_map(|a| a.strip_prefix("--service="))
        .unwrap_or("temp");
    if service != "temp" {
        eprintln!("freeyourdisk --headless: only --service=temp is supported");
        return 2;
    }

    let apply = args.iter().any(|a| a == "--apply");
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));

    let outcome = cache_cleanup(&home, MIN_AGE_DAYS, apply);

    if apply {
        if outcome.deleted_count > 0 {
            notify(outcome.freed_bytes, outcome.deleted_count);
        }
        println!(
            "freeyourdisk: freed {} from {} items",
            humanize(outcome.freed_bytes),
            outcome.deleted_count
        );
    } else {
        println!(
            "freeyourdisk: [dry-run] {} cleanup candidates",
            outcome.considered
        );
    }
    0
}

/// Windows: the elevated child. Reads the plan staged by the un-elevated parent
/// at `%TEMP%\fyd-apply-<token>-plan.json`, re-validates against the hard-coded
/// Windows root zone (`%WINDIR%\Temp`), deletes, and writes the report to
/// `%TEMP%\fyd-apply-<token>-report.json`. `token` is the parent PID (no spaces).
#[cfg(target_os = "windows")]
pub fn apply_elevated(token: &str) -> i32 {
    use core_trash::Zones;
    // Hardening: this runs ELEVATED. `token` (the parent PID) is interpolated
    // into a %TEMP% path — reject anything but ASCII digits so a crafted token
    // can never traverse out of %TEMP% (arbitrary admin file read/write).
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return 2;
    }
    let tmp = std::env::temp_dir();
    let plan_path = tmp.join(format!("fyd-apply-{token}-plan.json"));
    let report_path = tmp.join(format!("fyd-apply-{token}-report.json"));

    let Ok(raw) = std::fs::read_to_string(&plan_path) else {
        return 2;
    };
    let Ok(plan) = serde_json::from_str::<core_ipc::DeletionPlan>(&raw) else {
        return 2;
    };

    // Hard-coded privileged zone — NOT derived from %WINDIR%: env vars propagate
    // across same-user UAC elevation and are attacker-influenceable (classic
    // windir UAC-bypass vector), so an env-derived zone would be an admin-delete
    // EoP. Plain path is correct: core_trash::validate normalizes candidates with
    // dunce (no \\?\ verbatim prefix) so a plain zone matches; junctions inside
    // the zone are still resolved and refused by validate's symlink check.
    let zones = Zones(vec![std::path::PathBuf::from("C:\\Windows\\Temp")]);

    let report = match core_trash::execute_root_plan(&plan, &zones) {
        Ok(report) => report,
        Err(refusal) => refusal,
    };
    let json = serde_json::to_string(&report).unwrap_or_default();
    if std::fs::write(&report_path, json).is_err() {
        return 1;
    }
    0
}

/// Windows: the elevated SMART reader. Discovers devices with
/// `smartctl --scan-open` and reads each with `smartctl -a -j`, writing a
/// Vec<SmartInfo> to %TEMP%\fyd-smart-<token>-report.json. `token` = parent PID.
#[cfg(target_os = "windows")]
pub fn read_smart_elevated(token: &str) -> i32 {
    use core_ipc::SmartInfo;
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return 2;
    }
    let report_path = std::env::temp_dir().join(format!("fyd-smart-{token}-report.json"));

    // Elevated context: resolve smartctl ONLY from trusted absolute install
    // locations. NEVER fall back to PATH — a planted smartctl.exe on a PATH dir
    // would execute as admin (binary-planting EoP). Absent → SMART unavailable.
    let smartctl = match [
        "C:\\Program Files\\smartmontools\\bin\\smartctl.exe",
        "C:\\Program Files (x86)\\smartmontools\\bin\\smartctl.exe",
    ]
    .into_iter()
    .find(|p| std::path::Path::new(p).is_file())
    {
        Some(path) => path.to_string(),
        None => {
            let _ = std::fs::write(&report_path, "[]");
            return 0;
        }
    };

    let scan = std::process::Command::new(&smartctl)
        .args(["--scan-open", "-j"])
        .output();
    let mut results: Vec<SmartInfo> = Vec::new();
    if let Ok(out) = scan {
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            if let Some(devices) = json.get("devices").and_then(|d| d.as_array()) {
                for dev in devices {
                    let Some(name) = dev.get("name").and_then(|n| n.as_str()) else {
                        continue;
                    };
                    results.push(read_one_smart(&smartctl, name));
                }
            }
        }
    }
    let json = serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string());
    if std::fs::write(&report_path, json).is_err() {
        return 1;
    }
    0
}

/// Read SMART for one device token via `smartctl -a -j`.
#[cfg(target_os = "windows")]
fn read_one_smart(smartctl: &str, device: &str) -> core_ipc::SmartInfo {
    use core_ipc::SmartInfo;
    let unavailable = SmartInfo {
        device: device.to_string(),
        available: false,
        passed: None,
        power_on_hours: None,
        temperature_c: None,
    };
    let Ok(out) = std::process::Command::new(smartctl)
        .args(["-a", "-j", device])
        .output()
    else {
        return unavailable;
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return unavailable;
    };
    let passed = json
        .get("smart_status")
        .and_then(|s| s.get("passed"))
        .and_then(|v| v.as_bool());
    let power_on_hours = json
        .get("power_on_time")
        .and_then(|p| p.get("hours"))
        .and_then(|v| v.as_u64());
    let temperature_c = json
        .get("temperature")
        .and_then(|t| t.get("current"))
        .and_then(|v| v.as_i64());
    let available = passed.is_some() || power_on_hours.is_some() || temperature_c.is_some();
    SmartInfo {
        device: device.to_string(),
        available,
        passed,
        power_on_hours,
        temperature_c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::{Duration, SystemTime};

    fn backdate(path: &Path, days: u64) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
            .unwrap();
    }

    #[test]
    fn dry_run_frees_nothing() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join(".cache");
        std::fs::create_dir(&cache).unwrap();
        let f = cache.join("old.log");
        std::fs::write(&f, vec![0u8; 100]).unwrap();
        backdate(&f, 30);

        let outcome = cache_cleanup(home.path(), 7, false);
        assert!(!outcome.applied);
        assert_eq!(outcome.freed_bytes, 0);
        assert_eq!(outcome.deleted_count, 0);
        assert!(
            outcome.considered >= 1,
            "old cache file should be a candidate"
        );
        assert!(f.exists(), "dry-run must not delete");
    }

    #[test]
    fn recent_files_are_not_candidates() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join(".cache");
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join("fresh.log"), vec![0u8; 50]).unwrap();

        let outcome = cache_cleanup(home.path(), 7, false);
        assert_eq!(outcome.considered, 0, "recent files must be filtered out");
    }
}
