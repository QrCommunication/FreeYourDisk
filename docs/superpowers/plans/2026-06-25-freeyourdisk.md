# FreeYourDisk Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Construire FreeYourDisk, un utilitaire desktop Linux (Tauri) qui scanne le disque et nettoie en toute sécurité fichiers temporaires, gros fichiers, worktrees git obsolètes et caches de développement, avec UI moderne à graphes, livré en `.deb`.

**Architecture:** Split-privilege en 3 processus — UI Svelte (WebView, user) ↔ backend Tauri/Rust (user, sans privilège) ↔ helper root minimal via Polkit. Cœur métier en crates Rust testables isolément. Scan strictement en lecture seule ; suppression via corbeille XDG par défaut, dry-run + confirmation obligatoires.

**Tech Stack:** Rust (workspace Cargo), Tauri 2, Svelte + TypeScript + Vite, ECharts, `jwalk`/`rayon`, `git2`, crate `trash`, Polkit/`pkexec`, systemd user timer, `cargo-deb`, GitHub Actions.

## Global Constraints

- Licence : **GPL-3.0-or-later** — fichier `LICENSE` + en-tête SPDX (`// SPDX-License-Identifier: GPL-3.0-or-later`) en tête de chaque fichier source Rust.
- Nom binaire UI : `freeyourdisk` ; alias court `fyd` ; helper : `freeyourdisk-helper` ; ID applicatif : `io.freeyourdisk`.
- **Invariant 1 — Scan lecture seule** : aucune fonction de scan ne modifie le FS (testé).
- **Invariant 2 — Dry-run obligatoire** : toute suppression passe par `preview()` → confirmation → `execute()`.
- **Invariant 3 — Corbeille par défaut** : suppression définitive uniquement sur opt-in explicite.
- **Invariant 4 — Whitelist** : `execute()` (user) et le helper (root) refusent tout chemin hors zones autorisées ou via symlink sortant de zone.
- **Invariant 5 — Git safe** : aucune action git ne touche un worktree dont `git status` n'est pas propre.
- UI : toutes les chaînes en clés i18n (`svelte-i18n`), jamais de string en dur. FR + EN livrés.
- Toast de feedback (succès/erreur) sur chaque action utilisateur.
- Rust edition 2021, `clippy` sans warning, `cargo fmt`. Front : `eslint` + `prettier` propres.
- Versions planchers : Rust stable récent, Node 22 LTS, pnpm.

## Chargement des skills (au démarrage de chaque phase)

| Phase | Skills à invoquer AVANT de coder |
|-------|----------------------------------|
| Toute phase Rust (0–5, 7) | `workflow-clean-code`, `rust-pro`, `systems-programming-rust-project` |
| Phases async/IPC (1, 5) | + `rust-async-patterns` |
| Helper / mode headless (3, 7) | + `rust-cli-builder` |
| Front (6) | `workflow-clean-code`, `frontend-design`, `design-taste-frontend`, `ui-ux-pro-max` |
| Polish final (fin phase 6) | `web-design-reviewer`, `polish` |

## File Structure

```
FreeYourDisk/
├── LICENSE                         # GPL-3.0
├── Cargo.toml                      # workspace
├── crates/
│   ├── core-ipc/                   # DTOs partagés (serde) — contrats
│   ├── core-scan/                  # parcours FS lecture seule, tailles, top-N
│   ├── core-trash/                 # corbeille XDG + suppression définitive + whitelist
│   ├── core-services/              # 4 services (temp, big-files, git, dev-cache)
│   └── privhelper/                 # binaire root minimal
├── src-tauri/                      # app Tauri (bin freeyourdisk)
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── src/main.rs                 # UI + mode --headless
│   └── src/commands.rs             # commands Tauri
├── ui/                             # front Svelte
│   ├── package.json
│   ├── vite.config.ts
│   ├── src/lib/i18n/{en,fr}.json
│   ├── src/lib/api.ts              # wrappers invoke() typés depuis core-ipc
│   ├── src/lib/components/...
│   └── src/routes/...
├── packaging/
│   ├── io.freeyourdisk.policy      # Polkit
│   ├── freeyourdisk.desktop
│   ├── io.freeyourdisk.metainfo.xml# AppStream
│   ├── systemd/freeyourdisk.{service,timer}
│   └── icons/hicolor/...
└── .github/workflows/ci.yml
```

---

## Phase 0 — Fondations

### Task 0.1 : Workspace Cargo + Tauri scaffold + licence + CI squelette

