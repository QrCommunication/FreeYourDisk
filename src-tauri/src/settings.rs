// SPDX-License-Identifier: GPL-3.0-or-later
//! Persisted user settings (theme, language, autostart, low-space monitor).
//! Stored as JSON under the XDG config dir; no external dependency.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// "system" | "light" | "dark"
    pub theme: String,
    /// "system" | "fr" | "en"
    pub language: String,
    /// Launch FreeYourDisk on session start (XDG autostart).
    pub autostart: bool,
    /// Watch free disk space in the background.
    pub monitor_enabled: bool,
    /// Alert when free space on a mount drops below this percentage.
    pub monitor_threshold: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            language: "system".into(),
            autostart: false,
            monitor_enabled: true,
            monitor_threshold: 5,
        }
    }
}

/// XDG config dir for FreeYourDisk (`~/.config/freeyourdisk`). Also used by the
/// snapshot store.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("freeyourdisk")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// Load settings, falling back to defaults on any error.
pub fn load() -> Settings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Persist settings and apply the autostart side effect.
pub fn save(settings: &Settings) -> Result<(), String> {
    let dir = config_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(settings_path(), json).map_err(|e| e.to_string())?;
    apply_autostart(settings.autostart)?;
    Ok(())
}

fn autostart_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("autostart")
        .join("freeyourdisk.desktop")
}

/// Create or remove the XDG autostart entry.
pub fn apply_autostart(enabled: bool) -> Result<(), String> {
    let path = autostart_path();
    if enabled {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let desktop = "[Desktop Entry]\n\
            Type=Application\n\
            Name=FreeYourDisk\n\
            Exec=freeyourdisk\n\
            Icon=freeyourdisk\n\
            Comment=Safe disk cleanup\n\
            X-GNOME-Autostart-enabled=true\n";
        fs::write(&path, desktop).map_err(|e| e.to_string())?;
    } else {
        let _ = fs::remove_file(&path);
    }
    Ok(())
}
