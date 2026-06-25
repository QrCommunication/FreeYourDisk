// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared DTOs (contracts) exchanged between the UI, the Tauri backend and the
//! privileged helper. This crate is the single source of truth for the types
//! that cross process boundaries.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Identifies one of the four cleanup services.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ServiceId {
    Temp,
    BigFiles,
    GitRepos,
    DevCache,
    AppCache,
}

/// Where deleted items go.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    /// Recoverable XDG trash (default).
    Trash,
    /// Irreversible removal (opt-in only).
    Permanent,
}

/// What a scanned item represents.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    File,
    Dir,
    GitWorktree,
    GitBranch,
    DevCache,
}

/// A single item surfaced by a scan. `id` is a stable, opaque identifier used by
/// the UI to build selections without sending paths back and forth ambiguously.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ScanItem {
    pub id: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    /// Unix timestamp (seconds) of last access, when available.
    pub last_access: Option<i64>,
    pub kind: ItemKind,
    /// True if removing this item requires the privileged helper.
    pub requires_root: bool,
}

/// Result of a read-only scan for a given service.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ScanResult {
    pub service: ServiceId,
    pub items: Vec<ScanItem>,
    pub total_bytes: u64,
}

/// A dry-run plan: exactly what will be deleted, where to, and whether root is
/// required. Built by `preview()` and confirmed by the user before execution.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DeletionPlan {
    pub items: Vec<ScanItem>,
    pub destination: Destination,
    pub total_bytes: u64,
    pub requires_root: bool,
}

/// One failure encountered while executing a plan.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ItemError {
    pub path: PathBuf,
    pub message: String,
}

/// Outcome of executing a `DeletionPlan`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutionReport {
    pub freed_bytes: u64,
    pub deleted_count: usize,
    pub errors: Vec<ItemError>,
}

/// Per-mount disk usage, for the dashboard donut.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MountUsage {
    pub mount: String,
    pub total: u64,
    pub used: u64,
}

/// SMART health for one physical device (produced by the privileged helper,
/// which is the only component allowed to run `smartctl`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SmartInfo {
    pub device: String,
    /// False when SMART could not be read (no smartctl, denied, unsupported).
    pub available: bool,
    /// SMART overall-health self-assessment (true = PASSED).
    pub passed: Option<bool>,
    /// Power-on hours — the drive's total "uptime".
    pub power_on_hours: Option<u64>,
    pub temperature_c: Option<i64>,
}

/// Result of a privileged dependency install (e.g. nvme-cli / smartmontools).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    pub success: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn scan_item_roundtrips_json() {
        let item = ScanItem {
            id: "x".into(),
            path: PathBuf::from("/tmp/foo"),
            size_bytes: 42,
            last_access: None,
            kind: ItemKind::File,
            requires_root: false,
        };
        let json = serde_json::to_string(&item).unwrap();
        let back: ScanItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back.size_bytes, 42);
        assert_eq!(back.kind, ItemKind::File);
        assert_eq!(back.path, PathBuf::from("/tmp/foo"));
    }

    #[test]
    fn enums_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&ServiceId::BigFiles).unwrap(),
            "\"big_files\""
        );
        assert_eq!(
            serde_json::to_string(&Destination::Trash).unwrap(),
            "\"trash\""
        );
        assert_eq!(
            serde_json::to_string(&ItemKind::GitWorktree).unwrap(),
            "\"git_worktree\""
        );
    }

    #[test]
    fn deletion_plan_roundtrips() {
        let plan = DeletionPlan {
            items: vec![],
            destination: Destination::Permanent,
            total_bytes: 0,
            requires_root: true,
        };
        let json = serde_json::to_string(&plan).unwrap();
        let back: DeletionPlan = serde_json::from_str(&json).unwrap();
        assert!(back.requires_root);
        assert_eq!(back.destination, Destination::Permanent);
    }
}
