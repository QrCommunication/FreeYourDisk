// SPDX-License-Identifier: GPL-3.0-or-later
//! Installed-application inventory: apt (dpkg), flatpak, snap and AppImages,
//! ranked by disk space. Supports batch uninstall and batch update; checking
//! for newer versions is a separate, on-demand pass.

use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
use std::process::Command;

#[derive(Serialize, Clone, Debug)]
pub struct AppEntry {
    /// Unique id, prefixed by source: `apt:pkg`, `flatpak:id`, `snap:name`,
    /// `appimage:/abs/path`.
    pub id: String,
    pub name: String,
    /// "apt" | "flatpak" | "snap" | "appimage"
    pub source: String,
    pub version: Option<String>,
    pub size_bytes: u64,
    /// Removal/update needs administrator rights (apt, snap, system flatpak).
    pub requires_root: bool,
    /// Essential system component: updates allowed, uninstall forbidden.
    pub protected: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct AppActionReport {
    pub succeeded: Vec<String>,
    pub errors: Vec<String>,
}

#[cfg(target_os = "linux")]
const APT_TOP: usize = 80;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(target_os = "windows"))]
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

// ---- apt / dpkg -----------------------------------------------------------

#[cfg(target_os = "linux")]
fn detect_apt() -> Vec<AppEntry> {
    let Some(out) = run(
        "dpkg-query",
        &[
            "-W",
            "-f=${Package}\\t${Version}\\t${Installed-Size}\\t${Status}\\t${Essential}\\t${Priority}\\n",
        ],
    ) else {
        return Vec::new();
    };
    let mut apps: Vec<AppEntry> = out
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 4 || !f[3].contains("installed") {
                return None;
            }
            let size = f[2].parse::<u64>().unwrap_or(0) * 1024; // Installed-Size is KiB
                                                                // Essential=yes or Priority=required/important → forbidden to remove.
            let essential = f.get(4).map(|s| s.trim() == "yes").unwrap_or(false);
            let priority = f.get(5).map(|s| s.trim()).unwrap_or("");
            let protected = essential || priority == "required" || priority == "important";
            Some(AppEntry {
                id: format!("apt:{}", f[0]),
                name: f[0].to_string(),
                source: "apt".into(),
                version: Some(f[1].to_string()),
                size_bytes: size,
                requires_root: true,
                protected,
            })
        })
        .collect();
    apps.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
    apps.truncate(APT_TOP);
    apps
}

// ---- flatpak --------------------------------------------------------------

#[cfg(target_os = "linux")]
fn parse_human_size(s: &str) -> u64 {
    let s = s.trim().replace(',', ".");
    let (num, mult) = if let Some(n) = s.strip_suffix("GB").or_else(|| s.strip_suffix("GiB")) {
        (n, 1024u64.pow(3))
    } else if let Some(n) = s.strip_suffix("MB").or_else(|| s.strip_suffix("MiB")) {
        (n, 1024u64.pow(2))
    } else if let Some(n) = s
        .strip_suffix("kB")
        .or_else(|| s.strip_suffix("KB"))
        .or_else(|| s.strip_suffix("KiB"))
    {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix("B") {
        (n, 1)
    } else {
        (s.as_str(), 1)
    };
    (num.trim().parse::<f64>().unwrap_or(0.0) * mult as f64) as u64
}

#[cfg(target_os = "linux")]
fn detect_flatpak() -> Vec<AppEntry> {
    let Some(out) = run(
        "flatpak",
        &[
            "list",
            "--app",
            "--columns=application,name,version,size,installation",
        ],
    ) else {
        return Vec::new();
    };
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 4 {
                return None;
            }
            let installation = f.get(4).copied().unwrap_or("user");
            Some(AppEntry {
                id: format!("flatpak:{}", f[0]),
                name: if f[1].is_empty() {
                    f[0].to_string()
                } else {
                    f[1].to_string()
                },
                source: "flatpak".into(),
                version: (!f[2].is_empty()).then(|| f[2].to_string()),
                size_bytes: parse_human_size(f[3]),
                requires_root: installation.trim() == "system",
                protected: false,
            })
        })
        .collect()
}

// ---- snap -----------------------------------------------------------------

