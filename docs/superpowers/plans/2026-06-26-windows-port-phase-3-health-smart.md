# Windows Port — Phase 3: Disk Health & SMART — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

> **Plan series.** Phase **3 of 8**. Phases 0–2 merged. Spec §5.4/§5.5. Branch: `feat/win-phase-3`.

**Goal:** The Disk-health tab works on Windows: real disk list + host uptime (via `sysinfo`), and SMART via `smartctl` read by the elevated executor — with one-click `winget install smartmontools` when it's missing.

**Architecture:** Disk enumeration and uptime come from the already-present cross-platform `sysinfo` crate (no new deps; mirrors the macOS pattern of `read_bytes/write_bytes = 0`). SMART is read by `smartctl.exe` (smartmontools — covers NVMe on Windows via IOCTL, so `nvme-cli` is irrelevant), invoked by the same UAC-elevated headless self-relaunch built in Phase 2 — a new `--smart` mode alongside `--apply`. `smartctl` is detected (PATH + the winget install dir) and installed via `winget` (no bundling). Throughput stays `0` (PDH deferred, like macOS).

**Tech Stack:** Rust, `sysinfo` (existing), `smartctl` (external, winget), PowerShell RunAs (Phase-2 pattern), `core_ipc::SmartInfo`.

## Global Constraints

- **Target:** Windows 10 (1803+)/11 x64. Do NOT regress Linux or macOS.
- **License:** keep `// SPDX-License-Identifier: GPL-3.0-or-later`.
- **cfg rule:** split touched `#[cfg(not(target_os="macos"))]` into explicit `linux`/`windows` arms.
- **No new crate dependency** (sysinfo already present; elevation reuses Phase-2 PowerShell pattern; no `windows` crate).
- **SMART device tokens** come from `smartctl --scan-open` inside the elevated child (NOT from `sysinfo` mount points) — Windows smartctl uses its own `/dev/sdN`/`/dev/nvmeN` tokens.
- **Security:** the elevated `--smart` child runs ONLY `smartctl` (read-only) with device tokens it discovered itself via `--scan-open`; it never accepts a path/device from the un-elevated parent (the parent passes only the numeric PID, same as `--apply`). smartctl.exe is resolved from a fixed install path or PATH; prefer the fixed `C:\Program Files\smartmontools\bin\smartctl.exe`.
- **Verification:** per task = `cargo fmt --all --check` + `cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN. Windows-only arms are CI-compile-gated; SMART runtime is manual-smoke (no smartctl/real disk on CI, same as macOS).

---

### Task 1: Windows disk enumeration + uptime via sysinfo

**Files:** Modify `src-tauri/src/health.rs` (`host_uptime_secs`, the `platform` module).

**Interfaces:** Produces `health::disks() -> Vec<DiskInfo>` and `health::host_uptime_secs() -> u64` working on Windows. `DiskInfo` shape unchanged.

- [ ] **Step 1: Windows `host_uptime_secs`**

In `health.rs`, change the `#[cfg(not(target_os = "macos"))]` on `host_uptime_secs` (the `/proc/uptime` version) to `#[cfg(target_os = "linux")]`. Add a Windows arm (identical to the macOS one):

```rust
#[cfg(target_os = "windows")]
pub fn host_uptime_secs() -> u64 {
    sysinfo::System::uptime()
}
```

- [ ] **Step 2: Windows `platform::disks()`**

Change the `#[cfg(not(target_os = "macos"))]` on the Linux `mod platform` to `#[cfg(target_os = "linux")]`. Add a Windows `platform` module after the macOS one:

```rust
// ---------------------------------------------------------------------------
// Windows: sysinfo (model/rotational not exposed; throughput deferred = 0).
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
mod platform {
    use super::DiskInfo;
    use sysinfo::Disks;

    pub fn disks() -> Vec<DiskInfo> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for disk in Disks::new_with_refreshed_list().iter() {
            // sysinfo lists volumes; dedupe by device name, keep physical-ish.
            let name = disk.name().to_string_lossy().into_owned();
            let device = if name.is_empty() {
                disk.mount_point().to_string_lossy().into_owned()
            } else {
                name
            };
            if !seen.insert(device.clone()) {
                continue;
            }
            out.push(DiskInfo {
                device,
                model: None,
                size_bytes: disk.total_space(),
                rotational: false,
                read_bytes: 0,
                write_bytes: 0,
            });
        }
        out
    }
}
```

