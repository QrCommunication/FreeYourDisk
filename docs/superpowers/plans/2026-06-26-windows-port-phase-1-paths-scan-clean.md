# Windows Port — Phase 1: Paths, Scan & Clean — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

> **Plan series.** Phase **1 of 8**. Phase 0 (compile gate) is merged to `master`. Design spec:
> `docs/superpowers/specs/2026-06-26-windows-port-design.md`. Branch for this phase: `feat/win-phase-1`.

**Goal:** On Windows, scans target the real user home, find Windows app/temp caches, and the dashboard shows a real system footprint — so non-privileged scan + cleanup (Recycle Bin) is fully functional.

**Architecture:** Phase 0 left the Windows `not(macos)`→Linux code compiling but runtime-wrong for paths. Phase 1 fixes the runtime: adopt the `dirs` crate so `Config::detect` resolves `%USERPROFILE%` on Windows (the single foundational fix — every scan reads `Config.home`); replace the `du`-based `system_total` with an internal scan-engine sum on Windows; and add Windows cache/temp roots to the `app_cache` and `temp` services. `core-trash` already uses the Recycle Bin, so deletion needs no change.

**Tech Stack:** Rust (Tauri 2 workspace), `dirs` crate, `core-scan` (jwalk walker), `core-trash`.

## Global Constraints

- **Target:** Windows 10 (1803+) and 11, x64. Do NOT regress Linux or macOS.
- **License:** keep `// SPDX-License-Identifier: GPL-3.0-or-later` on every edited file.
- **cfg rule:** when you touch a `#[cfg(not(target_os = "macos"))]` site, split it into explicit `#[cfg(target_os = "linux")]` + a `#[cfg(target_os = "windows")]` arm. Never leave `not(macos)` meaning "Linux" in code you modify.
- **`dirs` placement:** add `dirs` only to `src-tauri/Cargo.toml`. The `core-services` crate stays dependency-free for paths — its Windows arms derive locations from the passed-in `home` (`home.join("AppData/Local")` = `%LOCALAPPDATA%`, `home.join("AppData/Roaming")` = `%APPDATA%`).
- **Behavior parity:** on Linux/macOS `dirs::home_dir()` resolves the same `$HOME` as before — confirm no path change.
- **Verification:** MANDATORY local gate per task = `cargo fmt --all --check` + `cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN, and `cargo test -p <touched crate>` GREEN for crates with tests. Windows compilation is authoritatively gated by the existing `windows` CI job on the PR (local `x86_64-pc-windows-gnu` cross-check is best-effort; MinGW is absent → C-dep failure is expected, not our Rust).
- **Windows-arm tests** are `#[cfg(target_os="windows")]` and won't run in CI this phase (no Windows `cargo test` job yet) — keep cross-platform logic covered by existing OS-agnostic tests; add Windows tests only where they document intent.

---

### Task 1: Adopt `dirs`; resolve the user home cross-platform

`Config::detect` (the root every scan reads) hardcodes `$HOME` → on Windows `$HOME` is usually unset → home becomes `/`, so every scan targets the wrong place. Fix it with `dirs::home_dir()`.

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `dirs` dependency)
- Modify: `src-tauri/src/state.rs:23-33` (`Config::detect`)

**Interfaces:**
- Produces: `Config { home, search_root, .. }` where `home` = `%USERPROFILE%` on Windows, `$HOME` on unix. Consumed by every scan command and by `services::make_service`. Signature unchanged.

- [ ] **Step 1: Add the `dirs` dependency**

In `src-tauri/Cargo.toml`, under `[dependencies]` (after the `tauri-plugin-global-shortcut = "2"` line), add:

```toml
dirs = "5"
```

- [ ] **Step 2: Resolve home via `dirs::home_dir()`**

Replace `Config::detect` (state.rs:22-34):

```rust
impl Config {
    pub fn detect() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            search_root: home.clone(),
            home,
            temp_min_age_days: 7,
            big_files_top: 50,
        }
    }
}
```

with:

```rust
impl Config {
    pub fn detect() -> Self {
        // `dirs::home_dir()` resolves $HOME on unix and %USERPROFILE% on Windows.
        let home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }));
        Self {
            search_root: home.clone(),
            home,
            temp_min_age_days: 7,
            big_files_top: 50,
        }
    }
}
```

- [ ] **Step 3: Verify no Linux regression**

Run: `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: PASS (on Linux `dirs::home_dir()` returns `$HOME` — same root as before).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml Cargo.lock src-tauri/src/state.rs
git commit -m "feat(win): resolve user home via dirs::home_dir (fixes %USERPROFILE% on Windows)"
```

---

### Task 2: Replace the `du`-based `system_total` with an internal sum on Windows

`system_total` shells out to `du` over POSIX roots. On Windows `du` doesn't exist and the roots don't either (returns 0). Add a Windows path that sums the real system directories with the internal scan engine (`core_scan::cache::cached_dir_total`, already used by `home_total`). Refactor the per-OS logic into a `measure_system()` helper so the async command stays tiny.