**Files:**
- Create: `Cargo.toml` (workspace), `LICENSE`, `.gitignore`, `rust-toolchain.toml`
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: workspace compilable `cargo build` ; app Tauri qui démarre une fenêtre vide.

- [ ] **Step 1 : Créer le workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/core-ipc", "crates/core-scan", "crates/core-trash", "crates/core-services", "crates/privhelper", "src-tauri"]

[workspace.package]
edition = "2021"
license = "GPL-3.0-or-later"
authors = ["rony <rony@rlconseil.net>"]
repository = "https://github.com/rony/FreeYourDisk"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
jwalk = "0.8"
rayon = "1"
git2 = "0.19"
trash = "5"
anyhow = "1"
```

- [ ] **Step 2 : Ajouter `LICENSE` (texte GPL-3.0 complet)**

Récupérer le texte officiel GPL-3.0 (`https://www.gnu.org/licenses/gpl-3.0.txt`) dans `LICENSE`.

- [ ] **Step 3 : Scaffold Tauri** (`src-tauri/` minimal)

`src-tauri/src/main.rs` :
```rust
// SPDX-License-Identifier: GPL-3.0-or-later
fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running FreeYourDisk");
}
```
`src-tauri/Cargo.toml` déclare `tauri = { version = "2", features = [] }` et `[package] name = "freeyourdisk"`.

- [ ] **Step 4 : Vérifier le build**

Run: `cargo build`
Expected: compile sans erreur (workspace + bin `freeyourdisk`).

- [ ] **Step 5 : CI squelette** `.github/workflows/ci.yml` (jobs : `fmt`, `clippy`, `test`, déclenchés sur push/PR). Job `build-deb` ajouté en Phase 8.

- [ ] **Step 6 : Commit**

```bash
git add -A && git commit -m "chore: workspace Cargo + Tauri scaffold + GPL-3.0 + CI skeleton"
```

### Task 0.2 : Contrats `core-ipc` (DTOs partagés)

**Files:**
- Create: `crates/core-ipc/Cargo.toml`, `crates/core-ipc/src/lib.rs`
- Test: `crates/core-ipc/src/lib.rs` (tests sérialisation)

**Interfaces:**
- Produces (source de vérité des types UI↔backend) :
  - `enum ServiceId { Temp, BigFiles, GitRepos, DevCache }`
  - `enum Destination { Trash, Permanent }`
  - `struct ScanItem { id: String, path: PathBuf, size_bytes: u64, last_access: Option<i64>, kind: ItemKind, requires_root: bool }`
  - `enum ItemKind { File, Dir, GitWorktree, GitBranch, DevCache }`
  - `struct ScanResult { service: ServiceId, items: Vec<ScanItem>, total_bytes: u64 }`
  - `struct DeletionPlan { items: Vec<ScanItem>, destination: Destination, total_bytes: u64, requires_root: bool }`
  - `struct ExecutionReport { freed_bytes: u64, deleted_count: usize, errors: Vec<ItemError> }`
  - `struct ItemError { path: PathBuf, message: String }`

- [ ] **Step 1 : Écrire le test de sérialisation (échoue)**

```rust
#[test]
fn scan_item_roundtrips_json() {
    let item = ScanItem { id: "x".into(), path: "/tmp/foo".into(), size_bytes: 42, last_access: None, kind: ItemKind::File, requires_root: false };
    let json = serde_json::to_string(&item).unwrap();
    let back: ScanItem = serde_json::from_str(&json).unwrap();
    assert_eq!(back.size_bytes, 42);
    assert_eq!(back.kind, ItemKind::File);
}
```

- [ ] **Step 2 : Run → FAIL** (`cargo test -p core-ipc`) : types non définis.

- [ ] **Step 3 : Implémenter les types** dans `lib.rs` (tous `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]`, en-tête SPDX). Énums sérialisés en `#[serde(rename_all = "snake_case")]`.

- [ ] **Step 4 : Run → PASS** (`cargo test -p core-ipc`).

- [ ] **Step 5 : Commit** `feat(core-ipc): shared DTOs for scan/preview/execute contracts`.

---

## Phase 1 — `core-scan` (lecture seule)

### Task 1.1 : Parcours FS parallèle + tailles + invariant lecture seule

**Files:**
- Create: `crates/core-scan/Cargo.toml`, `crates/core-scan/src/lib.rs`
- Test: `crates/core-scan/src/lib.rs`

