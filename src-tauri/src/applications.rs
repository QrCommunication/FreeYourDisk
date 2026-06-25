// SPDX-License-Identifier: GPL-3.0-or-later
//! Installed-application inventory: apt (dpkg), flatpak, snap and AppImages,
//! ranked by disk space. Supports batch uninstall and batch update; checking
//! for newer versions is a separate, on-demand pass.

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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

const APT_TOP: usize = 80;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

// ---- apt / dpkg -----------------------------------------------------------

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
pub fn list() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    apps.extend(detect_apt());
    apps.extend(detect_flatpak());
    apps.extend(detect_snap());
    apps.extend(detect_appimages());
    apps.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
    apps
}

/// Ids of applications with a newer version available (best-effort, may use the
/// network). Returns the subset of `id`s that are upgradable.
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
fn safe_value(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-')
}

/// Allowlisted AppImage/app-folder base directories.
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

/// Batch uninstall. apt/snap go through pkexec; flatpak and AppImages don't.
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

/// Batch update.
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