**Files:**
- Modify: `src-tauri/src/commands.rs:220-285` (`system_total`)

**Interfaces:**
- Consumes: `core_scan::cache::cached_dir_total(&Path) -> u64` and `core_scan::cache::save(&Path)` (already used at commands.rs:213/216); `settings::config_dir()`.
- Produces: `system_total() -> Result<u64, String>` (unchanged signature); new private `measure_system() -> u64` with per-OS arms.

- [ ] **Step 1: Extract `measure_system()` and add the Windows arm**

Replace the whole `system_total` function (commands.rs:228-285, the `#[tauri::command] pub async fn system_total ...` block) with:

```rust
/// Measured OS footprint (real system size), as opposed to the `used − home`
/// residual which wrongly absorbs reserved blocks.
///
/// unix: delegates to `du` (hardlink-dedup, true blocks, single-fs `-x`).
/// Windows: `du` is absent, so we sum the system roots with the internal
/// mtime-cached walker (same engine as `home_total`). Unreadable subtrees
/// (ACL-locked) are skipped by the walker — an approximation, like `du`.
#[tauri::command]
pub async fn system_total() -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(measure_system)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
fn measure_system() -> u64 {
    #[cfg(target_os = "linux")]
    const ROOTS: &[&str] = &["/usr", "/var", "/opt", "/boot", "/srv", "/root", "/swapfile"];
    #[cfg(target_os = "macos")]
    const ROOTS: &[&str] =
        &["/System", "/Library", "/usr", "/private/var", "/opt", "/Applications"];

    let present: Vec<&str> = ROOTS
        .iter()
        .copied()
        .filter(|p| std::path::Path::new(p).exists())
        .collect();
    if present.is_empty() {
        return 0;
    }

    // GNU du reports bytes (`--block-size=1`); BSD du (macOS) only does
    // 1024-byte blocks (`-k`), so scale there.
    #[cfg(target_os = "linux")]
    let (du_args, mult): (&[&str], u64) = (&["-scx", "--block-size=1"], 1);
    #[cfg(target_os = "macos")]
    let (du_args, mult): (&[&str], u64) = (&["-scxk"], 1024);

    let Ok(out) = std::process::Command::new("du")
        .args(du_args)
        .args(&present)
        .output()
    else {
        return 0;
    };
    // The last line is "<n>\ttotal".
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .and_then(|line| line.split_whitespace().next())
        .and_then(|n| n.parse::<u64>().ok())
        .map(|v| v * mult)
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn measure_system() -> u64 {
    // No `du` on Windows: sum the real system roots with the internal walker.
    const ROOTS: &[&str] = &[
        "C:\\Windows",
        "C:\\Program Files",
        "C:\\Program Files (x86)",
        "C:\\ProgramData",
    ];
    let total: u64 = ROOTS
        .iter()
        .map(std::path::Path::new)
        .filter(|p| p.exists())
        .map(core_scan::cache::cached_dir_total)
        .sum();
    core_scan::cache::save(&settings::config_dir().join("dir-cache.json"));
    total
}
```

Note: the only behavioral change on unix is the `not(target_os="macos")` arm becoming `target_os="linux"` (explicit) — identical code, per the cfg rule.

- [ ] **Step 2: Verify the unix path is unchanged + compiles**

Run: `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: PASS. (Confirm `cached_dir_total` accepts `&Path` — it is called as `core_scan::cache::cached_dir_total(&home)` at commands.rs:213, so `.map(core_scan::cache::cached_dir_total)` over `&Path` items type-checks; if the signature is `&PathBuf`, change the map to `.map(|p| core_scan::cache::cached_dir_total(p))` with `p: &Path` via `std::path::Path::new`. Adjust only if the compiler complains.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(win): system_total via internal scan of Windows system roots (du replacement)"
```

---

### Task 3: Windows app-cache roots in `app_cache`

The app-cache service scans XDG/Flatpak/Snap locations. On Windows, regenerable app/browser caches live under `%LOCALAPPDATA%` and `%APPDATA%` (Chrome/Edge/Electron `Cache`/`Code Cache`/`GPUCache` dirs). Add a Windows arm that runs the existing `collect()` over those roots. The npm/yarn/bun home-relative caches (`.npm/.yarn/.bun`) already work on Windows (under `%USERPROFILE%`).

**Files:**
- Modify: `crates/core-services/src/app_cache.rs:120-132` (the macOS block in `scan`)

**Interfaces:**
- Consumes: `Self::collect(root: &Path, depth, &mut Vec<ScanItem>)`, `self.home` (= `%USERPROFILE%` from Task 1). Produces additional cache `ScanItem`s on Windows.

- [ ] **Step 1: Add the Windows cache-roots block**

In `crates/core-services/src/app_cache.rs`, immediately after the existing macOS block (which ends at line 132 with its closing `}` for `#[cfg(target_os = "macos")] { ... }`), add:

