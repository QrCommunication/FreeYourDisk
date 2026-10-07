# Construire et distribuer FreeYourDisk sur macOS

La release **v0.6.5** fournit deux DMG macOS natifs :
`FreeYourDisk_0.6.5_aarch64.dmg` pour Apple Silicon et
`FreeYourDisk_0.6.5_x86_64.dmg` pour les Mac Intel. Le code spécifique à macOS
(santé disque via `diskutil`, dialogue d'autorisation natif, installation SMART
Homebrew, inventaire `.app`, LaunchAgent et caches `~/Library`) ne compile que
sur macOS. Les builds Linux et Windows ne sont pas affectés.

## Release officielle : CI GitHub

La distribution est produite par les jobs macOS des workflows
[release](../.github/workflows/release.yml) et
[macos](../.github/workflows/macos.yml), sous l'environnement GitHub
**`production`**. Un tag `v0.6.5` construit les deux architectures :

| Architecture | Cible Rust | Runner | Artefact |
| --- | --- | --- | --- |
| Apple Silicon | `aarch64-apple-darwin` | `macos-14` | `FreeYourDisk_0.6.5_aarch64.dmg` |
| Intel | `x86_64-apple-darwin` | runner Intel macOS | `FreeYourDisk_0.6.5_x86_64.dmg` |

La CI importe le certificat Developer ID dans un trousseau éphémère, copie le
helper dans le bundle, signe le helper puis l'application avec hardened runtime,
crée et signe le DMG, le notarie, l'agrafe et le vérifie avant publication. Les
secrets `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID` et
`APPLE_APP_SPECIFIC_PASSWORD`, ainsi que la variable `APPLE_TEAM_ID`, doivent
être définis dans `production`. Ils ne doivent jamais figurer dans le dépôt ou
les logs. L'absence d'un de ces éléments doit faire échouer la release, jamais
produire un DMG présenté comme signé.

### Compatibilité PKCS#12 avec le trousseau Apple

Avant `security import`, les deux workflows exécutent
`packaging/macOS/prepare-signing-certificate.sh`. Le script valide le secret
PKCS#12 avec OpenSSL 3, puis réexporte le même certificat et sa clé avec
`PBE-SHA1-3DES` pour le chiffrement de la clé et du certificat, et `sha1` pour
le MAC. Cela évite le refus `MAC verification failed during PKCS12 import`
du trousseau Apple avec certains exports utilisant les valeurs par défaut
modernes d'OpenSSL 3. Les caractères CR/LF de fin de mot de passe sont retirés
dans le script et dans l'étape d'import.

Cette conversion ne renouvelle pas le certificat et ne modifie pas les secrets
GitHub. Le PEM intermédiaire reste chiffré, les fichiers temporaires ont des
permissions privées et sont supprimés à la sortie. Les mots de passe passent
à OpenSSL par l'environnement, jamais par les logs. Sur macOS, OpenSSL provient
de Homebrew `openssl@3`; `OPENSSL_BIN` permet un chemin explicite pour un test.

Le test utilise uniquement un certificat jetable et vérifie les algorithmes,
l'empreinte du certificat conservée et le refus d'un mauvais mot de passe :

```bash
bash packaging/macOS/test-prepare-signing-certificate.sh
```

## Build local de diagnostic

Le build local doit être fait **sur un Mac** (Xcode, `codesign` et `notarytool`
sont propres à macOS). Il permet de diagnostiquer le bundle, mais ne remplace
pas la CI signée et notarisée de distribution.

### Prérequis

```bash
xcode-select --install                      # Xcode Command Line Tools
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install tauri-cli --version "^2"
# Node 22 + pnpm
brew install node pnpm                       # or corepack enable
```

Un build signé local requiert un certificat **Developer ID Application** et le
Team ID correspondants. Pour une distribution officielle, préférer la CI : elle
ne laisse pas le certificat ni le trousseau persister sur le poste de build.

### Compiler le frontend et le helper privilégié

```bash
cd ui && pnpm install && pnpm build && cd ..
# Apple Silicon : utiliser x86_64-apple-darwin sur un runner Intel
cargo build --release --target aarch64-apple-darwin -p freeyourdisk-helper
```

### Compiler l'application

```bash
cargo tauri build --target aarch64-apple-darwin --bundles app
# → target/aarch64-apple-darwin/release/bundle/macos/FreeYourDisk.app

# Intel : compiler nativement avec la cible x86_64-apple-darwin.
```

### Intégrer le helper privilégié dans le `.app`

The helper is invoked as root (via the native auth dialog) for `/var/tmp`
cleanup and SMART reads. It must live inside the bundle **before** signing:

```bash
APP="target/aarch64-apple-darwin/release/bundle/macos/FreeYourDisk.app"
cp target/aarch64-apple-darwin/release/freeyourdisk-helper "$APP/Contents/Resources/freeyourdisk-helper"
chmod +x "$APP/Contents/Resources/freeyourdisk-helper"
```

(The app resolves it from `Contents/Resources/freeyourdisk-helper` at runtime.)

### Codesign (hardened runtime) — helper avant l'application

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

### Notariser et agrafer (diagnostic local seulement)

```bash
# One-time: store credentials (uses an app-specific password from appleid.apple.com)
xcrun notarytool store-credentials fyd-notary \
  --apple-id "you@example.com" --team-id "TEAMID" --password "app-specific-password"

# Notarize the app
ditto -c -k --keepParent "$APP" /tmp/FreeYourDisk.zip
xcrun notarytool submit /tmp/FreeYourDisk.zip --keychain-profile fyd-notary --wait
xcrun stapler staple "$APP"

# Then sign + notarize the DMG too
DMG="FreeYourDisk_0.6.5_aarch64.dmg"
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
| Applications | Les formules et casks Homebrew peuvent être mis à jour avec leur identifiant exact. Les `.app` manuels restent inventoriés et désinstallables vers la Corbeille, mais leur motif d'absence de canal automatique est affiché. Les apps Mac App Store sont mises à jour uniquement par l'App Store : FreeYourDisk ne contourne ni son sandboxing ni ses reçus. |
| Privileged actions (/var/tmp, SMART) | Native admin auth dialog (`osascript … with administrator privileges`) |
| Autostart / weekly cleanup | LaunchAgent (`~/Library/LaunchAgents`) |
| Low-space alert | `osascript display notification` |

Le débit via IOKit reste une évolution distincte. L'inventaire distingue déjà
les casks/formules Homebrew gérés des bundles `.app` et applications Mac App
Store qui ne disposent pas d'un canal de mise à jour sûr dans FreeYourDisk.

## État de release v0.6.5 — 7 octobre 2026

Cette section prévaut sur les formulations historiques du guide :

- la distribution officielle fournit des DMG **Intel et Apple Silicon**, et non
  un unique artefact Apple Silicon ;
- la release est produite par la CI signée et notarisée de l'environnement
  GitHub `production`, avec le helper embarqué puis signé avant le bundle ;
- Homebrew formula et cask disposent d'un canal de mise à jour par identifiant
  exact ; un bundle `.app` copié manuellement reste explicitement non
  automatisable ;
- les applications Mac App Store restent sous la responsabilité de l'App Store.
  FreeYourDisk ne les met pas à jour et ne contourne pas ses protections.