#[cfg(target_os = "linux")]
fn snap_size(name: &str) -> u64 {
    // The installed snap is the squashfs at /var/lib/snapd/snaps/<name>_<rev>.snap
    let dir = Path::new("/var/lib/snapd/snaps");
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    read.flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{name}_"))
        })
        .filter_map(|e| e.metadata().ok().map(|m| m.len()))
        .max()
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn detect_snap() -> Vec<AppEntry> {
    let Some(out) = run("snap", &["list"]) else {
        return Vec::new();
    };
    out.lines()
        .skip(1) // header
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 2 {
                return None;
            }
            let name = f[0];
            // Base/runtime snaps underpin every other snap — never removable.
            let protected = matches!(
                name,
                "core" | "core18" | "core20" | "core22" | "core24" | "snapd" | "bare"
            ) || name.starts_with("gnome-")
                || name.starts_with("gtk-common");
            Some(AppEntry {
                id: format!("snap:{name}"),
                name: name.to_string(),
                source: "snap".into(),
                version: Some(f[1].to_string()),
                size_bytes: snap_size(name),
                requires_root: true,
                protected,
            })
        })
        .collect()
}

// ---- AppImages & app folders ---------------------------------------------

#[cfg(target_os = "linux")]
fn detect_appimages() -> Vec<AppEntry> {
    let h = home();
    let dirs = [
        h.join("Applications"),
        h.join("applications"),
        h.join(".local/bin"),
        h.join("bin"),
        h.join("Downloads"),
        h.join("Téléchargements"),
        PathBuf::from("/opt"),
    ];
    let mut apps = Vec::new();
    for dir in dirs.iter() {
        let Ok(read) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_appimage = name.to_lowercase().ends_with(".appimage");
            if meta.is_file() && is_appimage {
                apps.push(AppEntry {
                    id: format!("appimage:{}", path.to_string_lossy()),
                    name: name
                        .trim_end_matches(".AppImage")
                        .trim_end_matches(".appimage")
                        .to_string(),
                    source: "appimage".into(),
                    version: None,
                    size_bytes: meta.len(),
                    requires_root: !path.starts_with(&h),
                    protected: false,
                });
            } else if meta.is_dir() && (dir.ends_with("Applications") || dir == Path::new("/opt")) {
                let size = core_scan::cache::cached_dir_total(&path);
                if size == 0 {
                    continue;
                }
                apps.push(AppEntry {
                    id: format!("appimage:{}", path.to_string_lossy()),
                    name,
                    source: "appimage".into(),
                    version: None,
                    size_bytes: size,
                    requires_root: !path.starts_with(&h),
                    protected: false,
                });
            }
        }
    }
    apps
}

/// Full inventory, largest first.
#[cfg(target_os = "linux")]
pub fn list() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    apps.extend(detect_apt());
    apps.extend(detect_flatpak());
    apps.extend(detect_snap());
    apps.extend(detect_appimages());
    apps.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
    apps
}

// ---- Windows: classic apps via the registry Uninstall keys ----------------

/// Maps an id hive label to (predefined hive RegKey, Uninstall base subpath).
/// HKLM = 64-bit machine view, HKLM32 = 32-bit (WOW6432Node) machine view,
/// HKCU = per-user. Predefined RegKeys are not closed on drop (winreg special-
/// cases them), so returning an owned RegKey is cheap and correct.
#[cfg(target_os = "windows")]
fn registry_hive(label: &str) -> Option<(winreg::RegKey, &'static str)> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    const UNINSTALL32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    match label {
        "HKLM" => Some((RegKey::predef(HKEY_LOCAL_MACHINE), UNINSTALL)),
        "HKLM32" => Some((RegKey::predef(HKEY_LOCAL_MACHINE), UNINSTALL32)),
        "HKCU" => Some((RegKey::predef(HKEY_CURRENT_USER), UNINSTALL)),
        _ => None,
    }
}

