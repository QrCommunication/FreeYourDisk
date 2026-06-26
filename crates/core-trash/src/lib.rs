// SPDX-License-Identifier: GPL-3.0-or-later
//! XDG trash + opt-in permanent deletion, guarded by a zone whitelist.
//!
//! Invariant 4 (project-wide): every deletion path is validated against an
//! allowed-zone whitelist. Out-of-zone paths and symlinks whose target escapes
//! the zones are rejected before any filesystem action.

use core_ipc::{ExecutionReport, ItemError};
use std::fs;
use std::path::{Path, PathBuf};

/// Allowed deletion zones (absolute path prefixes). A path is in-zone if its
/// real location starts with one of these prefixes.
#[derive(Clone, Debug)]
pub struct Zones(pub Vec<PathBuf>);

impl Zones {
    fn contains(&self, path: &Path) -> bool {
        self.0.iter().any(|zone| path.starts_with(zone))
    }
}

/// Why a path was refused.
#[derive(Debug, thiserror::Error)]
pub enum TrashError {
    #[error("path is outside the allowed zones: {0}")]
    OutsideZone(PathBuf),
    #[error("symlink escapes the allowed zones: {0}")]
    SymlinkEscape(PathBuf),
    #[error("io error: {0}")]
    Io(String),
}

/// Validate that `path` may be deleted given `zones`.
///
/// Strategy: resolve the *parent* directory (following any symlinks in the
/// ancestry) and keep the final component — this gives the lexical location used
/// for zone membership. Then, if the final component is itself a symlink, ensure
/// its fully-resolved target also stays within the zones (anti TOCTOU/escape).
pub fn validate(path: &Path, zones: &Zones) -> Result<PathBuf, TrashError> {
    let lexical = match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let parent_real =
                dunce::canonicalize(parent).map_err(|e| TrashError::Io(e.to_string()))?;
            parent_real.join(name)
        }
        // path is "/" or ends with "..": resolve it whole.
        _ => dunce::canonicalize(path).map_err(|e| TrashError::Io(e.to_string()))?,
    };

    if !zones.contains(&lexical) {
        return Err(TrashError::OutsideZone(path.to_path_buf()));
    }

    let is_symlink = fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if is_symlink {
        let real = dunce::canonicalize(path).map_err(|e| TrashError::Io(e.to_string()))?;
        if !zones.contains(&real) {
            return Err(TrashError::SymlinkEscape(path.to_path_buf()));
        }
    }

    Ok(lexical)
}

/// Recursive on-disk size of a path (files: length; dirs: sum of children).
fn path_size(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        Ok(m) if m.is_dir() => fs::read_dir(path)
            .map(|rd| rd.flatten().map(|e| path_size(&e.path())).sum())
            .unwrap_or(0),
        _ => 0,
    }
}

