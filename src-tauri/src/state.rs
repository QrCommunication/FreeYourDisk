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
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            search_root: home.clone(),
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