- [ ] **Step 3: Verify** — `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN. (Windows `platform` is cfg-gated; read carefully. If clippy errors on `../ui/dist`, build the frontend once.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/health.rs
git commit -m "feat(win): disk enumeration + uptime via sysinfo (health tab)"
```

---

### Task 2: smartdeps Windows detection + winget install

**Files:** Modify `src-tauri/src/smartdeps.rs` (`has_binary`, `detect_manager`, `status`), `src-tauri/src/commands.rs` (`install_smart_deps`), `src-tauri/src/execute.rs` (a Windows install path, no pkexec).

**Interfaces:** Produces `smartdeps::status()` reporting `smartctl_installed` + `manager = Some("winget")` on Windows; `install_smart_deps` runs `winget install smartmontools.smartmontools`.

- [ ] **Step 1: `has_binary` Windows search paths**

In `smartdeps.rs::has_binary`, the extra-dirs list is currently POSIX. Add Windows locations under a cfg, and check `bin.exe` too. Replace the `for extra in [...]` block with a cfg-aware one:

```rust
    #[cfg(not(target_os = "windows"))]
    let extra = [
        "/usr/bin", "/bin", "/usr/sbin", "/sbin", "/usr/local/bin",
        "/usr/local/sbin", "/opt/homebrew/bin", "/opt/homebrew/sbin",
    ];
    #[cfg(target_os = "windows")]
    let extra = [
        "C:\\Program Files\\smartmontools\\bin",
        "C:\\Program Files (x86)\\smartmontools\\bin",
    ];
    for dir in extra {
        let pb = PathBuf::from(dir);
        if !dirs.contains(&pb) {
            dirs.push(pb);
        }
    }
    // On Windows, executables carry a .exe suffix.
    #[cfg(target_os = "windows")]
    let found = dirs.iter().any(|d| d.join(format!("{bin}.exe")).is_file());
    #[cfg(not(target_os = "windows"))]
    let found = dirs.iter().any(|d| d.join(bin).is_file());
    found
}
```

(Replace the existing trailing `dirs.iter().any(...)` accordingly; keep the PATH collection above unchanged.)

- [ ] **Step 2: `detect_manager` Windows arm**

Change the `#[cfg(not(target_os = "macos"))]` on `detect_manager` to `#[cfg(target_os = "linux")]`. Add:

```rust
/// Windows uses winget (App Installer, present on Win10 1809+/Win11).
#[cfg(target_os = "windows")]
pub fn detect_manager() -> Option<String> {
    if has_binary("winget") {
        Some("winget".to_string())
    } else {
        None
    }
}
```

- [ ] **Step 3: `status()` Windows needs-detection**

In `status()`, the `(nvme_needed, sata_needed)` cfg currently has macOS + `not(macos)`. Change `#[cfg(not(target_os = "macos"))]` to `#[cfg(target_os = "linux")]` and add a Windows arm (smartctl covers NVMe on Windows, so no separate nvme tool; flag smartmontools if any disk is present):

```rust
    #[cfg(target_os = "windows")]
    let (nvme_needed, sata_needed) = (false, !disks.is_empty());
```

(Leave the `nvme_installed`/`smartctl_installed`/`missing`/`can_install` logic as-is — on Windows `nvme_needed=false` so only `smartmontools` can be missing.)

- [ ] **Step 4: `install_smart_deps` Windows arm (commands.rs)**

In `commands.rs::install_smart_deps`, the body has `#[cfg(target_os = "macos")]` (brew) + `#[cfg(not(target_os = "macos"))]` (pkexec). Change the `not(macos)` to `#[cfg(target_os = "linux")]` and add:

