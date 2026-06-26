# Windows Port — Phase 0: Compile Gate — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Plan series.** This is **Phase 0 of 8** of the Windows 10/11 port. Design spec:
> `docs/superpowers/specs/2026-06-26-windows-port-design.md`. Each later phase
> (1 = paths/scan/clean, 2 = elevation, 3 = SMART/health, 4 = task manager,
> 5 = applications, 6 = scheduling/UX, 7 = packaging/CI polish) gets its own plan,
> written when reached so its code reflects the real state of the tree.

**Goal:** Make FreeYourDisk compile on Windows, gate all three OSes in CI, and have the Windows app launch cleanly with gracefully-degraded features.

**Architecture:** The codebase uses inline `#[cfg(target_os = ...)]` at function granularity, where `#[cfg(not(target_os = "macos"))]` currently means "Linux". Most Linux code (`std::fs`, `std::process::Command`, `std::env`) already compiles for the Windows target and only misbehaves at runtime — acceptable for Phase 0. The single true compile-blocker is `libc` usage in `taskmgr.rs`. This phase gates `libc` behind `cfg(unix)`, adds Windows arms for the cheap path/shortcut forks, configures the NSIS bundle, and wires a Windows CI job.

**Tech Stack:** Rust (Tauri 2 workspace), Svelte/Vite frontend, GitHub Actions, NSIS (Tauri bundler), WebView2.

## Global Constraints

- **Target:** Windows 10 (build 1803+) and Windows 11, x64. (Do not regress Linux or macOS.)
- **License:** GPL-3.0-or-later (`// SPDX-License-Identifier: GPL-3.0-or-later` header on every new/edited Rust file is already present — keep it).
- **cfg rule:** When you touch a `#[cfg(not(target_os = "macos"))]` site, split it into explicit `#[cfg(target_os = "linux")]` + a `#[cfg(target_os = "windows")]` arm. Never leave `not(macos)` meaning "Linux" in code you modify.
- **No new runtime deps in this phase.** Native Windows crates (`windows`, `winreg`, `wmi`) are added in the later phases that consume them (YAGNI).
- **Toolchain floors:** Rust stable, Node 22, pnpm 10 (match existing CI).
- **CI scope on Windows:** compile/lint gate only (`cargo clippy -p freeyourdisk --all-targets -D warnings`) + NSIS installer on tags. Do **not** run `cargo test` on Windows in this phase (deferred — some unit tests assume a POSIX runtime).

---

### Task 1: Gate `libc` behind `cfg(unix)`

The only thing that fails to **compile** for the Windows target. Move the `libc` dependency to a unix-only target table and split the two functions in `taskmgr.rs` that call it.

**Files:**
- Modify: `src-tauri/Cargo.toml` (lines 13-24, deps block)
- Modify: `src-tauri/src/taskmgr.rs:150-155` (`kill_process`) and `:263-270` (`raise_priority`)

**Interfaces:**
- Produces: `taskmgr::kill_process(pid: u32, force: bool) -> bool` (unix: real signal; windows: stub returning `false`); `taskmgr::raise_priority()` (unix: nice; windows: no-op). Signatures are identical across OSes so callers (`restart_process`, `panic_kill`, `main.rs`) are unchanged.

- [ ] **Step 1: Move `libc` to a unix-only dependency table**

In `src-tauri/Cargo.toml`, delete the line `libc = "0.2"` from `[dependencies]` (currently line 23) and add a new target table immediately after the `[dependencies]` block (before `[dev-dependencies]`):

```toml
[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

Resulting tail of the file:

```toml
sysinfo = "0.39"
jwalk = { workspace = true }
tauri-plugin-global-shortcut = "2"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 2: Split `kill_process` into unix + windows arms**

Replace the current `kill_process` (taskmgr.rs:150-155):

```rust
/// Send SIGTERM (graceful) or SIGKILL (force) to a pid.
pub fn kill_process(pid: u32, force: bool) -> bool {
    let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
    // Direct syscall: works regardless of the sampler's refresh state.
    unsafe { libc::kill(pid as i32, sig) == 0 }
}
```

with:

