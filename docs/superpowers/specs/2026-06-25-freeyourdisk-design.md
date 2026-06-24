# FreeYourDisk — Design / Spécification

- **Date** : 2026-06-25
- **Statut** : Approuvé (design)
- **Licence projet** : GPL-3.0-or-later
- **Cible** : Debian/Ubuntu (distribution publique visée), bureaux multiples

## 1. Objectif

FreeYourDisk est un utilitaire desktop Linux qui aide l'utilisateur à libérer
de l'espace disque en toute sécurité, via une interface visuelle moderne
(graphes), avec quatre services d'analyse/nettoyage et un packaging `.deb`
installable avec ses dépendances.

Le binaire s'appelle `freeyourdisk` (alias court `fyd`).

### Principes directeurs (non négociables)

1. **Moindre privilège** : l'UI ne tourne jamais en root. Un helper privilégié
   minimal est invoqué via Polkit uniquement pour les actions root.
2. **Scan strictement en lecture seule** : aucune phase de scan ne modifie le
   système de fichiers (propriété testée).
3. **Dry-run + confirmation obligatoires** avant toute suppression.
4. **Corbeille par défaut** (récupérable) ; suppression définitive uniquement
   en opt-in explicite.
5. **Zéro suppression de travail git non commité**.

## 2. Stack technique

| Couche | Choix |
|--------|-------|
| Framework app | Tauri (cœur Rust + WebView) |
| Backend | Rust (workspace Cargo multi-crates) |
| Front | Svelte + TypeScript + Vite |
| Graphes | ECharts (donut, treemap, bar charts) |
| i18n | `svelte-i18n` (clés dès le départ, FR + EN livrés) |
| Élévation privilèges | Polkit / `pkexec` + binaire helper dédié |
| Corbeille | crate `trash` (XDG) |
| Scan FS | `jwalk` + `rayon` (parcours parallèle) |
| Git | `git2` |
| Packaging | bundler Tauri + `cargo-deb` (contrôle fin du control file) |
| CI | GitHub Actions (matrice Ubuntu LTS + Debian) |

## 3. Architecture (split-privilege, 3 processus)

```
┌─────────────────────────────────────────────────────┐
│  UI Svelte (WebView, user)                           │
│  dashboard · graphes · listes · confirmations        │
└───────────────▲──────────────────────────────────────┘
                │ Tauri IPC (commands/events)
┌───────────────┴──────────────────────────────────────┐
│  Backend Tauri/Rust (process user, sans privilège)    │
│  orchestration · cache scan · appels aux crates cœur  │
└──────┬───────────────────────────────────┬───────────┘
       │ in-process (lib Rust)              │ pkexec (Polkit)
┌──────▼──────────────────┐      ┌──────────▼────────────┐
│  Crates cœur (lib)       │      │  Helper privilégié    │
│  scan · sizing · git ·   │      │  binaire root minimal │
│  trash · cache-detect    │      │  supprime une liste   │
│  (lecture + actions user)│      │  de chemins re-validés│
└─────────────────────────┘      └───────────────────────┘
```

L'invariant : le scan est toujours lecture seule, l'UI n'a aucun privilège,
seul le helper minimal (sans UI, sans réseau) supprime des chemins root après
re-validation.

### Découpage en crates (workspace Cargo)

| Crate | Rôle | Dépend de |
|-------|------|-----------|
| `core-scan` | Parcours FS parallèle, tailles, top fichiers/dossiers | — |
| `core-services` | Logique des 4 services (temp, gros fichiers, git, caches dev) | `core-scan`, `git2` |
| `core-trash` | Corbeille XDG + suppression définitive opt-in | — |
| `core-ipc` | Types/DTOs partagés UI↔backend (serde) — source de vérité des contrats | serde |
| `app` (bin Tauri) | Orchestration, commands Tauri, cache de scan, appel helper | toutes |
| `privhelper` (bin) | Binaire root minimal : reçoit chemins validés, re-valide, supprime | `core-trash` |