**Interfaces:**
- Consumes: rien.
- Produces:
  - `fn scan_dir(root: &Path, opts: &ScanOpts) -> Vec<RawEntry>` (RawEntry: `{ path, size_bytes, last_access, is_dir }`)
  - `struct ScanOpts { follow_symlinks: bool /* défaut false */, min_age_days: Option<u32> }`
  - `fn dir_sizes(root: &Path) -> HashMap<PathBuf, u64>` (taille agrégée par dossier)

- [ ] **Step 1 : Test « scan trouve les fichiers, sans rien modifier »**

```rust
#[test]
fn scan_lists_files_read_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
    let before = dir_snapshot(dir.path()); // helper de test: liste (path, mtime, len)
    let entries = scan_dir(dir.path(), &ScanOpts::default());
    let after = dir_snapshot(dir.path());
    assert_eq!(before, after, "scan must not modify the filesystem");
    assert!(entries.iter().any(|e| e.path.ends_with("a.txt") && e.size_bytes == 5));
}
```

- [ ] **Step 2 : Run → FAIL** (`cargo test -p core-scan`).

- [ ] **Step 3 : Implémenter `scan_dir`** avec `jwalk` (parallélisme `rayon`), `follow_symlinks=false` par défaut, lecture `metadata` seulement. `ScanOpts::default()` = `{ follow_symlinks: false, min_age_days: None }`.

- [ ] **Step 4 : Run → PASS**.

- [ ] **Step 5 : Test + impl `dir_sizes`** (agrégation bottom-up des tailles par dossier). Test sur arbo `tempfile` avec 2 sous-dossiers, vérifier les totaux.

- [ ] **Step 6 : Commit** `feat(core-scan): parallel read-only FS scan + dir size aggregation`.

### Task 1.2 : Top-N fichiers/dossiers

**Files:**
- Modify: `crates/core-scan/src/lib.rs`
- Test: idem

**Interfaces:**
- Consumes: `scan_dir`, `dir_sizes`.
- Produces: `fn top_n(root: &Path, n: usize) -> (Vec<RawEntry> /*files*/, Vec<(PathBuf,u64)> /*dirs*/)` triés desc par taille.

- [ ] **Step 1 : Test** : arbo avec fichiers de tailles connues → `top_n(root, 2)` renvoie les 2 plus gros fichiers et 2 plus gros dossiers, ordre décroissant.
- [ ] **Step 2 : Run → FAIL**.
- [ ] **Step 3 : Implémenter** (tri + troncature).
- [ ] **Step 4 : Run → PASS**.
- [ ] **Step 5 : Commit** `feat(core-scan): top-N largest files and directories`.

---

## Phase 2 — `core-trash` (suppression + whitelist)

### Task 2.1 : Corbeille XDG + suppression définitive opt-in + validation whitelist

**Files:**
- Create: `crates/core-trash/Cargo.toml`, `crates/core-trash/src/lib.rs`
- Test: `crates/core-trash/src/lib.rs`

**Interfaces:**
- Consumes: rien.
- Produces:
  - `struct Zones(Vec<PathBuf>)` — zones autorisées (préfixes absolus canonicalisés).
  - `fn validate(path: &Path, zones: &Zones) -> Result<PathBuf, TrashError>` — canonicalise, refuse symlink sortant de zone, refuse hors zone.
  - `fn to_trash(paths: &[PathBuf], zones: &Zones) -> ExecutionReport`
  - `fn delete_permanent(paths: &[PathBuf], zones: &Zones) -> ExecutionReport`
  - `enum TrashError { OutsideZone(PathBuf), SymlinkEscape(PathBuf), Io(String) }`

- [ ] **Step 1 : Test whitelist refuse hors zone + symlink escape**

```rust
#[test]
fn validate_rejects_outside_zone() {
    let dir = tempfile::tempdir().unwrap();
    let zones = Zones(vec![dir.path().to_path_buf()]);
    assert!(matches!(validate(Path::new("/etc/passwd"), &zones), Err(TrashError::OutsideZone(_))));
}

#[test]
fn validate_rejects_symlink_escaping_zone() {
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("evil");
    std::os::unix::fs::symlink("/etc", &link).unwrap();
    let zones = Zones(vec![dir.path().to_path_buf()]);
    assert!(matches!(validate(&link, &zones), Err(TrashError::SymlinkEscape(_))));
}
```

- [ ] **Step 2 : Run → FAIL**.

