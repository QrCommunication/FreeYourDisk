// SPDX-License-Identifier: GPL-3.0-or-later
//! FreeYourDisk — Tauri application entry point.
//!
//! Normal launch opens the WebView UI (user process, no privileges). With
//! `--headless`, runs a non-interactive cleanup for the systemd timer (Phase 7).

mod applications;
mod commands;
mod execute;
mod filetypes;
mod headless;
mod health;
mod monitor;
mod services;
mod settings;
mod shortcut;
mod smartdeps;
mod snapshot;
mod state;
mod taskmgr;
mod tray;

use state::AppState;
use tauri::WindowEvent;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--headless") {
        std::process::exit(headless::run(&args));
    }
    #[cfg(target_os = "windows")]
    if args.iter().any(|a| a == "--apply") {
        // `--apply` always terminates here (never falls through to the GUI). A
        // missing/invalid token is rejected by apply_elevated's digit guard.
        let token = args
            .iter()
            .position(|a| a == "--apply")
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
            .unwrap_or("");
        std::process::exit(headless::apply_elevated(token));
    }

    // Stay alive and responsive under memory pressure (best effort).
    taskmgr::raise_priority();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::new())
        .setup(|app| {
            core_scan::cache::load(&settings::config_dir().join("dir-cache.json"));
            tray::setup(app)?;
            monitor::start(app.handle().clone());
            // Register the user's summon hotkey for the task manager.
            shortcut::register(app.handle(), &settings::load().shortcut);
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
            commands::health_overview,
            commands::disk_smart,
            commands::smart_deps_status,
            commands::install_smart_deps,
            commands::file_types,
            commands::home_total,
            commands::system_total,
            commands::mem_stats,
            commands::process_list,
            commands::kill_process,
            commands::restart_process,
            commands::panic_kill,
            commands::list_applications,
            commands::app_updates,
            commands::uninstall_apps,
            commands::update_apps,
            commands::home_cache_load,
            commands::home_cache_save,
            commands::app_version,
            commands::get_settings,
            commands::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FreeYourDisk");
}
