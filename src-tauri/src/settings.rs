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
    /// Global hotkey that summons the window + task manager (Tauri accelerator).
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
}

#[cfg(target_os = "linux")]
fn default_shortcut() -> String {
    "Ctrl+Alt+Delete".to_string()
}

#[cfg(target_os = "macos")]
fn default_shortcut() -> String {
    "Cmd+Shift+M".to_string()
}

// Windows: Ctrl+Alt+Delete is the Secure Attention Sequence and cannot be
// registered by an app (silent failure), so use a capturable default.
#[cfg(target_os = "windows")]
fn default_shortcut() -> String {
    "Ctrl+Shift+M".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            language: "system".into(),
            autostart: false,
            monitor_enabled: true,
            monitor_threshold: 5,
            shortcut: default_shortcut(),
        }
    }
}

/// Per-user config dir for FreeYourDisk. Also used by the snapshot store.
/// Linux: XDG (`~/.config/freeyourdisk`). macOS: `~/Library/Application Support`.
/// Windows: `%APPDATA%\FreeYourDisk`.
#[cfg(target_os = "linux")]
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("freeyourdisk")
}

#[cfg(target_os = "macos")]
pub fn config_dir() -> PathBuf {
    home().join("Library/Application Support/FreeYourDisk")
}

#[cfg(target_os = "windows")]
pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("AppData/Roaming"))
        .join("FreeYourDisk")
}

#[cfg(unix)]
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(windows)]
fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"))
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

/// Create or remove the launch-at-login entry.
/// Linux: an XDG autostart `.desktop`. macOS: a LaunchAgent plist.
#[cfg(target_os = "linux")]
pub fn apply_autostart(enabled: bool) -> Result<(), String> {
    let path = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("autostart")
        .join("freeyourdisk.desktop");
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

#[cfg(target_os = "macos")]
pub fn apply_autostart(enabled: bool) -> Result<(), String> {
    let path = home().join("Library/LaunchAgents/com.qrcommunication.freeyourdisk.plist");
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\
             <key>Label</key><string>com.qrcommunication.freeyourdisk</string>\
             <key>ProgramArguments</key><array><string>{}</string></array>\
             <key>RunAtLoad</key><true/>\
             <key>ProcessType</key><string>Interactive</string>\
             </dict></plist>\n",
            exe.display()
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&path, plist).map_err(|e| e.to_string())?;
    } else {
        let _ = fs::remove_file(&path);
    }
    Ok(())
}

/// Create or remove the launch-at-login entry under
/// `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`.
/// Per-user (HKCU), so no elevation is required.
#[cfg(target_os = "windows")]
pub fn apply_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    // create_subkey_with_flags creates the key if missing, opens it otherwise.
    let (run, _) = hkcu
        .create_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_SET_VALUE,
        )
        .map_err(|e| e.to_string())?;

    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // Quote the path so an install dir with spaces (e.g. "Program Files")
        // is parsed as a single argument by the shell at login.
        run.set_value("FreeYourDisk", &format!("\"{}\"", exe.display()))
            .map_err(|e| e.to_string())?;
    } else {
        // Disabling when the value is absent must succeed (idempotent).
        match run.delete_value("FreeYourDisk") {
            Ok(()) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
