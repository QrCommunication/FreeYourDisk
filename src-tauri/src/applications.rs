// SPDX-License-Identifier: GPL-3.0-or-later
//! Installed-application inventory: apt (dpkg), flatpak, snap and AppImages
//! on Linux; `.app` bundles on macOS; registry entries and MSIX packages on
//! Windows. Ranked by disk space. Supports batch uninstall and batch update;
//! checking for newer versions is a separate, on-demand pass.

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
    /// "apt" | "flatpak" | "snap" | "appimage" | "app" (macOS) | "registry" | "msix" (Windows)
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

/// One result from the on-demand application update check.  `id` is an
/// action identifier owned by the package manager, not a display name: callers
/// must send it back unchanged to `update`.
///
/// Entries with `can_update == false` are deliberately included when we can
/// identify an application that has no safe automatic channel (for example an
/// AppImage or a manually copied macOS `.app`).  This keeps the UI honest
/// instead of silently making those applications look unsupported.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct AppUpdate {
    pub id: String,
    pub name: String,
    /// Package manager or delivery channel: apt, flatpak, snap, winget, brew,
    /// appimage, or manual-app.
    pub provider: String,
    pub current_version: Option<String>,
    pub available_version: Option<String>,
    pub can_update: bool,
    /// Human-readable explanation when no automatic update is safe.
    pub reason: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AppUpdatesReport {
    pub entries: Vec<AppUpdate>,
}

fn available_update(
    id: impl Into<String>,
    name: impl Into<String>,
    provider: impl Into<String>,
    current_version: Option<String>,
    available_version: Option<String>,
) -> AppUpdate {
    AppUpdate {
        id: id.into(),
        name: name.into(),
        provider: provider.into(),
        current_version,
        available_version,
        can_update: true,
        reason: None,
    }
}

#[cfg(any(target_os = "macos", test))]
fn non_automatic_update(
    id: impl Into<String>,
    name: impl Into<String>,
    provider: impl Into<String>,
    current_version: Option<String>,
    reason: impl Into<String>,
) -> AppUpdate {
    AppUpdate {
        id: id.into(),
        name: name.into(),
        provider: provider.into(),
        current_version,
        available_version: None,
        can_update: false,
        reason: Some(reason.into()),
    }
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

/// Build the lightweight inventory entry for an unpacked application folder.
/// Folder byte totals are deliberately not computed while this inventory opens.
#[cfg(target_os = "linux")]
fn app_folder_entry(path: &Path, name: String, requires_root: bool) -> AppEntry {
    AppEntry {
        id: format!("appimage:{}", path.to_string_lossy()),
        name,
        source: "appimage".into(),
        version: None,
        size_bytes: 0,
        requires_root,
        protected: false,
    }
}

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
                // Keep application folders visible, but never recursively walk
                // them while opening the Applications tab.  A recursive size
                // calculation can traverse a whole mounted tree below /opt or
                // ~/Applications and make the UI look frozen.  `0` explicitly
                // means that the folder size is not calculated in this view.
                apps.push(app_folder_entry(&path, name, !path.starts_with(&h)));
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
        // Best-effort: C:\Program Files\WindowsApps is ACL-restricted, so
        // cached_dir_total usually cannot enumerate a Store app without
        // elevation and returns 0 (the entry still lists; it just sorts low).
        // There is no reliable non-admin API for MSIX install size.
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

/// Applications with a newer version available (best-effort, may use the
/// network).  This must remain independent from `list()`: checking for package
/// updates happens when the Applications panel opens, while `list()` can walk
/// large user directories to discover AppImages and unpacked applications.
/// AppImages deliberately do not appear here because they have no standard,
/// safe update protocol.
#[cfg(target_os = "linux")]
fn parse_apt_updates(output: &str) -> Vec<AppUpdate> {
    output
        .lines()
        .filter_map(|line| {
            let (package_and_source, rest) = line.split_once(char::is_whitespace)?;
            let package = package_and_source.split('/').next()?.trim();
            let available_version = rest.split_whitespace().next()?.trim();
            let current_version = rest
                .split_once("[upgradable from:")?
                .1
                .strip_suffix(']')?
                .trim();
            if !safe_value(package) || available_version.is_empty() || current_version.is_empty() {
                return None;
            }
            Some(available_update(
                format!("apt:{package}"),
                package,
                "apt",
                Some(current_version.to_owned()),
                Some(available_version.to_owned()),
            ))
        })
        .collect()
}

/// Flatpak exposes the target version from the remote.  It does not include
/// the locally installed version in `remote-ls --updates`, so that field stays
/// empty instead of forcing a separate inventory query.
#[cfg(target_os = "linux")]
fn parse_flatpak_updates(output: &str) -> Vec<AppUpdate> {
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let application = fields.first()?.trim();
            let name = fields.get(1).map(|value| value.trim()).unwrap_or_default();
            let available_version = fields.get(2).map(|value| value.trim()).unwrap_or_default();
            if !safe_value(application) {
                return None;
            }
            Some(available_update(
                format!("flatpak:{application}"),
                if name.is_empty() { application } else { name },
                "flatpak",
                None,
                (!available_version.is_empty()).then(|| available_version.to_owned()),
            ))
        })
        .collect()
}

