# Windows Port — Phase 5: Applications — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

> **Plan series.** Phase **5 of 8**. Phases 0–4 merged. Spec §5.8. Branch: `feat/win-phase-5`.

**Goal:** The Applications tab is fully functional on Windows with no feature limitation — it lists classic Win32 apps (registry Uninstall keys, 64- and 32-bit views + per-user) **and** MSIX/Store apps, ranked by disk size; it batch-uninstalls both kinds safely; and it badges + applies updates via winget.

**Architecture:** `applications.rs` keeps three OS arms with an **identical public API** (`list`/`updates`/`uninstall`/`update`). Today the non-macOS bodies are gated `#[cfg(not(target_os = "macos"))]` (Linux) plus a `#[cfg(target_os = "macos")]` arm. We re-gate every Linux body to explicit `#[cfg(target_os = "linux")]` (attribute-only; bodies byte-identical) and add `#[cfg(target_os = "windows")]` arms. Windows inventory merges two sources sorted by size desc: (1) registry Uninstall subkeys read with the **`winreg`** crate, (2) MSIX packages read by shelling **PowerShell `Get-AppxPackage`** and parsing `ConvertTo-Json`. Uninstall is security-critical: the caller passes only an id; the backend re-resolves the action from a trusted source (re-read the registry's own Quiet/UninstallString, or `Remove-AppxPackage` on a shape-validated PackageFullName) and never runs a caller-supplied command string. Updates use winget from PATH (user context, non-elevated). macOS is untouched. The cross-platform helpers `run`, `split_ids`, `exec`, `validate_against_inventory` stay shared.

**Tech Stack:** Rust, `winreg` **0.56** (Windows-only dep — latest stable, no `unsafe`, no `windows` crate), `serde`/`serde_json` (already deps) for the `Get-AppxPackage` JSON, `std::process::Command` for PowerShell + winget.

## Global Constraints

- **Target:** Windows 10/11 x64. Do **NOT** regress Linux or macOS.
- **License:** keep `// SPDX-License-Identifier: GPL-3.0-or-later` (file header unchanged).
- **cfg rule:** replace **every** `#[cfg(not(target_os = "macos"))]` in `applications.rs` with `#[cfg(target_os = "linux")]` (15 occurrences; Linux bodies **byte-identical** — attribute only). macOS arms untouched. Add `#[cfg(target_os = "windows")]` arms for `list`, `updates`, `uninstall`, `update` + Windows-only helpers.
- **No `windows` crate, no `unsafe`.** Registry via `winreg`; MSIX via PowerShell. `winreg` lives ONLY under `[target.'cfg(windows)'.dependencies]`.
- **Absolute PowerShell path:** `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe` with `-NoProfile -NonInteractive -Command`.
- **Security:** the caller passes **ids only**; the backend re-resolves actions from trusted sources. Validate the registry id shape (known HIVE + non-empty subkey) before touching the registry. Validate every MSIX PackageFullName against `^[A-Za-z0-9._-]+$` before `Remove-AppxPackage`. Refuse `protected` entries and unknown id prefixes (enforced by the shared `validate_against_inventory` + per-arm guards).
- **Local clippy runs on Linux and will NOT compile the `#[cfg(windows)]` arms** — write them carefully; the Windows CI job is the compile gate.
- **Verification per task:** `cargo fmt --all --check` + `cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN on Linux (proves no Linux/macOS regression). Windows arms are CI-compile-gated; runtime = manual-smoke. (T6 frontend verifies via the UI build.)

---

### Task 1: `winreg` dep + cfg-split + registry inventory in Windows `list()`

**Files:** Modify `src-tauri/Cargo.toml` (Windows dep block) and `src-tauri/src/applications.rs` (cfg-split + Windows registry enumeration + Windows `list()`).

**Interfaces:** Produces a Windows `applications::list() -> Vec<AppEntry>` returning classic Win32 apps from the registry Uninstall keys (size desc). New private Windows helpers `registry_hive`, `detect_registry`. No public-API or signature change.

- [ ] **Step 1: Add the `winreg` Windows-only dependency**

In `src-tauri/Cargo.toml`, add a Windows target block (the file already has `[target.'cfg(unix)'.dependencies]` with `libc`; mirror that layout). Insert immediately after the `[target.'cfg(unix)'.dependencies]` block:

```toml
[target.'cfg(windows)'.dependencies]
# Registry Uninstall inventory + (later phases) autostart. Safe crate, no `unsafe`.
winreg = "0.56"
```

- [ ] **Step 2: Re-gate every Linux body from `not(macos)` to `linux`**

In `src-tauri/src/applications.rs`, replace **all 15** occurrences of the exact attribute `#[cfg(not(target_os = "macos"))]` with `#[cfg(target_os = "linux")]`. The string is identical at every site, so a single replace-all is correct. Do **not** change any function body. Affected items: `APT_TOP`, `detect_apt`, `parse_human_size`, `detect_flatpak`, `snap_size`, `detect_snap`, `detect_appimages`, `list`, `updates`, `safe_value`, `appimage_bases`, `within_allowed_base`, `remove_appimages`, `uninstall`, `update`.

> Why this is mandatory: `list()` is currently `not(macos)`, which is **true on Windows** — leaving it as-is would clash with the new `#[cfg(windows)] list()` (duplicate definition on Windows). Re-gating to `linux` keeps macOS untouched and lets the Windows arms coexist.

- [ ] **Step 3: Gate `home()` and the `PathBuf` import off Windows (avoid `-D warnings` dead-code/unused-import on the Windows CI)**

After Step 2, `home()` has no caller on Windows (its callers are Linux/macOS-only), and `PathBuf` is used only off-Windows. The Windows code uses only `Path`. Change the import line near the top of the file:

```rust
use std::path::{Path, PathBuf};
```

to:

```rust
use std::path::Path;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
```

And add a cfg to the existing `home()` helper (body unchanged):

```rust
#[cfg(not(target_os = "windows"))]
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
```

> `Path`, `HashSet`, `Command`, `Serialize` stay shared: `Path` is used by Windows `detect_msix` (T2); `HashSet`/`Command` by the shared validation/exec and the Windows arms; the `run`/`split_ids`/`exec`/`validate_against_inventory` helpers are unchanged.

- [ ] **Step 4: Add the Windows registry inventory + Windows `list()`**

Add the following `#[cfg(target_os = "windows")]` items to `applications.rs` (placement is free — cfg-gated; suggested: a "Windows applications" section right before the `#[cfg(target_os = "macos")] pub fn list()`). `registry_hive` is shared by `detect_registry` here and `uninstall` (T3), so define it now:

```rust
// ---- Windows: classic apps via the registry Uninstall keys ----------------

/// Maps an id hive label to (predefined hive RegKey, Uninstall base subpath).
/// HKLM = 64-bit machine view, HKLM32 = 32-bit (WOW6432Node) machine view,
/// HKCU = per-user. Predefined RegKeys are not closed on drop (winreg special-
/// cases them), so returning an owned RegKey is cheap and correct.
#[cfg(target_os = "windows")]
fn registry_hive(label: &str) -> Option<(winreg::RegKey, &'static str)> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    const UNINSTALL32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    match label {
        "HKLM" => Some((RegKey::predef(HKEY_LOCAL_MACHINE), UNINSTALL)),
        "HKLM32" => Some((RegKey::predef(HKEY_LOCAL_MACHINE), UNINSTALL32)),
        "HKCU" => Some((RegKey::predef(HKEY_CURRENT_USER), UNINSTALL)),
        _ => None,
    }
}

/// Enumerate the three Uninstall hives. Skip entries with no DisplayName, with
/// SystemComponent==1 (hidden component), or with no uninstall command (updates/
/// patches). id = `registry:<HIVE>:<subkey>` so uninstall re-opens the exact
/// hive/view. size = EstimatedSize (KB) * 1024. Dedupe identical (name, version).
#[cfg(target_os = "windows")]
fn detect_registry() -> Vec<AppEntry> {
    use winreg::enums::KEY_READ;
    let mut out = Vec::new();
    // (hive label, requires_root): HKLM/HKLM32 are machine-wide → admin to remove.
    for (label, requires_root) in [("HKLM", true), ("HKLM32", true), ("HKCU", false)] {
        let Some((hive, base)) = registry_hive(label) else {
            continue;
        };
        let Ok(uninstall) = hive.open_subkey_with_flags(base, KEY_READ) else {
            continue;
        };
        for name in uninstall.enum_keys().flatten() {
            let Ok(sub) = uninstall.open_subkey_with_flags(&name, KEY_READ) else {
                continue;
            };
            // DisplayName is mandatory.
            let Ok(display_name) = sub.get_value::<String, _>("DisplayName") else {
                continue;
            };
            if display_name.trim().is_empty() {
                continue;
            }
            // SystemComponent==1 → hidden component / update, not a user app.
            if sub.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
                continue;
            }
            // Must carry a usable uninstall command, else it is an update/patch.
            let quiet = sub.get_value::<String, _>("QuietUninstallString").ok();
            let plain = sub.get_value::<String, _>("UninstallString").ok();
            let has_cmd = quiet.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
                || plain.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
            if !has_cmd {
                continue;
            }
            let version = sub
                .get_value::<String, _>("DisplayVersion")
                .ok()
                .filter(|s| !s.trim().is_empty());
            let kb = sub.get_value::<u32, _>("EstimatedSize").unwrap_or(0);
            out.push(AppEntry {
                id: format!("registry:{label}:{name}"),
                name: display_name,
                source: "registry".into(),
                version,
                size_bytes: u64::from(kb) * 1024,
                requires_root,
                protected: false,
            });
        }
    }
    // Dedupe the same app surfaced in multiple hives; keep the first (HKLM wins).
    let mut seen: HashSet<(String, Option<String>)> = HashSet::new();
    out.retain(|a| seen.insert((a.name.clone(), a.version.clone())));
    out
}

/// Full inventory, largest first. (MSIX merged in Task 2.)
#[cfg(target_os = "windows")]
pub fn list() -> Vec<AppEntry> {
    let mut apps = detect_registry();
    apps.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
    apps
}
```

- [ ] **Step 5: Verify** — `cargo fmt --all --check && cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN on Linux (confirms the cfg-split + import/`home()` gating did not regress Linux/macOS; the Windows arms compile only on the Windows CI). If clippy complains about a missing `../ui/dist`, build the frontend once first.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/applications.rs
git commit -m "feat(win): registry Uninstall inventory in applications::list (winreg dep + cfg split)"
```

---

### Task 2: MSIX/Store inventory merged into Windows `list()`

**Files:** Modify `src-tauri/src/applications.rs` (add `POWERSHELL` const, `AppxRaw`, `detect_msix`; extend Windows `list()`).

**Interfaces:** Windows `list()` additionally returns MSIX apps (`id = msix:<PackageFullName>`, `source = "msix"`). New private Windows items `POWERSHELL`, `AppxRaw`, `detect_msix`. `POWERSHELL` is reused by `uninstall` (T3).

- [ ] **Step 1: Add the PowerShell path, the deserialize struct, and the MSIX detector**

Add these `#[cfg(target_os = "windows")]` items (e.g. just below `detect_registry`):

```rust
// ---- Windows: MSIX / Store apps via Get-AppxPackage --------------------------

/// Absolute path so PATH/profile cannot redirect to a fake powershell.
#[cfg(target_os = "windows")]
const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

/// One row of the `Get-AppxPackage | ConvertTo-Json` output. Version and
/// SignatureKind are forced to strings in the script (see SCRIPT below).
#[cfg(target_os = "windows")]
#[derive(serde::Deserialize)]
struct AppxRaw {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "PackageFullName")]
    package_full_name: Option<String>,
    #[serde(rename = "Version")]
    version: Option<String>,
    #[serde(rename = "InstallLocation")]
    install_location: Option<String>,
    #[serde(rename = "NonRemovable")]
    non_removable: Option<bool>,
    #[serde(rename = "SignatureKind")]
    signature_kind: Option<String>,
}

/// MSIX packages for the current user. Skips OS framework packages
/// (SignatureKind == "System"). protected = NonRemovable. size = best-effort dir
/// size of InstallLocation. requires_root = false (per-user removal).
#[cfg(target_os = "windows")]
fn detect_msix() -> Vec<AppEntry> {
    // `.ToString()` coerces System.Version and the SignatureKind enum to plain
    // strings; otherwise ConvertTo-Json emits Version as a nested object.
    const SCRIPT: &str = "Get-AppxPackage | Select-Object Name,PackageFullName,\
@{N='Version';E={$_.Version.ToString()}},InstallLocation,NonRemovable,\
@{N='SignatureKind';E={$_.SignatureKind.ToString()}} | ConvertTo-Json -Compress -Depth 3";
    let Some(out) = run(POWERSHELL, &["-NoProfile", "-NonInteractive", "-Command", SCRIPT]) else {
        return Vec::new();
    };
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    // ConvertTo-Json emits a bare object for a single package, an array otherwise.
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Vec::new();
    };
    let items = match value {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    let mut apps = Vec::new();
    for item in items {
        let Ok(pkg) = serde_json::from_value::<AppxRaw>(item) else {
            continue;
        };
        let (Some(name), Some(pfn)) = (
            pkg.name.filter(|s| !s.trim().is_empty()),
            pkg.package_full_name.filter(|s| !s.trim().is_empty()),
        ) else {
            continue;
        };
        // OS frameworks are signed "System" — not user-facing apps.
        if pkg.signature_kind.as_deref() == Some("System") {
            continue;
        }
        let size = pkg
            .install_location
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| core_scan::cache::cached_dir_total(Path::new(s)))
            .unwrap_or(0);
        apps.push(AppEntry {
            id: format!("msix:{pfn}"),
            name,
            source: "msix".into(),
            version: pkg.version.filter(|s| !s.trim().is_empty()),
            size_bytes: size,
            requires_root: false,
            protected: pkg.non_removable.unwrap_or(false),
        });
    }
    apps
}
```

- [ ] **Step 2: Merge MSIX into Windows `list()`**

Change the Windows `list()` body added in Task 1 to also extend with MSIX:

```rust
/// Full inventory (registry + MSIX), largest first.
#[cfg(target_os = "windows")]
pub fn list() -> Vec<AppEntry> {
    let mut apps = detect_registry();
    apps.extend(detect_msix());
    apps.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
    apps
}
```

- [ ] **Step 3: Verify** — `cargo fmt --all --check && cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN on Linux.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/applications.rs
git commit -m "feat(win): MSIX/Store inventory via Get-AppxPackage merged into list"
```

---

### Task 3: Windows `uninstall(ids)` — registry + MSIX (security-critical)

**Files:** Modify `src-tauri/src/applications.rs` (add `valid_pfn` + Windows `uninstall`).

**Interfaces:** `applications::uninstall(ids: &[String]) -> AppActionReport` on Windows (signature identical across OSes). Reuses shared `validate_against_inventory` (refuses unknown + `protected`), `split_ids`, `exec`, plus T1's `registry_hive` and T2's `POWERSHELL`.

- [ ] **Step 1: Add the PFN validator + the Windows `uninstall`**

```rust
/// MSIX PackageFullName shape guard: alphanumerics, dot, dash, underscore only
/// (== `^[A-Za-z0-9._-]+$`). Rejects every shell metacharacter, so the value is
/// safe to pass to Remove-AppxPackage.
#[cfg(target_os = "windows")]
fn valid_pfn(pfn: &str) -> bool {
    !pfn.is_empty()
        && pfn
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Batch uninstall. The caller supplies only ids; the backend re-resolves the
/// action from a trusted source and NEVER runs a caller-supplied command string.
/// Registry apps: re-open the exact Uninstall subkey and run ITS OWN
/// Quiet/UninstallString (installer-authored, trusted). MSIX: Remove-AppxPackage
/// with a shape-validated PackageFullName. Protected (NonRemovable) and unknown
/// ids are refused by validate_against_inventory + the per-arm guards below.
#[cfg(target_os = "windows")]
pub fn uninstall(ids: &[String]) -> AppActionReport {
    use winreg::enums::KEY_READ;
    let mut report = AppActionReport::default();
    let known = validate_against_inventory(ids, &mut report, true);

    // --- Registry (classic Win32) apps ---
    for rest in split_ids(&known, "registry:") {
        // rest == "<HIVE>:<subkey>"; split on the FIRST ':' so subkey may contain ':'.
        let Some((label, subkey)) = rest.split_once(':') else {
            report.errors.push(format!("refused (bad id): registry:{rest}"));
            continue;
        };
        let Some((hive, base)) = registry_hive(label) else {
            report
                .errors
                .push(format!("refused (unknown hive): registry:{rest}"));
            continue;
        };
        if subkey.is_empty() {
            report
                .errors
                .push(format!("refused (empty subkey): registry:{rest}"));
            continue;
        }
        let path = format!(r"{base}\{subkey}");
        let Ok(key) = hive.open_subkey_with_flags(&path, KEY_READ) else {
            report.errors.push(format!("registry:{rest}: subkey not found"));
            continue;
        };
        // Prefer the silent command; fall back to the interactive one.
        let cmd_str = key
            .get_value::<String, _>("QuietUninstallString")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                key.get_value::<String, _>("UninstallString")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            });
        let Some(cmd_str) = cmd_str else {
            report
                .errors
                .push(format!("registry:{rest}: no uninstall command"));
            continue;
        };
        // The command string comes from the registry (written by the app's own
        // installer), never from the caller. Run it via `cmd /C` as one argument.
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", &cmd_str]);
        exec(&mut report, &format!("uninstall {label}:{subkey}"), cmd);
    }

    // --- MSIX / Store apps ---
    for pfn in split_ids(&known, "msix:") {
        if !valid_pfn(&pfn) {
            report
                .errors
                .push(format!("refused (bad package name): msix:{pfn}"));
            continue;
        }
        let script = format!("Remove-AppxPackage -Package '{pfn}'");
        let mut cmd = Command::new(POWERSHELL);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        exec(&mut report, &format!("uninstall msix:{pfn}"), cmd);
    }
    report
}
```

- [ ] **Step 2: Verify** — `cargo fmt --all --check && cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN on Linux.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/applications.rs
git commit -m "feat(win): batch uninstall (registry UninstallString + Remove-AppxPackage)"
```

---

### Task 4: Windows `updates()` / `update(ids)` via winget

**Files:** Modify `src-tauri/src/applications.rs` (add `parse_winget_upgrade_names` + Windows `updates` + Windows `update`).

**Interfaces:** `applications::updates() -> Vec<String>` and `applications::update(ids: &[String]) -> AppActionReport` on Windows (signatures identical across OSes). `winget` is taken from PATH (user context, non-elevated). New private Windows helper `parse_winget_upgrade_names`.

> **Badging contract:** the frontend badges updates with `updateIds.has(app.id)` (`ui/src/lib/views/Applications.svelte`). So `updates()` must return values matching `AppEntry.id`, like the Linux arm. We therefore parse winget's upgradable **Names** and map them to inventory **ids** (case-insensitive). Names that match no inventory entry are dropped. (See the design risk note in the controller hand-off.)

- [ ] **Step 1: Add the winget table parser**

```rust
/// Heuristically parse `winget upgrade` table output → the Name column of each
/// upgradable row. Columns are padded with 2+ spaces, so the Name is everything
/// before the first double-space run (Names may contain single spaces). Rows
/// start after the dashed separator and end at the first blank line.
#[cfg(target_os = "windows")]
fn parse_winget_upgrade_names(out: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_table = false;
    for line in out.lines() {
        let line = line.trim_end();
        if !in_table {
            if line.starts_with("---") {
                in_table = true;
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        // Footer such as "N upgrades available." / pinned-package notes.
        let starts_digit = trimmed.chars().next().is_some_and(|c| c.is_ascii_digit());
        if starts_digit && trimmed.to_lowercase().contains("upgrade") {
            continue;
        }
        if let Some(name) = line.split("  ").next() {
            let name = name.trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }
    names
}
```

- [ ] **Step 2: Add the Windows `updates()` (winget Names → inventory ids)**

```rust
/// Ids of apps winget reports as upgradable (for UI badging). Maps winget Names
/// to inventory ids case-insensitively so the id-keyed frontend badging works
/// unchanged. Unmatched winget Names are dropped.
#[cfg(target_os = "windows")]
pub fn updates() -> Vec<String> {
    let Some(out) = run(
        "winget",
        &["upgrade", "--accept-source-agreements", "--disable-interactivity"],
    ) else {
        return Vec::new();
    };
    let names = parse_winget_upgrade_names(&out);
    if names.is_empty() {
        return Vec::new();
    }
    let lower: HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
    list()
        .into_iter()
        .filter(|app| lower.contains(&app.name.to_lowercase()))
        .map(|app| app.id)
        .collect()
}
```

- [ ] **Step 3: Add the Windows `update(ids)` (best-effort winget by name)**

```rust
/// Best-effort batch update via winget. Each id is resolved to its inventory
/// AppEntry name and updated by `winget upgrade --silent --name <name>`. winget
/// keys on its own package identity, so an entry whose name does not match a
/// winget package (or matches several) cannot be updated — that is reported as an
/// explicit error, never silently skipped.
#[cfg(target_os = "windows")]
pub fn update(ids: &[String]) -> AppActionReport {
    let mut report = AppActionReport::default();
    let known = validate_against_inventory(ids, &mut report, false);
    if known.is_empty() {
        return report;
    }
    // One extra inventory scan to map ids → names (on-demand action; acceptable).
    let inventory = list();
    for id in &known {
        let Some(app) = inventory.iter().find(|a| &a.id == id) else {
            report.errors.push(format!("{id}: not found in inventory"));
            continue;
        };
        // Anti argument-injection: a name starting with '-' would read as a flag.
        if app.name.trim().is_empty() || app.name.starts_with('-') {
            report.errors.push(format!("{id}: unsafe package name"));
            continue;
        }
        let mut cmd = Command::new("winget");
        cmd.args([
            "upgrade",
            "--silent",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
            "--name",
            &app.name,
        ]);
        exec(&mut report, &format!("winget upgrade {}", app.name), cmd);
    }
    report
}
```

- [ ] **Step 4: Verify** — `cargo fmt --all --check && cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN on Linux. (Confirms all four Windows arms are now defined; `commands.rs` needs no change — `list_applications`/`app_updates`/`uninstall_apps`/`update_apps` call the same signatures on every OS.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/applications.rs
git commit -m "feat(win): winget update detection (badging) + best-effort batch update"
```

---

### Task 5: Frontend — source labels/colours for `registry` + `msix`

**Files:** Modify `ui/src/lib/api.ts` (`AppSource` union) and `ui/src/lib/views/Applications.svelte` (`SOURCE_COLOR`). Cosmetic only; the cross-platform Rust contract is unchanged.

**Interfaces:** The two new Windows sources render with a coloured badge (already shown as raw text via `{app.source}`); this just adds the type members + colours.

- [ ] **Step 1: Extend the `AppSource` union** in `ui/src/lib/api.ts`:

```ts
export type AppSource =
  | "apt"
  | "flatpak"
  | "snap"
  | "appimage"
  | "registry"
  | "msix";
```

> `winget` is **not** an `AppSource` — winget is only the update channel; Windows apps surface as `registry` or `msix`.

- [ ] **Step 2: Add colours** to `SOURCE_COLOR` in `ui/src/lib/views/Applications.svelte`:

```ts
  const SOURCE_COLOR: Record<string, string> = {
    apt: "#d70a53",
    flatpak: "#4a90d9",
    snap: "#f5732b",
    appimage: "#f7a800",
    registry: "#0078d4",
    msix: "#5c2d91",
  };
```

- [ ] **Step 3: Verify** — `pnpm -C ui build` GREEN (TypeScript/Svelte compile; no type error from the union).

- [ ] **Step 4: Commit**

```bash
git add ui/src/lib/api.ts ui/src/lib/views/Applications.svelte
git commit -m "feat(win): Applications UI labels for registry + msix sources"
```

---

## Self-Review

**Spec coverage (§5.8):** inventory of classic apps via registry Uninstall keys — HKLM 64-bit, HKLM 32-bit (`WOW6432Node`), HKCU — via `winreg` (T1); MSIX/Store inventory (T2); uninstall classic (`Quiet`/`UninstallString`) + MSIX (`Remove-AppxPackage`) (T3); updates detection + execution via winget (T4); the whole `#[cfg(not(target_os="macos"))]` Linux block replaced by explicit `linux` arms while adding `windows` arms (T1 Step 2). Full parity, no limitation. **Deviation:** the spec prescribes the `windows` crate (`Management_Deployment` / `PackageManager` / `RemovePackageAsync`) for MSIX; per the controller's settled decision this plan uses PowerShell `Get-AppxPackage`/`Remove-AppxPackage` instead (no `windows` crate, no `unsafe`) — see "Notes for later" and the hand-off design-risk flag.

**Security:** caller passes ids only. Registry uninstall re-opens the exact subkey and runs the installer-authored Quiet/UninstallString (never a caller string); id shape validated (known HIVE via `registry_hive` + non-empty subkey) before touching the registry; `split_once(':')` tolerates colons in subkey names. MSIX uninstall validates the PFN against `^[A-Za-z0-9._-]+$` (`valid_pfn`) before `Remove-AppxPackage`. `validate_against_inventory(_, true)` refuses unknown ids AND `protected` (NonRemovable) entries; unknown id prefixes never reach an action arm (inventory only emits `registry:`/`msix:`). `update()` guards names against flag injection (`starts_with('-')`). Absolute PowerShell path throughout.

**Type/contract consistency:** `list`/`updates`/`uninstall`/`update` signatures identical across the three OS arms; `AppEntry`/`AppActionReport` unchanged; `commands.rs` untouched. Windows `updates()` returns `AppEntry.id`s (mapped from winget Names) so the existing `updateIds.has(app.id)` badging works without a frontend change. `id` follows the source-prefixed scheme (`registry:<HIVE>:<subkey>`, `msix:<PFN>`).

**No-regression / clippy hygiene:** Linux bodies are attribute-only edits; macOS arms untouched; `home()` and the `PathBuf` import gated off Windows, `Path` kept shared (used by `detect_msix`) — avoids `dead_code`/`unused_imports` under `-D warnings` on the Windows CI. New deps: only `winreg` under `[target.'cfg(windows)'.dependencies]`. `serde`/`serde_json` already present (derive feature already in use).

**Placeholder scan:** none. The lossy winget name-match (registry DisplayName ≠ winget package name) and non-elevated registry uninstall are documented best-effort behaviours with explicit error reporting, not stubs.

## Notes for later
- **Elevation:** machine-wide (HKLM) uninstallers needing admin run via `cmd /C` non-elevated — many self-elevate via UAC; those that don't return an error (surfaced; `requires_root` is shown in the UI). Wiring uninstall through the Phase-2 elevated-IPC path is a future enhancement.
- **winget mapping fidelity:** `update()` matches by name; registry DisplayName frequently differs from winget package Name/Id, so real update coverage can be low. Carrying a winget `--id` on `AppEntry` (contract change) would make it exact — out of scope here.
- **`windows` crate alternative:** if PowerShell `Get-AppxPackage` proves too slow at startup, the spec's `PackageManager` (windows crate, `Management_Deployment`) is the documented alternative for MSIX inventory/removal.
- **winget JSON:** if a future winget exposes stable machine-readable `upgrade` output, replace `parse_winget_upgrade_names` (table heuristic) with it.
