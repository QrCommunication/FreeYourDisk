// SPDX-License-Identifier: GPL-3.0-or-later
//! Largest files/dirs service: a read-only explorer surfacing the biggest
//! files and directories under a root. The user picks what to clean.

use crate::{path_id, Service};
use core_ipc::{ItemKind, ScanItem, ScanResult, ServiceId};
use core_scan::top_n;
use std::path::PathBuf;

/// Surfaces the `top` largest files and directories under `root`.
#[derive(Clone, Debug)]
pub struct BigFilesService {
    pub root: PathBuf,
    pub top: usize,
}

impl Service for BigFilesService {
    fn id(&self) -> ServiceId {
        ServiceId::BigFiles
    }

    fn scan(&self) -> ScanResult {
        let (files, dirs) = top_n(&self.root, self.top);

        // The "recoverable" total is the sum of the top files (dirs overlap with
        // files, so counting both would double-count).
        let total_bytes = files.iter().map(|e| e.size_bytes).sum();

        let mut items: Vec<ScanItem> = files
            .into_iter()
            .map(|e| ScanItem {
                id: path_id(&e.path),
                path: e.path,
                size_bytes: e.size_bytes,
                last_access: e.last_access,
                kind: ItemKind::File,
                requires_root: false,
            })
            .collect();

        for (path, size) in dirs {
            items.push(ScanItem {
                id: path_id(&path),
                path,
                size_bytes: size,
                last_access: None,
                kind: ItemKind::Dir,
                requires_root: false,
            });
        }

        ScanResult {
            service: ServiceId::BigFiles,
            items,
            total_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_returns_largest_files_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("small.bin"), vec![0u8; 10]).unwrap();
        std::fs::write(dir.path().join("big.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.path().join("mid.bin"), vec![0u8; 100]).unwrap();

        let svc = BigFilesService {
            root: dir.path().to_path_buf(),
            top: 2,
        };
        let result = svc.scan();

        let files: Vec<&ScanItem> = result
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::File)
            .collect();
        assert_eq!(files.len(), 2);
        assert!(files[0].path.ends_with("big.bin"));
        assert!(files[0].size_bytes >= files[1].size_bytes);
        assert!(result.items.iter().all(|i| !i.requires_root));
    }
}