/// `snap refresh --list` reports the version that can be installed, but not
/// the currently installed version.  A strict package-name guard also avoids
/// treating localised prose such as "all snaps are up to date" as a package.
#[cfg(target_os = "linux")]
fn parse_snap_updates(output: &str) -> Vec<AppUpdate> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let available_version = fields.next()?;
            let valid_name = !name.is_empty()
                && !name.starts_with('-')
                && !name.ends_with('-')
                && name.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
                });
            if !valid_name || available_version.is_empty() {
                return None;
            }
            Some(available_update(
                format!("snap:{name}"),
                name,
                "snap",
                None,
                Some(available_version.to_owned()),
            ))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_updates_report(
    apt_output: Option<&str>,
    flatpak_output: Option<&str>,
    snap_output: Option<&str>,
) -> AppUpdatesReport {
    let mut entries = Vec::new();
    if let Some(output) = apt_output {
        entries.extend(parse_apt_updates(output));
    }
    if let Some(output) = flatpak_output {
        entries.extend(parse_flatpak_updates(output));
    }
    if let Some(output) = snap_output {
        entries.extend(parse_snap_updates(output));
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    AppUpdatesReport { entries }
}

#[cfg(target_os = "linux")]
fn linux_update_candidates() -> AppUpdatesReport {
    // Each query has bounded package-manager output.  Do not call `list()`
    // here: it discovers AppImages by scanning user directories and made the
    // Applications tab appear to freeze on machines with large Downloads/opt.
    let apt_output = run("apt", &["list", "--upgradable"]);
    let flatpak_output = run(
        "flatpak",
        &[
            "remote-ls",
            "--updates",
            "--app",
            "--columns=application,name,version",
        ],
    );
    let snap_output = run("snap", &["refresh", "--list"]);
    linux_updates_report(
        apt_output.as_deref(),
        flatpak_output.as_deref(),
        snap_output.as_deref(),
    )
}

#[cfg(target_os = "linux")]
pub fn updates() -> AppUpdatesReport {
    linux_update_candidates()
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

/// Homebrew is intentionally resolved only through its documented default
/// installation locations.  Searching an arbitrary PATH would allow a shell
/// profile or another process to redirect a privileged update action.
#[cfg(target_os = "macos")]
fn brew_path() -> Option<&'static str> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
}

#[cfg(target_os = "macos")]
fn brew_version(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("installed_versions")
                .and_then(serde_json::Value::as_array)
                .and_then(|versions| versions.first())
                .and_then(serde_json::Value::as_str)
                .filter(|version| !version.is_empty())
                .map(str::to_owned)
        })
}

/// Only names understood by Homebrew's command line are accepted.  The value
/// is still re-derived from `brew outdated` at update time, so an arbitrary UI
/// id cannot select an unrelated formula or cask.
#[cfg(target_os = "macos")]
fn safe_brew_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '+' | '.' | '_' | '-' | '/'))
}

/// Homebrew's JSON report is the source of truth for formula and cask update
/// ids.  Manually copied `.app` bundles remain visible as non-automatic
/// entries, because replacing them safely is impossible without their own
/// updater.
#[cfg(target_os = "macos")]
pub fn updates() -> AppUpdatesReport {
    let inventory = list();
    let mut entries: Vec<AppUpdate> = inventory
        .iter()
        .map(|app| {
            non_automatic_update(
                app.id.clone(),
                app.name.clone(),
                "manual-app",
                app.version.clone(),
                "This .app bundle was not discovered through a supported package manager.",
            )
        })
        .collect();

    let Some(brew) = brew_path() else {
        entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        return AppUpdatesReport { entries };
    };
    let Some(out) = run(brew, &["outdated", "--json=v2"]) else {
        entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        return AppUpdatesReport { entries };
    };
    let Ok(report) = serde_json::from_str::<serde_json::Value>(&out) else {
        entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        return AppUpdatesReport { entries };
    };

    for (kind, field) in [("formula", "formulae"), ("cask", "casks")] {
        let Some(packages) = report.get(field).and_then(serde_json::Value::as_array) else {
            continue;
        };
        for package in packages {
            let Some(name) = package
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| safe_brew_name(name))
            else {
                continue;
            };
            let current_version = brew_version(package, "installed_version");
            let available_version = brew_version(package, "current_version");
            entries.push(available_update(
                format!("brew:{kind}:{name}"),
                name,
                format!("brew-{kind}"),
                current_version,
                available_version,
            ));
        }
    }
    entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    AppUpdatesReport { entries }
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

