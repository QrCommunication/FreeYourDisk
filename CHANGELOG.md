# Changelog

All notable changes to FreeYourDisk are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/) and the project adheres to
[Semantic Versioning](https://semver.org/).

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

[0.3.0]: https://github.com/QrCommunication/FreeYourDisk/releases/tag/v0.3.0
[0.1.0]: https://github.com/QrCommunication/FreeYourDisk/releases/tag/v0.1.0