Chaque crate a une responsabilité unique et est testable isolément. `core-ipc`
centralise les contrats pour éviter les divergences UI↔Rust. `privhelper` est
volontairement minimal pour réduire la surface d'attaque.

## 4. Services (v1)

Chaque service expose le même contrat : `scan() → résultats`, puis
`preview(sélection) → plan`, puis `execute(plan)`.

### 4.1 Fichiers temporaires système
- Cibles : `/tmp`, `/var/tmp`, `~/.cache`, `~/.local/share/Trash` (anciens),
  journaux rotés. Filtre par âge configurable (défaut > 7 jours).
- Destination : `/tmp` & `/var/tmp` → suppression définitive (volatiles) ;
  `~/.cache` → corbeille.
- Sécurité : whitelist de chemins racines, refus de tout chemin hors zones.

### 4.2 Top des plus gros fichiers/dossiers
- Scan récursif parallèle, agrégation par dossier, classement top-N.
- Lecture seule pure (explorateur). Visualisation treemap + table triable.
- L'utilisateur envoie manuellement une sélection vers un service de suppression.

### 4.3 Git worktrees + branches obsolètes
- Découverte des repos (scan de `~`). Pour chaque repo : worktrees prunables
  (`git worktree list` + chemins disparus), branches mergées, taille
  `.git/objects`, suggestion `git gc`.
- Actions : `git worktree prune`, suppression de worktree, `git branch -d`,
  `git gc`. **Vérif `git status` propre obligatoire** avant toute action
  touchant un worktree — jamais de suppression de travail non commité.

### 4.4 Caches de développement
- Détection par signature : `node_modules`, `~/.npm`, pnpm store, `target/`
  Rust, `.next`, `.turbo`, `.venv`, `vendor/` PHP, cache Docker
  (`docker system df`, lecture). Classement par taille + dernier accès.
- Destination : corbeille par défaut. Avertit si un `node_modules` appartient à
  un projet récemment actif.

## 5. Flux de données (invariant de sécurité)

```
[Scan: lecture seule] → cache résultats (backend)
        ↓
[Sélection utilisateur dans l'UI]
        ↓
[Preview/dry-run: liste exacte + taille totale + destination
 (corbeille / définitif / root)]
        ↓  ← confirmation explicite obligatoire
[Execute]
   ├─ chemins user  → core-trash (in-process)
   └─ chemins root  → pkexec → privhelper (liste RE-validée côté helper)
        ↓
[Rapport: libéré X Go, N éléments, erreurs] → re-scan delta
```

Le helper re-valide systématiquement : chemins absolus, dans zones autorisées,
pas de symlink hors zone. Il ne fait jamais confiance à ce que l'UI envoie.

## 6. UI / UX & graphes

- **Dashboard** : donut usage disque par montage, KPI espace récupérable estimé,
  cartes par service (taille détectée + bouton).
- **Vue service** : table triable/filtrable + graphe contextuel (treemap pour
  gros fichiers ; bar chart par catégorie pour caches/temp/git).
- **Drawer de confirmation** : récap dry-run avant action (nombre, taille,
  destination), badge rouge si suppression définitive ou root.
- **Rapport post-nettoyage** : animation espace libéré, graphe avant/après.
- Thème sombre/clair, responsive, transitions Svelte sobres.
- **Toast de feedback** (succès/erreur) sur chaque action, traduit via i18n.

## 7. Planification (systemd user timer, optionnel)

- Timer `systemd --user` activable depuis l'UI (jamais imposé). Cible uniquement
  le service « temporaires » (le plus sûr), avec config d'âge et de zones.
- Exécution : `freeyourdisk --headless --service=temp --apply` lancé par le
  timer ; notification desktop (`notify-send`).
