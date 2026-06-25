# Building FreeYourDisk for macOS

FreeYourDisk now has cfg-gated macOS support. The Linux build is unaffected; the
macOS-specific code (disk health via `diskutil`, privilege escalation via the
native admin dialog, Homebrew SMART install, `.app` inventory, LaunchAgent
autostart/schedule, `~/Library/Caches` cleanup) only compiles on macOS.

This must be built **on a Mac** (Xcode toolchain + codesign + notarytool are
macOS-only). These steps assume Apple Silicon; for Intel, swap the target.

## 1. Prerequisites (on the Mac)

```bash
xcode-select --install                      # Xcode Command Line Tools
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install tauri-cli --version "^2"
# Node 22 + pnpm
brew install node pnpm                       # or corepack enable
```

Apple Developer account: in **Keychain Access** make sure you have a
**"Developer ID Application"** certificate (for distribution outside the App
Store). Note your Team ID.

## 2. Build the frontend + the privileged helper

```bash
cd ui && pnpm install && pnpm build && cd ..
cargo build --release -p freeyourdisk-helper      # the SMART/root helper
```

## 3. Build the app + DMG

```bash
cargo tauri build --bundles app,dmg
# → src-tauri/target/release/bundle/macos/FreeYourDisk.app
# → src-tauri/target/release/bundle/dmg/FreeYourDisk_0.4.1_aarch64.dmg
```

## 4. Bundle the privileged helper into the .app

The helper is invoked as root (via the native auth dialog) for `/var/tmp`
cleanup and SMART reads. It must live inside the bundle **before** signing:

```bash
APP="src-tauri/target/release/bundle/macos/FreeYourDisk.app"
cp target/release/freeyourdisk-helper "$APP/Contents/Resources/freeyourdisk-helper"
chmod +x "$APP/Contents/Resources/freeyourdisk-helper"
```

(The app resolves it from `Contents/Resources/freeyourdisk-helper` at runtime.)

## 5. Codesign (hardened runtime) — helper first, then the app

```bash
IDENTITY="Developer ID Application: YOUR NAME (TEAMID)"

# Sign the helper (it's a nested executable, so sign it before the outer app).
codesign --force --options runtime --timestamp \
  --sign "$IDENTITY" "$APP/Contents/Resources/freeyourdisk-helper"

# Sign the whole app (do NOT use --deep; sign inner-to-outer).
codesign --force --options runtime --timestamp \
  --sign "$IDENTITY" "$APP"

# Verify
codesign --verify --deep --strict --verbose=2 "$APP"
spctl -a -vvv "$APP"   # may say "rejected" until notarised — that's expected
```

If you prefer, set `APPLE_SIGNING_IDENTITY` and the helper as a tauri resource
to let `cargo tauri build` sign automatically — but the manual order above is
the most predictable.

## 6. Notarize + staple

```bash
# One-time: store credentials (uses an app-specific password from appleid.apple.com)
xcrun notarytool store-credentials fyd-notary \
  --apple-id "you@example.com" --team-id "TEAMID" --password "app-specific-password"

# Notarize the app
ditto -c -k --keepParent "$APP" /tmp/FreeYourDisk.zip
xcrun notarytool submit /tmp/FreeYourDisk.zip --keychain-profile fyd-notary --wait
xcrun stapler staple "$APP"

# Then sign + notarize the DMG too
DMG="src-tauri/target/release/bundle/dmg/FreeYourDisk_0.4.1_aarch64.dmg"
codesign --force --timestamp --sign "$IDENTITY" "$DMG"
xcrun notarytool submit "$DMG" --keychain-profile fyd-notary --wait
xcrun stapler staple "$DMG"
```

Ship the stapled `.dmg`.

## What works / what's degraded on macOS (v1)

| Feature | macOS status |
|---|---|
| Home donut, file-type breakdown | Works (`du -skx`; system size is approximate vs. Linux) |
| Temp / app / browser caches | Works — `~/Library/Caches`, `~/Library/Application Support`, `/tmp` |
| Dev caches, largest files, git worktrees | Works (path-based, OS-agnostic) |
| Trash, dry-run preview, zone whitelist | Works (cross-platform) |
| Task manager (CPU/RAM/swap, per-core, temp, kill) | Works (sysinfo); OOM-immunity is Linux-only |
| Disk health — capacity / model / SSD / SMART | Works via `diskutil` + `smartctl`. **Real-time throughput graph reads 0** (not exposed without IOKit). Apple-internal NVMe SMART is often unsupported by smartctl. |
| SMART tool install | `brew install smartmontools` (user-level, no root) |
| Applications | `.app` bundles in /Applications + ~/Applications; uninstall = move to Trash. No update channel. |
| Privileged actions (/var/tmp, SMART) | Native admin auth dialog (`osascript … with administrator privileges`) |
| Autostart / weekly cleanup | LaunchAgent (`~/Library/LaunchAgents`) |
| Low-space alert | `osascript display notification` |

Throughput-via-IOKit and a richer macOS app inventory (Homebrew casks, Mac App
Store) are the obvious next steps once the build is validated on hardware.
