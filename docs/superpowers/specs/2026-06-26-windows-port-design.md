# Design — Portage Windows 10/11 de FreeYourDisk

- **Date** : 2026-06-26
- **Statut** : approuvé (design) — en attente de relecture spec avant plan d'implémentation
- **Cible** : Windows 10 (1803+) et Windows 11, x64
- **Base** : Tauri 2 (workspace Rust + frontend Svelte) supportant déjà Linux et macOS (cfg-gated)

---

## 1. Contexte et objectif

FreeYourDisk est un utilitaire de gestion/nettoyage de disque. Il expose 6 domaines
fonctionnels : tableau de bord (donut 3D + breakdown par type de fichier), nettoyage
(temp, gros fichiers, caches dev/applis, dépôts git), santé disque (SMART), gestionnaire
de tâches (CPU/RAM/process), applications installées (inventaire + désinstallation +
mises à jour), et un mode planifié/headless.

Le code supporte Linux (origine) et macOS (ajouté en cfg-gated, commit `fe7f2da`).
**Objectif** : ajouter un troisième backend Windows 10/11 en suivant le pattern
d'abstraction existant, avec **parité fonctionnelle complète**.

### Constat clé de l'exploration

La couche système est profondément Unix (signaux POSIX, Polkit, XDG, `du`, gestionnaires
de paquets), mais **le cœur algorithmique est déjà portable** :

- `core-scan` (jwalk/rayon/`std::fs::symlink_metadata`) — **aucune** dépendance aux
  métadonnées Unix (`st_blocks`/`st_dev`/`st_ino`/`MetadataExt`). Portable tel quel.
- `core-trash` (crate `trash` v5) — gère nativement la Corbeille Windows. 0 travail.
- `sysinfo` (CPU/RAM/uptime/process) — déjà cross-platform.
- Aucun crate natif (`nix`, `windows`) n'est tiré aujourd'hui : la divergence est 100 %
  en `#[cfg]` source. La seule occurrence `std::os::unix` du dépôt est dans un test.
- Le frontend Svelte est petit et propre (App.svelte = 67 lignes) ; les hypothèses
  Linux sont concentrées en ~6 endroits.

---

## 2. Décisions de cadrage (validées)

| # | Décision | Choix retenu |
|---|----------|--------------|
| A | Ambition v1 | **Parité complète** — température CPU, throughput disque et inventaire applis riche tous fonctionnels (dépendances natives Windows assumées). |
| B | Modèle d'élévation | **Process entier élevé**, réalisé via **relaunch headless élevé** (cf. §4) — pas de helper privilégié séparé sur Windows. |
| C | Onglet Applications | **Complet** : registre Uninstall + winget + **MSIX**, sans limitation. |
| D | Livraison | **NSIS non signé** (calque le `.dmg` macOS non signé). |

---

## 3. Stratégie d'abstraction

Le pattern actuel `#[cfg(target_os = ...)]` inline au grain de la fonction reste la base,
mais il évolue pour rester maintenable à 3 OS avec du code natif Windows volumineux.

1. **Scinder `#[cfg(not(target_os = "macos"))]` → `linux` / `windows` explicites.**
   C'est le piège central : ce gate signifie aujourd'hui « Linux » implicitement. Chaque
   site doit devenir trois arms distincts (`linux`, `macos`, `windows`).
2. **`libc` repassé sous `[target.'cfg(unix)'.dependencies]`** ; tout usage `libc::*`
   gardé `#[cfg(unix)]` (corrige notamment l'appel `setpriority` non gardé, cf. §5).
3. **Seam `platform` pour les 4 zones lourdes** — `applications`, `health`+capteurs,
   `execute`/élévation, scheduling :
   ```
   mod platform {
       #[cfg(target_os = "linux")]   mod linux;   pub use linux::*;
       #[cfg(target_os = "macos")]   mod macos;   pub use macos::*;
       #[cfg(target_os = "windows")] mod windows; pub use windows::*;
   }
   ```
   chaque sous-module implémentant une interface commune (ex. `trait AppInventory`,
   `fn disks() -> Vec<Disk>`, `fn run_privileged(plan) -> Report`). Fichiers focalisés,
   testables isolément.
4. **Forks triviaux** (chemins, défaut de raccourci, liste de process protégés) :
   restent en `#[cfg]` inline (pas de sur-ingénierie).
5. **Source unique de vérité pour l'allowlist privilégiée** : extraire la validation
   et l'exécution allowlistées (aujourd'hui dans `crates/privhelper/src/main.rs`,
   testées dans `crates/privhelper/tests/validation.rs`) vers une lib partagée
   (proposition : `crates/core-privexec`) appelée par :
   - le binaire `privhelper` (Linux via pkexec, macOS via osascript-admin),
   - le **mode élevé Windows** (cf. §4).
   La sécurité n'est jamais dupliquée entre OS.

