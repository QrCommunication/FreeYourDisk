// SPDX-License-Identifier: GPL-3.0-or-later
//! XDG trash + opt-in permanent deletion, guarded by a zone whitelist that
//! rejects out-of-zone paths and symlinks escaping the allowed zones.
//! Implemented in Phase 2 (Task 2.1).
