# Windows Port — Phase 2: Privilege Elevation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

> **Plan series.** Phase **2 of 8**. Phases 0–1 merged to `master`. Spec:
> `docs/superpowers/specs/2026-06-26-windows-port-design.md` §4. Branch: `feat/win-phase-2`.

**Goal:** Privileged cleanup (root-owned items, e.g. `%WINDIR%\Temp`) works on Windows: the un-elevated UI relaunches itself **headless + elevated** to execute the validated plan, with one UAC prompt per batch.

**Architecture:** Mirror the macOS osascript-admin pattern with PowerShell instead of raw WinAPI. The un-elevated app stages the root-only `DeletionPlan` to a temp file and relaunches its own exe via `powershell Start-Process -Verb RunAs -Wait` in a new `--apply` headless mode. The elevated child re-validates the plan against hard-coded Windows root zones and deletes, writing a report file the parent reads. Validation/execution is unified in a shared `core_trash::execute_root_plan` used by BOTH the Linux/macOS `privhelper` binary and the Windows in-process executor — single source of truth for the allowlist. No `unsafe`, no `windows` crate, no path-with-spaces in arguments (only the parent PID is passed; both sides derive `%TEMP%\fyd-apply-<pid>-{plan,report}.json`).

**Tech Stack:** Rust, `core-trash`/`core-ipc`, `std::process::Command` (powershell), filesystem IPC.

## Global Constraints

- **Target:** Windows 10 (1803+)/11 x64. Do NOT regress Linux or macOS.
- **License:** keep `// SPDX-License-Identifier: GPL-3.0-or-later` on every edited/new file.
- **cfg rule:** split any touched `#[cfg(not(target_os="macos"))]` into explicit `linux`/`windows` arms.
- **Security:** the elevated child re-validates EVERY path against hard-coded root zones and refuses the whole batch on any escape (all-or-nothing) — identical guarantee to `privhelper`. The shared `execute_root_plan` enforces this; callers must not bypass it.
- **No new crate dependency** in this phase (elevation via `powershell`, not the `windows` crate).
- **Elevation arg:** pass ONLY the parent PID (numeric, no spaces). Both parent and child derive the temp file paths from it. Single-quote the exe path in the PowerShell command (doubling embedded single quotes).
- **Verification:** MANDATORY local gate per task = `cargo fmt --all --check` + `cargo clippy -p freeyourdisk --all-targets -- -D warnings` + `cargo test -p core-trash -p freeyourdisk-helper` GREEN. Windows-only code (`#[cfg(target_os="windows")]`) is NOT compiled locally — it is authoritatively gated by the `windows` CI job on the PR; write it carefully.

---

### Task 1: Shared `execute_root_plan` in core-trash; refactor privhelper to use it

Unify the "validate every path against root zones (all-or-nothing) then delete" logic so the Linux/macOS helper and the future Windows executor share one implementation.

**Files:**
- Modify: `crates/core-trash/src/lib.rs` (add `execute_root_plan`; it already exports `validate`, `to_trash`, `delete_permanent`, `Zones`)
- Modify: `crates/core-trash/Cargo.toml` (confirm `core-ipc` dep — it is already used via `ExecutionReport` return types; add it only if missing)
- Modify: `crates/privhelper/src/main.rs:251-281` (call the shared fn)

**Interfaces:**
- Produces: `core_trash::execute_root_plan(plan: &core_ipc::DeletionPlan, zones: &Zones) -> Result<core_ipc::ExecutionReport, core_ipc::ExecutionReport>` — `Ok` = executed report; `Err` = refusal report (nothing deleted, all-or-nothing). Consumed by privhelper (Task 1) and the Windows executor (Task 2).

- [ ] **Step 1: Confirm core-trash depends on core-ipc**

Run: `grep -n "core-ipc" crates/core-trash/Cargo.toml`
Expected: a `core-ipc = { path = "../core-ipc" }` line. If absent, add it under `[dependencies]` (it is needed for `DeletionPlan`/`Destination`/`ExecutionReport`/`ItemError`). (The crate already returns `ExecutionReport` from `to_trash`, so it is almost certainly present.)