---

## 4. Modèle d'élévation Windows

Réalisation sûre de « process entier élevé » :

- L'UI est lancée **non-élevée** (navigation, scan, lecture sans UAC).
- Sur action privilégiée, le flux est :
  1. l'app **valide le plan côté serveur** contre l'inventaire/allowlist (lib `core-privexec`),
  2. écrit le plan dans un **fichier temporaire** (les pipes anonymes ne traversent pas
     la frontière UAC — calque exact du staging fichier-temp déjà utilisé par macOS),
  3. relance **son propre exécutable** via `ShellExecuteEx` (verbe **`runas`**) en
     **mode headless** `--apply <plan> <report>` — **aucune WebView chargée**,
  4. l'instance élevée exécute les opérations allowlistées, écrit le **rapport** dans un
     fichier, puis **sort**,
  5. l'UI (toujours vivante, non relancée) lit le rapport et met à jour l'affichage.
- **Invite UAC par lot privilégié** (cohérent avec la granularité Polkit/osascript).
- Le crate `privhelper` **n'est pas bundlé** sur Windows ; le « helper », c'est le
  binaire principal réinvoqué en mode élevé headless.
- **Posture sécurité** : la WebView2 ne tourne jamais en admin ; l'instance élevée est
  headless, courte et exécute uniquement un plan déjà validé.

> Réutilise et étend `src-tauri/src/headless.rs` (mode headless déjà existant pour le
> nettoyage planifié) pour accepter les invocations `--apply` et `--smart` élevées.

> Fallback documenté (non retenu sauf demande) : manifeste `requireAdministrator`
> (UAC à chaque lancement, WebView en admin) — plus simple mais sur-privilégié.

---

## 5. Backends Windows par fonctionnalité (parité complète)

### 5.1 Chemins, settings, cache
- Adopter la crate `dirs` : `config_dir()` → `%APPDATA%`, `cache_dir()` →
  `%LOCALAPPDATA%`, `home_dir()` → `%USERPROFILE%`.
- Sites : `settings.rs` (`config_dir`, `home`, autostart), `headless.rs` (`run()` home,
  cache dir), `core-services/src/app_cache.rs` et `temp.rs` (ajouter un arm `windows`
  poussant `%LOCALAPPDATA%`, `%LOCALAPPDATA%\Temp`, caches Edge/Chrome/Electron,
  npm/yarn/bun sous `%APPDATA%`/`%LOCALAPPDATA%`).