- [ ] **Step 3 : Implémenter `validate`** : `fs::canonicalize` (résout symlinks), vérifier que le chemin canonicalisé `starts_with` une zone ; si le chemin original est un symlink dont la cible sort de zone → `SymlinkEscape`.

- [ ] **Step 4 : Run → PASS**.

- [ ] **Step 5 : Implémenter `to_trash` / `delete_permanent`** : chaque chemin passe par `validate` AVANT action ; `to_trash` via crate `trash` ; `delete_permanent` via `fs::remove_*`. Accumuler succès/erreurs dans `ExecutionReport`. Test : un fichier valide → corbeille OK (report.deleted_count == 1) ; un hors zone → erreur, non supprimé.

- [ ] **Step 6 : Commit** `feat(core-trash): XDG trash + permanent delete guarded by zone whitelist`.

---

## Phase 3 — `privhelper` (binaire root minimal)

### Task 3.1 : Helper privilégié + re-validation + protocole stdin JSON

**Files:**
- Create: `crates/privhelper/Cargo.toml`, `crates/privhelper/src/main.rs`
- Create: `packaging/io.freeyourdisk.policy`
- Test: `crates/privhelper/tests/validation.rs`

**Interfaces:**
- Consumes: `core-trash::{validate, delete_permanent, to_trash, Zones}`, `core-ipc::ExecutionReport`.
- Produces: binaire `freeyourdisk-helper` qui lit un `DeletionPlan` JSON sur stdin, **re-valide** chaque chemin contre des zones root codées en dur, exécute, écrit `ExecutionReport` JSON sur stdout. Codes de sortie : 0 OK, 2 input invalide, 3 zone refusée.

- [ ] **Step 1 : Test (intégration) : le helper refuse un chemin hors zones root**

```rust
// tests/validation.rs : lance le binaire via Command, envoie un plan JSON pointant /home/... (hors zones root), attend exit 3 et report avec erreur.
```

- [ ] **Step 2 : Run → FAIL** (`cargo test -p privhelper`).