```rust
        #[cfg(target_os = "windows")]
        {
            execute::winget_install_smart()
        }
```

- [ ] **Step 5: `execute::winget_install_smart` (execute.rs)**

Add to `execute.rs` (Windows-only):

```rust
/// Windows: install smartmontools via winget. `--id` is a fixed allowlisted
/// package; nothing user-controlled reaches the command line.
#[cfg(target_os = "windows")]
pub fn winget_install_smart() -> InstallReport {
    let out = Command::new("winget")
        .args([
            "install", "--id", "smartmontools.smartmontools",
            "--accept-source-agreements", "--accept-package-agreements",
            "--silent",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => InstallReport {
            success: true,
            message: "Installed: smartmontools".to_string(),
        },
        Ok(o) => InstallReport {
            success: false,
            message: String::from_utf8_lossy(&o.stderr)
                .lines()
                .last()
                .unwrap_or("winget install failed")
                .to_string(),
        },
        Err(err) => InstallReport {
            success: false,
            message: format!("failed to run winget: {err}"),
        },
    }
}
```

The `use core_ipc::InstallReport;` import is currently `#[cfg(not(target_os = "macos"))]` — on Windows it is already in scope (not(macos) includes windows). Leave it.

- [ ] **Step 6: Verify** — `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/smartdeps.rs src-tauri/src/commands.rs src-tauri/src/execute.rs
git commit -m "feat(win): smartctl detection + winget install of smartmontools"
```

---

### Task 3: SMART read via elevated `--smart` executor

**Files:** Modify `src-tauri/src/headless.rs` (add `read_smart_elevated`), `src-tauri/src/main.rs` (dispatch `--smart`), `src-tauri/src/execute.rs` (Windows `pkexec_smart` real impl).

**Interfaces:** Produces a working `execute::pkexec_smart(&[String]) -> Vec<SmartInfo>` on Windows (devices arg ignored — the elevated child self-discovers via `smartctl --scan-open`). Reuses the Phase-2 elevation IPC (numeric PID token, `%TEMP%\fyd-smart-<pid>-report.json`).

- [ ] **Step 1: `read_smart_elevated` in headless.rs**

Add (Windows-only). It resolves smartctl, scans devices, reads each, writes a `Vec<SmartInfo>` JSON report:

```rust
/// Windows: the elevated SMART reader. Discovers devices with
/// `smartctl --scan-open` and reads each with `smartctl -a -j`, writing a
/// Vec<SmartInfo> to %TEMP%\fyd-smart-<token>-report.json. `token` = parent PID.
#[cfg(target_os = "windows")]
pub fn read_smart_elevated(token: &str) -> i32 {
    use core_ipc::SmartInfo;
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return 2;
    }
    let report_path =
        std::env::temp_dir().join(format!("fyd-smart-{token}-report.json"));

    // Fixed install path first, then PATH.
    let smartctl = {
        let fixed = std::path::Path::new(
            "C:\\Program Files\\smartmontools\\bin\\smartctl.exe",
        );
        if fixed.is_file() {
            fixed.to_string_lossy().into_owned()
        } else {
            "smartctl".to_string()
        }
    };

    let scan = std::process::Command::new(&smartctl)
        .args(["--scan-open", "-j"])
        .output();
    let mut results: Vec<SmartInfo> = Vec::new();
    if let Ok(out) = scan {
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            if let Some(devices) = json.get("devices").and_then(|d| d.as_array()) {
                for dev in devices {
                    let Some(name) = dev.get("name").and_then(|n| n.as_str()) else {
                        continue;
                    };
                    results.push(read_one_smart(&smartctl, name));
                }
            }
        }
    }
    let json = serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string());
    if std::fs::write(&report_path, json).is_err() {
        return 1;
    }
    0
}

/// Read SMART for one device token via `smartctl -a -j`.
#[cfg(target_os = "windows")]
fn read_one_smart(smartctl: &str, device: &str) -> core_ipc::SmartInfo {
    use core_ipc::SmartInfo;
    let unavailable = SmartInfo {
        device: device.to_string(),
        available: false,
        passed: None,
        power_on_hours: None,
        temperature_c: None,
    };
    let Ok(out) = std::process::Command::new(smartctl)
        .args(["-a", "-j", device])
        .output()
    else {
        return unavailable;
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return unavailable;
    };
    let passed = json
        .get("smart_status")
        .and_then(|s| s.get("passed"))
        .and_then(|v| v.as_bool());
    let power_on_hours = json
        .get("power_on_time")
        .and_then(|p| p.get("hours"))
        .and_then(|v| v.as_u64());
    let temperature_c = json
        .get("temperature")
        .and_then(|t| t.get("current"))
        .and_then(|v| v.as_i64());
    let available = passed.is_some() || power_on_hours.is_some() || temperature_c.is_some();
    SmartInfo {
        device: device.to_string(),
        available,
        passed,
        power_on_hours,
        temperature_c,
    }
}
```