```rust
/// Send SIGTERM (graceful) or SIGKILL (force) to a pid.
#[cfg(unix)]
pub fn kill_process(pid: u32, force: bool) -> bool {
    let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
    // Direct syscall: works regardless of the sampler's refresh state.
    unsafe { libc::kill(pid as i32, sig) == 0 }
}

/// Windows stub. Real TerminateProcess / WM_CLOSE arrives in the Task-manager
/// phase (Phase 4); returning `false` keeps `panic_kill`/`restart_process`
/// harmless until then.
#[cfg(windows)]
pub fn kill_process(_pid: u32, _force: bool) -> bool {
    false
}
```

- [ ] **Step 3: Gate the `libc::setpriority` call in `raise_priority`**

Replace the current `raise_priority` (taskmgr.rs:263-270):

```rust
pub fn raise_priority() {
    // OOM immunity is a Linux concept; macOS has no per-process oom score.
    #[cfg(target_os = "linux")]
    let _ = std::fs::write("/proc/self/oom_score_adj", "-1000");
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, -5);
    }
}
```

with:

```rust
pub fn raise_priority() {
    // OOM immunity is a Linux concept; macOS/Windows have no per-process oom score.
    #[cfg(target_os = "linux")]
    let _ = std::fs::write("/proc/self/oom_score_adj", "-1000");
    // Negative nice needs privilege on Unix; failures are ignored. Windows gets
    // SetPriorityClass in the Task-manager phase (Phase 4).
    #[cfg(unix)]
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, -5);
    }
}
```

- [ ] **Step 4: Verify no Linux regression**

Run: `cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: PASS (Linux build unchanged; `cfg(unix)` arms are active on Linux).

- [ ] **Step 5: Verify the Windows target now compiles past the `libc` blocker**

One-time setup (local cross-check; CI is the authoritative gate):

Run: `rustup target add x86_64-pc-windows-gnu`
Run: `pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build` (build.rs/`generate_context!` needs `ui/dist`)
Run: `cargo check -p freeyourdisk --target x86_64-pc-windows-gnu`
Expected: the previous `error[E0425]: cannot find value SIGKILL in crate libc` (and siblings) are GONE. If unrelated dependency errors appear that are not about our source, note them and rely on the CI Windows job (Task 5) as the authoritative check.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/taskmgr.rs
git commit -m "feat(win): gate libc behind cfg(unix) so taskmgr compiles on Windows"
```

---

### Task 2: Windows arms for paths, shortcut default, and autostart

`settings.rs` decides the config directory, the default global-shortcut, and the autostart entry. On Windows the current `not(macos)` (Linux) arms would write under `C:\.config` and default the hotkey to the un-registrable `Ctrl+Alt+Delete`. Give Windows correct, registrable behavior so the app launches and persists settings cleanly.

**Files:**
- Modify: `src-tauri/src/settings.rs:26-34` (`default_shortcut`), `:49-68` (`config_dir` + `home`), `:92-117` (`apply_autostart`)

**Interfaces:**
- Produces: `settings::config_dir() -> PathBuf` (windows: `%APPDATA%\FreeYourDisk`), `settings::default_shortcut() -> String` (windows: `"Ctrl+Shift+M"`), `settings::apply_autostart(enabled: bool) -> Result<(), String>` (windows: no-op `Ok(())` until Phase 6). Signatures unchanged across OSes.

- [ ] **Step 1: Split `default_shortcut` and add the Windows arm**

Replace settings.rs:26-34:

```rust
#[cfg(not(target_os = "macos"))]
fn default_shortcut() -> String {
    "Ctrl+Alt+Delete".to_string()
}

#[cfg(target_os = "macos")]
fn default_shortcut() -> String {
    "Cmd+Shift+M".to_string()
}
```

with:

```rust
#[cfg(target_os = "linux")]
fn default_shortcut() -> String {
    "Ctrl+Alt+Delete".to_string()
}

#[cfg(target_os = "macos")]
fn default_shortcut() -> String {
    "Cmd+Shift+M".to_string()
}

// Windows: Ctrl+Alt+Delete is the Secure Attention Sequence and cannot be
// registered by an app (silent failure), so use a capturable default.
#[cfg(target_os = "windows")]
fn default_shortcut() -> String {
    "Ctrl+Shift+M".to_string()
}
```