/// Enumerate the three Uninstall hives. Skip entries with no DisplayName, with
/// SystemComponent==1 (hidden component), or with no uninstall command (updates/
/// patches). id = `registry:<HIVE>:<subkey>` so uninstall re-opens the exact
/// hive/view. size = EstimatedSize (KB) * 1024. Dedupe identical (name, version).
#[cfg(target_os = "windows")]
fn detect_registry() -> Vec<AppEntry> {
    use winreg::enums::KEY_READ;
    let mut out = Vec::new();
    // (hive label, requires_root): HKLM/HKLM32 are machine-wide → admin to remove.
    for (label, requires_root) in [("HKLM", true), ("HKLM32", true), ("HKCU", false)] {
        let Some((hive, base)) = registry_hive(label) else {
            continue;
        };
        let Ok(uninstall) = hive.open_subkey_with_flags(base, KEY_READ) else {
            continue;
        };
        for name in uninstall.enum_keys().flatten() {
            let Ok(sub) = uninstall.open_subkey_with_flags(&name, KEY_READ) else {
                continue;
            };
            // DisplayName is mandatory.
            let Ok(display_name) = sub.get_value::<String, _>("DisplayName") else {
                continue;
            };
            if display_name.trim().is_empty() {
                continue;
            }
            // SystemComponent==1 → hidden component / update, not a user app.
            if sub.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
                continue;
            }
            // Must carry a usable uninstall command, else it is an update/patch.
            let quiet = sub.get_value::<String, _>("QuietUninstallString").ok();
            let plain = sub.get_value::<String, _>("UninstallString").ok();
            let has_cmd = quiet
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
                || plain
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
            if !has_cmd {
                continue;
            }
            let version = sub
                .get_value::<String, _>("DisplayVersion")
                .ok()
                .filter(|s| !s.trim().is_empty());
            let kb = sub.get_value::<u32, _>("EstimatedSize").unwrap_or(0);
            out.push(AppEntry {
                id: format!("registry:{label}:{name}"),
                name: display_name,
                source: "registry".into(),
                version,
                size_bytes: u64::from(kb) * 1024,
                requires_root,
                protected: false,
            });
        }
    }
    // Dedupe the same app surfaced in multiple hives; keep the first (HKLM wins).
    let mut seen: HashSet<(String, Option<String>)> = HashSet::new();
    out.retain(|a| seen.insert((a.name.clone(), a.version.clone())));
    out
}

// ---- Windows: MSIX / Store apps via Get-AppxPackage --------------------------

/// Absolute path so PATH/profile cannot redirect to a fake powershell.
#[cfg(target_os = "windows")]
const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

/// One row of the `Get-AppxPackage | ConvertTo-Json` output. Version and
/// SignatureKind are forced to strings in the script (see SCRIPT below).
#[cfg(target_os = "windows")]
#[derive(serde::Deserialize)]
struct AppxRaw {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "PackageFullName")]
    package_full_name: Option<String>,
    #[serde(rename = "Version")]
    version: Option<String>,
    #[serde(rename = "InstallLocation")]
    install_location: Option<String>,
    #[serde(rename = "NonRemovable")]
    non_removable: Option<bool>,
    #[serde(rename = "SignatureKind")]
    signature_kind: Option<String>,
}

/// MSIX packages for the current user. Skips OS framework packages
/// (SignatureKind == "System"). protected = NonRemovable. size = best-effort dir
/// size of InstallLocation. requires_root = false (per-user removal).
#[cfg(target_os = "windows")]
fn detect_msix() -> Vec<AppEntry> {
    // `.ToString()` coerces System.Version and the SignatureKind enum to plain
    // strings; otherwise ConvertTo-Json emits Version as a nested object.
    const SCRIPT: &str = "Get-AppxPackage | Select-Object Name,PackageFullName,\
@{N='Version';E={$_.Version.ToString()}},InstallLocation,NonRemovable,\
@{N='SignatureKind';E={$_.SignatureKind.ToString()}} | ConvertTo-Json -Compress -Depth 3";
    let Some(out) = run(
        POWERSHELL,
        &["-NoProfile", "-NonInteractive", "-Command", SCRIPT],
    ) else {
        return Vec::new();
    };
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    // ConvertTo-Json emits a bare object for a single package, an array otherwise.
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Vec::new();
    };
    let items = match value {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    let mut apps = Vec::new();
    for item in items {
        let Ok(pkg) = serde_json::from_value::<AppxRaw>(item) else {
            continue;
        };
        let (Some(name), Some(pfn)) = (
            pkg.name.filter(|s| !s.trim().is_empty()),
            pkg.package_full_name.filter(|s| !s.trim().is_empty()),
        ) else {
            continue;
        };
        // OS frameworks are signed "System" — not user-facing apps.
        if pkg.signature_kind.as_deref() == Some("System") {
            continue;
        }
        let size = pkg
            .install_location
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| core_scan::cache::cached_dir_total(Path::new(s)))
            .unwrap_or(0);
        apps.push(AppEntry {
            id: format!("msix:{pfn}"),
            name,
            source: "msix".into(),
            version: pkg.version.filter(|s| !s.trim().is_empty()),
            size_bytes: size,
            requires_root: false,
            protected: pkg.non_removable.unwrap_or(false),
        });
    }
    apps
}