### 5.2 Scan & empreinte système
- Moteur de scan : **inchangé** (déjà portable).
- `du` (mesure de l'empreinte système, hardlink-dedup/block-accurate) **n'existe pas**
  sur Windows → remplacer par une **somme interne** via le moteur de scan, bornée au
  volume (`commands.rs::system_total`). Pas de dépendance externe.

### 5.3 Corbeille
- **Inchangé** (`core-trash` → Recycle Bin via `trash` v5).

### 5.4 Santé disque / SMART
- **Bundler `smartctl.exe`** (smartmontools v7+ — couvre NVMe via IOCTL, `nvme-cli`
  inutile) dans les resources Windows. L'accès SMART requiert l'admin → lecture
  **ponctuelle** exécutée par le **mode élevé** (`--smart \\.\PhysicalDriveN`)
  **uniquement à l'ouverture/rafraîchissement manuel de l'onglet Santé** (1 invite UAC),
  résultat **mis en cache**. Jamais de lecture SMART en continu.
- Énumération des disques : `sysinfo::Disks` (déjà importé dans `monitor.rs`).
- Filtre « disque physique » (`health.rs::is_physical_disk`) : arm Windows acceptant
  `C:\`, `D:\`, rejetant les volumes virtuels.
- `health.rs::platform::disks()` : nouveau module Windows (modèle/rotational `None`/`false`
  acceptable, enrichissable via WMI plus tard).

### 5.5 Throughput + uptime
- Uptime : `sysinfo::System::uptime()` (arm Windows identique à macOS) — remplace
  `/proc/uptime`.
- Throughput réel : **compteurs PDH** `\PhysicalDisk(*)\Disk Read Bytes/sec` et
  `… Write Bytes/sec` (crate `windows`, feature `Win32_System_Performance`) — remplace
  `/proc/diskstats`.

### 5.6 Température CPU & load average
- Load average : `None` sur Windows (concept POSIX ; l'UI gère déjà `Option<f32>`).
- Température CPU : **module capteurs isolé** (`platform::sensors`), lecture **continue
  non-élevée** (jamais d'UAC par rafraîchissement), deux niveaux :
  - baseline : **WMI** `MSAcpi_ThermalZoneTemperature` (crate `wmi`) — best-effort ;
    selon l'OEM/les droits, peut renvoyer `None` quand l'app n'est pas élevée,
  - précis (par cœur, cross-OEM) : intégration optionnelle **LibreHardwareMonitor**.
- *Zone la plus incertaine* : conçue dégradable (retourne `None` sans crasher, l'UI
  masque proprement) pour ne jamais bloquer le reste si l'effort LHM s'avère
  disproportionné. Aucune lecture capteur ne passe par le relaunch élevé.

### 5.7 Gestionnaire de tâches
- `kill`/signaux POSIX (`taskmgr.rs`) → `OpenProcess(PROCESS_TERMINATE)` +
  `TerminateProcess` (force) ; `WM_CLOSE`/best-effort (gracieux). Pas de SIGTERM réel.
- `restart_process` : `WM_CLOSE` → délai → `TerminateProcess`.
- Priorité : `libc::setpriority` (actuellement **non gardé** — bug à corriger) →
  `#[cfg(windows)]` `SetPriorityClass(ABOVE_NORMAL_PRIORITY_CLASS)`.
- OOM immunity (`oom_score_adj`) : **pas d'équivalent**, déjà `#[cfg(target_os="linux")]`
  → capacité acceptée comme absente sur Windows.
- Liste de process protégés : arm Windows (`System`, `smss.exe`, `csrss.exe`,
  `wininit.exe`, `winlogon.exe`, `lsass.exe`, `services.exe`, `svchost.exe`,
  `FreeYourDisk.exe`).

### 5.8 Applications (complet : registre + winget + MSIX)
- **Inventaire** :
  - applications classiques : clés registre **Uninstall**
    (`HKLM\…\Uninstall`, vue 64 et 32 bits `WOW6432Node`, `HKCU\…\Uninstall`) via `winreg`,
  - **MSIX / Store** : `PackageManager` (crate `windows`, feature `Management_Deployment`).
- **Désinstallation** :
  - classique : `UninstallString` / `QuietUninstallString` (exécutée en mode élevé si requis),
  - MSIX : `RemovePackageAsync`.
- **Mises à jour** : détection et exécution via **winget** (`winget upgrade [--id …]`).
- Remplace tout le bloc `#[cfg(not(target_os="macos"))]` de `applications.rs`
  (detect_apt/flatpak/snap/appimages, list, updates, uninstall, update) par un module
  `platform::windows`.

### 5.9 Planification & autostart
- systemd timer (`packaging/systemd/*`) → **Task Scheduler** :
  `schtasks /create /sc WEEKLY /tn "FreeYourDisk Cleanup" /tr "<exe> --headless …" /f`.
- Autostart : clé registre `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- `commands.rs::schedule_enabled` / `set_schedule` : arms Windows.

### 5.10 Notifications
- `notify-send` (Linux) / `osascript` (macOS) → **toast Windows**
  (`tauri-plugin-notification`, sinon PowerShell BurntToast).
- Sites : `monitor.rs::raise_and_alert`, `headless.rs::notify`.

### 5.11 Raccourci global
- Défaut Windows ≠ `Ctrl+Alt+Delete` (Secure Attention Sequence, non interceptable —
  échec silencieux du plugin) → **`Ctrl+Shift+M`**.
- À corriger **côté Rust** (`settings.rs::default_shortcut`, arm Windows) **et côté
  frontend** (`ui/src/lib/settings.ts` défaut codé en dur).

### 5.12 Tray
- **Inchangé** (API Tauri cross-platform). `show_menu_on_left_click(true)` reste un
  comportement acceptable sur Windows. La dépendance `libayatana-appindicator3` est
  scopée au bundle `.deb` Linux et n'affecte pas le bundle Windows.

### 5.13 Localisation tray/headless
- `is_french()` (`tray.rs`, `headless.rs`) lit `LC_MESSAGES`/`LANG`/`LANGUAGE` (POSIX) ;
  sur Windows ces variables sont absentes → retourne `false` (anglais) par défaut.
  Acceptable v1 (l'UI web gère l'i18n complet via `settings.language`). Amélioration
  possible : lire la locale Windows via API.

---

## 6. Dépendances natives (ciblées Windows)

```toml
# src-tauri/Cargo.toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = [
  "Win32_UI_Shell",            # ShellExecuteEx runas (élévation)
  "Win32_System_Threading",    # SetPriorityClass, OpenProcess, TerminateProcess
  "Win32_System_Performance",  # PDH throughput disque
  "Management_Deployment",     # MSIX PackageManager
] }
winreg = "0.52"   # registre : inventaire Uninstall + autostart
wmi    = "0.14"   # capteurs température (best-effort)

