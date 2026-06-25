// SPDX-License-Identifier: GPL-3.0-or-later
//! Task manager: live memory/swap telemetry, a process inventory and crisis
//! actions (terminate / force-kill / restart / panic-kill the biggest hog).
//!
//! Reimplements the core of the standalone `mem-guard` tool inside FreeYourDisk.
//! The app raises its own priority and makes itself OOM-immune at startup so it
//! stays responsive — and killable-of-others — even when the machine thrashes.

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use sysinfo::{Components, Pid, ProcessRefreshKind, ProcessesToUpdate, System, Users};

/// One process row for the table.
#[derive(Serialize, Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    /// CPU usage percent (over the sampling interval; 0 on the first sample).
    pub cpu: f32,
    pub mem_bytes: u64,
    /// Resident memory as a percentage of total RAM.
    pub mem_pct: f32,
    pub user: String,
    pub cmd: String,
}

/// A point of memory/swap/CPU/load telemetry for the live graph.
#[derive(Serialize, Clone, Debug)]
pub struct MemStats {
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Overall CPU usage (%) across all logical cores.
    pub cpu_total: f32,
    /// Per-logical-core usage (%), index order — drives the core heatmap.
    pub cpus: Vec<f32>,
    /// CPU package temperature in °C, if a sensor is available.
    pub cpu_temp: Option<f32>,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
}

/// Persistent sampler: a process + CPU snapshot kept across calls so sysinfo can
/// compute CPU deltas between polls; components carry temperature sensors.
fn sampler() -> &'static Mutex<(System, Users, Components)> {
    static SAMPLER: OnceLock<Mutex<(System, Users, Components)>> = OnceLock::new();
    SAMPLER.get_or_init(|| {
        Mutex::new((
            System::new(),
            Users::new_with_refreshed_list(),
            Components::new_with_refreshed_list(),
        ))
    })
}

/// Pick the CPU package temperature from the available sensors (Intel
/// `Package id 0` / `Core 0`, AMD `Tctl`/`Tdie`/`k10temp`…).
fn cpu_temperature(components: &Components) -> Option<f32> {
    let mut fallback = None;
    for component in components {
        let Some(temp) = component.temperature() else {
            continue;
        };
        let label = component.label().to_lowercase();
        if label.contains("package")
            || label.contains("tctl")
            || label.contains("tdie")
            || label.contains("core 0")
        {
            return Some(temp);
        }
        if label.contains("coretemp") || label.contains("k10temp") || label.contains("cpu") {
            fallback = Some(temp);
        }
    }
    fallback
}

/// Current memory, swap, CPU (overall + per-core), temperature and load — cheap,
/// polled by the UI every second.
pub fn mem_stats() -> MemStats {
    let mut guard = sampler().lock().expect("sampler poisoned");
    let (sys, _, components) = &mut *guard;
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    components.refresh(false);
    let load = System::load_average();
    MemStats {
        mem_total: sys.total_memory(),
        mem_used: sys.used_memory(),
        swap_total: sys.total_swap(),
        swap_used: sys.used_swap(),
        cpu_total: sys.global_cpu_usage(),
        cpus: sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
        cpu_temp: cpu_temperature(components),
        load1: load.one,
        load5: load.five,
        load15: load.fifteen,
    }
}

/// Full process inventory, largest memory first.
pub fn process_list() -> Vec<ProcInfo> {
    let mut guard = sampler().lock().expect("sampler poisoned");
    let (sys, users, _) = &mut *guard;
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    sys.refresh_memory();
    let total_mem = sys.total_memory().max(1);

    let mut out: Vec<ProcInfo> = sys
        .processes()
        .values()
        .map(|p| {
            let mem = p.memory();
            let user = p
                .user_id()
                .and_then(|uid| users.get_user_by_id(uid))
                .map(|u| u.name().to_string())
                .unwrap_or_default();
            let cmd = p
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            ProcInfo {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                cpu: p.cpu_usage(),
                mem_bytes: mem,
                mem_pct: (mem as f64 / total_mem as f64 * 100.0) as f32,
                user,
                cmd,
            }
        })
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.mem_bytes));
    out
}

/// Send SIGTERM (graceful) or SIGKILL (force) to a pid.
pub fn kill_process(pid: u32, force: bool) -> bool {
    let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
    // Direct syscall: works regardless of the sampler's refresh state.
    unsafe { libc::kill(pid as i32, sig) == 0 }
}

/// Capture a process's command line/cwd, kill it, then relaunch it (best effort).
pub fn restart_process(pid: u32) -> Result<(), String> {
    let (exe, args, cwd) = {
        let mut guard = sampler().lock().expect("sampler poisoned");
        let (sys, _, _) = &mut *guard;
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
            true,
            ProcessRefreshKind::everything(),
        );
        let p = sys
            .process(Pid::from_u32(pid))
            .ok_or_else(|| "process not found".to_string())?;
        let exe = p.exe().map(|e| e.to_path_buf());
        let args: Vec<String> = p
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let cwd = p.cwd().map(|c| c.to_path_buf());
        (exe, args, cwd)
    };

    // Graceful, then forceful.
    kill_process(pid, false);
    std::thread::sleep(Duration::from_millis(400));
    kill_process(pid, true);
    std::thread::sleep(Duration::from_millis(150));

    let program = exe
        .or_else(|| args.first().map(PathBuf::from))
        .ok_or_else(|| "no executable to relaunch".to_string())?;
    let mut cmd = Command::new(program);
    if args.len() > 1 {
        cmd.args(&args[1..]);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Critical process names we never panic-kill (Linux desktop daemons).
#[cfg(not(target_os = "macos"))]
const PROTECTED: &[&str] = &[
    "systemd",
    "init",
    "Xorg",
    "Xwayland",
    "gnome-shell",
    "gnome-session",
    "kwin",
    "plasmashell",
    "pipewire",
    "wireplumber",
    "dbus-daemon",
    "dbus-broker",
    "pulseaudio",
    "freeyourdisk",
    "memguardd",
    "mem-guard",
    "sshd",
];

/// Critical process names we never panic-kill (macOS system services).
#[cfg(target_os = "macos")]
const PROTECTED: &[&str] = &[
    "launchd",
    "kernel_task",
    "WindowServer",
    "loginwindow",
    "Finder",
    "Dock",
    "SystemUIServer",
    "coreaudiod",
    "cfprefsd",
    "mds",
    "mds_stores",
    "mdworker",
    "distnoted",
    "UserEventAgent",
    "configd",
    "securityd",
    "freeyourdisk",
    "FreeYourDisk",
    "sshd",
];

/// Emergency: force-kill the largest non-critical memory consumer. The action
/// the user reaches for when the machine is thrashing.
pub fn panic_kill() -> Option<ProcInfo> {
    let self_pid = std::process::id();
    // process_list() is sorted by memory desc, so the first eligible row is the
    // biggest hog.
    let target = process_list()
        .into_iter()
        .find(|p| p.pid != self_pid && !PROTECTED.iter().any(|name| p.name.contains(name)))?;
    kill_process(target.pid, true);
    Some(target)
}

/// Best-effort: make this process OOM-immune and high priority so it survives
/// and stays responsive under memory pressure. `oom_score_adj = -1000` and a
/// negative nice both require privilege; failures are ignored (the bundled
/// systemd unit / pkexec helper grant the real thing).
pub fn raise_priority() {
    // OOM immunity is a Linux concept; macOS has no per-process oom score.
    #[cfg(target_os = "linux")]
    let _ = std::fs::write("/proc/self/oom_score_adj", "-1000");
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, -5);
    }
}
