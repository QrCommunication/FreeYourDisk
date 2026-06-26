# Windows Port — Phase 7: Packaging & Release — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

> **Plan series.** Phase **7 of 8** (final). Phases 0–6 merged. Branch: `feat/win-phase-7`.

**Goal:** Ship-ready Windows release: version bumped to 0.5.0, a CHANGELOG entry documenting full Windows support, a README Windows section, and a final polish of accumulated cosmetic minors. The unsigned NSIS installer (already wired in `windows.yml`) is validated end-to-end via `workflow_dispatch`.

**Architecture:** This is a release/docs/polish phase — no new platform runtime code. Version lives in the workspace `Cargo.toml` (`[workspace.package] version`) and `ui/package.json`; `tauri.conf.json` has no `version` key so it inherits from Cargo. The NSIS installer build is `windows.yml` (`cargo tauri build --bundles nsis`, triggered on `v*` tags / manual).

**Tech Stack:** TOML/JSON/Markdown; existing Rust/Svelte (cosmetic touches only).

## Global Constraints

- Do NOT regress Linux/macOS/Windows. Cosmetic/doc changes only in code; no behavior change.
- Keep `// SPDX-License-Identifier: GPL-3.0-or-later`.
- Version is a single source bumped consistently: workspace `Cargo.toml` + `ui/package.json` (both 0.4.1 → 0.5.0). `tauri.conf.json` inherits from Cargo (no version key — leave it).
- Verification per task: `cargo fmt --all --check` + `cargo clippy -p freeyourdisk --all-targets -- -D warnings` GREEN (for code touches); `pnpm --dir ui check` + `pnpm --dir ui build` GREEN (for frontend touches); CHANGELOG/README are prose (no build impact).

---

### Task 1: Version bump 0.5.0 + CHANGELOG

**Files:** Modify `Cargo.toml` (`[workspace.package] version`), `ui/package.json` (`version`), `CHANGELOG.md` (new top entry).

**Interfaces:** none (release metadata).

- [ ] **Step 1: Bump workspace Cargo version**

In `Cargo.toml`, change the `[workspace.package]` `version = "0.4.1"` to `version = "0.5.0"`.

- [ ] **Step 2: Bump UI version**

In `ui/package.json`, change `"version": "0.4.1"` to `"version": "0.5.0"`.

- [ ] **Step 3: CHANGELOG entry**

Insert this entry in `CHANGELOG.md` immediately above the `## [0.4.1] - 2026-06-25` entry:

```markdown
## [0.5.0] - 2026-06-27

### Added

- **Windows 10/11 support (full feature parity).** FreeYourDisk now runs natively
  on Windows alongside Linux and macOS, with the same dashboard, disk breakdown,
  application manager, disk-health/SMART, task manager, scheduling and low-space
  monitor:
  - **Paths & scan.** Disk usage, app caches (`%LOCALAPPDATA%`/`%APPDATA%`) and
    temp (`%LOCALAPPDATA%\Temp`, `%WINDIR%\Temp`) are enumerated with Windows-aware
    roots; system size sums the real Windows system roots.
  - **Privileged cleanup.** Elevation uses a UAC self-relaunch (PowerShell
    `Start-Process -Verb RunAs`) into a headless `--apply` mode — no bundled
    service. Deletions are re-validated against a hard-coded `C:\Windows\Temp`
    zone; the elevated IPC uses unguessable random-nonce temp paths.
  - **Applications.** Inventory from the registry Uninstall keys + MSIX/Store
    packages (`Get-AppxPackage`); uninstall via the app's own uninstaller /
    `Remove-AppxPackage`; update detection + best-effort update via `winget`.
  - **Disk health / SMART.** Disk list + uptime via `sysinfo`; SMART read through
    an elevated `smartctl` (guided `winget install smartmontools`).
  - **Task manager.** Process termination (`TerminateProcess` via `sysinfo`) with
    a Windows critical-process safelist.
  - **Scheduling & UX.** Weekly cleanup via the Task Scheduler (`schtasks`);
    autostart via the `HKCU\…\Run` key; native WinRT toast notifications.
- **Unsigned NSIS installer** built on a Windows runner (`windows.yml`), bundling
  the WebView2 bootstrapper.

### Notes

- The Windows installer is unsigned (no Authenticode certificate); SmartScreen may
  warn on first run. Code signing can be added later via certificate secrets.
- Toast notifications currently display under the PowerShell app identity; a
  dedicated Start-menu AppUserModelID is a future refinement.
```

