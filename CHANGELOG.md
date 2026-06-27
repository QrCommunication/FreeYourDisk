# Changelog

All notable changes to FreeYourDisk are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/) and the project adheres to
[Semantic Versioning](https://semver.org/).

## [0.4.5] - 2026-06-27

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

## [0.4.1] - 2026-06-25

### Added

- **Guided SMART tooling install.** The Disk health tab now detects which CLI
  tools this machine needs — `nvme-cli` for NVMe drives, `smartmontools` for
  SATA/SAS — and which are missing, then offers a **one-click privileged install**
  using the host's package manager (apt / dnf / pacman / zypper). The package set
  and manager are re-derived server-side and the helper enforces a hard-coded
  package allowlist, so the UI can never trigger an arbitrary root install.

## [0.4.0] - 2026-06-25

### Added

- **Task manager tab** (reimplements the core of the standalone `mem-guard`):
  - Real-time **CPU / RAM / swap** graph, plus CPU, temperature, RAM, swap and
    load gauges.
  - A **per-core utilization heatmap** (logical processors, green→red).
  - **CPU temperature** from the package/core sensors (Intel/AMD).
  - Sortable, filterable **process table** (CPU %, RAM %, RAM, user, PID).
  - Crisis actions: **terminate** (SIGTERM), **force** (SIGKILL), **restart**,
    and **panic-kill** the largest non-critical memory hog.
  - A **configurable global summon hotkey** (default `Ctrl+Alt+Delete`) that
    raises the window onto the task manager; the app raises its own priority and
    requests OOM immunity to stay responsive under memory pressure.

### Fixed

- The app version is now served by a backend command sourced from `Cargo.toml`,
  so the footer shows the real version regardless of core-plugin ACL.

## [0.3.0] - 2026-06-25

A major release that turns the per-service tool into a unified, accurate disk
dashboard.

### Added

- **Unified home dashboard** with a true **3D usage donut** (three.js): one
  "Scan now" launches every scan + the file-type breakdown and fills the donut
  with reclaimable (gold) and selected (green) layers.
- **File-type breakdown bar** covering the whole disk — images, videos, audio,
  archives, ISO, applications, executables, documents, **caches &
  dependencies**, **system** and **reserved (filesystem)** — each clickable to
  list its largest files with full paths.
- **Applications section**: inventory of installed apps (apt, flatpak, snap,
  AppImage) ranked by space, update checking (automatic on open), batch
  uninstall and batch update. Essential system packages are protected
  (update-only, uninstall blocked) and greyed out.
- **App & browser caches** cleanup: a new service that reclaims the regenerable
  caches missed by the `~/.cache` sweep — Chromium/Electron app caches under
  `~/.config`, Flatpak (`~/.var/app/*/cache`), Snap and npm/yarn/bun caches.
- **Disk health tab**: per-disk SMART via **nvme-cli** (NVMe) or `smartctl`
  (SATA) — health, power-on hours, temperature — plus real-time read/write
  throughput graphs and system uptime.
- **Light / dark / system theme** and **French / English / system language** in
  Settings, with **launch at startup** and a configurable **low-disk-space
  monitor** (background popup with a cleanup CTA).
- **Incremental scan engine**: a persisted, mtime-validated directory-size cache
  so rescans skip unchanged trees; the app shows the last results instantly on
  open and highlights what is new since last time.

### Changed

- The **system footprint is now measured accurately** (delegated to `du`:
  hardlink-deduplicated, block-accurate, single-filesystem) instead of a
  `used − home` residual that wrongly absorbed ext4 reserved blocks. Reserved
  filesystem blocks are shown as their own honest category.
- Full-width layout across views; larger donut; lists adapt to the window.
- Application folders are excluded from the other scans (no more app binaries in
  "Largest files").

### Security

- Batch app operations are validated server-side against the live inventory;
  AppImage deletions are allowlisted to known app directories and canonicalised;
  package commands use `--` separators and reject flag-like values.
- `nvme-cli` and `smartmontools` are recommended (not required) dependencies.

## [0.1.0] - 2026-06-25

### Added

- Initial release: scanning and safe cleanup of temporary files, largest files,
  git worktrees and developer caches.
- Recoverable-by-default deletion (XDG trash), dry-run preview, Polkit helper
  for privileged cleanup.
- System tray icon with a disk-usage popover widget.

[0.4.1]: https://github.com/QrCommunication/FreeYourDisk/releases/tag/v0.4.1
[0.4.0]: https://github.com/QrCommunication/FreeYourDisk/releases/tag/v0.4.0
[0.3.0]: https://github.com/QrCommunication/FreeYourDisk/releases/tag/v0.3.0
[0.1.0]: https://github.com/QrCommunication/FreeYourDisk/releases/tag/v0.1.0
