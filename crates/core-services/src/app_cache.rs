// SPDX-License-Identifier: GPL-3.0-or-later
//! Application & browser cache service: the regenerable caches that escape the
//! `~/.cache` sweep — Chromium/Electron app caches under `~/.config`, Flatpak
//! per-app caches (`~/.var/app/*/cache`), Snap caches (`~/snap/*/.cache`) and
//! package-manager caches (npm). All are safe to remove; apps rebuild them.

use crate::{dir_total, path_id, Service};
use core_ipc::{ItemKind, ScanItem, ScanResult, ServiceId};
use std::fs;
use std::path::{Path, PathBuf};

/// Directory basenames that hold regenerable cache data inside an app's config
/// (Chromium/Electron family: browsers, VS Code, Slack, Discord, Spotify…).
const CACHE_DIR_NAMES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "CachedData",
    "ShaderCache",
    "GrShaderCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "component_crx_cache",
    "blob_storage",
];

/// Scans well-known application/browser cache locations under the home dir.
#[derive(Clone, Debug)]
pub struct AppCacheService {
    pub home: PathBuf,
}

impl AppCacheService {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    fn push_dir(items: &mut Vec<ScanItem>, path: PathBuf) {
        let size = dir_total(&path);
        if size == 0 {
            return;
        }
        items.push(ScanItem {
            id: path_id(&path),
            path,
            size_bytes: size,
            last_access: None,
            kind: ItemKind::Dir,
            requires_root: false,
        });
    }

    /// Collect cache dirs nested under `root` up to `depth` levels, without
    /// descending into a directory once it is recognised as a cache.
    fn collect(root: &Path, depth: usize, items: &mut Vec<ScanItem>) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if CACHE_DIR_NAMES.contains(&name.as_str()) {
                Self::push_dir(items, path);
            } else {
                Self::collect(&path, depth - 1, items);
            }
        }
    }
}

impl Service for AppCacheService {
    fn id(&self) -> ServiceId {
        ServiceId::AppCache
    }

    fn scan(&self) -> ScanResult {
        let h = &self.home;
        let mut items = Vec::new();

        // 1. Chromium/Electron app caches under ~/.config (Brave, Chrome, Code…).
        Self::collect(&h.join(".config"), 4, &mut items);

        // 2. Flatpak per-app caches.
        if let Ok(apps) = fs::read_dir(h.join(".var/app")) {
            for app in apps.flatten() {
                let cache = app.path().join("cache");
                if cache.is_dir() {
                    Self::push_dir(&mut items, cache);
                }
            }
        }

        // 3. Snap per-app caches.
        if let Ok(snaps) = fs::read_dir(h.join("snap")) {
            for snap in snaps.flatten() {
                for sub in ["common/.cache", "current/.cache"] {
                    let cache = snap.path().join(sub);
                    if cache.is_dir() {
                        Self::push_dir(&mut items, cache);
                    }
                }
            }
        }

        // 4. Package-manager caches that live outside ~/.cache.
        for rel in [".npm/_cacache", ".yarn/cache", ".bun/install/cache"] {
            let cache = h.join(rel);
            if cache.is_dir() {
                Self::push_dir(&mut items, cache);
            }
        }

        let total_bytes = items.iter().map(|item| item.size_bytes).sum();
        ScanResult {
            service: ServiceId::AppCache,
            items,
            total_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nested_browser_cache() {
        let home = tempfile::tempdir().unwrap();
        let cache = home
            .path()
            .join(".config/BraveSoftware/Brave-Browser/Default/Cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("blob"), vec![0u8; 4096]).unwrap();

        let svc = AppCacheService::new(home.path().to_path_buf());
        let result = svc.scan();

        assert!(
            result.items.iter().any(|i| i.path.ends_with("Cache")),
            "browser Cache dir surfaced"
        );
        assert!(result.items.iter().all(|i| !i.requires_root));
    }
}
