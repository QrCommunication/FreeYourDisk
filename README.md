# FreeYourDisk

> Reclaim your disk, safely.

**English** · [Français](README.fr.md)

A modern Linux desktop utility that scans your disk and **safely** cleans
temporary files, oversized files, stale git worktrees and developer caches —
with a visual dashboard, charts, and a recoverable-by-default deletion model.

Built with **Tauri** (Rust core + WebView), licensed **GPL-3.0-or-later**.

![FreeYourDisk dashboard](docs/screenshots/dashboard.png)

---

## Features

- **Temporary files** — old files in `/tmp`, `/var/tmp` and `~/.cache`, filtered
  by age.
- **Largest files & folders** — a read-only explorer of what takes the most
  space, visualised as a treemap.
- **Git worktrees** — detect prunable or clean linked worktrees to reclaim their
  disk space. **Never touches a worktree with uncommitted changes.**
- **Dev caches** — `node_modules`, Rust `target/`, `.next`, `.turbo`, `.venv`,
  PHP `vendor/` and more, detected by signature.
- **System tray** — the app lives in the tray; its menu opens a popover widget
  with a disk-usage summary and a quick action.

![Per-service view](docs/screenshots/service-view.png)

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
(e.g. `/var/tmp`), a **minimal helper** is invoked via **Polkit** — the WebView
itself never runs as root.

## Tech stack

| Layer        | Choice                                              |
| ------------ | --------------------------------------------------- |
| App shell    | Tauri 2 (Rust core + WebView)                       |
| Backend      | Rust workspace (`core-scan`, `core-trash`, `core-services`, `core-ipc`, `privhelper`) |
| Frontend     | Svelte 5 + TypeScript + Vite 6                      |
| Styling      | Tailwind CSS v4 (CSS-first `@theme`)                |
| Charts       | Apache ECharts                                      |
| Privileges   | Polkit / `pkexec` + dedicated helper binary         |

## Build from source

### Prerequisites (Debian / Ubuntu)

```bash
sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libgtk-3-dev cmake
# Rust (https://rustup.rs) and Node 22+ / pnpm are also required.
cargo install tauri-cli
```

### Run in development

```bash
cd ui && pnpm install && cd ..
cargo tauri dev
```

### Build a release binary / .deb

```bash
cd ui && pnpm build && cd ..
cargo tauri build          # produces a standalone binary and a .deb
```

The standalone binary is at `target/release/freeyourdisk`.

## Project layout

```
crates/
  core-ipc/        shared DTOs (the back/front contract)
  core-scan/       read-only parallel filesystem scanning
  core-trash/      XDG trash + permanent delete, zone whitelist
  core-services/   the four cleanup services
  privhelper/      minimal privileged helper (Polkit)
src-tauri/         Tauri app: commands, execution routing, tray
ui/                Svelte frontend
```

## License

[GPL-3.0-or-later](LICENSE).