- [ ] **Step 2: Write a failing test for `execute_root_plan`**

Add to the `#[cfg(test)] mod tests` in `crates/core-trash/src/lib.rs`:

```rust
    #[test]
    fn execute_root_plan_refuses_whole_batch_on_escape() {
        let zone = tempfile::tempdir().unwrap();
        let inside = zone.path().join("junk.tmp");
        std::fs::write(&inside, b"x").unwrap();
        let outside = tempfile::tempdir().unwrap();
        let escape = outside.path().join("keep.txt");
        std::fs::write(&escape, b"important").unwrap();

        let zones = Zones(vec![zone.path().to_path_buf()]);
        let plan = core_ipc::DeletionPlan {
            items: vec![
                core_ipc::ScanItem {
                    id: inside.to_string_lossy().into_owned(),
                    path: inside.clone(),
                    size_bytes: 1,
                    last_access: None,
                    kind: core_ipc::ItemKind::File,
                    requires_root: true,
                },
                core_ipc::ScanItem {
                    id: escape.to_string_lossy().into_owned(),
                    path: escape.clone(),
                    size_bytes: 9,
                    last_access: None,
                    kind: core_ipc::ItemKind::File,
                    requires_root: true,
                },
            ],
            destination: core_ipc::Destination::Permanent,
            total_bytes: 10,
            requires_root: true,
        };

        let result = execute_root_plan(&plan, &zones);
        assert!(result.is_err(), "any escape must refuse the whole batch");
        assert!(inside.exists(), "nothing deleted on refusal");
        assert!(escape.exists(), "the out-of-zone file must never be touched");
    }

    #[test]
    fn execute_root_plan_deletes_when_all_in_zone() {
        let zone = tempfile::tempdir().unwrap();
        let f = zone.path().join("junk.tmp");
        std::fs::write(&f, vec![0u8; 50]).unwrap();
        let zones = Zones(vec![zone.path().to_path_buf()]);
        let plan = core_ipc::DeletionPlan {
            items: vec![core_ipc::ScanItem {
                id: f.to_string_lossy().into_owned(),
                path: f.clone(),
                size_bytes: 50,
                last_access: None,
                kind: core_ipc::ItemKind::File,
                requires_root: true,
            }],
            destination: core_ipc::Destination::Permanent,
            total_bytes: 50,
            requires_root: true,
        };
        let result = execute_root_plan(&plan, &zones);
        assert!(result.is_ok());
        assert!(!f.exists(), "in-zone file deleted");
    }
```

(If `core_ipc` test types need importing, add `use core_ipc::...` at the top of the test module or qualify as above.)

- [ ] **Step 3: Run the test — expect FAIL (function not defined)**

Run: `cargo test -p core-trash execute_root_plan 2>&1 | tail -15`
Expected: compile error `cannot find function execute_root_plan`.

- [ ] **Step 4: Implement `execute_root_plan`**

Add to `crates/core-trash/src/lib.rs` (public API, near `to_trash`/`delete_permanent`). Adjust the `use`/paths to match the crate's existing imports:

```rust
/// Validate every path in `plan` against `zones` (all-or-nothing) and, if none
/// escape, execute the deletion. `Ok` = executed report; `Err` = refusal report
/// (nothing deleted). Shared by the privileged helper (Linux/macOS) and the
/// Windows elevated executor so the allowlist has a single source of truth.
pub fn execute_root_plan(
    plan: &core_ipc::DeletionPlan,
    zones: &Zones,
) -> Result<core_ipc::ExecutionReport, core_ipc::ExecutionReport> {
    use std::path::PathBuf;
    let paths: Vec<PathBuf> = plan.items.iter().map(|item| item.path.clone()).collect();

    let refusals: Vec<core_ipc::ItemError> = paths
        .iter()
        .filter_map(|path| match validate(path, zones) {
            Ok(_) => None,
            Err(err) => Some(core_ipc::ItemError {
                path: path.clone(),
                message: err.to_string(),
            }),
        })
        .collect();

    if !refusals.is_empty() {
        return Err(core_ipc::ExecutionReport {
            freed_bytes: 0,
            deleted_count: 0,
            errors: refusals,
        });
    }

    let report = match plan.destination {
        core_ipc::Destination::Trash => to_trash(&paths, zones),
        core_ipc::Destination::Permanent => delete_permanent(&paths, zones),
    };
    Ok(report)
}
```

