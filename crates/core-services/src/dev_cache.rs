// SPDX-License-Identifier: GPL-3.0-or-later
//! Dev cache service: detects developer caches (node_modules, Rust target,
//! .next, .turbo, .venv, PHP vendor) under a search root by directory signature.

use crate::{dir_total, path_id, Service};
use core_ipc::{ItemKind, ScanItem, ScanResult, ServiceId};
use std::fs;
use std::path::{Path, PathBuf};

/// Directory names that are always developer caches.
const UNCONDITIONAL: &[&str] = &["node_modules", ".next", ".turbo", ".venv", ".gradle"];

/// Directories we never descend into while searching (speed + correctness).
const SKIP_DESCENT: &[&str] = &[
    "node_modules",
    ".git",
    ".next",
    ".turbo",
    "target",
    "vendor",
];

/// Detects developer caches under `search_root`.
#[derive(Clone, Debug)]
pub struct DevCacheService {
    pub search_root: PathBuf,
    pub max_depth: usize,
}

impl DevCacheService {
    pub fn new(search_root: PathBuf) -> Self {
        Self {
            search_root,
            max_depth: 8,
        }
    }
}

/// Is `dir` (named `name`, with parent `parent`) a developer cache?
fn is_cache_dir(name: &str, parent: &Path) -> bool {
    if UNCONDITIONAL.contains(&name) {
        return true;
    }
    match name {
        // `target` only counts next to a Cargo.toml (Rust build dir).
        "target" => parent.join("Cargo.toml").exists(),
        // `vendor` only counts next to a composer.json (PHP deps).
        "vendor" => parent.join("composer.json").exists(),
        _ => false,
    }
}

fn last_access(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.accessed().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<ScanItem>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if is_cache_dir(name, dir) {
            out.push(ScanItem {
                id: path_id(&path),
                size_bytes: dir_total(&path),
                last_access: last_access(&path),
                path,
                kind: ItemKind::DevCache,
                requires_root: false,
            });
            continue; // never descend into a detected cache
        }

        if depth > 0 && !SKIP_DESCENT.contains(&name) {
            walk(&path, depth - 1, out);
        }
    }
}

impl Service for DevCacheService {
    fn id(&self) -> ServiceId {
        ServiceId::DevCache
    }

    fn scan(&self) -> ScanResult {
        let mut items = Vec::new();
        walk(&self.search_root, self.max_depth, &mut items);
        let total_bytes = items.iter().map(|item| item.size_bytes).sum();
        ScanResult {
            service: ServiceId::DevCache,
            items,
            total_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_node_modules_and_rust_target() {
        let root = tempfile::tempdir().unwrap();

        // Node project: package.json + node_modules/
        let node_proj = root.path().join("web");
        std::fs::create_dir_all(node_proj.join("node_modules")).unwrap();
        std::fs::write(node_proj.join("package.json"), b"{}").unwrap();
        std::fs::write(node_proj.join("node_modules/dep.js"), vec![0u8; 200]).unwrap();

        // Rust project: Cargo.toml + target/
        let rust_proj = root.path().join("cli");
        std::fs::create_dir_all(rust_proj.join("target")).unwrap();
        std::fs::write(rust_proj.join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(rust_proj.join("target/app"), vec![0u8; 300]).unwrap();

        let svc = DevCacheService::new(root.path().to_path_buf());
        let result = svc.scan();

        let kinds: Vec<String> = result
            .items
            .iter()
            .map(|i| i.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(kinds.contains(&"node_modules".to_string()));
        assert!(kinds.contains(&"target".to_string()));
        assert!(result.items.iter().all(|i| !i.requires_root));
        assert!(result.items.iter().all(|i| i.kind == ItemKind::DevCache));
        // node_modules total should reflect its contents.
        let nm = result
            .items
            .iter()
            .find(|i| i.path.ends_with("node_modules"))
            .unwrap();
        assert_eq!(nm.size_bytes, 200);
    }

    #[test]
    fn bare_target_without_cargo_toml_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        let proj = root.path().join("notrust");
        std::fs::create_dir_all(proj.join("target")).unwrap();
        std::fs::write(proj.join("target/x"), vec![0u8; 10]).unwrap();

        let svc = DevCacheService::new(root.path().to_path_buf());
        let result = svc.scan();
        assert!(
            result.items.is_empty(),
            "a 'target' dir without Cargo.toml must not be flagged"
        );
    }
}