- [ ] **Step 3 : Implémenter** `main.rs` : zones root autorisées = `["/tmp", "/var/tmp"]` (codées en dur, jamais reçues de l'UI). Parser stdin → `DeletionPlan` ; pour chaque item, `validate(path, ROOT_ZONES)` ; exécuter `delete_permanent` (temporaires) ; émettre report. SPDX header. Aucune dépendance réseau.

- [ ] **Step 4 : Run → PASS**.

- [ ] **Step 5 : Écrire la policy Polkit** `packaging/io.freeyourdisk.policy` :

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN" "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <action id="io.freeyourdisk.clean-system">
    <description>Clean protected system temporary files</description>
    <message>Authentication is required to clean system temporary files</message>
    <defaults>
      <allow_any>auth_admin</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>auth_admin</allow_active>
    </defaults>
    <annotate key="org.freedesktop.policykit.exec.path">/usr/lib/freeyourdisk/freeyourdisk-helper</annotate>
  </action>
</policyconfig>
```

- [ ] **Step 6 : Commit** `feat(privhelper): minimal root helper with re-validation + Polkit policy`.

---

## Phase 4 — `core-services` (les 4 services)

Chaque service implémente le trait commun :
```rust
pub trait Service {
    fn id(&self) -> ServiceId;
    fn scan(&self) -> ScanResult;                       // lecture seule
    fn preview(&self, selection: &[String]) -> DeletionPlan; // dry-run
}
```
(L'`execute` est centralisé côté backend : il route selon `requires_root`.)

### Task 4.1 : Service Temp

**Files:** Create `crates/core-services/{Cargo.toml,src/lib.rs,src/temp.rs}` ; Test `src/temp.rs`.

**Interfaces:**
- Consumes: `core-scan`, `core-ipc`.
- Produces: `struct TempService { min_age_days: u32 }` impl `Service`. Zones : `/tmp`, `/var/tmp` (→ `requires_root=true`, `Destination::Permanent`), `~/.cache` (→ user, `Trash`).

- [ ] **Step 1 : Test** : arbo simulant `~/.cache` avec fichiers vieux/récents ; `scan()` ne retient que > `min_age_days` ; items `~/.cache` ont `requires_root=false`. (Utiliser un root injectable pour tester sans toucher au vrai `/tmp`.)
- [ ] **Step 2 : Run → FAIL**.
- [ ] **Step 3 : Implémenter** `TempService` (paramétrer les racines pour testabilité ; en prod, racines réelles).
- [ ] **Step 4 : Run → PASS**.
- [ ] **Step 5 : Test `preview`** : la sélection produit un `DeletionPlan` avec `total_bytes` correct, `requires_root` vrai si un item `/var/tmp` est inclus.
- [ ] **Step 6 : Commit** `feat(core-services): temp files service`.

### Task 4.2 : Service Big Files

**Files:** Create `crates/core-services/src/big_files.rs` ; Test idem.

**Interfaces:**
- Consumes: `core-scan::top_n`.
- Produces: `struct BigFilesService { root: PathBuf, top: usize }` impl `Service`. `scan()` renvoie top-N (kind `File`/`Dir`). `preview()` = sélection → plan (Trash par défaut).

- [ ] **Step 1 : Test** : arbo tailles connues → `scan()` renvoie top-N triés desc. **Lecture seule** (aucune mutation).
- [ ] **Step 2 : Run → FAIL**. **Step 3 :** impl. **Step 4 : PASS**.
- [ ] **Step 5 : Commit** `feat(core-services): largest files/dirs service`.

### Task 4.3 : Service Git (invariant git-safe)

**Files:** Create `crates/core-services/src/git_repos.rs` ; Test idem (fixtures repos via `git2`).

**Interfaces:**
- Consumes: `git2`, `core-scan`.
- Produces: `struct GitService { search_root: PathBuf }` impl `Service`. Détecte repos, worktrees prunables, branches mergées, taille `.git/objects`. `preview()` exclut tout worktree dont `status` n'est pas propre.

- [ ] **Step 1 : Test git-safe** : créer un repo + worktree avec un fichier non commité ; `preview([ce worktree])` NE l'inclut PAS (invariant 5). Créer un worktree propre → inclus.
- [ ] **Step 2 : Run → FAIL**.
- [ ] **Step 3 : Implémenter** : énumération worktrees (`git2` / lecture `.git/worktrees`), `statuses()` pour vérifier propreté, détection branches mergées (`git2` merge-base), taille `.git/objects` via `core-scan`.
- [ ] **Step 4 : Run → PASS**.
- [ ] **Step 5 : Test branches mergées** détectées correctement. **Step 6 : Commit** `feat(core-services): git worktree/branch cleanup (status-safe)`.

### Task 4.4 : Service Dev Cache

**Files:** Create `crates/core-services/src/dev_cache.rs` ; Test idem.

**Interfaces:**
- Consumes: `core-scan`.
- Produces: `struct DevCacheService { search_root: PathBuf }` impl `Service`. Détection par signature : `node_modules`, `target` (Rust, si `Cargo.toml` frère), `.next`, `.turbo`, `.venv`, `vendor` (PHP, si `composer.json` frère), `~/.npm`, pnpm store. `last_access` renseigné. `preview()` → Trash.

- [ ] **Step 1 : Test** : arbo simulant un projet Rust (`Cargo.toml` + `target/`) et un projet Node (`package.json` + `node_modules/`) → `scan()` détecte les 2, calcule les tailles, `requires_root=false`.
- [ ] **Step 2 : Run → FAIL**. **Step 3 :** impl (signatures + heuristique frère de fichier manifeste). **Step 4 : PASS**.
- [ ] **Step 5 : Commit** `feat(core-services): dev cache detection service`.

---

## Phase 5 — Backend Tauri (commands + routage exécution)

### Task 5.1 : Commands `scan` / `preview` / `execute` + cache + appel helper

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs` (register handlers, parse `--headless`)
- Modify: `src-tauri/Cargo.toml` (dépendances crates cœur)
- Test: `src-tauri/src/commands.rs` (tests de routage execute)

**Interfaces:**
- Consumes: tous les `core-*`.
- Produces (commands Tauri, names figés pour le front) :
  - `scan(service: ServiceId) -> ScanResult`
  - `preview(service: ServiceId, selection: Vec<String>) -> DeletionPlan`
  - `execute(plan: DeletionPlan) -> ExecutionReport`
  - `disk_usage() -> Vec<MountUsage>` (`{ mount: String, total: u64, used: u64 }`)
- Routage `execute` : si `plan.requires_root` → sérialiser le plan, lancer `pkexec /usr/lib/freeyourdisk/freeyourdisk-helper`, passer le plan sur stdin, lire le report ; sinon → `core-trash` in-process selon `plan.destination`.

- [ ] **Step 1 : Test routage** : un `DeletionPlan { requires_root: false, destination: Trash }` avec chemins en zone temp → `execute` appelle `core-trash::to_trash` (vérifier via fixture `tempfile`, report.deleted_count correct).
- [ ] **Step 2 : Run → FAIL** (`cargo test -p freeyourdisk`).
- [ ] **Step 3 : Implémenter** `execute` + `scan`/`preview` (dispatch sur `ServiceId`) + `disk_usage` (lecture des montages, ex. `sysinfo`). Cache du dernier `ScanResult` par service dans un `State` Tauri (Mutex).
- [ ] **Step 4 : Run → PASS**.
- [ ] **Step 5 : Implémenter l'appel helper** (chemin root) — testé en intégration léger (mock du binaire helper via une var d'env de chemin override en test).
- [ ] **Step 6 : Register `invoke_handler`** dans `main.rs` + flag `--headless` (parse args ; si présent, ne pas lancer la WebView — branché en Phase 7).
- [ ] **Step 7 : Commit** `feat(app): Tauri commands scan/preview/execute + root routing via pkexec`.