/// Full inventory (registry + MSIX), largest first.
#[cfg(target_os = "windows")]
pub fn list() -> Vec<AppEntry> {
    let mut apps = detect_registry();
    apps.extend(detect_msix());
    apps.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
    apps
}

#[cfg(target_os = "macos")]
pub fn list() -> Vec<AppEntry> {
    let mut apps = detect_macos_apps();
    apps.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
    apps
}

/// Recursive byte size of an `.app` bundle (BSD `du -skx` → KB).
#[cfg(target_os = "macos")]
fn app_dir_size(path: &Path) -> u64 {
    Command::new("du")
        .args(["-skx"])
        .arg(path)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
        })
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

/// `.app` bundles in /Applications and ~/Applications, ranked by size.
#[cfg(target_os = "macos")]
fn detect_macos_apps() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    let bases = [PathBuf::from("/Applications"), home().join("Applications")];
    for base in bases {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("app") {
                continue;
            }
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            apps.push(AppEntry {
                id: format!("app:{}", path.to_string_lossy()),
                name,
                source: "app".into(),
                version: None,
                size_bytes: app_dir_size(&path),
                requires_root: false,
                protected: false,
            });
        }
    }
    apps
}

/// macOS app bundles live under /Applications or ~/Applications only.
#[cfg(target_os = "macos")]
fn within_macos_app_base(path: &Path) -> bool {
    let Ok(canon) = std::fs::canonicalize(path) else {
        return false;
    };
    [PathBuf::from("/Applications"), home().join("Applications")]
        .iter()
        .any(|base| {
            std::fs::canonicalize(base)
                .map(|b| canon.starts_with(&b))
                .unwrap_or(false)
        })
}

/// Ids of applications with a newer version available (best-effort, may use the
/// network). Returns the subset of `id`s that are upgradable.
#[cfg(target_os = "linux")]
pub fn updates() -> Vec<String> {
    let mut out = Vec::new();
    // apt: uses the local index (no root). Lines: "pkg/repo version arch [upgradable from: ...]"
    if let Some(list) = run("apt", &["list", "--upgradable"]) {
        for line in list.lines() {
            if let Some(pkg) = line.split('/').next() {
                if line.contains("upgradable") {
                    out.push(format!("apt:{}", pkg.trim()));
                }
            }
        }
    }
    // flatpak updates.
    if let Some(list) = run(
        "flatpak",
        &["remote-ls", "--updates", "--columns=application"],
    ) {
        for app in list.lines() {
            let app = app.trim();
            if !app.is_empty() {
                out.push(format!("flatpak:{app}"));
            }
        }
    }
    // snap refresh candidates.
    if let Some(list) = run("snap", &["refresh", "--list"]) {
        for line in list.lines().skip(1) {
            if let Some(name) = line.split_whitespace().next() {
                out.push(format!("snap:{name}"));
            }
        }
    }
    out
}

fn split_ids(ids: &[String], prefix: &str) -> Vec<String> {
    ids.iter()
        .filter_map(|id| id.strip_prefix(prefix).map(str::to_string))
        .collect()
}

/// Safe argv value: non-empty and never looks like a flag (anti argument
/// injection). Package/app ids are also cross-checked against the live
/// inventory, so this is defence in depth.
#[cfg(target_os = "linux")]
fn safe_value(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-')
}

/// Allowlisted AppImage/app-folder base directories.
#[cfg(target_os = "linux")]
fn appimage_bases() -> Vec<PathBuf> {
    let h = home();
    vec![
        h.join("Applications"),
        h.join("applications"),
        h.join(".local/bin"),
        h.join("bin"),
        h.join("Downloads"),
        h.join("Téléchargements"),
        PathBuf::from("/opt"),
    ]
}

