// SPDX-License-Identifier: GPL-3.0-or-later
//! Minimal privileged helper for FreeYourDisk.
//!
//! Reads a `DeletionPlan` (JSON) on stdin, **re-validates** every path against
//! hard-coded root zones (never trusting the caller), executes, and writes an
//! `ExecutionReport` (JSON) on stdout.
//!
//! Security stance: all-or-nothing. If *any* path escapes the root zones the
//! entire batch is refused — a caller cannot smuggle a malicious path alongside
//! legitimate ones to obtain partial execution.
//!
//! Exit codes: 0 = success, 2 = invalid input, 3 = a path was refused.

use core_ipc::{DeletionPlan, Destination, ExecutionReport, ItemError};
use core_trash::{delete_permanent, to_trash, validate, Zones};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// Root-owned zones this helper is ever allowed to touch. Hard-coded — never
/// received from the (semi-trusted) caller.
const ROOT_ZONES: &[&str] = &["/tmp", "/var/tmp"];

fn write_report(report: &ExecutionReport) {
    if let Ok(json) = serde_json::to_string(report) {
        let _ = std::io::stdout().write_all(json.as_bytes());
    }
}

fn main() -> ExitCode {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("freeyourdisk-helper: failed to read stdin");
        return ExitCode::from(2);
    }

    let plan: DeletionPlan = match serde_json::from_str(&input) {
        Ok(plan) => plan,
        Err(err) => {
            eprintln!("freeyourdisk-helper: invalid plan: {err}");
            return ExitCode::from(2);
        }
    };

    let zones = Zones(ROOT_ZONES.iter().map(PathBuf::from).collect());
    let paths: Vec<PathBuf> = plan.items.iter().map(|item| item.path.clone()).collect();

    // Pre-validate every path; refuse the whole batch on any escape.
    let refusals: Vec<ItemError> = paths
        .iter()
        .filter_map(|path| match validate(path, &zones) {
            Ok(_) => None,
            Err(err) => Some(ItemError {
                path: path.clone(),
                message: err.to_string(),
            }),
        })
        .collect();

    if !refusals.is_empty() {
        write_report(&ExecutionReport {
            freed_bytes: 0,
            deleted_count: 0,
            errors: refusals,
        });
        return ExitCode::from(3);
    }

    let report = match plan.destination {
        Destination::Trash => to_trash(&paths, &zones),
        Destination::Permanent => delete_permanent(&paths, &zones),
    };
    write_report(&report);
    ExitCode::SUCCESS
}
