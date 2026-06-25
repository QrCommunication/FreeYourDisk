// SPDX-License-Identifier: GPL-3.0-or-later
//! FreeYourDisk — Tauri application entry point.
//!
//! Normal launch opens the WebView UI (user process, no privileges). With
//! `--headless`, runs a non-interactive cleanup for the systemd timer (Phase 7).

mod commands;
mod execute;
mod headless;
mod services;
mod state;
mod tray;

use state::AppState;
use tauri::WindowEvent;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--headless") {
        std::process::exit(headless::run(&args));
    }

    tauri::Builder::default()
        .manage(AppState::new())
        .setup(|app| {
            tray::setup(app)?;
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the main window hides it to the tray instead of quitting.
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                let _ = window.hide();
                api.prevent_close();
            }
            // The popover dismisses itself when it loses focus.
            WindowEvent::Focused(false) if window.label() == "tray" => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan,
            commands::preview,
            commands::execute,
            commands::disk_usage,
            commands::schedule_enabled,
            commands::set_schedule,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FreeYourDisk");
}