/// Update Homebrew formulae and casks only when their exact id is still present
/// in a fresh `brew outdated --json=v2` report.  Manual `.app` bundles are
/// rejected with an explicit explanation rather than pretending to update.
#[cfg(target_os = "macos")]
pub fn update(ids: &[String]) -> AppActionReport {
    let mut action_report = AppActionReport::default();
    let available: HashSet<String> = updates()
        .entries
        .into_iter()
        .filter(|entry| entry.can_update)
        .map(|entry| entry.id)
        .collect();
    let Some(brew) = brew_path() else {
        action_report
            .errors
            .push("Homebrew is not installed in a supported location.".into());
        return action_report;
    };

    for id in ids {
        if id.starts_with("app:") {
            action_report.errors.push(format!(
                "{id}: manually installed .app bundles must be updated by their publisher."
            ));
            continue;
        }
        if !available.contains(id) {
            action_report
                .errors
                .push(format!("refused (not currently upgradable): {id}"));
            continue;
        }
        let Some((kind, name)) = id
            .strip_prefix("brew:")
            .and_then(|value| value.split_once(':'))
        else {
            action_report.errors.push(format!("refused (bad id): {id}"));
            continue;
        };
        if !matches!(kind, "formula" | "cask") || !safe_brew_name(name) {
            action_report.errors.push(format!("refused (bad id): {id}"));
            continue;
        }
        let flag = if kind == "formula" {
            "--formula"
        } else {
            "--cask"
        };
        let mut cmd = Command::new(brew);
        cmd.args(["upgrade", flag, "--", name]);
        exec(
            &mut action_report,
            &format!("brew upgrade {kind} {name}"),
            cmd,
        );
    }
    action_report
}

/// Batch update.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct LinuxUpdateTargets {
    apt: Vec<String>,
    snap: Vec<String>,
    flatpak: Vec<String>,
}

