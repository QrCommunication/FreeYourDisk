// SPDX-License-Identifier: GPL-3.0-or-later
//! Execution routing: user-owned items are deleted in-process (validated
//! against the home zone); root-owned items are handed to the privileged
//! helper via `pkexec`. The helper invocation is injected so the routing logic
//! is unit-testable without root.

#[cfg(not(target_os = "macos"))]
use core_ipc::InstallReport;
use core_ipc::{DeletionPlan, Destination, ExecutionReport, ItemError, ScanItem, SmartInfo};
use core_trash::Zones;
// `Write`/`Stdio` are used only by the Linux pkexec helper (stdin-piped IPC);
// macOS (osascript) and Windows (PowerShell) stage the plan via temp files.
#[cfg(target_os = "linux")]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::Stdio;

/// Installed location of the privileged helper.
pub const HELPER_PATH: &str = "/usr/lib/freeyourdisk/freeyourdisk-helper";

/// Resolve the helper binary: the installed path in production, or a sibling of
/// the running executable when developing (`cargo tauri dev`).
pub fn resolve_helper_path() -> PathBuf {
    let installed = PathBuf::from(HELPER_PATH);
    if installed.exists() {
        return installed;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Dev build / Linux: sibling of the running binary.
            let sibling = dir.join("freeyourdisk-helper");
            if sibling.exists() {
                return sibling;
            }
            // macOS .app bundle: Contents/MacOS/<exe> → Contents/Resources/helper.
            #[cfg(target_os = "macos")]
            {
                let resource = dir.join("../Resources/freeyourdisk-helper");
                if resource.exists() {
                    return resource;
                }
            }
        }
    }
    installed
}

/// User-deletion zone: anything under the user's home.
fn user_zones(home: &Path) -> Zones {
    Zones(vec![home.to_path_buf()])
}

/// Delete the user-owned items (validated against the home zone).
fn execute_user_items(
    items: &[ScanItem],
    destination: Destination,
    home: &Path,
) -> ExecutionReport {
    if items.is_empty() {
        return ExecutionReport::default();
    }
    let paths: Vec<PathBuf> = items.iter().map(|item| item.path.clone()).collect();
    let zones = user_zones(home);
    match destination {
        Destination::Trash => core_trash::to_trash(&paths, &zones),
        Destination::Permanent => core_trash::delete_permanent(&paths, &zones),
    }
}

fn merge(into: &mut ExecutionReport, other: ExecutionReport) {
    into.freed_bytes += other.freed_bytes;
    into.deleted_count += other.deleted_count;
    into.errors.extend(other.errors);
}

/// Route a plan: user items in-process, root items to `invoke_helper`.
pub fn execute_plan(
    plan: &DeletionPlan,
    home: &Path,
    invoke_helper: impl Fn(&DeletionPlan) -> ExecutionReport,
) -> ExecutionReport {
    let (root_items, user_items): (Vec<ScanItem>, Vec<ScanItem>) = plan
        .items
        .iter()
        .cloned()
        .partition(|item| item.requires_root);

    let mut report = execute_user_items(&user_items, plan.destination, home);

    if !root_items.is_empty() {
        let total_bytes = root_items.iter().map(|item| item.size_bytes).sum();
        let root_plan = DeletionPlan {
            items: root_items,
            destination: Destination::Permanent,
            total_bytes,
            requires_root: true,
        };
        merge(&mut report, invoke_helper(&root_plan));
    }

    report
}

fn err_report(plan: &DeletionPlan, message: &str) -> ExecutionReport {
    ExecutionReport {
        freed_bytes: 0,
        deleted_count: 0,
        errors: plan
            .items
            .iter()
            .map(|item| ItemError {
                path: item.path.clone(),
                message: message.to_string(),
            })
            .collect(),
    }
}

/// Real helper invocation: `pkexec <helper>`, plan on stdin, report on stdout.
#[cfg(target_os = "linux")]
pub fn pkexec_helper(plan: &DeletionPlan) -> ExecutionReport {
    let json = match serde_json::to_string(plan) {
        Ok(json) => json,
        Err(err) => return err_report(plan, &err.to_string()),
    };

    let child = Command::new("pkexec")
        .arg(resolve_helper_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(err) => return err_report(plan, &format!("pkexec spawn failed: {err}")),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(json.as_bytes());
    }

    match child.wait_with_output() {
        Ok(out) => serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
            err_report(
                plan,
                "helper returned no report (authentication cancelled?)",
            )
        }),
        Err(err) => err_report(plan, &err.to_string()),
    }
}

