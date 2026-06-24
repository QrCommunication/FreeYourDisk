// SPDX-License-Identifier: GPL-3.0-or-later
//! The four cleanup services (temp files, largest files, git repos, dev caches).
//! Each implements the `Service` trait: read-only `scan()` then dry-run
//! `preview()`. Execution is centralised in the Tauri backend.
//! Implemented in Phase 4 (Tasks 4.1 to 4.4).