- [ ] **Step 2: Split `config_dir`, add Windows arm, and make `home()` cross-platform**

Replace settings.rs:49-68 (the `config_dir` pair plus the `home` helper):

```rust
/// Per-user config dir for FreeYourDisk. Also used by the snapshot store.
/// Linux: XDG (`~/.config/freeyourdisk`). macOS: `~/Library/Application Support`.
#[cfg(not(target_os = "macos"))]
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("freeyourdisk")
}

#[cfg(target_os = "macos")]
pub fn config_dir() -> PathBuf {
    home().join("Library/Application Support/FreeYourDisk")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
```

with:

```rust
/// Per-user config dir for FreeYourDisk. Also used by the snapshot store.
/// Linux: XDG (`~/.config/freeyourdisk`). macOS: `~/Library/Application Support`.
/// Windows: `%APPDATA%\FreeYourDisk`.
#[cfg(target_os = "linux")]
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("freeyourdisk")
}

#[cfg(target_os = "macos")]
pub fn config_dir() -> PathBuf {
    home().join("Library/Application Support/FreeYourDisk")
}

#[cfg(target_os = "windows")]
pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("AppData/Roaming"))
        .join("FreeYourDisk")
}

#[cfg(unix)]
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(windows)]
fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"))
}
```

- [ ] **Step 3: Split `apply_autostart` and add a Windows no-op stub**

In settings.rs, change the Linux `apply_autostart` attribute from `#[cfg(not(target_os = "macos"))]` to `#[cfg(target_os = "linux")]` (line 94, the attribute directly above `pub fn apply_autostart`). Leave the macOS arm unchanged. Then add this Windows arm immediately after the macOS `apply_autostart` function (after settings.rs:143):

```rust
// Windows autostart (HKCU\...\Run) lands in Phase 6; no-op for now so the
// settings save path succeeds.
#[cfg(target_os = "windows")]
pub fn apply_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}
```

- [ ] **Step 4: Verify no Linux regression**

Run: `cargo clippy -p freeyourdisk --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Verify the Windows target still checks clean**

Run: `cargo check -p freeyourdisk --target x86_64-pc-windows-gnu`
Expected: no errors from `settings.rs` (the `windows` arms resolve; `home()`/`config_dir()`/`default_shortcut()` each have exactly one active arm).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "feat(win): Windows config dir (%APPDATA%), capturable shortcut default, autostart stub"
```

---

### Task 3: Gate the unix-only test in `core-trash`

