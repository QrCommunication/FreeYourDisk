// SPDX-License-Identifier: GPL-3.0-or-later
//! Persisted, mtime-validated directory-size cache for incremental rescans.
//!
//! A directory whose mtime is unchanged since the last scan returns its cached
//! subtree size without being re-walked. Large, stable trees (node_modules,
//! caches) are therefore traversed only once; subsequent scans reuse the size.
//!
//! Caveat: a directory's mtime reflects add/remove/rename of its direct
//! entries, not deep content growth. A "deep rescan" can be forced by clearing
//! the cache file. This is the standard fast-scanner tradeoff.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::UNIX_EPOCH;

/// Directory names owned by dedicated services. The file-level walkers (largest
/// files, file types) skip descending into them so they never traverse the
/// millions of files inside caches; their size is still computed (and cached)
/// by [`cached_dir_total`] for the services that target them.
pub const SKIP_DESCENT: &[&str] = &[
    "node_modules",
    ".cache",
    "target",
    ".venv",
    ".next",
    ".turbo",
    "vendor",
    ".git",
    ".cargo",
    ".gradle",
    // Application folders are owned by the Applications section.
    "Applications",
    "applications",
];

pub fn should_skip(name: &str) -> bool {
    SKIP_DESCENT.contains(&name)
}

#[derive(Serialize, Deserialize, Default)]
struct DirCache {
    entries: HashMap<PathBuf, (u64, u64)>, // path -> (mtime_secs, total_size)
}

static CACHE: OnceLock<RwLock<DirCache>> = OnceLock::new();

fn cache() -> &'static RwLock<DirCache> {
    CACHE.get_or_init(|| RwLock::new(DirCache::default()))
}

fn mtime_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load the cache from disk (call once at startup).
pub fn load(path: &Path) {
    if let Ok(raw) = std::fs::read_to_string(path) {
        if let Ok(loaded) = serde_json::from_str::<DirCache>(&raw) {
            if let Ok(mut guard) = cache().write() {
                *guard = loaded;
            }
        }
    }
}

/// Persist the cache to disk (atomic via temp + rename).
pub fn save(path: &Path) {
    let json = {
        let Ok(guard) = cache().read() else { return };
        match serde_json::to_string(&*guard) {
            Ok(json) => json,
            Err(_) => return,
        }
    };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

const HEAVY: u64 = 16 * 1024 * 1024; // only cache subtrees >= 16 MB

/// Recursive subtree size, reusing the cached size of any directory whose mtime
/// is unchanged. Never follows symlinks. Heavy subtrees are (re)cached.
pub fn cached_dir_total(root: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mtime = mtime_secs(&meta);
    if let Ok(guard) = cache().read() {
        if let Some((cached_mtime, size)) = guard.entries.get(root) {
            if *cached_mtime == mtime {
                return *size;
            }
        }
    }

    let mut total = 0;
    if let Ok(read) = std::fs::read_dir(root) {
        for entry in read.flatten() {
            // DirEntry::metadata() does not follow symlinks.
            let Ok(m) = entry.metadata() else { continue };
            if m.is_dir() {
                total += cached_dir_total(&entry.path());
            } else if m.is_file() {
                total += m.len();
            }
        }
    }

    if total >= HEAVY {
        if let Ok(mut guard) = cache().write() {
            guard.entries.insert(root.to_path_buf(), (mtime, total));
        }
    }
    total
}
