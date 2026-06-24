// SPDX-License-Identifier: GPL-3.0-or-later
//! Execution routing: user-owned items are deleted in-process (validated
//! against the home zone); root-owned items are handed to the privileged
//! helper via `pkexec`. The helper invocation is injected so the routing logic
//! is unit-testable without root.

use core_ipc::{DeletionPlan, Destination, ExecutionReport, ItemError, ScanItem};
use core_trash::Zones;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Installed location of the privileged helper.
pub const HELPER_PATH: &str = "/usr/lib/freeyourdisk/freeyourdisk-helper";

/// User-deletion zone: anything under the user's home.
fn user_zones(home: &Path) -> Zones {
    Zones(vec![home.to_path_buf()])
}

/// Delete the user-owned items (validated against the home zone).
fn execute_user_items(
    items: &[ScanItem],
    destination: Destination,
    home: &Path,
) -> ExecutionReport {
    if items.is_empty() {
        return ExecutionReport::default();
    }
    let paths: Vec<PathBuf> = items.iter().map(|item| item.path.clone()).collect();
    let zones = user_zones(home);
    match destination {
        Destination::Trash => core_trash::to_trash(&paths, &zones),
        Destination::Permanent => core_trash::delete_permanent(&paths, &zones),
    }
}

fn merge(into: &mut ExecutionReport, other: ExecutionReport) {
    into.freed_bytes += other.freed_bytes;
    into.deleted_count += other.deleted_count;
    into.errors.extend(other.errors);
}

/// Route a plan: user items in-process, root items to `invoke_helper`.
pub fn execute_plan(
    plan: &DeletionPlan,
    home: &Path,
    invoke_helper: impl Fn(&DeletionPlan) -> ExecutionReport,
) -> ExecutionReport {
    let (root_items, user_items): (Vec<ScanItem>, Vec<ScanItem>) = plan
        .items
        .iter()
        .cloned()
        .partition(|item| item.requires_root);

    let mut report = execute_user_items(&user_items, plan.destination, home);

    if !root_items.is_empty() {
        let total_bytes = root_items.iter().map(|item| item.size_bytes).sum();
        let root_plan = DeletionPlan {
            items: root_items,
            destination: Destination::Permanent,
            total_bytes,
            requires_root: true,
        };
        merge(&mut report, invoke_helper(&root_plan));
    }

    report
}

fn err_report(plan: &DeletionPlan, message: &str) -> ExecutionReport {
    ExecutionReport {
        freed_bytes: 0,
        deleted_count: 0,
        errors: plan
            .items
            .iter()
            .map(|item| ItemError {
                path: item.path.clone(),
                message: message.to_string(),
            })
            .collect(),
    }
}

/// Real helper invocation: `pkexec <helper>`, plan on stdin, report on stdout.
pub fn pkexec_helper(plan: &DeletionPlan) -> ExecutionReport {
    let json = match serde_json::to_string(plan) {
        Ok(json) => json,
        Err(err) => return err_report(plan, &err.to_string()),
    };

    let child = Command::new("pkexec")
        .arg(HELPER_PATH)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(err) => return err_report(plan, &format!("pkexec spawn failed: {err}")),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(json.as_bytes());
    }

    match child.wait_with_output() {
        Ok(out) => serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
            err_report(
                plan,
                "helper returned no report (authentication cancelled?)",
            )
        }),
        Err(err) => err_report(plan, &err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_ipc::ItemKind;
    use std::cell::Cell;

    fn item(path: PathBuf, size: u64, root: bool) -> ScanItem {
        ScanItem {
            id: path.to_string_lossy().into_owned(),
            path,
            size_bytes: size,
            last_access: None,
            kind: ItemKind::File,
            requires_root: root,
        }
    }

    #[test]
    fn routes_user_to_disk_and_root_to_helper() {
        let home = tempfile::tempdir().unwrap();
        let f = home.path().join("cache/junk.bin");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, vec![0u8; 20]).unwrap();

        let plan = DeletionPlan {
            items: vec![
                item(f.clone(), 20, false),
                item(PathBuf::from("/tmp/whatever"), 5, true),
            ],
            destination: Destination::Permanent,
            total_bytes: 25,
            requires_root: true,
        };

        let called = Cell::new(false);
        let report = execute_plan(&plan, home.path(), |p| {
            called.set(true);
            assert!(
                p.items.iter().all(|i| i.requires_root),
                "only root items go to helper"
            );
            ExecutionReport {
                freed_bytes: 5,
                deleted_count: 1,
                errors: vec![],
            }
        });

        assert!(called.get(), "root items must be routed to the helper");
        assert!(!f.exists(), "user item must be deleted");
        assert_eq!(report.deleted_count, 2);
        assert_eq!(report.freed_bytes, 25);
    }

    #[test]
    fn user_item_outside_home_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let victim = other.path().join("keep.txt");
        std::fs::write(&victim, b"important").unwrap();

        let plan = DeletionPlan {
            items: vec![item(victim.clone(), 1, false)],
            destination: Destination::Permanent,
            total_bytes: 1,
            requires_root: false,
        };

        let report = execute_plan(&plan, home.path(), |_| ExecutionReport::default());
        assert_eq!(report.deleted_count, 0);
        assert!(victim.exists(), "a file outside home must never be deleted");
        assert_eq!(report.errors.len(), 1);
    }
}
