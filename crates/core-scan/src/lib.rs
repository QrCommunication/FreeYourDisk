// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only parallel filesystem scanning, size aggregation and top-N ranking.
//!
//! Invariant 1 (project-wide): nothing in this module ever mutates the
//! filesystem. The `scan_lists_files_read_only` test pins that contract.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod cache;

/// A raw filesystem entry produced by a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
    /// Unix timestamp (seconds) of last access, when available.
    pub last_access: Option<i64>,
    pub is_dir: bool,
}

/// Options controlling a scan. Default: don't follow symlinks, no age filter.
#[derive(Clone, Debug, Default)]
pub struct ScanOpts {
    /// Follow symlinks during traversal. Default: false (safer).
    pub follow_symlinks: bool,
    /// If set, only keep entries not modified within the last N days.
    pub min_age_days: Option<u32>,
}

const SECS_PER_DAY: u64 = 86_400;

/// Walk `root` (read-only, parallel via `jwalk`) and return every entry below it.
/// The `root` itself is not included. With `min_age_days`, recently-modified
/// entries are filtered out.
pub fn scan_dir(root: &Path, opts: &ScanOpts) -> Vec<RawEntry> {
    let now = SystemTime::now();
    jwalk::WalkDir::new(root)
        .follow_links(opts.follow_symlinks)
        .into_iter()
        .filter_map(|res| res.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if path == root {
                return None;
            }
            let meta = entry.metadata().ok()?;
            let is_dir = meta.is_dir();
            let size_bytes = if is_dir { 0 } else { meta.len() };
            let last_access = meta
                .accessed()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);

            if let Some(days) = opts.min_age_days {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age < Duration::from_secs(days as u64 * SECS_PER_DAY) {
                            return None;
                        }
                    }
                }
            }

            Some(RawEntry {
                path,
                size_bytes,
                last_access,
                is_dir,
            })
        })
        .collect()
}

/// Aggregate the total size of every directory below `root` (inclusive of
/// `root`), bottom-up. Each file's size is added to all of its ancestor
/// directories up to and including `root`.
pub fn dir_sizes(root: &Path) -> HashMap<PathBuf, u64> {
    let mut sizes: HashMap<PathBuf, u64> = HashMap::new();
    sizes.insert(root.to_path_buf(), 0);

    for entry in jwalk::WalkDir::new(root).into_iter().filter_map(|r| r.ok()) {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let len = meta.len();
        let mut current = entry.path();
        while let Some(parent) = current.parent() {
            *sizes.entry(parent.to_path_buf()).or_insert(0) += len;
            if parent == root {
                break;
            }
            current = parent.to_path_buf();
        }
    }
    sizes
}

/// Return the `n` largest files and the `n` largest directories (excluding
/// `root` itself), each sorted by descending size.
pub fn top_n(root: &Path, n: usize) -> (Vec<RawEntry>, Vec<(PathBuf, u64)>) {
    let mut files: Vec<RawEntry> = Vec::new();
    let mut sizes: HashMap<PathBuf, u64> = HashMap::new();

    // Single traversal: collect files and accumulate directory sizes at once.
    // Cache/dev directories are pruned (owned by their dedicated services), so
    // this never traverses the millions of files inside node_modules/.cache.
    let walk = jwalk::WalkDir::new(root).process_read_dir(|_, _, _, children| {
        children.retain(|entry| {
            entry
                .as_ref()
                .map(|e| !cache::should_skip(&e.file_name().to_string_lossy()))
                .unwrap_or(true)
        });
    });
    for entry in walk.into_iter().filter_map(|r| r.ok()) {
        let path = entry.path();
        if path == root {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let len = meta.len();
        let last_access = meta
            .accessed()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        files.push(RawEntry {
            path: path.clone(),
            size_bytes: len,
            last_access,
            is_dir: false,
        });

        let mut current = path;
        while let Some(parent) = current.parent() {
            *sizes.entry(parent.to_path_buf()).or_insert(0) += len;
            if parent == root {
                break;
            }
            current = parent.to_path_buf();
        }
    }

    files.sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    files.truncate(n);

    let mut dirs: Vec<(PathBuf, u64)> = sizes.into_iter().filter(|(p, _)| p != root).collect();
    dirs.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    dirs.truncate(n);

    (files, dirs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// (len, mtime_secs) per path — used to assert the FS is untouched.
    fn dir_snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, u64)> {
        let mut map = BTreeMap::new();
        for e in jwalk::WalkDir::new(root).into_iter().filter_map(|r| r.ok()) {
            let m = e.metadata().unwrap();
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            map.insert(e.path(), (m.len(), mtime));
        }
        map
    }

    #[test]
    fn scan_lists_files_read_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        let before = dir_snapshot(dir.path());
        let entries = scan_dir(dir.path(), &ScanOpts::default());
        let after = dir_snapshot(dir.path());
        assert_eq!(before, after, "scan must not modify the filesystem");
        assert!(entries
            .iter()
            .any(|e| e.path.ends_with("a.txt") && e.size_bytes == 5));
    }

    #[test]
    fn dir_sizes_aggregates_bottom_up() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(dir.path().join("a.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(sub.join("b.txt"), vec![0u8; 50]).unwrap();

        let sizes = dir_sizes(dir.path());
        assert_eq!(sizes.get(&sub).copied(), Some(50));
        assert_eq!(sizes.get(dir.path()).copied(), Some(150));
    }

    #[test]
    fn top_n_returns_largest_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("small.txt"), vec![0u8; 10]).unwrap();
        std::fs::write(dir.path().join("big.txt"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.path().join("mid.txt"), vec![0u8; 100]).unwrap();

        let (files, _dirs) = top_n(dir.path(), 2);
        assert_eq!(files.len(), 2);
        assert!(files[0].path.ends_with("big.txt"));
        assert!(files[1].path.ends_with("mid.txt"));
        assert!(files[0].size_bytes >= files[1].size_bytes);
    }
}
