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

use state::AppState;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--headless") {
        std::process::exit(headless::run(&args));
    }

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::scan,
            commands::preview,
            commands::execute,
            commands::disk_usage,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FreeYourDisk");
}