/// True only if `path`, after canonicalisation, lives inside an allowed base.
#[cfg(target_os = "linux")]
fn within_allowed_base(path: &Path) -> bool {
    let Ok(canon) = std::fs::canonicalize(path) else {
        return false;
    };
    appimage_bases().iter().any(|base| {
        std::fs::canonicalize(base)
            .map(|b| canon.starts_with(&b))
            .unwrap_or(false)
    })
}

fn exec(report: &mut AppActionReport, label: &str, mut cmd: Command) {
    match cmd.status() {
        Ok(s) if s.success() => report.succeeded.push(label.to_string()),
        Ok(s) => report
            .errors
            .push(format!("{label}: exit {}", s.code().unwrap_or(-1))),
        Err(e) => report.errors.push(format!("{label}: {e}")),
    }
}

/// Keep only ids that exist in the current inventory. Anything the frontend
/// sends that isn't a real installed app is refused — the backend never trusts
/// caller-supplied package names or paths. When `block_protected` is set,
/// essential system components are also refused (uninstall path only).
fn validate_against_inventory(
    ids: &[String],
    report: &mut AppActionReport,
    block_protected: bool,
) -> Vec<String> {
    let inventory = list();
    let known_ids: HashSet<&str> = inventory.iter().map(|app| app.id.as_str()).collect();
    let protected_ids: HashSet<&str> = inventory
        .iter()
        .filter(|a| a.protected)
        .map(|a| a.id.as_str())
        .collect();
    let mut known = Vec::new();
    for id in ids {
        if !known_ids.contains(id.as_str()) {
            report.errors.push(format!("refused (unknown app): {id}"));
        } else if block_protected && protected_ids.contains(id.as_str()) {
            report
                .errors
                .push(format!("refused (protected system app): {id}"));
        } else {
            known.push(id.clone());
        }
    }
    known
}

/// Delete the AppImages/app folders among `ids`, but only those that resolve
/// inside an allowed base directory.
#[cfg(target_os = "linux")]
fn remove_appimages(known: &[String], report: &mut AppActionReport) {
    for path in split_ids(known, "appimage:") {
        let p = Path::new(&path);
        if !within_allowed_base(p) {
            report
                .errors
                .push(format!("refused (outside allowed dirs): {path}"));
            continue;
        }
        // Classify by the real filesystem entry, never blindly recurse.
        let result = match std::fs::symlink_metadata(p) {
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(p),
            Ok(_) => std::fs::remove_file(p),
            Err(e) => Err(e),
        };
        match result {
            Ok(_) => report.succeeded.push(format!("removed {path}")),
            Err(e) => report.errors.push(format!("{path}: {e}")),
        }
    }
}

/// macOS: `.app` bundles have no built-in update channel, so nothing is reported
/// as upgradable here.
#[cfg(target_os = "macos")]
pub fn updates() -> Vec<String> {
    Vec::new()
}

/// Batch uninstall. apt/snap go through pkexec; flatpak and AppImages don't.
#[cfg(target_os = "linux")]
pub fn uninstall(ids: &[String]) -> AppActionReport {
    let mut report = AppActionReport::default();
    let known = validate_against_inventory(ids, &mut report, true);

    let apt: Vec<String> = split_ids(&known, "apt:")
        .into_iter()
        .filter(|s| safe_value(s))
        .collect();
    if !apt.is_empty() {
        let mut cmd = Command::new("pkexec");
        cmd.args(["apt-get", "remove", "-y", "--"]).args(&apt);
        exec(&mut report, &format!("apt remove ({})", apt.len()), cmd);
    }
    for name in split_ids(&known, "snap:")
        .into_iter()
        .filter(|s| safe_value(s))
    {
        let mut cmd = Command::new("pkexec");
        cmd.args(["snap", "remove", &name]);
        exec(&mut report, &format!("snap remove {name}"), cmd);
    }
    let flatpak: Vec<String> = split_ids(&known, "flatpak:")
        .into_iter()
        .filter(|s| safe_value(s))
        .collect();
    if !flatpak.is_empty() {
        let mut cmd = Command::new("flatpak");
        cmd.args(["uninstall", "-y", "--"]).args(&flatpak);
        exec(
            &mut report,
            &format!("flatpak uninstall ({})", flatpak.len()),
            cmd,
        );
    }
    remove_appimages(&known, &mut report);
    report
}