[target.'cfg(unix)'.dependencies]
libc = "0.2"      # déplacé ici (signaux/priorité/OOM Unix uniquement)
```

Aucune nouvelle dépendance dans `core-scan`, `core-trash`, `core-ipc`, `core-services`
(seulement des arms `#[cfg]` supplémentaires pour les chemins).

---

## 7. Frontend (petit, localisé)

| Fichier | Changement |
|---------|------------|
| `ui/src/lib/api.ts` | `AppSource` += `"registry" \| "winget" \| "msix"` (et champs SMART déjà présents) |
| `ui/src/lib/settings.ts` | défaut raccourci conditionnel OS (ou résolu via commande backend) |
| `ui/src/lib/views/Applications.svelte` | couleurs/labels des nouvelles sources Windows |
| `ui/src/lib/views/TrayWidget.svelte` | filtre `/snap` neutralisé hors Linux |
| `ui/src/lib/i18n/{en,fr}.json` | libellés caches adaptés (« AppData, Edge/Chrome, npm… ») |

> Tauri sur Windows utilise WebView2 (Chromium) au lieu de WebKitGTK : pas de CSS/JS
> WebKit-spécifique repéré dans le frontend (à reconfirmer au build).

---

## 8. Packaging / CI

- `src-tauri/tauri.conf.json` :
  - `bundle.targets` += `"nsis"`,
  - section `bundle.windows` (icône `.ico` déjà présente dans `icons`),
  - `bundle.windows.webviewInstallMode = { type: "embedBootstrapper" }` (couvre
    Win10 < 1803 sans WebView2 préinstallé),
  - déclaration des resources bundlées (`smartctl.exe`).
- **Conformité GPL (smartmontools)** : `smartctl.exe` est GPL-3.0 — compatible avec
  FreeYourDisk (GPL-3.0-or-later). Inclure le texte de licence et une mention/lien vers
  les sources de smartmontools dans les resources Windows et l'installeur.
- **Nouveau job CI `windows-latest`** (calqué sur `.github/workflows/macos.yml`) :
  - setup Rust + Node/pnpm, build frontend, `cargo tauri build --bundles nsis`,
  - artefact `FreeYourDisk_x64-setup.exe` attaché à la GitHub Release (**non signé**).