/// Windows: relaunch THIS exe elevated (UAC) in headless `--apply` mode to run
/// the root plan. Only the parent PID is passed as an argument (no spaces); both
/// sides derive `%TEMP%\fyd-apply-<token>-{plan,report}.json`. Elevation uses
/// `powershell Start-Process -Verb RunAs -Wait` — the WinAPI-free analogue of the
/// macOS osascript-admin path.
#[cfg(target_os = "windows")]
pub fn pkexec_helper(plan: &DeletionPlan) -> ExecutionReport {
    let json = match serde_json::to_string(plan) {
        Ok(json) => json,
        Err(err) => return err_report(plan, &err.to_string()),
    };
    let token = elevation_token();
    let tmp = std::env::temp_dir();
    let plan_path = tmp.join(format!("fyd-apply-{token}-plan.json"));
    let report_path = tmp.join(format!("fyd-apply-{token}-report.json"));
    let _ = std::fs::remove_file(&report_path);
    if std::fs::write(&plan_path, &json).is_err() {
        return err_report(plan, "failed to stage deletion plan");
    }

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            let _ = std::fs::remove_file(&plan_path);
            return err_report(plan, &format!("cannot locate exe: {err}"));
        }
    };
    // Single-quote the exe path for PowerShell, doubling any embedded single quote.
    let exe_ps = exe.to_string_lossy().replace('\'', "''");
    let ps = format!(
        "Start-Process -FilePath '{exe_ps}' -ArgumentList '--apply','{token}' -Verb RunAs -Wait -WindowStyle Hidden"
    );

    // Absolute path, not a PATH lookup: a PATH-poisoned powershell.exe could
    // control the RunAs target shown in the UAC prompt. Fixed on Win10/11.
    let status = Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status();

    let result = match status {
        Ok(s) if s.success() => std::fs::read_to_string(&report_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(|| err_report(plan, "elevated helper returned no report")),
        Ok(_) => err_report(plan, "elevation cancelled or failed"),
        Err(err) => err_report(plan, &format!("failed to launch elevation: {err}")),
    };
    let _ = std::fs::remove_file(&plan_path);
    let _ = std::fs::remove_file(&report_path);
    result
}