- [ ] **Step 4: Verify** — `cargo fmt --all --check` (no Rust change, just confirm clean); `pnpm --dir ui check` (package.json version change is inert). Confirm `cargo metadata --no-deps --format-version 1 | grep -o '"version":"0.5.0"'` resolves (workspace picked up the bump).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock ui/package.json CHANGELOG.md
git commit -m "chore(release): v0.5.0 — Windows 10/11 support"
```

---

### Task 2: README Windows section

**Files:** Modify `README.md`.

**Interfaces:** none (docs).

- [ ] **Step 1: Add a Windows section**

Add a "Windows" subsection to README.md under the existing install/platform documentation (mirror the structure used for Linux/macOS). Cover:
- **Requirements:** Windows 10 (1803+) or 11, x64. WebView2 runtime (the installer bundles the bootstrapper, which fetches it if absent — needs internet on first install).
- **Install:** download the unsigned `*-setup.exe` from the latest release (or build it — see below); SmartScreen may warn (unsigned) → "More info" → "Run anyway".
- **Features:** same as Linux/macOS — dashboard, disk breakdown, applications (registry + Microsoft Store), disk health/SMART (`winget install smartmontools` for SMART), task manager, weekly scheduled cleanup, autostart, low-space toasts. Privileged cleanup prompts via UAC.
- **Build from source:** `pnpm --dir ui install && pnpm --dir ui build` then `cargo tauri build --bundles nsis` (Rust + Node 22 + the Tauri CLI). Note the `--bundles nsis` flag is required.

Match the README's existing tone/heading depth. Do not restructure unrelated sections.

- [ ] **Step 2: Verify** — render-check the Markdown (headings balanced, code fences closed). No build impact.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(win): README Windows install + features + build-from-source"
```

---

### Task 3: Final polish of cosmetic minors

**Files:** Modify `src-tauri/src/applications.rs` (stale doc comments), `src-tauri/src/commands.rs` (Linux schedule doc orphan), `ui/src/lib/api.ts` + `ui/src/lib/views/Applications.svelte` (add macOS `app` source).

**Interfaces:** none (cosmetic + completing an enum).

- [ ] **Step 1: Refresh applications.rs stale docs**

In `applications.rs`, update the module doc and the `AppEntry.source` field comment so they list all sources, not just the Linux ones. Change the `source` comment from the Linux-only list to: `"apt" | "flatpak" | "snap" | "appimage" | "app" (macOS) | "registry" | "msix" (Windows)`. Update the module-level doc similarly if it enumerates sources. No code change.

- [ ] **Step 2: Strip the macOS parenthetical from the Linux schedule doc**

In `commands.rs`, the `#[cfg(target_os = "linux")]` `schedule_enabled` doc comment reads "…systemd on Linux, launchd on macOS." Since this arm is now Linux-only, change it to just describe Linux (e.g., "Whether the weekly cleanup systemd user timer is enabled."). The macOS arm has its own doc.

- [ ] **Step 3: Add the macOS `app` source to the frontend AppSource**

In `ui/src/lib/api.ts`, add `"app"` to the `AppSource` union (the macOS backend emits `source: "app"` for `/Applications` bundles). In `ui/src/lib/views/Applications.svelte`, add an `app` entry to `SOURCE_COLOR` (a macOS-appropriate colour, e.g. a neutral grey `#8e8e93`) and ensure its label renders. This removes the uncolored-badge gap for macOS.

- [ ] **Step 4: Verify**

```bash
cargo fmt --all && cargo clippy -p freeyourdisk --all-targets -- -D warnings   # GREEN (doc-only Rust)
pnpm --dir ui check && pnpm --dir ui build                                       # GREEN (AppSource change)
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/applications.rs src-tauri/src/commands.rs ui/src/lib/api.ts ui/src/lib/views/Applications.svelte
git commit -m "polish(win): refresh app-source docs, Linux schedule doc, add macOS 'app' source label"
```

---

## Self-Review

**Spec coverage:** version bump (Task 1), CHANGELOG documenting all 6 feature phases (Task 1), README Windows section (Task 2), cosmetic polish of accumulated minors (Task 3). The NSIS installer build is validated by the controller via `workflow_dispatch` on `windows.yml` (the per-push CI clippy gate only compiles — it never builds the installer).

**Placeholder scan:** none. The unsigned-installer and AUMID items are documented release notes (intentional, deferred), not placeholders.

**Type consistency:** version "0.5.0" applied to both Cargo workspace + ui/package.json (tauri.conf inherits). `AppSource` union gains `"app"`; `SOURCE_COLOR` gains the matching key (frontend exhaustiveness improved).

## Notes for later (explicitly deferred, non-blocking)
- Authenticode code signing of the NSIS installer (needs a certificate).
- A dedicated FreeYourDisk AppUserModelID + Start-menu shortcut so toasts show the app name (Phase-6 toast reuses the PowerShell AUMID).
- smartctl detection (`smartdeps` PATH+ProgramFiles) vs the elevated reader (ProgramFiles-only) can disagree for PATH-only smartctl installs (Phase-3 minor; winget happy-path is fine).
- macOS privhelper `ROOT_ZONES` `/tmp` not canonicalized to `/private/tmp` (pre-existing macOS bug, not Windows).
