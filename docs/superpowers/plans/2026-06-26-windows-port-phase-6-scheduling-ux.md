# Windows Port Phase 6 — Scheduling + Autostart + Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make FreeYourDisk's launch-at-login, weekly cleanup scheduling, and desktop notifications work natively on Windows, with Linux and macOS behaviour unchanged.

**Architecture:** Each feature already has Linux + macOS arms gated by `#[cfg(...)]`. Where an existing `#[cfg(not(target_os = "macos"))]` arm is actually Linux-specific (it shells out to `systemctl` / `notify-send`), split it into a `#[cfg(target_os = "linux")]` arm (body kept byte-identical) plus a new `#[cfg(target_os = "windows")]` arm. The Windows arms use only `winreg` (already a dependency), the built-in `schtasks.exe`, and `powershell.exe` (WinRT toast) — no new crate, no `unsafe`, no `windows` crate.

**Tech Stack:** Rust, Tauri 2, `winreg` 0.56 (Windows-only dep, already present from Phase 5), `schtasks.exe`, Windows PowerShell 5.1 (`C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`) driving the WinRT `ToastNotificationManager`.

## Global Constraints

- **SPDX header preserved verbatim** on every file: first line stays `// SPDX-License-Identifier: GPL-3.0-or-later`.
- **No `unsafe`.** No `windows` / `windows-sys` crate. Windows integration = `winreg` + `schtasks` + `powershell.exe` only.
- **No new dependency.** `winreg = "0.56"` already exists under `[target.'cfg(windows)'.dependencies]` in `src-tauri/Cargo.toml` — reuse it.
- **macOS arms are untouched.** Do not edit any `#[cfg(target_os = "macos")]` block.
- **Linux bodies stay byte-identical.** When splitting `#[cfg(not(target_os = "macos"))]` → `#[cfg(target_os = "linux")]`, change ONLY the attribute; the statements inside the arm are copied character-for-character.
- **cfg split rule:** Linux-specific `not(macos)` arms (systemctl / notify-send) become `#[cfg(target_os = "linux")]` + a new `#[cfg(target_os = "windows")]` sibling.
- **Errors, never panics.** Windows arms return `Ok(())`/`Ok(enabled)` or `Err(String)`; notifications are best-effort (`let _ = …`).
- **Windows code is compile-gated to a Windows target.** Linux `cargo clippy` does NOT compile any `#[cfg(target_os = "windows")]` arm, so `winreg`/`schtasks`/PowerShell typos surface only on a Windows build. A `windows-latest` CI job is the authoritative gate for the Windows arms (Phase 5's `winreg` Uninstall-inventory code already requires one). `cargo fmt --all --check` *does* format-check all arms regardless of target.
- **Per-task green gate (Linux):** `cargo fmt --all --check` and `cargo clippy -p freeyourdisk --all-targets -- -D warnings` must pass. Watch specifically for `unused_imports` / `dead_code` introduced on the Linux side of each cfg split.

---

## Task 1: Windows autostart via HKCU\…\Run (replace the no-op stub)

**Files:**
- Modify: `src-tauri/src/settings.rs:169-174` (the `#[cfg(target_os = "windows")] apply_autostart` stub)

**Interfaces:**
- Consumes: nothing new. `apply_autostart(enabled: bool) -> Result<(), String>` is already called by `settings::save()` (`src-tauri/src/settings.rs:112`).
- Produces: a working `#[cfg(target_os = "windows")] pub fn apply_autostart(enabled: bool) -> Result<(), String>` with the same signature as the Linux/macOS arms (so `save()` is unchanged).

- [ ] **Step 1: Replace the Windows no-op stub with the winreg implementation**

In `src-tauri/src/settings.rs`, replace this block (currently lines 169-174):

```rust
// Windows autostart (HKCU\...\Run) lands in Phase 6; no-op for now so the
// settings save path succeeds.
#[cfg(target_os = "windows")]
pub fn apply_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}
```

with:

```rust
/// Create or remove the launch-at-login entry under
/// `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`.
/// Per-user (HKCU), so no elevation is required.
#[cfg(target_os = "windows")]
pub fn apply_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    // create_subkey_with_flags creates the key if missing, opens it otherwise.
    let (run, _) = hkcu
        .create_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_SET_VALUE,
        )
        .map_err(|e| e.to_string())?;

    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // Quote the path so an install dir with spaces (e.g. "Program Files")
        // is parsed as a single argument by the shell at login.
        run.set_value("FreeYourDisk", &format!("\"{}\"", exe.display()))
            .map_err(|e| e.to_string())?;
    } else {
        // Disabling when the value is absent must succeed (idempotent).
        match run.delete_value("FreeYourDisk") {
            Ok(()) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
```

Notes for the implementer:
- `KEY_SET_VALUE` is sufficient for both `set_value` and `delete_value` (deleting a value requires set-value rights, not a separate flag).
- Do **not** add a `use std::path::PathBuf;` — `current_exe()` returns a `PathBuf` and `.display()` is called inline; the file's existing top-level `use std::path::PathBuf;` (line 7) is already used by `config_dir()` and stays.
- Do not touch the Linux (`:118-141`) or macOS (`:143-167`) arms.

- [ ] **Step 2: Format + lint (Linux gate)**

Run: `cargo fmt --all --check`
Expected: exits 0 (no diff). If it reports the new block, run `cargo fmt --all` and re-run `--check`.

Run: `cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: PASS. (On Linux this compiles the Linux `apply_autostart` arm only; it confirms nothing on the Linux side regressed. The Windows arm is validated in Step 3 / CI.)

- [ ] **Step 3: Windows compile check (authoritative for this arm)**

Run on a Windows host or `windows-latest` CI (or, if a mingw cross toolchain is installed locally, `cargo clippy -p freeyourdisk --target x86_64-pc-windows-gnu --all-targets -- -D warnings`):
Expected: PASS — confirms the `winreg` calls (`predef`, `create_subkey_with_flags`, `set_value`, `delete_value`) type-check.

Manual smoke (Windows): in the app, enable "Launch at login", then `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v FreeYourDisk` → shows the quoted exe path. Disable it → the same query returns "ERROR: … unable to find". Toggle twice to confirm idempotence (no error on the second disable).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "feat(windows): autostart via HKCU Run key (replaces Phase 6 no-op stub)"
```

---

## Task 2: Windows weekly cleanup scheduling via schtasks

**Files:**
- Modify: `src-tauri/src/commands.rs:107-132` (split the two `#[cfg(not(target_os = "macos"))]` arms to Linux + add Windows arms)
- Modify: `src-tauri/src/headless.rs:24-56` (extract `clean_root`; keep `cache_cleanup` as a thin wrapper) and `:98-114` (split `run()`'s cleanup-root resolution per-OS so Windows cleans `%LOCALAPPDATA%\Temp`)

**Interfaces:**
- Consumes: `std::env::current_exe()` for the task action path.
- Produces: `#[cfg(target_os = "windows")] pub fn schedule_enabled() -> bool` and `#[cfg(target_os = "windows")] pub fn set_schedule(enabled: bool) -> Result<bool, String>`, same names/signatures as the Linux + macOS arms. No change to `tauri::generate_handler!` in `main.rs` is needed — exactly one cfg arm of each fn compiles per target, and all arms share the names `schedule_enabled` / `set_schedule`.
- Produces (headless): `fn clean_root(cache_root: &Path, zone_root: &Path, min_age_days: u32, apply: bool) -> HeadlessOutcome` (the extracted core scan+trash); `cache_cleanup(home, …)` becomes the thin wrapper `clean_root(&home.join(".cache"), home, …)`. `headless::run()` calls `cache_cleanup` on Linux/macOS and `clean_root(&temp, &temp, …)` on Windows.

> ### Design rationale — `--apply`, `--service=temp`, and the Windows cleanup target
>
> A control-flow trace of `src-tauri/src/main.rs` settles three choices encoded below (pending final maintainer sign-off):
>
> 1. **`--apply` is required (and safe).** `main.rs:28` matches `--headless` **first** and calls `headless::run(&args)`, which `process::exit`s **before** the Windows `--apply` elevation interception at `main.rs:31-42`. So `--headless --apply` runs the **un-elevated, user-level** cleanup (`headless::run` → trash); it never reaches `apply_elevated`, never elevates, never shows UAC. (Even if it somehow did, a scheduled `apply_elevated` finds no staged `%TEMP%` plan and returns `2` harmlessly.) Without `--apply`, `headless::run` (`headless.rs:109`) is a **dry-run that frees nothing** and never notifies — so `/TR` includes `--apply`.
> 2. **`--service=temp` is included explicitly** (Step 3), mirroring the macOS LaunchAgent (`commands.rs:159`) for clarity, even though `headless::run` already defaults the service to `"temp"` when the flag is absent (`headless.rs:100-103`).
> 3. **The Windows cleanup target is `%LOCALAPPDATA%\Temp`, not `%USERPROFILE%\.cache`.** The XDG-style `.cache` dir is essentially always empty on Windows, so cleaning it would free nothing (a broken feature). `%LOCALAPPDATA%\Temp` is the **non-root user temp** root from Phase 1's `temp.rs` (often GBs of stale files), deletable without elevation and disjoint from `%WINDIR%\Temp` (the admin zone). `headless::run` therefore resolves the root per-OS (Step 5): Linux/macOS keep `~/.cache` via `cache_cleanup`; Windows uses `%LOCALAPPDATA%\Temp` via `clean_root`.

- [ ] **Step 1: Narrow the `schedule_enabled` Linux arm to `cfg(linux)`**

In `src-tauri/src/commands.rs`, change ONLY the attribute on line 108. Before:

```rust
/// Whether the weekly cleanup timer is enabled (systemd on Linux, launchd on macOS).
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn schedule_enabled() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-enabled", "freeyourdisk.timer"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}
```

After (only the `#[cfg(...)]` line changes; the body is byte-identical):

```rust
/// Whether the weekly cleanup timer is enabled (systemd on Linux, launchd on macOS).
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn schedule_enabled() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-enabled", "freeyourdisk.timer"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}
```

- [ ] **Step 2: Narrow the `set_schedule` Linux arm to `cfg(linux)`**

In `src-tauri/src/commands.rs`, change ONLY the attribute on line 119. Before:

```rust
/// Enable or disable (and start/stop) the weekly cleanup timer.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn set_schedule(enabled: bool) -> Result<bool, String> {
```

After (body unchanged):

```rust
/// Enable or disable (and start/stop) the weekly cleanup timer.
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn set_schedule(enabled: bool) -> Result<bool, String> {
```

(Leave the rest of that function — `commands.rs:122-132` — exactly as is.)

- [ ] **Step 3: Add the Windows `schtasks` arms**

In `src-tauri/src/commands.rs`, insert the following block immediately **after** the end of the Linux `set_schedule` function (after its closing `}` at line 132) and **before** the macOS `cleanup_plist_path` block (line 134):

```rust
/// Windows: Task Scheduler task name for the weekly cleanup.
#[cfg(target_os = "windows")]
const CLEANUP_TASK_NAME: &str = "FreeYourDisk Cleanup";

/// Windows: whether the weekly cleanup task exists in Task Scheduler.
#[cfg(target_os = "windows")]
#[tauri::command]
pub fn schedule_enabled() -> bool {
    std::process::Command::new("schtasks")
        .args(["/Query", "/TN", CLEANUP_TASK_NAME])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Windows: register or remove a weekly user-level cleanup task (Sunday 03:00).
/// The action runs our own exe with `--headless --service=temp --apply`, which
/// takes the un-elevated user-temp cleanup path (it never reaches the elevated
/// `--apply` branch, which requires `--apply` WITHOUT `--headless`).
#[cfg(target_os = "windows")]
#[tauri::command]
pub fn set_schedule(enabled: bool) -> Result<bool, String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // Inner quotes so an install path with spaces is one token for schtasks.
        let action = format!("\"{}\" --headless --service=temp --apply", exe.display());
        let out = std::process::Command::new("schtasks")
            .args([
                "/Create",
                "/TN",
                CLEANUP_TASK_NAME,
                "/TR",
                &action,
                "/SC",
                "WEEKLY",
                "/D",
                "SUN",
                "/ST",
                "03:00",
                "/F",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(true)
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    } else {
        let out = std::process::Command::new("schtasks")
            .args(["/Delete", "/TN", CLEANUP_TASK_NAME, "/F"])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(false)
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}
```

- [ ] **Step 4: Format + lint (Linux gate)**

Run: `cargo fmt --all --check`
Expected: exits 0.

Run: `cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: PASS. On Linux the new `#[cfg(target_os = "windows")]` const + arms are **not compiled**, so confirm specifically that the Linux `schedule_enabled` / `set_schedule` arms still compile and that the cfg change from `not(macos)` → `linux` produced no warning.

- [ ] **Step 5: Refactor `headless` so the Windows run cleans `%LOCALAPPDATA%\Temp`**

**5a — Extract `clean_root`; make `cache_cleanup` a thin wrapper.** In `src-tauri/src/headless.rs`, replace the whole `cache_cleanup` function (lines 24-56). Before:

```rust
/// Scan (and optionally trash) old files under `~/.cache`. User-only by
/// construction: the single root is the user's cache, marked non-root.
pub fn cache_cleanup(home: &Path, min_age_days: u32, apply: bool) -> HeadlessOutcome {
    let service = TempService {
        roots: vec![TempRoot {
            path: home.join(".cache"),
            requires_root: false,
        }],
        min_age_days,
    };
    let items = service.scan().items;
    let considered = items.len();

    if !apply {
        return HeadlessOutcome {
            considered,
            freed_bytes: 0,
            deleted_count: 0,
            applied: false,
        };
    }

    let zones = Zones(vec![home.to_path_buf()]);
    let paths: Vec<PathBuf> = items.iter().map(|item| item.path.clone()).collect();
    let report = to_trash(&paths, &zones);

    HeadlessOutcome {
        considered,
        freed_bytes: report.freed_bytes,
        deleted_count: report.deleted_count,
        applied: true,
    }
}
```

After (the scan+trash logic moves verbatim into `clean_root`, parameterised by explicit `cache_root` + `zone_root`; `cache_cleanup` becomes a one-line wrapper):

```rust
/// Scan (and optionally trash) old files under `cache_root`, confining every
/// deletion to `zone_root`. User-only by construction: the single temp root is
/// marked non-root, so no privileged path is ever touched.
fn clean_root(
    cache_root: &Path,
    zone_root: &Path,
    min_age_days: u32,
    apply: bool,
) -> HeadlessOutcome {
    let service = TempService {
        roots: vec![TempRoot {
            path: cache_root.to_path_buf(),
            requires_root: false,
        }],
        min_age_days,
    };
    let items = service.scan().items;
    let considered = items.len();

    if !apply {
        return HeadlessOutcome {
            considered,
            freed_bytes: 0,
            deleted_count: 0,
            applied: false,
        };
    }

    let zones = Zones(vec![zone_root.to_path_buf()]);
    let paths: Vec<PathBuf> = items.iter().map(|item| item.path.clone()).collect();
    let report = to_trash(&paths, &zones);

    HeadlessOutcome {
        considered,
        freed_bytes: report.freed_bytes,
        deleted_count: report.deleted_count,
        applied: true,
    }
}

/// Scan (and optionally trash) old files under `~/.cache`, confined to the home
/// directory. Thin wrapper over `clean_root`, byte-identical to the pre-refactor
/// behaviour. Used by the Linux/macOS run path and the tests; not compiled in
/// non-test Windows builds (Windows cleans `%LOCALAPPDATA%\Temp` via `clean_root`).
#[cfg(any(not(target_os = "windows"), test))]
pub fn cache_cleanup(home: &Path, min_age_days: u32, apply: bool) -> HeadlessOutcome {
    clean_root(&home.join(".cache"), home, min_age_days, apply)
}
```

`clean_root` is private but reachable everywhere it's used: `cache_cleanup` (Linux/macOS + tests) and `run()`'s Windows arm are in the same module, and the `#[cfg(test)]` tests reach it via `use super::*;`. It is therefore never `dead_code` on any target.

**5b — Split `run()`'s root resolution per-OS.** In the same file, replace the home lookup + cleanup call in `run()` (lines 110-114). Before:

```rust
    let apply = args.iter().any(|a| a == "--apply");
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));

    let outcome = cache_cleanup(&home, MIN_AGE_DAYS, apply);
```

After (the Linux/macOS arm is byte-identical to the original; the Windows arm targets the user temp):

```rust
    let apply = args.iter().any(|a| a == "--apply");

    // Linux/macOS: clean `~/.cache`. Windows: clean `%LOCALAPPDATA%\Temp` (the
    // non-root user temp from Phase 1) — never `%WINDIR%\Temp` (admin).
    #[cfg(not(target_os = "windows"))]
    let outcome = {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        cache_cleanup(&home, MIN_AGE_DAYS, apply)
    };

    #[cfg(target_os = "windows")]
    let outcome = {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("C:\\"))
                    .join("AppData")
                    .join("Local")
            });
        let root = local.join("Temp");
        clean_root(&root, &root, MIN_AGE_DAYS, apply)
    };
```

**5c — Add a `clean_root` test.** In the `#[cfg(test)] mod tests` block at the bottom of `headless.rs`, add this test after `recent_files_are_not_candidates` (it runs on all platforms, including Linux CI):

```rust
    #[test]
    fn clean_root_scans_an_arbitrary_root() {
        // Mirrors the Windows path: a temp-style root that is NOT `<home>/.cache`.
        let root = tempfile::tempdir().unwrap();
        let f = root.path().join("stale.tmp");
        std::fs::write(&f, vec![0u8; 100]).unwrap();
        backdate(&f, 30);

        let outcome = clean_root(root.path(), root.path(), 7, false);
        assert!(!outcome.applied);
        assert!(
            outcome.considered >= 1,
            "old file in an arbitrary root should be a candidate"
        );
        assert!(f.exists(), "dry-run must not delete");
    }
```

- [ ] **Step 6: Format + lint + test after the headless refactor (Linux gate)**

Run: `cargo fmt --all --check`
Expected: exits 0.

Run: `cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: PASS. On Linux confirm specifically: no `dead_code` for `clean_root` (used by `cache_cleanup`); the `cache_cleanup` cfg gate `any(not(target_os = "windows"), test)` keeps it compiled on Linux; no unused import.

Run: `cargo test -p freeyourdisk`
Expected: PASS — `dry_run_frees_nothing` and `recent_files_are_not_candidates` still pass (now routed through the `cache_cleanup` → `clean_root` wrapper), plus the new `clean_root_scans_an_arbitrary_root`.

- [ ] **Step 7: Windows compile + smoke (authoritative for the Windows arms)**

Windows build (host / `windows-latest` CI / `--target x86_64-pc-windows-gnu`): expect PASS, including `cargo test -p freeyourdisk` (the `clean_root` test compiles cross-platform).

Manual smoke (Windows):
- In the app, enable scheduling, then `schtasks /Query /TN "FreeYourDisk Cleanup" /V /FO LIST` → task exists, Schedule = Weekly, Start Time 03:00, "Task To Run" shows `"…\freeyourdisk.exe" --headless --service=temp --apply`.
- Ensure some stale (>7-day-old) files exist under `%LOCALAPPDATA%\Temp`, then `schtasks /Run /TN "FreeYourDisk Cleanup"` (or wait for Sun 03:00): confirm **no UAC prompt** and that those old files under `%LOCALAPPDATA%\Temp` are trashed (a toast appears once Task 3 lands). `%WINDIR%\Temp` must be untouched.
- Disable scheduling → `schtasks /Query /TN "FreeYourDisk Cleanup"` returns "ERROR: The system cannot find the file specified."

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/headless.rs
git commit -m "feat(windows): weekly cleanup via schtasks targeting %LOCALAPPDATA%\\Temp"
```

---

## Task 3: Windows desktop notifications (WinRT toast via PowerShell)

**Files:**
- Create: `src-tauri/src/toast.rs` (Windows-only module: PowerShell-driven WinRT toast + escaping helper + unit tests)
- Modify: `src-tauri/src/main.rs:20-21` (declare `#[cfg(target_os = "windows")] mod toast;`)
- Modify: `src-tauri/src/headless.rs:10` and `:84-95` (gate the `Command` import; split `notify` into per-OS arms)
- Modify: `src-tauri/src/monitor.rs:12` and `:78-84` (gate the `Command` import; split the low-space alert into per-OS arms)

**Interfaces:**
- Produces: `#[cfg(target_os = "windows")]` module `toast` exposing `pub(crate) fn show(title: &str, body: &str)` (best-effort, returns `()`), backed by `fn escape_ps_literal(s: &str) -> String`.
- Consumes: `crate::toast::show(title, body)` is called from `headless::notify` and `monitor::raise_and_alert`, only inside their `#[cfg(target_os = "windows")]` arms.

- [ ] **Step 1: Write the failing escaping test + the toast module**

Create `src-tauri/src/toast.rs` with the full content below (the whole file is compiled only on Windows because `mod toast;` is cfg-gated in Step 3, so no inner `#[cfg]` attributes are needed):

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows desktop notifications via the WinRT `ToastNotificationManager`,
//! driven through PowerShell. No extra crate, no `unsafe`. Best-effort: any
//! failure is swallowed — a missing toast must never break a cleanup or the
//! low-space monitor.

/// Show a Windows toast with the given title and body. Best-effort (errors are
/// ignored).
pub(crate) fn show(title: &str, body: &str) {
    // Absolute path — never resolve `powershell` from PATH (binary-planting).
    const PS: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
    // Reuse PowerShell's registered AppUserModelID so the toast actually
    // displays without an installed shortcut. A dedicated "FreeYourDisk" AUMID
    // (cleaner sender name than "Windows PowerShell") needs a Start-menu
    // shortcut registered by the installer — deferred to Phase 7 packaging.
    const APP_ID: &str =
        r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

    // title/body are embedded in PowerShell single-quoted literals, so the only
    // escape needed is doubling single quotes. XML special chars are handled by
    // CreateTextNode (a DOM text node), not string interpolation.
    let script = format!(
        "[Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]|Out-Null;\
         $x=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);\
         $t=$x.GetElementsByTagName('text');\
         $t.Item(0).AppendChild($x.CreateTextNode('{title}'))|Out-Null;\
         $t.Item(1).AppendChild($x.CreateTextNode('{body}'))|Out-Null;\
         $n=[Windows.UI.Notifications.ToastNotification]::new($x);\
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{app}').Show($n)",
        title = escape_ps_literal(title),
        body = escape_ps_literal(body),
        app = APP_ID,
    );

    let _ = std::process::Command::new(PS)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status();
}

/// Double single quotes so a string is safe inside a PowerShell single-quoted
/// literal (`'...'`) — the only escaping such literals require.
fn escape_ps_literal(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::escape_ps_literal;

    #[test]
    fn doubles_single_quotes() {
        assert_eq!(escape_ps_literal("it's a 'test'"), "it''s a ''test''");
    }

    #[test]
    fn leaves_plain_text_unchanged() {
        assert_eq!(escape_ps_literal("12.3 GB freed"), "12.3 GB freed");
    }
}
```

Implementer notes:
- The format string contains **no** literal `{`/`}` other than the `{title}`/`{body}`/`{app}` placeholders, so no brace-doubling is required. `APP_ID`'s braces are data (a raw string passed as the `{app}` argument), not format syntax.
- `ToastText02` is a 2-line template: `Item(0)` = bold heading (title), `Item(1)` = body line.

- [ ] **Step 2: Run the escaping tests to verify they pass (Windows)**

Run (on Windows / `windows-latest`, since the module only compiles there):
`cargo test -p freeyourdisk toast::`
Expected: `doubles_single_quotes` and `leaves_plain_text_unchanged` PASS. (On Linux these tests are not compiled; that is expected.)

- [ ] **Step 3: Declare the module (Windows-only)**

In `src-tauri/src/main.rs`, add the cfg-gated module declaration. Insert it immediately after `mod taskmgr;` (line 20), before `mod tray;` (line 21):

```rust
mod taskmgr;
#[cfg(target_os = "windows")]
mod toast;
mod tray;
```

Gating the `mod` (rather than the items inside) keeps the module absent on Linux/macOS, so its `pub(crate) fn show` is never "dead code" there.

- [ ] **Step 4: Wire the toast into `headless::notify`**

In `src-tauri/src/headless.rs`, first gate the `Command` import. Change line 10. Before:

```rust
use std::process::Command;
```

After (on Windows the bare `Command` alias becomes unused — `notify`'s Windows arm uses `crate::toast`, and `read_smart_elevated` already uses fully-qualified `std::process::Command`):

```rust
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;
```

Then replace the `notify` function body (lines 84-95). Before:

```rust
fn notify(freed_bytes: u64, count: usize) {
    let body = if is_french() {
        format!("{} libérés · {count} éléments", humanize(freed_bytes))
    } else {
        format!("{} freed · {count} items", humanize(freed_bytes))
    };
    let _ = Command::new("notify-send")
        .arg("--app-name=FreeYourDisk")
        .arg("FreeYourDisk")
        .arg(body)
        .status();
}
```

After (the `body` computation is unchanged; the notify-send call is gated to Linux byte-identically; Windows + macOS arms added so `body` is consumed on every target):

```rust
fn notify(freed_bytes: u64, count: usize) {
    let body = if is_french() {
        format!("{} libérés · {count} éléments", humanize(freed_bytes))
    } else {
        format!("{} freed · {count} items", humanize(freed_bytes))
    };

    #[cfg(target_os = "linux")]
    let _ = Command::new("notify-send")
        .arg("--app-name=FreeYourDisk")
        .arg("FreeYourDisk")
        .arg(body)
        .status();

    #[cfg(target_os = "windows")]
    crate::toast::show("FreeYourDisk", &body);

    #[cfg(target_os = "macos")]
    {
        let safe = body.replace('"', "");
        let _ = Command::new("osascript")
            .args([
                "-e",
                &format!("display notification \"{safe}\" with title \"FreeYourDisk\""),
            ])
            .status();
    }
}
```

Implementer note: `headless::notify` previously called `notify-send` on *all* targets (no cfg), so on macOS it silently failed (command absent). Adding the macOS `osascript` arm both keeps `body` used on every target (avoids an `unused_variable` error after the split) and gives the macOS scheduled cleanup a real notification, mirroring `monitor.rs`. The three arms are mutually exclusive, so `body` is consumed exactly once per target (moved into `.arg(body)` on Linux; borrowed on Windows/macOS).

- [ ] **Step 5: Wire the toast into `monitor::raise_and_alert`**

In `src-tauri/src/monitor.rs`, first gate the `Command` import. Change line 12. Before:

```rust
use std::process::Command;
```

After:

```rust
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;
```

Then change the notify-send arm (lines 78-84). Before:

```rust
    #[cfg(not(target_os = "macos"))]
    let _ = Command::new("notify-send")
        .arg("--app-name=FreeYourDisk")
        .arg("--urgency=critical")
        .arg("FreeYourDisk")
        .arg(&body)
        .status();
```

After (attribute narrowed to Linux, body byte-identical; new Windows arm added; the macOS `osascript` block at lines 86-95 is left untouched):

```rust
    #[cfg(target_os = "linux")]
    let _ = Command::new("notify-send")
        .arg("--app-name=FreeYourDisk")
        .arg("--urgency=critical")
        .arg("FreeYourDisk")
        .arg(&body)
        .status();

    #[cfg(target_os = "windows")]
    crate::toast::show("FreeYourDisk", &body);
```

`body` is consumed on every target (Linux notify-send, Windows toast, macOS osascript), so no `unused_variable` warning.

- [ ] **Step 6: Format + lint (Linux gate)**

Run: `cargo fmt --all --check`
Expected: exits 0 (rustfmt formats `toast.rs` and all cfg arms even though they don't compile on Linux).

Run: `cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: PASS. Confirm specifically:
- no `unused_imports` for `std::process::Command` in `headless.rs` / `monitor.rs` on Linux (it's still used by the Linux notify-send arms),
- no `dead_code` for `toast::show` (the module isn't compiled on Linux),
- the existing `headless::tests` still pass: `cargo test -p freeyourdisk` → PASS.

- [ ] **Step 7: Windows compile + smoke (authoritative for the Windows arms)**

Windows build (host / `windows-latest` CI / `--target x86_64-pc-windows-gnu`): expect PASS, including `cargo test -p freeyourdisk toast::`.

Manual smoke (Windows):
- Trigger a low-space alert (or temporarily lower `monitor_threshold`) → a critical toast titled "FreeYourDisk" appears in the Action Center.
- Run the scheduled cleanup with stale (>7-day-old) files present under `%LOCALAPPDATA%\Temp` → a toast reports "N.N <unit> freed · N items".
- Verify the body text survives an apostrophe (e.g. a French locale "éléments" line and any path containing `'`) without breaking the toast.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/toast.rs src-tauri/src/main.rs src-tauri/src/headless.rs src-tauri/src/monitor.rs
git commit -m "feat(windows): WinRT toast notifications for scheduled cleanup + low-space alert"
```

---

## Self-Review

**1. Spec coverage**
- Autostart (settings.rs Windows arm via winreg) → Task 1. ✓ (replaces the `:171-174` no-op; HKCU\…\Run; create-if-needed; `KEY_SET_VALUE`; set quoted exe / delete ignoring not-found; `Ok`/`Err`, no panic.)
- Scheduling (commands.rs split + Windows `schtasks`) → Task 2. ✓ (`not(macos)`→`linux` byte-identical; task name const `"FreeYourDisk Cleanup"`; `/Query`, `/Create … /SC WEEKLY /D SUN /ST 03:00 /F`, `/Delete … /F`; `Command::args`; `Ok(enabled)`/`Err(stderr)`.)
- Notifications (headless.rs + monitor.rs Windows toast) → Task 3. ✓ (absolute PowerShell path; `-NoProfile -NonInteractive -Command`; self-contained WinRT snippet; single-quote escaping; best-effort `let _`; both call sites wired; Linux notify-send / macOS osascript preserved.)
- `winreg` reused, no new dep. ✓ (Global Constraints; Task 1 reuses the existing `cfg(windows)` dep.)
- SPDX preserved / no `unsafe` / no `windows` crate. ✓ (Global Constraints; new `toast.rs` starts with the SPDX line.)

**2. Placeholder scan** — No "TBD"/"handle errors"/"similar to" placeholders; every code step shows full code (both the Linux "before" and the Windows "after" of each cfg split are spelled out in full, as is the `clean_root` extraction).

**3. Type consistency** — `apply_autostart(bool) -> Result<(), String>`, `schedule_enabled() -> bool`, `set_schedule(bool) -> Result<bool, String>` match the existing Linux/macOS signatures (so `generate_handler!` and `settings::save` are unchanged). `crate::toast::show(&str, &str)` is defined in Task 3 and called with that exact signature in `headless::notify` and `monitor::raise_and_alert`. `escape_ps_literal(&str) -> String` is defined and used in `toast::show` and its tests. The `Command` import gates (`any(linux, macos)`) line up with the only remaining bare-`Command` users on each target.

**Resolved decisions (encoded; pending the maintainer's own sign-off — see the Task 2 rationale block):**
- **`--apply` is included** in `/TR` — `--headless --apply` is the un-elevated, user-level cleanup path (proven by the `main.rs` control-flow trace), so it runs without UAC. Without it the weekly task frees nothing.
- **Windows cleans `%LOCALAPPDATA%\Temp`** (not `%USERPROFILE%\.cache`, which is empty on Windows) via the new `clean_root`, confined to that temp dir, never `%WINDIR%\Temp`.
- **`--service=temp` is listed explicitly** in `/TR`, matching the macOS plist (it is also the `headless::run` default).
- **Toast AppId reuses PowerShell's AUMID** for reliable display without an installed shortcut; the sender shows as "Windows PowerShell". A dedicated "FreeYourDisk" AUMID needs an installer-registered Start-menu shortcut — deferred to Phase 7 packaging. Best-effort (`let _`) regardless.

**Remaining risk to confirm:**
- **Windows CI** — the Windows arms compile only on a Windows target; ensure a `windows-latest` job exists (Phase 5's `winreg` code already needs one). Linux `clippy` cannot catch `winreg`/`schtasks`/PowerShell typos; `cargo fmt --all --check` does format-check them.