- Réutilise le même `core-services` que l'UI (zéro duplication).
- Le mode headless ne déclenche **jamais** d'action root sans pré-autorisation :
  le timer ne touche qu'aux zones user.

## 8. Packaging `.deb`

- Build : `tauri build` → bundle ; `.deb` via bundler Tauri complété par
  `cargo-deb` (contrôle fin du control file).
- Contenu du paquet :
  - `/usr/bin/freeyourdisk` (UI)
  - `/usr/lib/freeyourdisk/freeyourdisk-helper` (privhelper)
  - `/usr/share/polkit-1/actions/io.freeyourdisk.policy`
  - `/usr/share/applications/freeyourdisk.desktop` + icônes hicolor
  - `/usr/share/metainfo/io.freeyourdisk.metainfo.xml` (AppStream)
  - unités systemd user (timer/service, **désactivées par défaut**)
- **Dépendances** (`Depends:`) : `libwebkit2gtk-4.1-0`, `policykit-1` (ou
  `polkit`), runtime GTK requis par Tauri, `git`. Versions calées sur Debian
  stable + Ubuntu LTS.
- `postinst`/`postrm` : enregistrement icônes/AppStream, idempotents, aucune
  action destructive.

## 9. Distribution publique

- **i18n** dès le départ : chaînes UI en clés, FR + EN livrés, structure prête
  pour d'autres langues.
- **AppStream metainfo** complet (résumé, description, captures, catégories).
- **Licence** : GPL-3.0-or-later (fichier `LICENSE` + en-têtes).
- **CI** GitHub Actions : lint (clippy + eslint), tests, build `.deb`, vérif
  `lintian`, artefact téléchargeable. Matrice Ubuntu LTS + Debian.
- Diffusion : release GitHub avec `.deb` attaché (PPA/dépôt apt optionnel,
  ultérieur).

## 10. Tests

- **Rust** : tests unitaires par crate (scan sur arbo `tempfile`, détection
  caches, parsing git sur repos fixtures). `privhelper` : refus de chemins hors
  zone, refus symlink, validation stricte.
- **Invariant de sécurité testé** : `scan()` ne modifie jamais le FS ;
  `execute()` refuse tout chemin hors whitelist.
- **Front** : tests composants (Vitest) sur la logique sélection/preview ; e2e
  léger (WebDriver Tauri) sur scan→preview→annuler.
- **Packaging** : `lintian` en CI + smoke-test d'installation du `.deb` en
  conteneur.

## 11. Sécurité — récap des garde-fous

1. Moindre privilège : UI sans droit root ; helper minimal isolé.
2. Scan strictement lecture seule (testé).
3. Dry-run + confirmation obligatoires avant suppression.
4. Corbeille par défaut ; définitif uniquement opt-in explicite.
5. Whitelist de zones + re-validation côté helper (anti chemin/symlink piégé).
6. Git : jamais de suppression touchant du travail non commité.
7. Polkit : action ciblée, pas de blanc-seing root.

## 12. Hors périmètre v1 (YAGNI)

- Nettoyage de paquets système (`apt autoremove`, snaps) — non inclus.
- Analyse/déduplication de fichiers (doublons) — non inclus.
- Synchronisation cloud / sauvegarde — hors sujet.
- Support non-Debian (rpm, Flatpak, Snap) — ultérieur éventuel.

## 13. Questions résolues

| Sujet | Décision |
|-------|----------|
| Stack | Tauri (Rust + Svelte) |
| Services v1 | Les 4 (temp, gros fichiers, git, caches dev) |
| Suppression | Corbeille par défaut + dry-run obligatoire |
| Privilèges | UI user + helper Polkit (split-privilege) |
| Cible | Debian/Ubuntu, distribution publique |
| Planification | systemd user timer optionnel (service temp) |
| Périmètre spec | Tout en une seule spec (cœur + planif + distribution) |
| Nom | FreeYourDisk (`freeyourdisk` / `fyd`) |
| Licence | GPL-3.0-or-later |
