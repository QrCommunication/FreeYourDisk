// SPDX-License-Identifier: GPL-3.0-or-later
//! Maps a `ServiceId` to its configured service implementation.

use crate::state::Config;
use core_ipc::ServiceId;
use core_services::{BigFilesService, DevCacheService, GitService, Service, TempService};

pub fn make_service(id: ServiceId, cfg: &Config) -> Box<dyn Service> {
    match id {
        ServiceId::Temp => Box::new(TempService::with_defaults(&cfg.home, cfg.temp_min_age_days)),
        ServiceId::BigFiles => Box::new(BigFilesService {
            root: cfg.search_root.clone(),
            top: cfg.big_files_top,
        }),
        ServiceId::GitRepos => Box::new(GitService::new(cfg.search_root.clone())),
        ServiceId::DevCache => Box::new(DevCacheService::new(cfg.search_root.clone())),
    }
}