---

## Phase 6 — Front Svelte (UI + graphes)

> **Avant cette phase : charger `frontend-design`, `design-taste-frontend`, `ui-ux-pro-max`.**

### Task 6.1 : Setup front (Svelte + Vite + i18n + design tokens)

**Files:** Create `ui/package.json`, `ui/vite.config.ts`, `ui/tsconfig.json`, `ui/src/app.css` (tokens), `ui/src/lib/i18n/{en,fr}.json`, `ui/src/lib/i18n/index.ts`, `ui/src/lib/api.ts` ; Modify `src-tauri/tauri.conf.json` (devUrl/frontendDist).

**Interfaces:**
- Produces: `ui/src/lib/api.ts` — wrappers typés : `scan(service)`, `preview(...)`, `execute(plan)`, `diskUsage()` appelant `invoke()` avec les types miroirs de `core-ipc`.

- [ ] **Step 1 :** Scaffold Svelte+TS+Vite, intégration Tauri (`@tauri-apps/api`). Tokens CSS (couleurs, espacements, thème sombre/clair via `prefers-color-scheme` + toggle).
- [ ] **Step 2 :** Setup `svelte-i18n` avec `en.json`/`fr.json` (clés initiales : `dashboard.title`, `service.temp`, etc.). `index.ts` initialise locale depuis navigateur, fallback `en`.
- [ ] **Step 3 :** `api.ts` : types TS miroirs (`ScanItem`, `DeletionPlan`, ...) + wrappers `invoke`. Test Vitest : `api.scan` appelle `invoke('scan', {service})` (mock `@tauri-apps/api`).
- [ ] **Step 4 :** Run `pnpm test` → PASS ; `pnpm build` → OK.
- [ ] **Step 5 : Commit** `feat(ui): Svelte+Vite scaffold, i18n (fr/en), design tokens, typed API`.

### Task 6.2 : Dashboard + donut usage disque

**Files:** Create `ui/src/routes/+page.svelte` (dashboard), `ui/src/lib/components/DiskDonut.svelte`, `ui/src/lib/components/ServiceCard.svelte`.

**Interfaces:**
- Consumes: `api.diskUsage()`, `api.scan()`.
- Produces: dashboard affichant donut (ECharts) + 1 carte par service (taille détectée + bouton « Analyser »).

- [ ] **Step 1 :** `DiskDonut.svelte` — ECharts donut alimenté par `diskUsage()` (used/total par montage). État chargement + erreur (avec toast i18n).
- [ ] **Step 2 :** `ServiceCard.svelte` — props `{ id, label, sizeBytes }`, bouton émettant un event navigation. Toutes les chaînes via `$_(...)`.
- [ ] **Step 3 :** Dashboard compose donut + 4 cartes ; KPI « espace récupérable estimé » (somme des scans).
- [ ] **Step 4 :** Vérif visuelle (`pnpm tauri dev`) : donut rendu, cartes peuplées.
- [ ] **Step 5 : Commit** `feat(ui): dashboard with disk donut + service cards`.

### Task 6.3 : Vue service (table + treemap/bar chart)

**Files:** Create `ui/src/routes/service/[id]/+page.svelte`, `ui/src/lib/components/{ItemsTable,Treemap,CategoryBar}.svelte`.

**Interfaces:**
- Consumes: `api.scan(service)`, `api.preview(...)`.
- Produces: table triable/filtrable des `ScanItem` + graphe contextuel (treemap pour big-files, bar par catégorie sinon) + sélection multiple.

