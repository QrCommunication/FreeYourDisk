// SPDX-License-Identifier: GPL-3.0-or-later
//! Per-service scan snapshots, for the incremental "what's new" diff.
//!
//! After each scan we persist a manifest (`item id -> size`). The next scan
//! compares against it: an item is *new* if its id is absent or its size
//! changed. The first scan of a service has no manifest and is reported as
//! such (everything is "baseline", nothing badged).

use core_ipc::{ScanItem, ServiceId};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

type Manifest = HashMap<String, u64>;

fn service_key(service: ServiceId) -> &'static str {
    match service {
        ServiceId::Temp => "temp",
        ServiceId::BigFiles => "big_files",
        ServiceId::GitRepos => "git_repos",
        ServiceId::DevCache => "dev_cache",
        ServiceId::AppCache => "app_cache",
    }
}

fn manifest_path(service: ServiceId) -> PathBuf {
    crate::settings::config_dir().join(format!("snapshot-{}.json", service_key(service)))
}

fn load(service: ServiceId) -> Option<Manifest> {
    let raw = fs::read_to_string(manifest_path(service)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save(service: ServiceId, items: &[ScanItem]) {
    let manifest: Manifest = items.iter().map(|i| (i.id.clone(), i.size_bytes)).collect();
    let dir = crate::settings::config_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(&manifest) {
        let _ = fs::write(manifest_path(service), json);
    }
}

/// Diff against the stored manifest, then overwrite it with the current items.
/// Returns `(first_scan, new_ids)`.
pub fn diff_and_record(service: ServiceId, items: &[ScanItem]) -> (bool, Vec<String>) {
    let previous = load(service);
    let first_scan = previous.is_none();
    let new_ids = match &previous {
        None => Vec::new(),
        Some(manifest) => items
            .iter()
            .filter(|item| manifest.get(&item.id) != Some(&item.size_bytes))
            .map(|item| item.id.clone())
            .collect(),
    };
    save(service, items);
    (first_scan, new_ids)
}
