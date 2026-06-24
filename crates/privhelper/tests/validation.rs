// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for the privileged helper: it must refuse any path outside
//! its hard-coded root zones, and only act on in-zone temp paths.

use std::io::Write;
use std::process::{Command, Stdio};

const HELPER: &str = env!("CARGO_BIN_EXE_freeyourdisk-helper");

fn run(plan_json: &str) -> (Option<i32>, serde_json::Value) {
    let mut child = Command::new(HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn helper");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(plan_json.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let report = serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
    (out.status.code(), report)
}

#[test]
fn refuses_path_outside_root_zones() {
    // /home/... is outside the hard-coded root zones (/tmp, /var/tmp).
    let plan = r#"{"items":[{"id":"1","path":"/home/freeyourdisk-out-of-zone","size_bytes":10,"last_access":null,"kind":"file","requires_root":true}],"destination":"permanent","total_bytes":10,"requires_root":true}"#;
    let (code, report) = run(plan);
    assert_eq!(
        code,
        Some(3),
        "out-of-zone path must be refused with exit 3"
    );
    assert_eq!(report["deleted_count"], 0);
    assert!(report["errors"]
        .as_array()
        .map(|e| !e.is_empty())
        .unwrap_or(false));
}

#[test]
fn rejects_invalid_input() {
    let (code, _) = run("not json at all");
    assert_eq!(code, Some(2), "invalid input must exit with code 2");
}

#[test]
fn deletes_in_zone_temp_file() {
    let path =
        std::path::PathBuf::from("/tmp").join(format!("fyd-helper-it-{}", std::process::id()));
    std::fs::write(&path, b"temp").unwrap();

    let plan = format!(
        r#"{{"items":[{{"id":"1","path":"{}","size_bytes":4,"last_access":null,"kind":"file","requires_root":true}}],"destination":"permanent","total_bytes":4,"requires_root":true}}"#,
        path.display()
    );
    let (code, report) = run(&plan);
    assert_eq!(code, Some(0), "in-zone temp file must be accepted");
    assert_eq!(report["deleted_count"], 1);
    assert!(!path.exists(), "the temp file must be deleted");
}
