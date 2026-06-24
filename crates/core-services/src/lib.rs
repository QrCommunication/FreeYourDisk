// SPDX-License-Identifier: GPL-3.0-or-later
//! The four cleanup services (temp files, largest files, git repos, dev caches).
//!
//! Each implements the [`Service`] trait: a read-only `scan()` and a dry-run
//! `preview()` (default impl: filter the scan by the selected ids). Execution is
//! centralised in the Tauri backend; services never delete anything.

use core_ipc::{DeletionPlan, Destination, ScanItem, ScanResult, ServiceId};
use std::path::Path;

pub mod big_files;
pub mod dev_cache;
pub mod git_repos;
pub mod temp;

pub use big_files::BigFilesService;
pub use dev_cache::DevCacheService;
pub use git_repos::GitService;
pub use temp::{TempRoot, TempService};

/// A cleanup service: read-only discovery plus a dry-run plan builder.
pub trait Service {
    fn id(&self) -> ServiceId;

    /// Read-only discovery of cleanup candidates.
    fn scan(&self) -> ScanResult;

    /// Dry-run: build a deletion plan from the selected item ids. The default
    /// re-scans and keeps only the selected items — services that need extra
    /// safety (e.g. git) already exclude unsafe items from `scan()`.
    fn preview(&self, selection: &[String]) -> DeletionPlan {
        let items: Vec<ScanItem> = self
            .scan()
            .items
            .into_iter()
            .filter(|item| selection.contains(&item.id))
            .collect();
        plan_from_items(items)
    }
}

/// Build a Trash-by-default plan from selected items. The UI may switch the
/// destination to `Permanent` (opt-in) before execution.
pub fn plan_from_items(items: Vec<ScanItem>) -> DeletionPlan {
    let total_bytes = items.iter().map(|item| item.size_bytes).sum();
    let requires_root = items.iter().any(|item| item.requires_root);
    DeletionPlan {
        items,
        destination: Destination::Trash,
        total_bytes,
        requires_root,
    }
}

/// Stable opaque id derived from a path (its lossy string form).
pub(crate) fn path_id(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Total recursive size of a directory (0 if missing). Thin wrapper over
/// `core_scan::dir_sizes`.
pub(crate) fn dir_total(path: &Path) -> u64 {
    core_scan::dir_sizes(path).get(path).copied().unwrap_or(0)
}