/// macOS: move the selected `.app` bundles to the Trash (recoverable). Only
/// bundles validated against the live inventory and living under an allowed base
/// are touched.
#[cfg(target_os = "macos")]
pub fn uninstall(ids: &[String]) -> AppActionReport {
    let mut report = AppActionReport::default();
    let known = validate_against_inventory(ids, &mut report, true);
    for path in split_ids(&known, "app:") {
        let p = Path::new(&path);
        if !within_macos_app_base(p) {
            report
                .errors
                .push(format!("refused (outside /Applications): {path}"));
            continue;
        }
        let script = format!(
            "tell application \"Finder\" to delete POSIX file \"{}\"",
            path.replace('"', "")
        );
        let mut cmd = Command::new("osascript");
        cmd.args(["-e", &script]);
        exec(&mut report, &format!("trashed {path}"), cmd);
    }
    report
}

/// MSIX PackageFullName shape guard: alphanumerics, dot, dash, underscore only
/// (== `^[A-Za-z0-9._-]+$`). Rejects every shell metacharacter, so the value is
/// safe to pass to Remove-AppxPackage.
#[cfg(target_os = "windows")]
fn valid_pfn(pfn: &str) -> bool {
    !pfn.is_empty()
        && pfn
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Batch uninstall. The caller supplies only ids; the backend re-resolves the
/// action from a trusted source and NEVER runs a caller-supplied command string.
/// Registry apps: re-open the exact Uninstall subkey and run ITS OWN
/// Quiet/UninstallString (installer-authored, trusted). MSIX: Remove-AppxPackage
/// with a shape-validated PackageFullName. Protected (NonRemovable) and unknown
/// ids are refused by validate_against_inventory + the per-arm guards below.
#[cfg(target_os = "windows")]
pub fn uninstall(ids: &[String]) -> AppActionReport {
    use winreg::enums::KEY_READ;
    let mut report = AppActionReport::default();
    let known = validate_against_inventory(ids, &mut report, true);

    // --- Registry (classic Win32) apps ---
    for rest in split_ids(&known, "registry:") {
        // rest == "<HIVE>:<subkey>"; split on the FIRST ':' so subkey may contain ':'.
        let Some((label, subkey)) = rest.split_once(':') else {
            report
                .errors
                .push(format!("refused (bad id): registry:{rest}"));
            continue;
        };
        let Some((hive, base)) = registry_hive(label) else {
            report
                .errors
                .push(format!("refused (unknown hive): registry:{rest}"));
            continue;
        };
        if subkey.is_empty() {
            report
                .errors
                .push(format!("refused (empty subkey): registry:{rest}"));
            continue;
        }
        let path = format!(r"{base}\{subkey}");
        let Ok(key) = hive.open_subkey_with_flags(&path, KEY_READ) else {
            report
                .errors
                .push(format!("registry:{rest}: subkey not found"));
            continue;
        };
        // Prefer the silent command; fall back to the interactive one.
        let cmd_str = key
            .get_value::<String, _>("QuietUninstallString")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                key.get_value::<String, _>("UninstallString")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            });
        let Some(cmd_str) = cmd_str else {
            report
                .errors
                .push(format!("registry:{rest}: no uninstall command"));
            continue;
        };
        // The command string comes from the registry (written by the app's own
        // installer), never from the caller. Run it via `cmd /C` as one argument.
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", &cmd_str]);
        exec(&mut report, &format!("uninstall {label}:{subkey}"), cmd);
    }

    // --- MSIX / Store apps ---
    for pfn in split_ids(&known, "msix:") {
        if !valid_pfn(&pfn) {
            report
                .errors
                .push(format!("refused (bad package name): msix:{pfn}"));
            continue;
        }
        let script = format!("Remove-AppxPackage -Package '{pfn}'");
        let mut cmd = Command::new(POWERSHELL);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        exec(&mut report, &format!("uninstall msix:{pfn}"), cmd);
    }
    report
}

/// macOS: `.app` bundles have no in-place update mechanism, so this is a no-op.
#[cfg(target_os = "macos")]
pub fn update(_ids: &[String]) -> AppActionReport {
    AppActionReport::default()
}