fn remove(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Move every validated path to the XDG trash (recoverable). Invalid paths are
/// recorded as errors and never touched.
pub fn to_trash(paths: &[PathBuf], zones: &Zones) -> ExecutionReport {
    execute(paths, zones, |real| {
        trash::delete(real).map_err(|e| e.to_string())
    })
}

/// Permanently delete every validated path (irreversible — opt-in only).
pub fn delete_permanent(paths: &[PathBuf], zones: &Zones) -> ExecutionReport {
    execute(paths, zones, |real| remove(real).map_err(|e| e.to_string()))
}

/// Validate every path in `plan` against `zones` (all-or-nothing) and, if none
/// escape, execute the deletion. `Ok` = executed report; `Err` = refusal report
/// (nothing deleted). Shared by the privileged helper (Linux/macOS) and the
/// Windows elevated executor so the allowlist has a single source of truth.
pub fn execute_root_plan(
    plan: &core_ipc::DeletionPlan,
    zones: &Zones,
) -> Result<core_ipc::ExecutionReport, core_ipc::ExecutionReport> {
    let paths: Vec<PathBuf> = plan.items.iter().map(|item| item.path.clone()).collect();

    let refusals: Vec<core_ipc::ItemError> = paths
        .iter()
        .filter_map(|path| match validate(path, zones) {
            Ok(_) => None,
            Err(err) => Some(core_ipc::ItemError {
                path: path.clone(),
                message: err.to_string(),
            }),
        })
        .collect();

    if !refusals.is_empty() {
        return Err(core_ipc::ExecutionReport {
            freed_bytes: 0,
            deleted_count: 0,
            errors: refusals,
        });
    }

    let report = match plan.destination {
        core_ipc::Destination::Trash => to_trash(&paths, zones),
        core_ipc::Destination::Permanent => delete_permanent(&paths, zones),
    };
    Ok(report)
}

fn execute(
    paths: &[PathBuf],
    zones: &Zones,
    mut action: impl FnMut(&Path) -> Result<(), String>,
) -> ExecutionReport {
    let mut report = ExecutionReport::default();
    for original in paths {
        match validate(original, zones) {
            Ok(real) => {
                let size = path_size(&real);
                match action(&real) {
                    Ok(()) => {
                        report.deleted_count += 1;
                        report.freed_bytes += size;
                    }
                    Err(message) => {
                        report.errors.push(ItemError {
                            path: original.clone(),
                            message,
                        });
                    }
                }
            }
            Err(err) => {
                report.errors.push(ItemError {
                    path: original.clone(),
                    message: err.to_string(),
                });
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_outside_zone() {
        // Cross-platform: a real file in a different tempdir is out of zone.
        let zone_dir = tempfile::tempdir().unwrap();
        let outside_dir = tempfile::tempdir().unwrap();
        let outside = outside_dir.path().join("victim.txt");
        std::fs::write(&outside, b"x").unwrap();
        let zones = Zones(vec![zone_dir.path().to_path_buf()]);
        assert!(matches!(
            validate(&outside, &zones),
            Err(TrashError::OutsideZone(_))
        ));
    }

    #[cfg(unix)] // uses std::os::unix::fs::symlink
    #[test]
    fn validate_rejects_symlink_escaping_zone() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("evil");
        std::os::unix::fs::symlink("/etc", &link).unwrap();
        let zones = Zones(vec![dir.path().to_path_buf()]);
        assert!(matches!(
            validate(&link, &zones),
            Err(TrashError::SymlinkEscape(_))
        ));
    }

    #[test]
    fn validate_accepts_in_zone_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        let zones = Zones(vec![dir.path().to_path_buf()]);
        assert!(validate(&f, &zones).is_ok());
    }

    #[test]
    fn delete_permanent_removes_in_zone_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, vec![0u8; 7]).unwrap();
        let zones = Zones(vec![dir.path().to_path_buf()]);

        let report = delete_permanent(std::slice::from_ref(&f), &zones);
        assert_eq!(report.deleted_count, 1);
        assert_eq!(report.freed_bytes, 7);
        assert!(report.errors.is_empty());
        assert!(!f.exists());
    }

    #[test]
    fn delete_permanent_refuses_out_of_zone() {
        let zone_dir = tempfile::tempdir().unwrap();
        let other_dir = tempfile::tempdir().unwrap();
        let victim = other_dir.path().join("keep.txt");
        std::fs::write(&victim, b"important").unwrap();
        let zones = Zones(vec![zone_dir.path().to_path_buf()]);

        let report = delete_permanent(std::slice::from_ref(&victim), &zones);
        assert_eq!(report.deleted_count, 0);
        assert_eq!(report.errors.len(), 1);
        assert!(victim.exists(), "out-of-zone file must never be deleted");
    }

    #[test]
    fn to_trash_refuses_out_of_zone_without_touching_it() {
        let zone_dir = tempfile::tempdir().unwrap();
        let other_dir = tempfile::tempdir().unwrap();
        let victim = other_dir.path().join("keep.txt");
        std::fs::write(&victim, b"important").unwrap();
        let zones = Zones(vec![zone_dir.path().to_path_buf()]);

        let report = to_trash(std::slice::from_ref(&victim), &zones);
        assert_eq!(report.deleted_count, 0);
        assert_eq!(report.errors.len(), 1);
        assert!(victim.exists());
    }

    #[test]
    fn execute_root_plan_refuses_whole_batch_on_escape() {
        let zone = tempfile::tempdir().unwrap();
        let inside = zone.path().join("junk.tmp");
        std::fs::write(&inside, b"x").unwrap();
        let outside = tempfile::tempdir().unwrap();
        let escape = outside.path().join("keep.txt");
        std::fs::write(&escape, b"important").unwrap();

        let zones = Zones(vec![zone.path().to_path_buf()]);
        let plan = core_ipc::DeletionPlan {
            items: vec![
                core_ipc::ScanItem {
                    id: inside.to_string_lossy().into_owned(),
                    path: inside.clone(),
                    size_bytes: 1,
                    last_access: None,
                    kind: core_ipc::ItemKind::File,
                    requires_root: true,
                },
                core_ipc::ScanItem {
                    id: escape.to_string_lossy().into_owned(),
                    path: escape.clone(),
                    size_bytes: 9,
                    last_access: None,
                    kind: core_ipc::ItemKind::File,
                    requires_root: true,
                },
            ],
            destination: core_ipc::Destination::Permanent,
            total_bytes: 10,
            requires_root: true,
        };

        let result = execute_root_plan(&plan, &zones);
        assert!(result.is_err(), "any escape must refuse the whole batch");
        assert!(inside.exists(), "nothing deleted on refusal");
        assert!(
            escape.exists(),
            "the out-of-zone file must never be touched"
        );
    }

    #[test]
    fn execute_root_plan_deletes_when_all_in_zone() {
        let zone = tempfile::tempdir().unwrap();
        let f = zone.path().join("junk.tmp");
        std::fs::write(&f, vec![0u8; 50]).unwrap();
        let zones = Zones(vec![zone.path().to_path_buf()]);
        let plan = core_ipc::DeletionPlan {
            items: vec![core_ipc::ScanItem {
                id: f.to_string_lossy().into_owned(),
                path: f.clone(),
                size_bytes: 50,
                last_access: None,
                kind: core_ipc::ItemKind::File,
                requires_root: true,
            }],
            destination: core_ipc::Destination::Permanent,
            total_bytes: 50,
            requires_root: true,
        };
        let result = execute_root_plan(&plan, &zones);
        assert!(result.is_ok());
        assert!(!f.exists(), "in-zone file deleted");
    }
}
