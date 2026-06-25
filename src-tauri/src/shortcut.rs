// SPDX-License-Identifier: GPL-3.0-or-later
//! Global summon shortcut: a user-configurable hotkey that raises the window
//! and opens the task manager — the "bring up the killer" gesture.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// (Re)register the global summon shortcut, replacing any previous binding.
/// On press it shows + focuses the main window and asks the UI to open the
/// task manager. An empty accelerator disables the shortcut.
pub fn register(app: &AppHandle, accelerator: &str) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if accelerator.trim().is_empty() {
        return;
    }
    let _ = gs.on_shortcut(accelerator, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
            let _ = app.emit("summon-taskmgr", ());
        }
    });
}