- [ ] **Step 1 :** `ItemsTable.svelte` — colonnes (chemin, taille humanisée, dernier accès, badge `requires_root`), tri par taille, cases à cocher → store de sélection.
- [ ] **Step 2 :** `Treemap.svelte` (ECharts treemap) pour le service big-files ; `CategoryBar.svelte` (bar) pour temp/git/dev-cache.
- [ ] **Step 3 :** Page service : charge `scan(id)`, affiche table + graphe selon `id`, bouton « Prévisualiser le nettoyage » (→ Task 6.4) actif si sélection non vide.
- [ ] **Step 4 :** Vérif visuelle + Vitest sur l'humanisation des tailles + tri.
- [ ] **Step 5 : Commit** `feat(ui): per-service view with sortable table + treemap/bar charts`.

### Task 6.4 : Drawer de confirmation (dry-run) + exécution + rapport

**Files:** Create `ui/src/lib/components/{ConfirmDrawer,Report}.svelte`, `ui/src/lib/stores/selection.ts`, `ui/src/lib/format.ts`.

**Interfaces:**
- Consumes: `api.preview`, `api.execute`.
- Produces: flux complet sélection → preview → confirmation → execute → rapport.

- [ ] **Step 1 :** `ConfirmDrawer.svelte` — affiche le `DeletionPlan` (nombre, `total_bytes` humanisé, destination), **badge rouge** si `Permanent` ou `requires_root`, toggle « suppression définitive » (opt-in explicite, défaut corbeille). Bouton « Confirmer » désactivé tant que non lu.
- [ ] **Step 2 :** À la confirmation : `execute(plan)` → toast succès/erreur (i18n) → `Report.svelte` (animation espace libéré, graphe avant/après) → re-scan delta.
- [ ] **Step 3 :** Vitest : le drawer affiche bien le badge root/définitif ; un plan vide ne permet pas de confirmer.
- [ ] **Step 4 :** Vérif visuelle du flux complet (sur fixtures sûres, ex. `~/.cache` factice).
- [ ] **Step 5 : Commit** `feat(ui): dry-run confirm drawer + execution + post-clean report`.

### Task 6.5 : Polish UI (passe finale)

> **Avant : charger `web-design-reviewer` puis `polish`.**

**Files:** Modify composants existants (transitions, états vides/chargement/erreur, accessibilité, responsive).

- [ ] **Step 1 :** Passer chaque écran au crible `polish` : micro-interactions (transitions Svelte), états vides explicites, focus clavier, contrastes, cohérence des espacements/typo.
- [ ] **Step 2 :** Revue `web-design-reviewer` ; appliquer les retours.
- [ ] **Step 3 :** Vérif a11y (navigation clavier, aria sur table/drawer), responsive.
- [ ] **Step 4 : Commit** `style(ui): final polish pass (interactions, empty states, a11y)`.

---

## Phase 7 — Planification (systemd user timer)

### Task 7.1 : Mode `--headless` + unités systemd user

**Files:**
- Modify: `src-tauri/src/main.rs` (brancher `--headless --service=temp --apply`)
- Create: `packaging/systemd/freeyourdisk.service`, `packaging/systemd/freeyourdisk.timer`
- Create: `src-tauri/src/headless.rs`
- Test: `src-tauri/src/headless.rs`

**Interfaces:**
- Consumes: `core-services::TempService`, `core-trash`.
- Produces: exécution non interactive du service temp sur zones **user uniquement** (jamais root sans pré-autorisation), + `notify-send` du résultat.