/// Read SMART for every device in one privileged call. Returns an empty vec if
/// pkexec is cancelled or the helper/smartctl is unavailable (graceful).
#[cfg(target_os = "linux")]
pub fn pkexec_smart(devices: &[String]) -> Vec<SmartInfo> {
    if devices.is_empty() {
        return Vec::new();
    }
    let mut cmd = Command::new("pkexec");
    cmd.arg(resolve_helper_path()).arg("smart");
    for dev in devices {
        cmd.arg(dev);
    }
    match cmd.output() {
        Ok(out) => serde_json::from_slice(&out.stdout).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

#[cfg(target_os = "windows")]
pub fn pkexec_smart(_devices: &[String]) -> Vec<SmartInfo> {
    // The elevated child self-discovers devices via `smartctl --scan-open`, so
    // the caller's device list is unused. One UAC prompt; report read from file.
    let token = elevation_token();
    let report_path = std::env::temp_dir().join(format!("fyd-smart-{token}-report.json"));
    let _ = std::fs::remove_file(&report_path);

    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let exe_ps = exe.to_string_lossy().replace('\'', "''");
    let ps = format!(
        "Start-Process -FilePath '{exe_ps}' -ArgumentList '--smart','{token}' -Verb RunAs -Wait -WindowStyle Hidden"
    );
    let status = Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status();
    let result = match status {
        Ok(s) if s.success() => std::fs::read_to_string(&report_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let _ = std::fs::remove_file(&report_path);
    result
}

// ---------------------------------------------------------------------------
// macOS: privilege escalation via the native auth dialog
// (`osascript … with administrator privileges`). The plan is staged to a temp
// file the helper reads from stdin; the helper re-validates everything.
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn osascript_admin(shell_cmd: &str) -> Option<String> {
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        shell_cmd.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let out = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None; // user cancelled the auth dialog
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "macos")]
pub fn pkexec_helper(plan: &DeletionPlan) -> ExecutionReport {
    let json = match serde_json::to_string(plan) {
        Ok(json) => json,
        Err(err) => return err_report(plan, &err.to_string()),
    };
    let tmp = std::env::temp_dir().join(format!("fyd-plan-{}.json", std::process::id()));
    if std::fs::write(&tmp, &json).is_err() {
        return err_report(plan, "failed to stage deletion plan");
    }
    let shell = format!(
        "{} < {}",
        shell_quote(&resolve_helper_path().to_string_lossy()),
        shell_quote(&tmp.to_string_lossy())
    );
    let result = osascript_admin(&shell);
    let _ = std::fs::remove_file(&tmp);
    match result {
        Some(stdout) => serde_json::from_str(stdout.trim())
            .unwrap_or_else(|_| err_report(plan, "helper returned no report (cancelled?)")),
        None => err_report(plan, "authentication cancelled"),
    }
}

#[cfg(target_os = "macos")]
pub fn pkexec_smart(devices: &[String]) -> Vec<SmartInfo> {
    if devices.is_empty() {
        return Vec::new();
    }
    let mut shell = shell_quote(&resolve_helper_path().to_string_lossy());
    shell.push_str(" smart");
    for dev in devices {
        shell.push(' ');
        shell.push_str(&shell_quote(dev));
    }
    match osascript_admin(&shell) {
        Some(stdout) => serde_json::from_str(stdout.trim()).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Install SMART tools as root via the helper (`install-deps <manager> <pkg>…`).
/// The helper re-validates the package names against its own allowlist.
/// (Linux only — macOS installs via Homebrew at user level.)
#[cfg(not(target_os = "macos"))]
pub fn pkexec_install_deps(manager: &str, packages: &[String]) -> InstallReport {
    let mut cmd = Command::new("pkexec");
    cmd.arg(resolve_helper_path())
        .arg("install-deps")
        .arg(manager);
    for pkg in packages {
        cmd.arg(pkg);
    }
    match cmd.output() {
        Ok(out) => serde_json::from_slice(&out.stdout).unwrap_or(InstallReport {
            success: false,
            message: "installation cancelled or helper unavailable".to_string(),
        }),
        Err(err) => InstallReport {
            success: false,
            message: format!("pkexec spawn failed: {err}"),
        },
    }
}

/// Windows: install smartmontools via winget. `--id` is a fixed allowlisted
/// package; nothing user-controlled reaches the command line.
#[cfg(target_os = "windows")]
pub fn winget_install_smart() -> InstallReport {
    let out = Command::new("winget")
        .args([
            "install",
            "--id",
            "smartmontools.smartmontools",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--silent",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => InstallReport {
            success: true,
            message: "Installed: smartmontools".to_string(),
        },
        Ok(o) => InstallReport {
            success: false,
            message: String::from_utf8_lossy(&o.stderr)
                .lines()
                .last()
                .unwrap_or("winget install failed")
                .to_string(),
        },
        Err(err) => InstallReport {
            success: false,
            message: format!("failed to run winget: {err}"),
        },
    }
}

/// A random, unguessable token (20 decimal digits) for the elevated-IPC temp
/// file names. Random — not the PID — so a local same-user attacker cannot
/// pre-create a junction/file at a predictable path (TOCTOU) to redirect the
/// admin write or inject a forged report. Digit-only to satisfy the elevated
/// child's token guard. Falls back to the PID only if the OS RNG is unavailable.
#[allow(dead_code)] // used only by the Windows elevated executors
fn elevation_token() -> String {
    let mut buf = [0u8; 8];
    match getrandom::getrandom(&mut buf) {
        Ok(()) => format!("{:020}", u64::from_le_bytes(buf)),
        Err(_) => std::process::id().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_ipc::ItemKind;
    use std::cell::Cell;

    fn item(path: PathBuf, size: u64, root: bool) -> ScanItem {
        ScanItem {
            id: path.to_string_lossy().into_owned(),
            path,
            size_bytes: size,
            last_access: None,
            kind: ItemKind::File,
            requires_root: root,
        }
    }

    #[test]
    fn routes_user_to_disk_and_root_to_helper() {
        let home = tempfile::tempdir().unwrap();
        let f = home.path().join("cache/junk.bin");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, vec![0u8; 20]).unwrap();

        let plan = DeletionPlan {
            items: vec![
                item(f.clone(), 20, false),
                item(PathBuf::from("/tmp/whatever"), 5, true),
            ],
            destination: Destination::Permanent,
            total_bytes: 25,
            requires_root: true,
        };

        let called = Cell::new(false);
        let report = execute_plan(&plan, home.path(), |p| {
            called.set(true);
            assert!(
                p.items.iter().all(|i| i.requires_root),
                "only root items go to helper"
            );
            ExecutionReport {
                freed_bytes: 5,
                deleted_count: 1,
                errors: vec![],
            }
        });

        assert!(called.get(), "root items must be routed to the helper");
        assert!(!f.exists(), "user item must be deleted");
        assert_eq!(report.deleted_count, 2);
        assert_eq!(report.freed_bytes, 25);
    }

    #[test]
    fn user_item_outside_home_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let victim = other.path().join("keep.txt");
        std::fs::write(&victim, b"important").unwrap();

        let plan = DeletionPlan {
            items: vec![item(victim.clone(), 1, false)],
            destination: Destination::Permanent,
            total_bytes: 1,
            requires_root: false,
        };

        let report = execute_plan(&plan, home.path(), |_| ExecutionReport::default());
        assert_eq!(report.deleted_count, 0);
        assert!(victim.exists(), "a file outside home must never be deleted");
        assert_eq!(report.errors.len(), 1);
    }
}