```rust
        // 6. Windows: Chromium/Electron app caches under %LOCALAPPDATA% and
        //    %APPDATA% (e.g. Chrome/Edge/Electron Cache, Code Cache, GPUCache).
        #[cfg(target_os = "windows")]
        {
            Self::collect(&h.join("AppData/Local"), 4, &mut items);
            Self::collect(&h.join("AppData/Roaming"), 4, &mut items);
        }
```

(Place it after the macOS block and before the `let total_bytes = ...` line.)

- [ ] **Step 2: Verify no regression**

Run: `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings && cargo test -p core-services`
Expected: PASS (the existing `finds_nested_browser_cache` test is OS-agnostic and unaffected; the Windows block is inert on Linux).

- [ ] **Step 3: Commit**

```bash
git add crates/core-services/src/app_cache.rs
git commit -m "feat(win): scan Windows app caches under %LOCALAPPDATA%/%APPDATA%"
```

---

### Task 4: Windows temp roots in `temp`

`TempService::with_defaults` adds `/tmp`, `/var/tmp` (root) and `~/.cache` (user), plus a macOS arm. On Windows the equivalents are `%LOCALAPPDATA%\Temp` (user) and `%WINDIR%\Temp` (root). Add a Windows arm.

**Files:**
- Modify: `crates/core-services/src/temp.rs:26-53` (`with_defaults`)

**Interfaces:**
- Produces: `TempService::with_defaults(home: &Path, min_age_days) -> TempService` with Windows temp roots. Signature unchanged.

- [ ] **Step 1: Add the Windows temp-roots arm**

In `crates/core-services/src/temp.rs`, the macOS arm currently reads:

```rust
        // macOS keeps user caches under ~/Library/Caches.
        #[cfg(target_os = "macos")]
        roots.push(TempRoot {
            path: home.join("Library/Caches"),
            requires_root: false,
        });
```

Add, immediately after it (before `Self { roots, min_age_days }`):

```rust
        // Windows: %LOCALAPPDATA%\Temp (user) and %WINDIR%\Temp (privileged).
        #[cfg(target_os = "windows")]
        {
            roots.push(TempRoot {
                path: home.join("AppData/Local/Temp"),
                requires_root: false,
            });
            let windir = std::env::var_os("WINDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("C:\\Windows"));
            roots.push(TempRoot {
                path: windir.join("Temp"),
                requires_root: true,
            });
        }
```

Note: the existing `/tmp` and `/var/tmp` `TempRoot`s remain in the vec on Windows but are harmless — they don't exist, so `scan()` skips them (its `read_dir` returns `Err` and `continue`s). Leaving them avoids restructuring the shared base vec; the Windows roots are simply appended, mirroring the macOS pattern.

- [ ] **Step 2: Verify no regression**

Run: `cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings && cargo test -p core-services`
Expected: PASS (the `#[allow(unused_mut)]` on `roots` already covers the case; existing temp tests are OS-agnostic).

- [ ] **Step 3: Commit**

```bash
git add crates/core-services/src/temp.rs
git commit -m "feat(win): scan Windows temp roots (%LOCALAPPDATA%\\Temp + %WINDIR%\\Temp)"
```

---

## Self-Review

**1. Spec coverage (Phase 1 = spec §10 step 1 + §5.1/§5.2):**
- Paths: `dirs` adoption + `%USERPROFILE%` home — Task 1. ✓ (settings.rs `config_dir` already Windows-correct from Phase 0; left as-is to avoid changing Linux/macOS paths — documented deviation: full `dirs::config_dir` unification deferred as unnecessary.)
- Scan engine: portable already (no task needed). ✓
- `du` replacement (internal sum) — Task 2. ✓
- Windows caches (`%LOCALAPPDATA%`) — Task 3. ✓
- Temp roots — Task 4. ✓
- Recycle Bin deletion: `core-trash` already handles it — no task (verified Phase 0). ✓
- Headless cache path (spec §5.1 mentions): DEFERRED to Phase 6 (headless is only invoked by the scheduler, which doesn't exist on Windows until Phase 6 — out of Phase 1's interactive scan/clean scope). Documented.

**2. Placeholder scan:** No TBD/TODO. Task 2 Step 2 contains a conditional fallback instruction (adjust the `.map` form only if the compiler complains about `&Path` vs `&PathBuf`) — this is a concrete, bounded contingency, not a placeholder; the primary form is given in full.

**3. Type consistency:** `Config.home: PathBuf` (Task 1) feeds `services::make_service(&cfg.home)` → `AppCacheService::new(home)` and `TempService::with_defaults(&home)` (Tasks 3/4) and `system_total` reads system roots independently (Task 2). `measure_system() -> u64` matches `spawn_blocking`'s expected return. All consistent.

## Notes for later phases (not this plan)
- Phase 6: Windows headless cache path + Task Scheduler + autostart Run-key + toasts.
- Optional: unify `settings::config_dir` via `dirs::config_dir()` (would collapse the 3 cfg arms) — only if it does not change the existing Linux `freeyourdisk` / macOS `FreeYourDisk` directory names.