- [ ] **Step 1 : Test** : `run_headless(ServiceId::Temp, apply=false)` (dry-run) ne supprime rien et renvoie un report « 0 freed » ; `apply=true` sur fixture `~/.cache` factice supprime vers corbeille. Aucun chemin `requires_root` n'est traité en headless.
- [ ] **Step 2 : Run → FAIL**. **Step 3 :** implémenter `headless.rs` (filtre `requires_root==false`, exécute, `notify-send`). **Step 4 : PASS**.
- [ ] **Step 5 :** Écrire `freeyourdisk.service` (`ExecStart=/usr/bin/freeyourdisk --headless --service=temp --apply`, `Type=oneshot`) + `freeyourdisk.timer` (`OnCalendar=weekly`, `Persistent=true`). **Désactivés par défaut** (activés depuis l'UI via `systemctl --user enable`).
- [ ] **Step 6 :** UI : toggle « nettoyage planifié » dans les réglages → `enable/disable` le timer user via command Tauri.
- [ ] **Step 7 : Commit** `feat: headless mode + optional systemd user timer`.

---

## Phase 8 — Packaging `.deb`

### Task 8.1 : `cargo-deb` + assets desktop/Polkit/AppStream/icons + postinst

**Files:**
- Modify: `src-tauri/Cargo.toml` (section `[package.metadata.deb]`)
- Create: `packaging/freeyourdisk.desktop`, `packaging/io.freeyourdisk.metainfo.xml`, `packaging/icons/hicolor/{256x256,scalable}/apps/io.freeyourdisk.{png,svg}`
- Create: `packaging/postinst`, `packaging/postrm`
- Modify: `.github/workflows/ci.yml` (job `build-deb` + `lintian`)

**Interfaces:**
- Produces: un `.deb` installable qui place tous les fichiers aux bons endroits avec les bonnes dépendances.

- [ ] **Step 1 :** `[package.metadata.deb]` : `depends = "libwebkit2gtk-4.1-0, policykit-1 | polkit, git, ..."`, `assets` mappant :
  - `target/release/freeyourdisk` → `/usr/bin/freeyourdisk`
  - `target/release/freeyourdisk-helper` → `/usr/lib/freeyourdisk/freeyourdisk-helper` (0755 root)
  - `packaging/io.freeyourdisk.policy` → `/usr/share/polkit-1/actions/`
  - `packaging/freeyourdisk.desktop` → `/usr/share/applications/`
  - `packaging/io.freeyourdisk.metainfo.xml` → `/usr/share/metainfo/`
  - icônes → `/usr/share/icons/hicolor/...`
  - unités systemd → `/usr/lib/systemd/user/`
- [ ] **Step 2 :** `.desktop` (Name, Exec=freeyourdisk, Icon=io.freeyourdisk, Categories=System;Utility;) + `metainfo.xml` AppStream (id, name, summary, description, screenshots placeholder URLs à remplacer, `<project_license>GPL-3.0-or-later</project_license>`).
- [ ] **Step 3 :** `postinst`/`postrm` idempotents : `update-desktop-database`, `gtk-update-icon-cache`, `update-appstream` si dispo ; aucune action destructive.
- [ ] **Step 4 :** Build : `cargo build --release` (UI + helper) → copier le build front dans le bundle → `cargo deb`. Vérifier le contenu : `dpkg -c target/debian/*.deb`.
- [ ] **Step 5 :** `lintian target/debian/*.deb` → corriger les erreurs (priorité, section, etc.).
- [ ] **Step 6 :** Smoke-test install : dans un conteneur Debian, `apt install ./*.deb` puis vérifier `which freeyourdisk` + présence policy/desktop.
- [ ] **Step 7 :** CI : ajouter le job `build-deb` (matrice Ubuntu LTS + Debian) + `lintian` + artefact uploadé.
- [ ] **Step 8 : Commit** `build: .deb packaging (cargo-deb) with Polkit/desktop/AppStream/systemd assets`.

---

## Phase 9 — Distribution publique (finitions)

### Task 9.1 : i18n complète + AppStream finalisé + release CI

**Files:** Modify `ui/src/lib/i18n/{en,fr}.json` (couverture 100 %), `packaging/io.freeyourdisk.metainfo.xml` (captures réelles), `.github/workflows/release.yml` (release sur tag).

- [ ] **Step 1 :** Audit i18n : aucun string en dur (grep des composants) ; FR + EN complets, mêmes clés.
- [ ] **Step 2 :** Captures d'écran réelles → AppStream `<screenshots>` + validation `appstreamcli validate`.
- [ ] **Step 3 :** `release.yml` : sur tag `v*`, build `.deb` (matrice), `lintian`, crée une GitHub Release avec le `.deb` attaché.
- [ ] **Step 4 :** README (installation, sécurité/Polkit, licence GPL-3.0).
- [ ] **Step 5 : Commit** `chore: complete i18n, AppStream screenshots, release workflow, README`.

---

## Notes d'exécution

- **TDD strict** : chaque tâche commence par un test qui échoue (sauf scaffolding/packaging où le « test » est une vérification de build/lintian/smoke).
- **Commits fréquents** : un commit par tâche minimum.
- **Concurrence** : `git add <chemins explicites>` (pas `git add -A` en contexte multi-instances).
- **Skills** : recharger les skills listés en tête de chaque phase AVANT d'écrire du code (discipline CLAUDE.md).
- **Invariants de sécurité** : ne jamais relâcher les Invariants 1–5 ; ils ont chacun un test dédié.