- Installeur NSIS : créer la **tâche planifiée** (et optionnellement l'entrée autostart)
  à l'installation, la retirer à la désinstallation.

---

## 9. Stratégie de tests

- **Unitaires `#[cfg(target_os = "windows")]`** : parsing des clés registre Uninstall,
  mapping inventaire MSIX, résolution des chemins (`%APPDATA%`/`%LOCALAPPDATA%`),
  défaut de raccourci, filtre disque physique.
- **Allowlist privilégiée partagée** : réutiliser/étendre
  `crates/privhelper/tests/validation.rs` contre la nouvelle lib `core-privexec`
  (garantit la parité de sécurité entre OS).
- **Gate CI Windows** : `cargo build` + `cargo clippy` sur `windows-latest` (sur un port
  cfg, « ça compile » couvre la moitié du risque).
- **Smoke manuel Win10 + Win11** : UAC sur action privilégiée, scan, nettoyage temp/caches,
  désinstallation classique **et** MSIX, lecture SMART, tâche planifiée, raccourci global,
  toast bas-disque.

---

## 10. Séquencement (chaque phase compile et tourne)

0. **Gate compilation** : scinder `not(macos)`, garder `libc` sous `cfg(unix)`, ajouter
   les deps Windows + **stubs** de tous les arms `windows` → app Windows qui **build et
   démarre** (fonctionnalités dégradées). Inclut le job CI Windows en mode build.
1. **Nettoyage de base** : chemins/settings (`dirs`), scan, corbeille, remplacement de
   `du` (somme interne) → scan + nettoyage non-privilégié fonctionnels.
2. **Élévation** : relaunch headless élevé (`ShellExecuteEx runas` + `--apply`) + lib
   `core-privexec` partagée → nettoyage privilégié fonctionnel.
3. **Santé/SMART** : bundle `smartctl.exe`, lecture via mode élevé, énumération disques
   (`sysinfo::Disks`), throughput PDH, uptime.
4. **Gestionnaire de tâches** : Terminate/Priority/liste protégée + capteurs température
   (WMI baseline, LHM optionnel).
5. **Applications** : registre Uninstall + winget + MSIX (liste / désinstallation / update).
6. **Planification & UX** : Task Scheduler (`schtasks`), autostart, toasts, défaut de
   raccourci + ajustements frontend.
7. **Packaging/CI** : NSIS + `webviewInstallMode` + bundle `smartctl.exe` + tâche
   planifiée installeur + docs/README + CHANGELOG + bump de version.

---

## 11. Risques et mitigations

| # | Risque | Mitigation |
|---|--------|------------|
| 1 | Température CPU cross-OEM (WMI fragmenté ; LibreHardwareMonitor = .NET) | Module capteurs isolé et dégradable (`None`) ; LHM optionnel, descopable sans bloquer le reste |
| 2 | Désinstallation MSIX (droits/API async) | Tests dédiés + fallback liste-seule si blocage runtime |
| 3 | `ShellExecuteEx runas` + mode headless : préservation de l'état UI | Rapport renvoyé par fichier ; l'UI n'est pas relancée → pas de perte d'état |
| 4 | SmartScreen (installeur non signé) | Accepté en v1 (calque le `.dmg` macOS non signé) ; signature Authenticode en itération ultérieure |
| 5 | Régression Linux/macOS lors du split `not(macos)` | Tests existants + build CI 3 OS ; le split est mécanique et vérifiable à la compilation |

---

## 12. Hors périmètre (v1)

- Signature Authenticode de l'installeur (itération ultérieure).
- Installeur MSI/WiX (NSIS retenu).
- Auto-update intégré.
- Localisation native du tray/headless Windows (l'UI reste localisée).
- Backend Windows ARM64 (x64 uniquement en v1).

---

## 13. Critères d'acceptation

1. `cargo build`/`clippy` verts sur `windows-latest` **et** Linux **et** macOS (aucune
   régression).
2. L'app démarre sur Win10/Win11, scanne et nettoie (corbeille) sans élévation.
3. Une action privilégiée déclenche **une** invite UAC et s'exécute via le mode headless
   élevé ; la WebView reste non-élevée.
4. Onglet Santé : SMART lu via `smartctl.exe` bundlé (NVMe + SATA), throughput non nul.
5. Onglet Applications : liste classiques + MSIX, désinstalle les deux, propose les
   updates winget.
6. Gestionnaire de tâches : termine/force/redémarre des process, respecte la liste
   protégée ; température affichée si disponible (sinon masquée proprement).
7. Tâche planifiée créée/supprimée par l'installeur ; mode planifié déclenche un toast.
8. Raccourci global par défaut fonctionne (≠ Ctrl+Alt+Delete).
9. CI produit `FreeYourDisk_x64-setup.exe` (NSIS, non signé) attaché à la Release.
