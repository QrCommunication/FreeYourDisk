// SPDX-License-Identifier: GPL-3.0-or-later
//! Application state: runtime configuration and the per-service scan cache.

use core_ipc::{ScanResult, ServiceId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// Runtime configuration, detected at startup.
#[derive(Clone, Debug)]
pub struct Config {
    /// User home — the user-deletion zone and default search root.
    pub home: PathBuf,
    /// Root under which big-files / git / dev-cache services search.
    pub search_root: PathBuf,
    /// Minimum age (days) for a temp file to be eligible.
    pub temp_min_age_days: u32,
    /// How many top entries the big-files service returns.
    pub big_files_top: usize,
}

impl Config {
    pub fn detect() -> Self {
        // `dirs::home_dir()` resolves $HOME on unix and %USERPROFILE% on Windows.
        let home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }));

        // On Linux, scanning $HOME directly causes multi-minute hangs because
        // the walker traverses massive system/cache trees (.cache, .local/share,
        // .steam, etc.) before reaching user project directories. Use $HOME as
        // the search root only on Windows/macOS where exclusions are sufficient.
        // On Linux, prefer common project parent directories if they exist; fall
        // back to $HOME with the understanding that should_skip_dir() will prune
        // the worst offenders.
        #[cfg(target_os = "linux")]
        let search_root = {
            let candidates = ["Projets", "Projects", "dev", "src", "code", "workspace"];
            candidates
                .iter()
                .map(|c| home.join(c))
                .find(|p| p.is_dir())
                .unwrap_or_else(|| home.clone())
        };
        #[cfg(not(target_os = "linux"))]
        let search_root = home.clone();

        Self {
            search_root,
            home,
            temp_min_age_days: 7,
            big_files_top: 50,
        }
    }
}

/// Shared application state managed by Tauri.
pub struct AppState {
    pub config: Mutex<Config>,
    pub cache: Mutex<HashMap<ServiceId, ScanResult>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(Config::detect()),
            cache: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
