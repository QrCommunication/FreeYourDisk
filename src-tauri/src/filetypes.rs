// SPDX-License-Identifier: GPL-3.0-or-later
//! Disk-usage breakdown by file type (images, videos, archives, ...), with the
//! largest files per category, for the home-screen distribution bar.

use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Serialize, Clone, Debug)]
pub struct FileEntry {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct TypeBucket {
    /// Stable key (the UI localises it): images | videos | audio | archives |
    /// disk_images | executables | documents | other.
    pub category: String,
    pub bytes: u64,
    pub count: u64,
    pub top: Vec<FileEntry>,
}

const TOP_N: usize = 40;

fn classify(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp" | "tiff" | "tif" | "heic"
        | "heif" | "avif" | "ico" | "raw" | "cr2" | "nef" | "arw" | "dng" | "psd" => "images",
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "mpg" | "mpeg" | "3gp"
        | "ts" | "m2ts" => "videos",
        "mp3" | "flac" | "wav" | "ogg" | "aac" | "m4a" | "opus" | "wma" | "aiff" => "audio",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "lz4" | "lzma"
        | "cab" => "archives",
        "iso" | "img" | "dmg" | "vdi" | "qcow2" | "vmdk" | "vhd" | "vhdx" | "ova" => "disk_images",
        // Installable applications / packages.
        "appimage" | "deb" | "rpm" | "msi" | "apk" | "exe" | "flatpak" | "flatpakref" | "snap"
        | "pkg" | "run" => "applications",
        // Libraries / raw binaries.
        "bin" | "dll" | "so" | "dylib" | "a" | "o" | "ko" => "executables",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "txt" | "rtf" | "epub" | "mobi" | "csv" => "documents",
        _ => "other",
    }
}

#[derive(Default)]
struct Bucket {
    bytes: u64,
    count: u64,
    top: Vec<(String, u64)>,
}

impl Bucket {
    fn push(&mut self, path: String, size: u64) {
        self.bytes += size;
        self.count += 1;
        self.top.push((path, size));
        // Bound memory: prune to the current top-N once the buffer grows large.
        if self.top.len() > TOP_N * 4 {
            self.top.sort_unstable_by_key(|x| std::cmp::Reverse(x.1));
            self.top.truncate(TOP_N);
        }
    }

    fn finish(mut self, category: &str) -> TypeBucket {
        self.top.sort_unstable_by_key(|x| std::cmp::Reverse(x.1));
        self.top.truncate(TOP_N);
        TypeBucket {
            category: category.to_string(),
            bytes: self.bytes,
            count: self.count,
            top: self
                .top
                .into_iter()
                .map(|(path, size_bytes)| FileEntry { path, size_bytes })
                .collect(),
        }
    }
}

/// Walk `root` (read-only) and bucket every file by type, largest first.
/// Cache/dev directories are pruned (owned by their services), so this reflects
/// the user's own files and stays fast.
pub fn scan_types(root: &Path) -> Vec<TypeBucket> {
    let mut buckets: HashMap<&'static str, Bucket> = HashMap::new();
    let walk = jwalk::WalkDir::new(root).process_read_dir(|_, _, _, children| {
        children.retain(|entry| {
            entry
                .as_ref()
                .map(|e| !core_scan::cache::should_skip(&e.file_name().to_string_lossy()))
                .unwrap_or(true)
        });
    });
    for entry in walk.into_iter().filter_map(|r| r.ok()) {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        buckets
            .entry(classify(&ext))
            .or_default()
            .push(path.to_string_lossy().into_owned(), meta.len());
    }
    let mut out: Vec<TypeBucket> = buckets
        .into_iter()
        .map(|(cat, bucket)| bucket.finish(cat))
        .collect();
    out.sort_unstable_by_key(|b| std::cmp::Reverse(b.bytes));
    out
}
