# Windows Port — Phase 4: Task Manager — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

> **Plan series.** Phase **4 of 8**. Phases 0–3 merged. Spec §5.6. Branch: `feat/win-phase-4`.

**Goal:** The Task-manager tab is fully functional on Windows: terminating, force-killing, restarting, and emergency "panic-kill the biggest hog" all work, and the emergency kill never targets a critical Windows system process.

**Architecture:** Process enumeration and memory/CPU stats already work on Windows (cross-platform `sysinfo` in `taskmgr.rs`). Only two gaps remain: `kill_process` is a Windows stub returning `false`, and the `PROTECTED` critical-process allowlist is Linux-specific (`cfg(not(macos))`). We implement Windows termination via `sysinfo::Process::kill()` (TerminateProcess under the hood — no `windows` crate, no `unsafe`) and add a Windows `PROTECTED` list. CPU temperature degrades to `None` on Windows (sysinfo exposes no thermal sensor there, like macOS); the app's own `raise_priority` is already a Windows no-op.

**Tech Stack:** Rust, `sysinfo` (existing dep; `Process::kill`, `refresh_processes_specifics`, `ProcessRefreshKind::everything`, `ProcessesToUpdate`, `Pid`).

## Global Constraints

- **Target:** Windows 10/11 x64. Do NOT regress Linux or macOS.
- **License:** keep `// SPDX-License-Identifier: GPL-3.0-or-later`.
- **cfg rule:** split the touched `#[cfg(not(target_os = "macos"))]` `PROTECTED` into explicit `linux` + `windows`. The Linux body stays byte-identical.
- **No new crate dependency.** Windows termination uses `sysinfo` only (no `windows` crate, no `unsafe`).
- **Use only sysinfo calls already compiled on Windows** (`refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid]), true, ProcessRefreshKind::everything())` is used by the cross-platform `process_list`/`restart_process` today) plus the standard cross-platform `Process::kill()`. Local clippy runs on Linux and will NOT compile the `#[cfg(windows)]` arm — write it carefully; the Windows CI job is the gate.
- **Verification:** `cargo fmt --all --check` + `cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN on Linux. Windows-only arms are CI-compile-gated; runtime termination is manual-smoke.

---

### Task 1: Windows process termination + critical-process allowlist

**Files:** Modify `src-tauri/src/taskmgr.rs` (`kill_process` Windows arm; `PROTECTED` const).

**Interfaces:** Produces a working `taskmgr::kill_process(pid: u32, force: bool) -> bool` on Windows (signature unchanged across OSes), enabling `restart_process` and `panic_kill` on Windows. Adds a Windows `PROTECTED: &[&str]`.

- [ ] **Step 1: Real Windows `kill_process`**

Replace the Windows stub (the `#[cfg(windows)] pub fn kill_process(_pid, _force) -> bool { false }` block, including its doc comment) with a real TerminateProcess via sysinfo. Windows has no graceful-signal analogue, so `force` is moot (both terminate forcefully):

```rust
/// Windows: TerminateProcess via sysinfo (always forceful — Windows has no
/// graceful signal analogue, so `force` is ignored). Returns false if the
/// process is already gone or the caller lacks rights (e.g. a protected system
/// process terminated without elevation). A graceful WM_CLOSE path would need
/// window enumeration (windows crate) — deferred; force-terminate is the
/// task-manager's primary action.
#[cfg(windows)]
pub fn kill_process(pid: u32, _force: bool) -> bool {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::everything(),
    );
    sys.process(Pid::from_u32(pid))
        .map(|p| p.kill())
        .unwrap_or(false)
}
```

- [ ] **Step 2: Split `PROTECTED` to explicit `linux`, add Windows arm**

Change the existing `#[cfg(not(target_os = "macos"))]` above the Linux `PROTECTED` const to `#[cfg(target_os = "linux")]` (the array body is unchanged). Then add a Windows `PROTECTED` const immediately after the Linux one (before the macOS one):

```rust
/// Critical process names we never panic-kill (Windows system processes).
/// `panic_kill` matches with `name.contains(...)` and sysinfo reports Windows
/// names with the `.exe` suffix, so bare stems match (e.g. "svchost" matches
/// "svchost.exe"). Over-matching only over-protects, which is the safe side.
#[cfg(target_os = "windows")]
const PROTECTED: &[&str] = &[
    "System",
    "Registry",
    "smss",
    "csrss",
    "wininit",
    "winlogon",
    "services",
    "lsass",
    "svchost",
    "dwm",
    "fontdrvhost",
    "explorer",
    "ctfmon",
    "RuntimeBroker",
    "freeyourdisk",
    "FreeYourDisk",
];
```

- [ ] **Step 3: Verify** — `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN on Linux (this confirms no Linux/macOS regression; the Windows arm compiles only on the Windows CI). If clippy errors on `../ui/dist`, build the frontend once.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/taskmgr.rs
git commit -m "feat(win): process termination via sysinfo + Windows critical-process allowlist"
```

---

## Self-Review

**Spec coverage (§5.6):** kill/terminate/restart/panic-kill work on Windows (Step 1 unblocks `kill_process`, which `restart_process` and `panic_kill` already call); panic-kill safety via a Windows `PROTECTED` list (Step 2). Process enumeration + memory/CPU stats already cross-platform (`process_list`/`mem_stats` via sysinfo — unchanged). CPU temperature: `None` on Windows (sysinfo exposes no thermal sensor; documented degradation, matches macOS). App self-priority: `raise_priority` already a Windows no-op.

**Placeholder scan:** none. The graceful-WM_CLOSE path and CPU-temperature-via-WMI are explicit, documented deferrals (forceful terminate + `None` temp), not placeholders.

**Type consistency:** `kill_process(u32, bool) -> bool` identical across OSes (callers `restart_process`/`panic_kill` unchanged). `PROTECTED: &[&str]` shape identical across the three arms; `panic_kill`'s `name.contains` match logic unchanged.

## Notes for later
- Graceful WM_CLOSE (vs forceful TerminateProcess) on Windows needs window enumeration (windows crate) — deferred.
- CPU temperature via WMI (`MSAcpi_ThermalZoneTemperature`) — deferred (often unavailable; per-poll PowerShell is heavy). Degrades to `None`.
- Per-process priority setting is not exposed by the task-manager UI, so SetPriorityClass is out of scope.
