# FreeYourDisk

> Free your disk, safely.

**English** · [Français](README.fr.md)

A modern Linux desktop utility that scans your disk and **safely** reclaims
space: temporary files, oversized files, stale git worktrees, developer caches,
**installed applications** and a **file-type breakdown** — around a 3D usage
donut, with a recoverable-by-default deletion model.

Built with **Tauri** (Rust core + WebView), licensed **GPL-3.0-or-later**.

![FreeYourDisk dashboard](docs/screenshots/dashboard.png)

---

## Features

### Home — one-click unified scan

- A **3D usage donut** (three.js) showing used / reclaimable / free space.
- **Scan now** launches all cleanup scans + the file-type breakdown at once and
  fills the donut: a **gold** layer for reclaimable space and a **green** layer
  that grows as you select items, with live figures.
- Results are grouped by category (collapsible); the global "reclaimable" total
  always equals the sum of the categories and never exceeds the disk.

### Cleanup categories

- **Temporary files** — `/tmp`, `/var/tmp` and `~/.cache`, aggregated per cache
  folder and filtered by age.
- **Largest files & folders** — a read-only explorer of what takes the most
  space (cache/app folders excluded — they have their own sections).
- **Git worktrees** — prunable or clean linked worktrees. **Never touches a
  worktree with uncommitted changes.**
- **Dev caches** — `node_modules`, Rust `target/`, `.next`, `.turbo`, `.venv`,
  PHP `vendor/` and more.
- **App & browser caches** — the regenerable caches the `~/.cache` sweep misses:
  Chromium/Electron caches under `~/.config`, Flatpak (`~/.var/app/*/cache`),
  Snap and npm/yarn/bun caches.

### Breakdown by file type

A clickable distribution bar that accounts for the **whole disk**: images,
videos, audio, archives, disk images / ISO, applications, executables,
documents, **caches & dependencies**, **system** and **reserved (filesystem)**.
Click a category to list its largest files with full paths. The system figure is
measured accurately (via `du`: hardlink-deduplicated, block-accurate,
single-filesystem) so reserved ext4 blocks are shown honestly rather than
inflating "system".

### Applications

Inventory of installed apps from **apt**, **flatpak**, **snap** and
**AppImages**, ranked by disk space, with available updates surfaced on open and
a filter to show only updatable apps. **Batch uninstall** or **batch update** the
selection; essential system packages are **protected** (update-only, uninstall
blocked). App folders are excluded from the other scans.

### Disk health

Per-disk SMART (health, power-on hours, temperature) via **nvme-cli** for NVMe
drives (or `smartctl` for SATA), plus **real-time read/write throughput graphs**
and system uptime.

### Settings, scheduling & monitoring

- **Light / dark / system** theme and **French / English / system** language.
- **Launch at startup** (XDG autostart).
- **Low-disk-space monitor** — a background watcher raises a popup with a clean
  CTA when free space drops below a configurable threshold.
- **Scheduled cleanup** — a weekly systemd user timer.

### Incremental & instant

- A persisted, **mtime-validated directory-size cache**: unchanged trees
  (`node_modules`, caches) are not re-walked, so rescans are fast.
- On launch the app shows the **last results instantly** from cache, then
  refreshes in the background and **highlights what is new** since last time.

### System tray

The app lives in the tray; its menu opens a popover widget with a disk-usage
summary and a quick action. Closing the window keeps it running in the tray.

## Safety model

FreeYourDisk is built around five non-negotiable invariants:

1. **Read-only scans** — scanning never modifies the filesystem (enforced by tests).
2. **Dry-run first** — every deletion shows an exact preview (count, size,
   destination) and requires explicit confirmation.
3. **Trash by default** — files go to the recoverable XDG trash; permanent
   deletion is an explicit, per-action opt-in.
4. **Zone whitelist** — deletions are validated against allowed zones; paths
   outside them and symlinks escaping them are refused.
5. **Git-safe** — git actions never remove uncommitted work.

### Least privilege

The UI runs as a normal user with **no privileges**. When an action needs root
(e.g. `/var/tmp`, reading NVMe SMART, removing an apt/snap package), a **minimal
helper** is invoked via **Polkit / pkexec** — the WebView itself never runs as
root.

## Tech stack

| Layer      | Choice                                                                                |
| ---------- | ------------------------------------------------------------------------------------- |
| App shell  | Tauri 2 (Rust core + WebView)                                                         |
| Backend    | Rust workspace (`core-scan`, `core-trash`, `core-services`, `core-ipc`, `privhelper`) |
| Frontend   | Svelte 5 + TypeScript + Vite 6                                                        |
| Styling    | Tailwind CSS v4 (CSS-first `@theme`, light/dark)                                      |
| Charts     | Apache ECharts (graphs) + three.js (3D donut)                                         |
| Privileges | Polkit / `pkexec` + dedicated helper binary                                          |

## Build from source

### Prerequisites (Debian / Ubuntu)

```bash
sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libgtk-3-dev cmake
# Recommended for the Disk health section:
sudo apt install -y nvme-cli smartmontools
# Rust (https://rustup.rs) and Node 22+ / pnpm are also required.
cargo install tauri-cli
```

### Run in development

```bash
cd ui && pnpm install && cd ..
cargo build --release -p freeyourdisk-helper   # the privileged helper (SMART, root deletes)
cargo tauri dev
```

### Build a release / .deb / .rpm / .AppImage

```bash
cd ui && pnpm build && cd ..
cargo build --release -p freeyourdisk-helper
cargo tauri build          # produces deb, rpm and AppImage bundles
```

The standalone binary is at `target/release/freeyourdisk`.

## Project layout

```
crates/
  core-ipc/        shared DTOs (the back/front contract)
  core-scan/       read-only scanning + persisted mtime dir-size cache
  core-trash/      XDG trash + permanent delete, zone whitelist
  core-services/   the cleanup services (temp, app/browser caches, big files,
                   git worktrees, dev caches)
  privhelper/      minimal privileged helper (deletes + SMART, via Polkit)
src-tauri/         Tauri app: commands, file-type & app inventory, health,
                   low-space monitor, tray, scheduling
ui/                Svelte frontend (home/3D donut, categories, applications,
                   health, settings)
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

[GPL-3.0-or-later](LICENSE).
