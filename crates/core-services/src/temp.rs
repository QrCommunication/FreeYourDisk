// SPDX-License-Identifier: GPL-3.0-or-later
//! Temp files service: `/tmp`, `/var/tmp` (root) and `~/.cache` (user), filtered
//! by minimum age.

use crate::{path_id, Service};
use core_ipc::{ItemKind, ScanItem, ScanResult, ServiceId};
use core_scan::{scan_dir, ScanOpts};
use std::path::{Path, PathBuf};

/// A temp root to scan, and whether removing files there needs root.
#[derive(Clone, Debug)]
pub struct TempRoot {
    pub path: PathBuf,
    pub requires_root: bool,
}

/// Scans temporary directories for files older than `min_age_days`.
#[derive(Clone, Debug)]
pub struct TempService {
    pub roots: Vec<TempRoot>,
    pub min_age_days: u32,
}

impl TempService {
    /// Default roots for a given home directory.
    pub fn with_defaults(home: &Path, min_age_days: u32) -> Self {
        Self {
            roots: vec![
                TempRoot {
                    path: PathBuf::from("/tmp"),
                    requires_root: true,
                },
                TempRoot {
                    path: PathBuf::from("/var/tmp"),
                    requires_root: true,
                },
                TempRoot {
                    path: home.join(".cache"),
                    requires_root: false,
                },
            ],
            min_age_days,
        }
    }
}

impl Service for TempService {
    fn id(&self) -> ServiceId {
        ServiceId::Temp
    }

    fn scan(&self) -> ScanResult {
        let opts = ScanOpts {
            follow_symlinks: false,
            min_age_days: Some(self.min_age_days),
        };
        let mut items = Vec::new();

        for root in &self.roots {
            if !root.path.exists() {
                continue;
            }
            for entry in scan_dir(&root.path, &opts) {
                if entry.is_dir {
                    continue; // temp cleanup targets files only
                }
                items.push(ScanItem {
                    id: path_id(&entry.path),
                    path: entry.path,
                    size_bytes: entry.size_bytes,
                    last_access: entry.last_access,
                    kind: ItemKind::File,
                    requires_root: root.requires_root,
                });
            }
        }

        let total_bytes = items.iter().map(|item| item.size_bytes).sum();
        ScanResult {
            service: ServiceId::Temp,
            items,
            total_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn backdate(path: &Path, days: u64) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
            .unwrap();
    }

    #[test]
    fn scan_filters_by_age_and_marks_user_items() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join(".cache");
        std::fs::create_dir(&cache).unwrap();

        let old = cache.join("old.log");
        std::fs::write(&old, vec![0u8; 100]).unwrap();
        backdate(&old, 30);
        std::fs::write(cache.join("recent.log"), vec![0u8; 50]).unwrap();

        let svc = TempService {
            roots: vec![TempRoot {
                path: cache.clone(),
                requires_root: false,
            }],
            min_age_days: 7,
        };
        let result = svc.scan();

        let names: Vec<String> = result
            .items
            .iter()
            .map(|i| i.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"old.log".to_string()), "old file kept");
        assert!(
            !names.contains(&"recent.log".to_string()),
            "recent file filtered out"
        );
        assert!(result.items.iter().all(|i| !i.requires_root));
    }

    #[test]
    fn preview_reflects_selection_totals() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join(".cache");
        std::fs::create_dir(&cache).unwrap();
        let f = cache.join("a.log");
        std::fs::write(&f, vec![0u8; 42]).unwrap();
        backdate(&f, 30);

        let svc = TempService {
            roots: vec![TempRoot {
                path: cache,
                requires_root: false,
            }],
            min_age_days: 1,
        };
        let result = svc.scan();
        let ids: Vec<String> = result.items.iter().map(|i| i.id.clone()).collect();

        let plan = svc.preview(&ids);
        assert_eq!(plan.total_bytes, 42);
        assert!(!plan.requires_root);
        assert_eq!(plan.items.len(), 1);
    }
}