`crates/core-trash/src/lib.rs:149-159` has a test that calls `std::os::unix::fs::symlink`, which does not exist on Windows. It is not compiled by `cargo clippy -p freeyourdisk` (core-trash is a dependency, its tests aren't built), but gating it now unblocks a future `cargo test --workspace` / `clippy --workspace --all-targets` on Windows. One-line attribute.

**Files:**
- Modify: `crates/core-trash/src/lib.rs:149` (add attribute above the test fn)

**Interfaces:**
- Produces: nothing consumed by other tasks; this is hygiene that keeps the workspace test suite Windows-compilable.

- [ ] **Step 1: Add `#[cfg(unix)]` to the symlink-escape test**

Change (core-trash/src/lib.rs:149-150):

```rust
    #[test]
    fn validate_rejects_symlink_escaping_zone() {
```

to:

```rust
    #[cfg(unix)] // uses std::os::unix::fs::symlink
    #[test]
    fn validate_rejects_symlink_escaping_zone() {
```

- [ ] **Step 2: Verify Linux tests still pass (test still runs on Linux)**

Run: `cargo test -p core-trash`
Expected: PASS, and `validate_rejects_symlink_escaping_zone` is listed/run (it is `cfg(unix)`, active on Linux).

- [ ] **Step 3: Commit**

```bash
git add crates/core-trash/src/lib.rs
git commit -m "test(win): gate unix-only symlink test behind cfg(unix)"
```

---

### Task 4: Add the Windows NSIS bundle configuration

`tauri.conf.json` has `bundle.linux` and `bundle.macOS` but no `bundle.windows`. Add a minimal Windows bundle section so `cargo tauri build --bundles nsis` produces a self-contained installer that works on Win10 < 1803 (no preinstalled WebView2). The `icon.ico` referenced in `bundle.icon` already exists.

**Files:**
- Modify: `src-tauri/tauri.conf.json:98-105` (add a `windows` key in the `bundle` object, after `macOS`)

**Interfaces:**
- Produces: a `bundle.windows.webviewInstallMode` config consumed by the Tauri bundler in Task 5's CI job.

- [ ] **Step 1: Add the `bundle.windows` section**

In `src-tauri/tauri.conf.json`, the `bundle` object currently ends with the `macOS` block (lines 98-105):

```json
    "macOS": {
      "minimumSystemVersion": "11.0",
      "dmg": {
        "appPosition": { "x": 180, "y": 220 },
        "applicationFolderPosition": { "x": 480, "y": 220 },
        "windowSize": { "width": 660, "height": 420 }
      }
    }
  }
}
```

Add a `windows` sibling after `macOS` (mind the comma after the `macOS` block):

```json
    "macOS": {
      "minimumSystemVersion": "11.0",
      "dmg": {
        "appPosition": { "x": 180, "y": 220 },
        "applicationFolderPosition": { "x": 480, "y": 220 },
        "windowSize": { "width": 660, "height": 420 }
      }
    },
    "windows": {
      "webviewInstallMode": { "type": "embedBootstrapper" }
    }
  }
}
```

- [ ] **Step 2: Validate the JSON is well-formed**

Run: `node -e "JSON.parse(require('fs').readFileSync('src-tauri/tauri.conf.json','utf8')); console.log('valid')"`
Expected: `valid`

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "build(win): add NSIS bundle config (embedBootstrapper WebView2)"
```

---

### Task 5: Wire Windows CI — PR compile gate + tagged installer build

Two pieces, mirroring the existing `ci.yml` (PR gate) and `macos.yml` (release artifact): a `windows` job added to `ci.yml` that compile-gates every PR, and a new `windows.yml` that builds the unsigned NSIS installer on version tags.

**Files:**
- Modify: `.github/workflows/ci.yml` (add a `windows` job after the `rust` job)
- Create: `.github/workflows/windows.yml`

**Interfaces:**
- Consumes: the `bundle.windows` config (Task 4) and the compiling Windows source (Tasks 1-3).
- Produces: a CI gate that fails any PR breaking the Windows build, and `FreeYourDisk_<ver>_x64-setup.exe` attached to GitHub Releases on tags.

- [ ] **Step 1: Add a Windows compile-gate job to `ci.yml`**

Append this job to `.github/workflows/ci.yml` under `jobs:` (a sibling of `rust:` and `front:`):

```yaml
  windows:
    name: Windows (clippy compile gate)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v7

      - uses: actions/setup-node@v4
        with:
          node-version: '22'
      - uses: pnpm/action-setup@v6
        with:
          version: 10

      - name: Build frontend (required to compile src-tauri)
        working-directory: ui
        run: |
          pnpm install --frozen-lockfile
          pnpm build

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2

      - name: clippy (app only — privhelper is Linux/macOS)
        run: cargo clippy -p freeyourdisk --all-targets -- -D warnings
```

- [ ] **Step 2: Create the tagged-release Windows installer workflow**

Create `.github/workflows/windows.yml`:

```yaml
name: Windows build

# Builds an UNSIGNED NSIS installer on a GitHub Windows runner. Trigger
# manually, or on version tags. Code signing (Authenticode) can be added later
# via certificate secrets.

on:
  workflow_dispatch:
  push:
    tags: ['v*']

permissions:
  contents: write

jobs:
  build:
    name: Build Windows installer (NSIS, unsigned)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v7

      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - uses: actions/setup-node@v4
        with:
          node-version: '22'
      - uses: pnpm/action-setup@v6
        with:
          version: 10

      - name: Build frontend
        working-directory: ui
        run: |
          pnpm install --frozen-lockfile
          pnpm build

      - name: Install Tauri CLI
        run: cargo install tauri-cli --locked

      - name: Build NSIS installer (unsigned)
        run: cargo tauri build --bundles nsis

      - name: Upload installer artifact
        uses: actions/upload-artifact@v4
        with:
          name: freeyourdisk-windows
          path: target/release/bundle/nsis/*-setup.exe

      - name: Attach to release (on tag)
        if: startsWith(github.ref, 'refs/tags/')
        uses: softprops/action-gh-release@v3
        with:
          files: target/release/bundle/nsis/*-setup.exe
```

- [ ] **Step 3: Validate workflow YAML locally (if `actionlint` is available)**

Run: `actionlint .github/workflows/ci.yml .github/workflows/windows.yml`
Expected: no errors. (If `actionlint` is not installed, skip — the push in Step 4 validates on GitHub.)

- [ ] **Step 4: Commit and push to a branch to trigger the gate**

```bash
git add .github/workflows/ci.yml .github/workflows/windows.yml
git commit -m "ci(win): Windows clippy gate on PRs + NSIS installer build on tags"
git push -u origin feat/windows-port
```

- [ ] **Step 5: Confirm the Windows CI job is green**

Run: `gh run list --branch feat/windows-port --limit 3`
Then: `gh run watch` (or open the run in the browser).
Expected: the `windows` job in `CI` passes (`clippy -p freeyourdisk --all-targets -D warnings` succeeds on `windows-latest`). This is the authoritative proof the Windows build compiles.

- [ ] **Step 6 (optional, manual): produce and smoke-test the installer**

Run: `gh workflow run "Windows build"` (dispatches `windows.yml`), then download the `freeyourdisk-windows` artifact and, on a Windows 10/11 machine, install and launch it.
Expected: the app installs and the FreeYourDisk window opens. Features are degraded (no real SMART, app inventory empty, scheduling/notifications inert) — this is the intended Phase 0 outcome. Verify it does not crash on launch.

---

## Self-Review

**1. Spec coverage (Phase 0 = spec §10 step 0 + §3 cfg-split + §6/§8 build/CI bootstrap):**
- Split `not(macos)` → `linux`/`windows` for touched sites — Tasks 1 (taskmgr), 2 (settings). ✓
- Keep `libc` under `cfg(unix)` — Task 1. ✓
- Windows app builds & launches degraded — Tasks 1-4 (compile) + Task 5 step 6 (launch). ✓
- CI build/clippy gate on `windows-latest` — Task 5. ✓
- NSIS bundle config — Task 4. ✓
- Native deps (`windows`/`winreg`/`wmi`): intentionally deferred to consuming phases per Global Constraints (documented deviation from spec §6, which front-loaded them; YAGNI). ✓
- `default_shortcut` Windows fix (spec §5.11) brought forward into Phase 0 because it is a one-line change at a site already being split. ✓

**2. Placeholder scan:** No "TBD"/"implement later" in steps. The `kill_process`/`apply_autostart` Windows bodies are deliberate, complete stubs (return `false` / `Ok(())`) with a phase reference — they are valid compiling code, not placeholders.

**3. Type consistency:** `kill_process(u32, bool) -> bool`, `raise_priority()`, `config_dir() -> PathBuf`, `default_shortcut() -> String`, `apply_autostart(bool) -> Result<(), String>` keep identical signatures across all OS arms, so existing callers (`main.rs`, `restart_process`, `panic_kill`, `save`) compile unchanged on every target. ✓

## Notes for later phases (not this plan)

- Phase 4 replaces the `kill_process`/`raise_priority` Windows stubs with `TerminateProcess`/`SetPriorityClass` (adds the `windows` crate) and adds a Windows `PROTECTED` process list.
- Phase 6 replaces the `apply_autostart` Windows stub with an `HKCU\...\Run` registry entry and adds Task Scheduler integration.
- Phase 1 adopts the `dirs` crate / refines `%LOCALAPPDATA%` cache paths and replaces the `du`-based system-footprint measurement.