/// Batch update.
#[cfg(target_os = "linux")]
pub fn update(ids: &[String]) -> AppActionReport {
    let mut report = AppActionReport::default();
    let known = validate_against_inventory(ids, &mut report, false);

    let apt: Vec<String> = split_ids(&known, "apt:")
        .into_iter()
        .filter(|s| safe_value(s))
        .collect();
    if !apt.is_empty() {
        let mut cmd = Command::new("pkexec");
        cmd.args(["apt-get", "install", "--only-upgrade", "-y", "--"])
            .args(&apt);
        exec(&mut report, &format!("apt upgrade ({})", apt.len()), cmd);
    }
    let snap: Vec<String> = split_ids(&known, "snap:")
        .into_iter()
        .filter(|s| safe_value(s))
        .collect();
    if !snap.is_empty() {
        let mut cmd = Command::new("pkexec");
        cmd.args(["snap", "refresh"]).args(&snap);
        exec(&mut report, &format!("snap refresh ({})", snap.len()), cmd);
    }
    let flatpak: Vec<String> = split_ids(&known, "flatpak:")
        .into_iter()
        .filter(|s| safe_value(s))
        .collect();
    if !flatpak.is_empty() {
        let mut cmd = Command::new("flatpak");
        cmd.args(["update", "-y", "--"]).args(&flatpak);
        exec(
            &mut report,
            &format!("flatpak update ({})", flatpak.len()),
            cmd,
        );
    }
    report
}

// ---- Windows: winget update detection + batch upgrade ----------------------

/// Heuristically parse `winget upgrade` table output → the Name column of each
/// upgradable row. Columns are padded with 2+ spaces, so the Name is everything
/// before the first double-space run (Names may contain single spaces). Rows
/// start after the dashed separator and end at the first blank line.
#[cfg(target_os = "windows")]
fn parse_winget_upgrade_names(out: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_table = false;
    for line in out.lines() {
        let line = line.trim_end();
        if !in_table {
            if line.starts_with("---") {
                in_table = true;
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        // Footer such as "N upgrades available." / pinned-package notes.
        let starts_digit = trimmed.chars().next().is_some_and(|c| c.is_ascii_digit());
        if starts_digit && trimmed.to_lowercase().contains("upgrade") {
            continue;
        }
        if let Some(name) = line.split("  ").next() {
            let name = name.trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Ids of apps winget reports as upgradable (for UI badging). Maps winget Names
/// to inventory ids case-insensitively so the id-keyed frontend badging works
/// unchanged. Unmatched winget Names are dropped.
#[cfg(target_os = "windows")]
pub fn updates() -> Vec<String> {
    let Some(out) = run(
        "winget",
        &[
            "upgrade",
            "--accept-source-agreements",
            "--disable-interactivity",
        ],
    ) else {
        return Vec::new();
    };
    let names = parse_winget_upgrade_names(&out);
    if names.is_empty() {
        return Vec::new();
    }
    let lower: HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
    list()
        .into_iter()
        .filter(|app| lower.contains(&app.name.to_lowercase()))
        .map(|app| app.id)
        .collect()
}

/// Best-effort batch update via winget. Each id is resolved to its inventory
/// AppEntry name and updated by `winget upgrade --silent --name <name>`. winget
/// keys on its own package identity, so an entry whose name does not match a
/// winget package (or matches several) cannot be updated — that is reported as an
/// explicit error, never silently skipped.
#[cfg(target_os = "windows")]
pub fn update(ids: &[String]) -> AppActionReport {
    let mut report = AppActionReport::default();
    let known = validate_against_inventory(ids, &mut report, false);
    if known.is_empty() {
        return report;
    }
    // One extra inventory scan to map ids → names (on-demand action; acceptable).
    let inventory = list();
    for id in &known {
        let Some(app) = inventory.iter().find(|a| &a.id == id) else {
            report.errors.push(format!("{id}: not found in inventory"));
            continue;
        };
        // Anti argument-injection: a name starting with '-' would read as a flag.
        if app.name.trim().is_empty() || app.name.starts_with('-') {
            report.errors.push(format!("{id}: unsafe package name"));
            continue;
        }
        let mut cmd = Command::new("winget");
        cmd.args([
            "upgrade",
            "--silent",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
            "--name",
            &app.name,
        ]);
        exec(&mut report, &format!("winget upgrade {}", app.name), cmd);
    }
    report
}