- [ ] **Step 5: Run the tests — expect PASS**

Run: `cargo test -p core-trash 2>&1 | tail -15`
Expected: all pass, including the two new tests.

- [ ] **Step 6: Refactor privhelper to call the shared fn (preserve exit codes)**

In `crates/privhelper/src/main.rs`, replace the block from `let zones = Zones(...)` through the end of `main` (lines ~251-280, the pre-validate loop + execute) with:

```rust
    let zones = Zones(ROOT_ZONES.iter().map(PathBuf::from).collect());
    match core_trash::execute_root_plan(&plan, &zones) {
        Ok(report) => {
            write_report(&report);
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            write_report(&refusal);
            ExitCode::from(3)
        }
    }
```

Remove now-unused imports (`validate`, `Destination`, `ItemError`, `to_trash`, `delete_permanent` if no longer referenced elsewhere in the file — `smart`/`install-deps` paths don't use them). Let clippy guide which imports to drop.

- [ ] **Step 7: Verify privhelper + workspace**

Run: `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings && cargo test -p core-trash -p freeyourdisk-helper 2>&1 | tail -20`
Expected: GREEN; existing privhelper validation tests still pass (behavior unchanged: same stdout report, exit 3 on refusal, 0 on success). If clippy errors about the Tauri dist, run `pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build` once.

- [ ] **Step 8: Commit**

```bash
git add crates/core-trash/src/lib.rs crates/core-trash/Cargo.toml crates/privhelper/src/main.rs
git commit -m "feat: core_trash::execute_root_plan shared validator; privhelper uses it"
```

---

### Task 2: Windows elevated `--apply` executor in main.rs

The callee side: a new headless mode the elevated child runs. It reads the staged plan, executes it against hard-coded Windows root zones via `execute_root_plan`, and writes the report. No WebView is created (arg dispatch happens before `tauri::Builder`).

**Files:**
- Modify: `src-tauri/src/main.rs` (add `--apply` dispatch before the Tauri builder)
- Modify: `src-tauri/src/headless.rs` (add the `apply_elevated` function) OR add a small `src-tauri/src/elevated.rs` module. Use `headless.rs` to keep related code together.

**Interfaces:**
- Consumes: `core_trash::execute_root_plan` (Task 1), `core_ipc::DeletionPlan`.
- Produces: `headless::apply_elevated(token: &str) -> i32` (process exit code). `main` calls it for `--apply <token>` on Windows.

- [ ] **Step 1: Add `apply_elevated` to headless.rs**

Add to `src-tauri/src/headless.rs` (Windows-only):

```rust
/// Windows: the elevated child. Reads the plan staged by the un-elevated parent
/// at `%TEMP%\fyd-apply-<token>-plan.json`, re-validates against the hard-coded
/// Windows root zone (`%WINDIR%\Temp`), deletes, and writes the report to
/// `%TEMP%\fyd-apply-<token>-report.json`. `token` is the parent PID (no spaces).
#[cfg(target_os = "windows")]
pub fn apply_elevated(token: &str) -> i32 {
    use core_trash::Zones;
    let tmp = std::env::temp_dir();
    let plan_path = tmp.join(format!("fyd-apply-{token}-plan.json"));
    let report_path = tmp.join(format!("fyd-apply-{token}-report.json"));

    let Ok(raw) = std::fs::read_to_string(&plan_path) else {
        return 2;
    };
    let Ok(plan) = serde_json::from_str::<core_ipc::DeletionPlan>(&raw) else {
        return 2;
    };

    // Hard-coded Windows privileged zone (must match temp.rs's requires_root root).
    let windir = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("C:\\Windows"));
    let zones = Zones(vec![windir.join("Temp")]);

    let report = match core_trash::execute_root_plan(&plan, &zones) {
        Ok(report) => report,
        Err(refusal) => refusal,
    };
    let json = serde_json::to_string(&report).unwrap_or_default();
    if std::fs::write(&report_path, json).is_err() {
        return 1;
    }
    0
}
```

Ensure `core_ipc` and `core_trash` are usable from `src-tauri` (they are workspace deps of `freeyourdisk` already). Add `use core_ipc;` only if needed (the fully-qualified `core_ipc::DeletionPlan` above avoids an import).

- [ ] **Step 2: Dispatch `--apply` in main.rs before the Tauri builder**

In `src-tauri/src/main.rs`, the `main()` starts with the `--headless` check. Add the Windows `--apply` dispatch right after it:

```rust
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--headless") {
        std::process::exit(headless::run(&args));
    }
    #[cfg(target_os = "windows")]
    if let Some(token) = args.iter().position(|a| a == "--apply").and_then(|i| args.get(i + 1)) {
        std::process::exit(headless::apply_elevated(token));
    }
```

(Place the `#[cfg(target_os = "windows")] if let ...` block immediately after the existing `--headless` block, before `taskmgr::raise_priority();`.)

- [ ] **Step 3: Verify**

Run: `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: GREEN. (The `apply_elevated` fn is `#[cfg(target_os="windows")]` so it is not compiled on Linux; the `main.rs` dispatch is also cfg-gated. Read both carefully — only CI compiles them.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/headless.rs src-tauri/src/main.rs
git commit -m "feat(win): elevated --apply executor (headless child runs the validated root plan)"
```

---

### Task 3: Windows elevation in execute.rs (PowerShell RunAs)

The caller side: replace the Linux `pkexec` helper invocation with a Windows arm that relaunches the app elevated via PowerShell and reads the report file. Split the touched `#[cfg(not(target_os="macos"))]` functions into `linux` + `windows`.

**Files:**
- Modify: `src-tauri/src/execute.rs` (`pkexec_helper`, `pkexec_smart` — split `not(macos)` → `linux`/`windows`)

**Interfaces:**
- Consumes: the elevated `--apply` executor (Task 2) via self-relaunch; `execute::execute_plan`'s `invoke_helper` closure already routes the root plan here.
- Produces: `#[cfg(target_os="windows")] pub fn pkexec_helper(plan: &DeletionPlan) -> ExecutionReport` and a `#[cfg(target_os="windows")] pub fn pkexec_smart(devices: &[String]) -> Vec<SmartInfo>` (stub — real Windows SMART is Phase 3). Same signatures as the Linux/macOS arms so `commands.rs` callers are unchanged.

- [ ] **Step 1: Split `pkexec_helper` — change the Linux attribute and add the Windows arm**

In `src-tauri/src/execute.rs`, change the existing `#[cfg(not(target_os = "macos"))]` on `pub fn pkexec_helper` (the pkexec version, ~line 117) to `#[cfg(target_os = "linux")]`. Then add this Windows arm immediately after it:

```rust
/// Windows: relaunch THIS exe elevated (UAC) in headless `--apply` mode to run
/// the root plan. Only the parent PID is passed as an argument (no spaces); both
/// sides derive `%TEMP%\fyd-apply-<pid>-{plan,report}.json`. Elevation uses
/// `powershell Start-Process -Verb RunAs -Wait` — the WinAPI-free analogue of the
/// macOS osascript-admin path.
#[cfg(target_os = "windows")]
pub fn pkexec_helper(plan: &DeletionPlan) -> ExecutionReport {
    let json = match serde_json::to_string(plan) {
        Ok(json) => json,
        Err(err) => return err_report(plan, &err.to_string()),
    };
    let token = std::process::id().to_string();
    let tmp = std::env::temp_dir();
    let plan_path = tmp.join(format!("fyd-apply-{token}-plan.json"));
    let report_path = tmp.join(format!("fyd-apply-{token}-report.json"));
    let _ = std::fs::remove_file(&report_path);
    if std::fs::write(&plan_path, &json).is_err() {
        return err_report(plan, "failed to stage deletion plan");
    }

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            let _ = std::fs::remove_file(&plan_path);
            return err_report(plan, &format!("cannot locate exe: {err}"));
        }
    };
    // Single-quote the exe path for PowerShell, doubling any embedded single quote.
    let exe_ps = exe.to_string_lossy().replace('\'', "''");
    let ps = format!(
        "Start-Process -FilePath '{exe_ps}' -ArgumentList '--apply','{token}' -Verb RunAs -Wait -WindowStyle Hidden"
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status();

    let result = match status {
        Ok(s) if s.success() => std::fs::read_to_string(&report_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(|| err_report(plan, "elevated helper returned no report")),
        Ok(_) => err_report(plan, "elevation cancelled or failed"),
        Err(err) => err_report(plan, &format!("failed to launch elevation: {err}")),
    };
    let _ = std::fs::remove_file(&plan_path);
    let _ = std::fs::remove_file(&report_path);
    result
}
```

- [ ] **Step 2: Split `pkexec_smart` — Linux arm + Windows stub**

Change the existing `#[cfg(not(target_os = "macos"))]` on `pub fn pkexec_smart` (~line 153) to `#[cfg(target_os = "linux")]`. Add after it:

```rust
/// Windows SMART is implemented in Phase 3 (bundled smartctl.exe via the elevated
/// executor). Until then, return no SMART data (the UI degrades gracefully).
#[cfg(target_os = "windows")]
pub fn pkexec_smart(_devices: &[String]) -> Vec<SmartInfo> {
    Vec::new()
}
```

- [ ] **Step 3: Check the `InstallReport` import and `pkexec_install_deps`**

`pkexec_install_deps` and the top `use core_ipc::InstallReport;` are `#[cfg(not(target_os = "macos"))]`. They compile on Windows (Command-based) and are only reached at runtime when `detect_manager()` returns `Some`, which is `None` on Windows — so they are inert on Windows. Leave them as `not(macos)` for this phase (Windows SMART-tool install is Phase 3). Do NOT change them. Confirm via clippy that nothing references a now-missing symbol.

- [ ] **Step 4: Verify**

Run: `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: GREEN on Linux (the Windows `pkexec_helper`/`pkexec_smart` arms are cfg-gated and not compiled here; the Linux arms are unchanged in behavior). Read the Windows arm carefully — CI compiles it.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/execute.rs
git commit -m "feat(win): elevate via PowerShell RunAs self-relaunch (--apply); SMART stub for Phase 3"
```

---

## Self-Review

**1. Spec coverage (Phase 2 = spec §4 elevation):**
- Whole-process elevation realized as headless `--apply` self-relaunch (no separate helper on Windows) — Tasks 2+3. ✓
- UI never elevated; elevated child is headless (arg dispatch before Tauri builder) — Task 2. ✓
- Plan staged to temp file; report read back (pipes don't cross UAC) — Task 3. ✓
- One UAC per privileged batch (`Start-Process -Verb RunAs -Wait`) — Task 3. ✓
- Single source of truth for the allowlist (`execute_root_plan`, used by privhelper + Windows executor) — Task 1. ✓
- `privhelper` not bundled on Windows: unchanged (it's a separate binary built only for Linux/macOS bundles; not invoked on Windows). ✓

**2. Placeholder scan:** No TBD. `pkexec_smart` Windows returns `Vec::new()` — a deliberate Phase-3 stub (documented), not a placeholder.

**3. Type consistency:** `execute_root_plan(&DeletionPlan, &Zones) -> Result<ExecutionReport, ExecutionReport>` (Task 1) is consumed by privhelper (Task 1 Step 6) and `apply_elevated` (Task 2). `pkexec_helper(&DeletionPlan) -> ExecutionReport` / `pkexec_smart(&[String]) -> Vec<SmartInfo>` keep identical signatures across all OS arms, so `execute_plan` and `commands.rs` callers are unchanged. The parent (Task 3) and child (Task 2) agree on the temp path format `fyd-apply-<token>-{plan,report}.json` and the token = parent PID.

## Notes for later phases
- Phase 3: real Windows SMART — bundle `smartctl.exe`, add a `--smart` elevated mode (same self-relaunch pattern), implement the Windows `pkexec_smart` arm.
- Assumption: elevated child shares the parent's `%TEMP%` (same-user UAC elevation — the standard case). If a separate admin account is used, the temp dir would differ; revisit if reported.
