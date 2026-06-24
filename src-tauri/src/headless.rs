// SPDX-License-Identifier: GPL-3.0-or-later
//! Headless mode for the optional systemd user timer. Fully implemented in
//! Phase 7 (Task 7.1).

/// Run a headless cleanup. Returns a process exit code.
pub fn run(_args: &[String]) -> i32 {
    eprintln!("freeyourdisk: headless mode is not yet implemented");
    1
}
