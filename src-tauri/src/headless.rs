// SPDX-License-Identifier: GPL-3.0-or-later
//! Headless mode for the optional systemd user timer.
//!
//! Runs a non-interactive cleanup of old files in `~/.cache` only — strictly
//! user-owned zones, never root. The timer never triggers privileged actions.

use core_services::{Service, TempRoot, TempService};
use core_trash::{to_trash, Zones};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Outcome of a headless run.
#[derive(Debug, PartialEq, Eq)]
pub struct HeadlessOutcome {
    pub considered: usize,
    pub freed_bytes: u64,
    pub deleted_count: usize,
    pub applied: bool,
}

/// Default age threshold for the scheduled cache cleanup.
const MIN_AGE_DAYS: u32 = 7;

/// Scan (and optionally trash) old files under `~/.cache`. User-only by
/// construction: the single root is the user's cache, marked non-root.
pub fn cache_cleanup(home: &Path, min_age_days: u32, apply: bool) -> HeadlessOutcome {
    let service = TempService {
        roots: vec![TempRoot {
            path: home.join(".cache"),
            requires_root: false,
        }],
        min_age_days,
    };
    let items = service.scan().items;
    let considered = items.len();

    if !apply {
        return HeadlessOutcome {
            considered,
            freed_bytes: 0,
            deleted_count: 0,
            applied: false,
        };
    }

    let zones = Zones(vec![home.to_path_buf()]);
    let paths: Vec<PathBuf> = items.iter().map(|item| item.path.clone()).collect();
    let report = to_trash(&paths, &zones);

    HeadlessOutcome {
        considered,
        freed_bytes: report.freed_bytes,
        deleted_count: report.deleted_count,
        applied: true,
    }
}

fn humanize(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut i = 0;
    while value >= 1024.0 && i < UNITS.len() - 1 {
        value /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[i])
    }
}

fn is_french() -> bool {
    std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LANG"))
        .or_else(|_| std::env::var("LANGUAGE"))
        .map(|lang| lang.to_lowercase().starts_with("fr"))
        .unwrap_or(false)
}

fn notify(freed_bytes: u64, count: usize) {
    let body = if is_french() {
        format!("{} libérés · {count} éléments", humanize(freed_bytes))
    } else {
        format!("{} freed · {count} items", humanize(freed_bytes))
    };
    let _ = Command::new("notify-send")
        .arg("--app-name=FreeYourDisk")
        .arg("FreeYourDisk")
        .arg(body)
        .status();
}

/// CLI entry point for `--headless`. Returns a process exit code.
pub fn run(args: &[String]) -> i32 {
    // Only the temp/cache service is supported in headless mode.
    let service = args
        .iter()
        .find_map(|a| a.strip_prefix("--service="))
        .unwrap_or("temp");
    if service != "temp" {
        eprintln!("freeyourdisk --headless: only --service=temp is supported");
        return 2;
    }

    let apply = args.iter().any(|a| a == "--apply");
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));

    let outcome = cache_cleanup(&home, MIN_AGE_DAYS, apply);

    if apply {
        if outcome.deleted_count > 0 {
            notify(outcome.freed_bytes, outcome.deleted_count);
        }
        println!(
            "freeyourdisk: freed {} from {} items",
            humanize(outcome.freed_bytes),
            outcome.deleted_count
        );
    } else {
        println!(
            "freeyourdisk: [dry-run] {} cleanup candidates",
            outcome.considered
        );
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::{Duration, SystemTime};

    fn backdate(path: &Path, days: u64) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
            .unwrap();
    }

    #[test]
    fn dry_run_frees_nothing() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join(".cache");
        std::fs::create_dir(&cache).unwrap();
        let f = cache.join("old.log");
        std::fs::write(&f, vec![0u8; 100]).unwrap();
        backdate(&f, 30);

        let outcome = cache_cleanup(home.path(), 7, false);
        assert!(!outcome.applied);
        assert_eq!(outcome.freed_bytes, 0);
        assert_eq!(outcome.deleted_count, 0);
        assert!(
            outcome.considered >= 1,
            "old cache file should be a candidate"
        );
        assert!(f.exists(), "dry-run must not delete");
    }

    #[test]
    fn recent_files_are_not_candidates() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join(".cache");
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join("fresh.log"), vec![0u8; 50]).unwrap();

        let outcome = cache_cleanup(home.path(), 7, false);
        assert_eq!(outcome.considered, 0, "recent files must be filtered out");
    }
}
