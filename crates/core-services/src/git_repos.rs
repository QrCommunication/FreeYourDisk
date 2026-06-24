// SPDX-License-Identifier: GPL-3.0-or-later
//! Git service: finds git repositories under a search root and surfaces their
//! prunable or clean linked worktrees as cleanup candidates.
//!
//! Invariant 5 (project-wide): a worktree with uncommitted changes is NEVER
//! surfaced. Removal frees the worktree directory's disk space; the dangling
//! admin entry can later be cleared with `git worktree prune`.

use crate::{dir_total, path_id, Service};
use core_ipc::{ItemKind, ScanItem, ScanResult, ServiceId};
use git2::{Repository, StatusOptions};
use std::fs;
use std::path::{Path, PathBuf};

const SKIP_DESCENT: &[&str] = &[
    "node_modules",
    "target",
    ".next",
    ".turbo",
    "vendor",
    ".venv",
];

/// Finds git repos under `search_root` and surfaces removable worktrees.
#[derive(Clone, Debug)]
pub struct GitService {
    pub search_root: PathBuf,
    pub max_depth: usize,
}

impl GitService {
    pub fn new(search_root: PathBuf) -> Self {
        Self {
            search_root,
            max_depth: 8,
        }
    }
}

/// Recursively collect directories that are git repositories (contain `.git`).
/// Does not descend into a repository once found.
fn find_repos(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if dir.join(".git").exists() {
        out.push(dir.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skip = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| SKIP_DESCENT.contains(&n))
            .unwrap_or(false);
        if !skip {
            find_repos(&path, depth - 1, out);
        }
    }
}

/// True if the worktree at `path` has a clean status (no changes, no untracked).
fn is_worktree_clean(path: &Path) -> bool {
    let Ok(repo) = Repository::open(path) else {
        return false;
    };
    let mut opts = StatusOptions::new();
    opts.include_untracked(true);
    // Bind before the block ends so the borrowing `Statuses` temporary drops
    // before `repo`.
    let clean = match repo.statuses(Some(&mut opts)) {
        Ok(statuses) => statuses.is_empty(),
        Err(_) => false, // cannot determine → treat as unsafe
    };
    clean
}

/// Removable worktree items for a single repository.
fn worktree_items(repo_path: &Path) -> Vec<ScanItem> {
    let Ok(repo) = Repository::open(repo_path) else {
        return Vec::new();
    };
    let Ok(names) = repo.worktrees() else {
        return Vec::new();
    };

    let mut items = Vec::new();
    for name in names.iter().flatten() {
        let Ok(worktree) = repo.find_worktree(name) else {
            continue;
        };
        let wt_path = worktree.path().to_path_buf();

        let prunable = worktree.is_prunable(None).unwrap_or(false);
        let exists = wt_path.exists();
        let clean = exists && is_worktree_clean(&wt_path);

        // Surface only safe candidates: a clean existing worktree, or one whose
        // working copy is already gone (prunable). Never a dirty one.
        if !(clean || prunable) {
            continue;
        }

        items.push(ScanItem {
            id: path_id(&wt_path),
            size_bytes: dir_total(&wt_path),
            last_access: None,
            path: wt_path,
            kind: ItemKind::GitWorktree,
            requires_root: false,
        });
    }
    items
}

impl Service for GitService {
    fn id(&self) -> ServiceId {
        ServiceId::GitRepos
    }

    fn scan(&self) -> ScanResult {
        let mut repos = Vec::new();
        find_repos(&self.search_root, self.max_depth, &mut repos);

        let mut items = Vec::new();
        for repo in &repos {
            items.extend(worktree_items(repo));
        }

        let total_bytes = items.iter().map(|item| item.size_bytes).sum();
        ScanResult {
            service: ServiceId::GitRepos,
            items,
            total_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(args: &[&str], cwd: &Path) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn surfaces_clean_worktree_but_never_dirty_one() {
        let base = tempfile::tempdir().unwrap();
        let repo = base.path().join("repo");
        std::fs::create_dir(&repo).unwrap();

        git(&["init", "-q", "-b", "main"], &repo);
        git(&["config", "user.email", "t@example.com"], &repo);
        git(&["config", "user.name", "Test"], &repo);
        std::fs::write(repo.join("README.md"), b"hello").unwrap();
        git(&["add", "."], &repo);
        git(&["commit", "-qm", "init"], &repo);

        let clean_wt = base.path().join("wt-clean");
        let dirty_wt = base.path().join("wt-dirty");
        git(
            &[
                "worktree",
                "add",
                "-q",
                clean_wt.to_str().unwrap(),
                "-b",
                "feat-clean",
            ],
            &repo,
        );
        git(
            &[
                "worktree",
                "add",
                "-q",
                dirty_wt.to_str().unwrap(),
                "-b",
                "feat-dirty",
            ],
            &repo,
        );
        // Make the second worktree dirty with an untracked file.
        std::fs::write(dirty_wt.join("WIP.txt"), b"uncommitted").unwrap();

        let svc = GitService::new(base.path().to_path_buf());
        let result = svc.scan();
        let paths: Vec<PathBuf> = result.items.iter().map(|i| i.path.clone()).collect();

        assert!(
            paths.iter().any(|p| p.ends_with("wt-clean")),
            "clean worktree must be surfaced"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("wt-dirty")),
            "dirty worktree must NEVER be surfaced (invariant 5)"
        );
        assert!(result.items.iter().all(|i| i.kind == ItemKind::GitWorktree));
    }
}