/// Select update targets from an already-fresh package-manager candidate set.
/// This deliberately does not consult `list()`: presentation inventory is
/// capped and may inspect AppImage roots, neither of which is appropriate for
/// authorising a package-manager update action.
#[cfg(target_os = "linux")]
fn select_linux_update_targets(
    ids: &[String],
    available: &HashSet<String>,
    report: &mut AppActionReport,
) -> LinuxUpdateTargets {
    let mut targets = LinuxUpdateTargets::default();
    let mut seen = HashSet::new();

    for id in ids {
        if id.starts_with("appimage:") {
            report.errors.push(format!(
                "{id}: AppImages and unpacked application folders do not have a standard safe automatic update channel"
            ));
            continue;
        }
        let Some((provider, value)) = id.split_once(':') else {
            report.errors.push(format!("refused (bad update id): {id}"));
            continue;
        };
        if !matches!(provider, "apt" | "snap" | "flatpak") || !safe_value(value) {
            report.errors.push(format!("refused (bad update id): {id}"));
            continue;
        }
        if !available.contains(id) {
            report
                .errors
                .push(format!("refused (not currently upgradable): {id}"));
            continue;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        match provider {
            "apt" => targets.apt.push(value.to_owned()),
            "snap" => targets.snap.push(value.to_owned()),
            "flatpak" => targets.flatpak.push(value.to_owned()),
            _ => unreachable!("provider was allowlisted above"),
        }
    }

    targets
}

#[cfg(target_os = "linux")]
pub fn update(ids: &[String]) -> AppActionReport {
    let mut report = AppActionReport::default();
    // Re-check against package managers' current candidates rather than the
    // inventory.  APT's `--only-upgrade` is a second guard: it cannot install
    // a package that is not already installed if the state changes after this
    // check.
    let available: HashSet<String> = linux_update_candidates()
        .entries
        .into_iter()
        .filter(|entry| entry.can_update)
        .map(|entry| entry.id)
        .collect();
    let targets = select_linux_update_targets(ids, &available, &mut report);

    if !targets.apt.is_empty() {
        let mut cmd = Command::new("pkexec");
        cmd.args(["apt-get", "install", "--only-upgrade", "-y", "--"])
            .args(&targets.apt);
        exec(
            &mut report,
            &format!("apt upgrade ({})", targets.apt.len()),
            cmd,
        );
    }
    if !targets.snap.is_empty() {
        let mut cmd = Command::new("pkexec");
        cmd.args(["snap", "refresh"]).args(&targets.snap);
        exec(
            &mut report,
            &format!("snap refresh ({})", targets.snap.len()),
            cmd,
        );
    }
    if !targets.flatpak.is_empty() {
        let mut cmd = Command::new("flatpak");
        cmd.args(["update", "-y", "--"]).args(&targets.flatpak);
        exec(
            &mut report,
            &format!("flatpak update ({})", targets.flatpak.len()),
            cmd,
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_records_keep_the_action_id_and_capability_explicit() {
        let automatic = available_update(
            "apt:freeyourdisk",
            "FreeYourDisk",
            "apt",
            Some("0.6.4".into()),
            Some("0.6.5".into()),
        );
        assert!(automatic.can_update);
        assert_eq!(automatic.id, "apt:freeyourdisk");
        assert_eq!(automatic.available_version.as_deref(), Some("0.6.5"));
        assert_eq!(automatic.reason, None);

        let manual = non_automatic_update(
            "appimage:/home/rony/Applications/FreeYourDisk.AppImage",
            "FreeYourDisk",
            "appimage",
            None,
            "No standard update channel.",
        );
        assert!(!manual.can_update);
        assert_eq!(manual.provider, "appimage");
        assert_eq!(
            manual.reason.as_deref(),
            Some("No standard update channel.")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_package_values_cannot_be_interpreted_as_flags() {
        assert!(safe_value("org.example.App"));
        assert!(!safe_value(""));
        assert!(!safe_value("--assume-yes"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn apt_update_output_supplies_versions_without_an_inventory_scan() {
        let updates = parse_apt_updates(
            "Listing... Done\n\
             freeyourdisk/stable 0.6.5 amd64 [upgradable from: 0.6.4]\n\
             malformed package line\n",
        );

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].id, "apt:freeyourdisk");
        assert_eq!(updates[0].name, "freeyourdisk");
        assert_eq!(updates[0].current_version.as_deref(), Some("0.6.4"));
        assert_eq!(updates[0].available_version.as_deref(), Some("0.6.5"));
        assert!(updates[0].can_update);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_update_report_never_invents_appimage_candidates() {
        let report = linux_updates_report(
            Some("freeyourdisk/stable 0.6.5 amd64 [upgradable from: 0.6.4]\n"),
            Some("io.github.FreeYourDisk\tFreeYourDisk\t0.6.5\n"),
            Some("Name Version Rev Size Publisher Notes\nfreeyourdisk 0.6.5 12 50MB QR -\n"),
        );

        assert_eq!(report.entries.len(), 3);
        assert!(report
            .entries
            .iter()
            .all(|entry| entry.provider != "appimage" && !entry.id.starts_with("appimage:")));
        assert!(report.entries.iter().all(|entry| entry.can_update));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn snap_status_message_is_not_parsed_as_a_candidate() {
        let updates = parse_snap_updates("Tous les paquets Snaps sont à jour.\n");
        assert!(updates.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn application_folder_entries_do_not_calculate_recursive_sizes() {
        let entry = app_folder_entry(
            Path::new("/opt/ExampleApplication"),
            "ExampleApplication".into(),
            true,
        );

        assert_eq!(entry.id, "appimage:/opt/ExampleApplication");
        assert_eq!(entry.size_bytes, 0);
        assert!(entry.requires_root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn apt_candidate_is_authorised_without_being_in_the_display_inventory() {
        let id = "apt:package-after-display-cap".to_owned();
        let available = HashSet::from([id.clone()]);
        let mut report = AppActionReport::default();

        let targets = select_linux_update_targets(&[id], &available, &mut report);

        assert_eq!(targets.apt, vec!["package-after-display-cap"]);
        assert!(targets.snap.is_empty());
        assert!(targets.flatpak.is_empty());
        assert!(report.errors.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn homebrew_names_reject_flags_and_shell_syntax() {
        assert!(safe_brew_name("homebrew/cask/firefox@esr"));
        assert!(!safe_brew_name("--cask"));
        assert!(!safe_brew_name("firefox;rm"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn winget_ids_reject_flags_and_shell_syntax() {
        assert!(valid_winget_id("Microsoft.PowerToys"));
        assert!(!valid_winget_id("--id"));
        assert!(!valid_winget_id("Microsoft.PowerToys;cmd"));
    }
}

// ---- Windows: winget update detection + batch upgrade ----------------------

/// A current winget upgrade candidate.  Only `PackageIdentifier` is used for
/// the mutation; display names are intentionally never used as selectors.
#[cfg(target_os = "windows")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct WingetUpgrade {
    package_id: String,
    name: String,
    current_version: Option<String>,
    available_version: Option<String>,
}

#[cfg(target_os = "windows")]
fn json_field(
    value: &serde_json::Map<String, serde_json::Value>,
    names: &[&str],
) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(serde_json::Value::as_str)
            .filter(|field| !field.trim().is_empty())
            .map(str::to_owned)
    })
}

/// Walk the documented JSON output instead of parsing a locale-dependent table.
/// Different winget releases wrap package rows under source objects, hence the
/// recursive traversal.  A row is accepted only with an exact package id and an
/// advertised target version.
#[cfg(target_os = "windows")]
fn collect_winget_upgrades(value: &serde_json::Value, out: &mut Vec<WingetUpgrade>) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                collect_winget_upgrades(value, out);
            }
        }
        serde_json::Value::Object(object) => {
            let package_id = json_field(object, &["PackageIdentifier", "PackageId", "Id"]);
            let available_version = json_field(object, &["AvailableVersion", "Available"]);
            if let (Some(package_id), Some(available_version)) = (package_id, available_version) {
                if valid_winget_id(&package_id) {
                    let name = json_field(object, &["PackageName", "Name"])
                        .unwrap_or_else(|| package_id.clone());
                    out.push(WingetUpgrade {
                        package_id,
                        name,
                        current_version: json_field(object, &["InstalledVersion", "Version"]),
                        available_version: Some(available_version),
                    });
                }
            }
            for child in object.values() {
                collect_winget_upgrades(child, out);
            }
        }
        _ => {}
    }
}

/// Winget package identifiers are command arguments, never shell fragments.
#[cfg(target_os = "windows")]
fn valid_winget_id(package_id: &str) -> bool {
    !package_id.is_empty()
        && !package_id.starts_with('-')
        && package_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

#[cfg(target_os = "windows")]
fn winget_upgrades() -> Vec<WingetUpgrade> {
    let Some(out) = run(
        "winget",
        &[
            "upgrade",
            "--accept-source-agreements",
            "--disable-interactivity",
            "--output",
            "json",
        ],
    ) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&out) else {
        return Vec::new();
    };
    let mut upgrades = Vec::new();
    collect_winget_upgrades(&json, &mut upgrades);
    upgrades.sort_by(|left, right| left.package_id.cmp(&right.package_id));
    upgrades.dedup_by(|left, right| left.package_id == right.package_id);
    upgrades
}

/// Exact winget ids, versions, and names from the current package-manager
/// response.  This deliberately does not guess a link to a registry/MSIX
/// record: display-name matching can target the wrong application.
#[cfg(target_os = "windows")]
pub fn updates() -> AppUpdatesReport {
    AppUpdatesReport {
        entries: winget_upgrades()
            .into_iter()
            .map(|upgrade| {
                available_update(
                    format!("winget:{}", upgrade.package_id),
                    upgrade.name,
                    "winget",
                    upgrade.current_version,
                    upgrade.available_version,
                )
            })
            .collect(),
    }
}

/// Batch update via winget using an exact package identifier freshly obtained
/// from winget.  Registry and MSIX inventory ids cannot be used here because
/// neither is a reliable winget identity.
#[cfg(target_os = "windows")]
pub fn update(ids: &[String]) -> AppActionReport {
    let mut report = AppActionReport::default();
    let available: HashSet<String> = winget_upgrades()
        .into_iter()
        .map(|upgrade| upgrade.package_id)
        .collect();
    for id in ids {
        let Some(package_id) = id.strip_prefix("winget:") else {
            report.errors.push(format!(
                "{id}: this application has no verified winget update identifier"
            ));
            continue;
        };
        if !valid_winget_id(package_id) || !available.contains(package_id) {
            report
                .errors
                .push(format!("refused (not currently upgradable): {id}"));
            continue;
        }
        let mut cmd = Command::new("winget");
        cmd.args([
            "upgrade",
            "--silent",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
            "--id",
            package_id,
            "--exact",
        ]);
        exec(&mut report, &format!("winget upgrade {package_id}"), cmd);
    }
    report
}
