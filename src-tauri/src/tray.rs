// SPDX-License-Identifier: GPL-3.0-or-later
//! System tray icon.
//!
//! On Linux the tray uses the StatusNotifier/AppIndicator protocol, which does
//! NOT deliver raw click events to the application — interaction goes through
//! the tray MENU. The "Résumé de l'espace" item opens the graphical popover
//! widget; "Ouvrir FreeYourDisk" restores the main window.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    App, AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow,
};

/// Build the tray icon and its menu.
pub fn setup(app: &App) -> tauri::Result<()> {
    let summary = MenuItem::with_id(app, "summary", "Résumé de l'espace", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Ouvrir FreeYourDisk", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&summary, &open, &sep, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("FreeYourDisk")
        .menu(&menu)
        // Left-click shows the menu (the only reliable interaction on Linux).
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "summary" => toggle_popover(app),
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Show or hide the popover widget window.
fn toggle_popover(app: &AppHandle) {
    let Some(win) = app.get_webview_window("tray") else {
        return;
    };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
    } else {
        position_popover(&win);
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Anchor the popover to the bottom-right of the primary monitor (above a
/// typical taskbar).
fn position_popover(win: &WebviewWindow) {
    let size = win.outer_size().unwrap_or(PhysicalSize {
        width: 320,
        height: 470,
    });
    if let Ok(Some(monitor)) = win.primary_monitor() {
        let m = monitor.size();
        let origin = monitor.position();
        let margin = 12i32;
        let taskbar = 56i32;
        let x = origin.x + m.width as i32 - size.width as i32 - margin;
        let y = origin.y + m.height as i32 - size.height as i32 - taskbar;
        let _ = win.set_position(PhysicalPosition::new(
            x.max(origin.x + margin),
            y.max(origin.y + margin),
        ));
    }
}