- [ ] **Step 2: Dispatch `--smart` in main.rs**

After the `--apply` block in `main()`, add:

```rust
    #[cfg(target_os = "windows")]
    if args.iter().any(|a| a == "--smart") {
        let token = args
            .iter()
            .position(|a| a == "--smart")
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
            .unwrap_or("");
        std::process::exit(headless::read_smart_elevated(token));
    }
```

- [ ] **Step 3: Real Windows `pkexec_smart` in execute.rs**

Replace the Windows `pkexec_smart` stub (returns `Vec::new()`) with the elevated reader (mirror the `pkexec_helper` PowerShell pattern, `--smart` verb, read `fyd-smart-<pid>-report.json`):

```rust
#[cfg(target_os = "windows")]
pub fn pkexec_smart(_devices: &[String]) -> Vec<SmartInfo> {
    // The elevated child self-discovers devices via `smartctl --scan-open`, so
    // the caller's device list is unused. One UAC prompt; report read from file.
    let token = std::process::id().to_string();
    let report_path =
        std::env::temp_dir().join(format!("fyd-smart-{token}-report.json"));
    let _ = std::fs::remove_file(&report_path);

    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let exe_ps = exe.to_string_lossy().replace('\'', "''");
    let ps = format!(
        "Start-Process -FilePath '{exe_ps}' -ArgumentList '--smart','{token}' -Verb RunAs -Wait -WindowStyle Hidden"
    );
    let status = Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status();
    let result = match status {
        Ok(s) if s.success() => std::fs::read_to_string(&report_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let _ = std::fs::remove_file(&report_path);
    result
}
```

- [ ] **Step 4: Verify** — `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN. (All Windows-only; CI-compile-gated. SMART runtime is manual-smoke.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/headless.rs src-tauri/src/main.rs src-tauri/src/execute.rs
git commit -m "feat(win): SMART read via elevated smartctl (--smart executor)"
```

---

## Self-Review

**Spec coverage (§5.4/§5.5):** disk enum + uptime via sysinfo (Task 1); smartctl detection + winget install, nvme-cli not needed (Task 2); SMART read via elevated smartctl (Task 3). Throughput = 0 (PDH deferred, documented, matches macOS). `is_physical_disk` not needed on Windows (sysinfo lists real volumes; deduped by device).

**Placeholder scan:** none. The deferred PDH throughput is an explicit, documented degradation (returns 0), not a placeholder.

**Type consistency:** `pkexec_smart(&[String]) -> Vec<SmartInfo>` signature identical across OSes (callers unchanged). `read_smart_elevated`/`read_one_smart` produce `core_ipc::SmartInfo`. The `--smart` elevation reuses the Phase-2 numeric-PID token + `%TEMP%` report-file IPC; security guard (digit-only token) replicated.

## Notes for later
- Throughput (PDH `\PhysicalDisk(*)\Disk R/W Bytes/sec`) deferred — revisit if the live graph matters on Windows.
- SMART read is manual-smoke only (no smartctl/real disk on CI; same as macOS).
- Device model/rotational via WMI is a future enhancement (sysinfo doesn't expose them).
