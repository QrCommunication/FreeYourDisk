import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Mirror of the Rust `core-ipc` DTOs. Kept in lockstep with that crate.
export type ServiceId =
  | "temp"
  | "big_files"
  | "git_repos"
  | "dev_cache"
  | "app_cache";
export type Destination = "trash" | "permanent";
export type ItemKind =
  | "file"
  | "dir"
  | "git_worktree"
  | "git_branch"
  | "dev_cache";

export interface ScanItem {
  id: string;
  path: string;
  size_bytes: number;
  last_access: number | null;
  kind: ItemKind;
  requires_root: boolean;
}

export interface ScanResult {
  service: ServiceId;
  items: ScanItem[];
  total_bytes: number;
}

export interface ScanResponse {
  result: ScanResult;
  first_scan: boolean;
  new_ids: string[];
}

export interface DeletionPlan {
  items: ScanItem[];
  destination: Destination;
  total_bytes: number;
  requires_root: boolean;
}

export interface ItemError {
  path: string;
  message: string;
}

export interface ExecutionReport {
  freed_bytes: number;
  deleted_count: number;
  errors: ItemError[];
}

export interface MountUsage {
  mount: string;
  total: number;
  used: number;
}

export interface DiskInfo {
  device: string;
  model: string | null;
  size_bytes: number;
  rotational: boolean;
  read_bytes: number;
  write_bytes: number;
}

export interface HealthOverview {
  uptime_secs: number;
  disks: DiskInfo[];
}

export interface SmartInfo {
  device: string;
  available: boolean;
  passed: boolean | null;
  power_on_hours: number | null;
  temperature_c: number | null;
}

export interface SmartDepsStatus {
  nvme_needed: boolean;
  nvme_installed: boolean;
  sata_needed: boolean;
  smartctl_installed: boolean;
  manager: string | null;
  missing: string[];
  can_install: boolean;
}

export interface InstallReport {
  success: boolean;
  message: string;
}

export interface FileEntry {
  path: string;
  size_bytes: number;
}

export interface TypeBucket {
  category: string;
  bytes: number;
  count: number;
  top: FileEntry[];
}

export type AppSource = "apt" | "flatpak" | "snap" | "appimage";

export interface AppEntry {
  id: string;
  name: string;
  source: AppSource;
  version: string | null;
  size_bytes: number;
  requires_root: boolean;
  protected: boolean;
}

export interface AppActionReport {
  succeeded: string[];
  errors: string[];
}

export type ThemePref = "system" | "light" | "dark";
export type LangPref = "system" | "fr" | "en";

export interface Settings {
  theme: ThemePref;
  language: LangPref;
  autostart: boolean;
  monitor_enabled: boolean;
  monitor_threshold: number;
  shortcut: string;
}

export interface ProcInfo {
  pid: number;
  name: string;
  cpu: number;
  mem_bytes: number;
  mem_pct: number;
  user: string;
  cmd: string;
}

export interface MemStats {
  mem_total: number;
  mem_used: number;
  swap_total: number;
  swap_used: number;
  cpu_total: number;
  cpus: number[];
  cpu_temp: number | null;
  load1: number;
  load5: number;
  load15: number;
}

export interface LowSpaceAlert {
  mount: string;
  free_percent: number;
  free_bytes: number;
  total_bytes: number;
}

export const SERVICES: ServiceId[] = [
  "temp",
  "app_cache",
  "big_files",
  "git_repos",
  "dev_cache",
];

export const api = {
  scan: (service: ServiceId) => invoke<ScanResponse>("scan", { service }),
  preview: (service: ServiceId, selection: string[]) =>
    invoke<DeletionPlan>("preview", { service, selection }),
  execute: (plan: DeletionPlan) => invoke<ExecutionReport>("execute", { plan }),
  diskUsage: () => invoke<MountUsage[]>("disk_usage"),
  scheduleEnabled: () => invoke<boolean>("schedule_enabled"),
  setSchedule: (enabled: boolean) =>
    invoke<boolean>("set_schedule", { enabled }),
  healthOverview: () => invoke<HealthOverview>("health_overview"),
  diskSmart: () => invoke<SmartInfo[]>("disk_smart"),
  smartDepsStatus: () => invoke<SmartDepsStatus>("smart_deps_status"),
  installSmartDeps: () => invoke<InstallReport>("install_smart_deps"),
  fileTypes: () => invoke<TypeBucket[]>("file_types"),
  homeTotal: () => invoke<number>("home_total"),
  systemTotal: () => invoke<number>("system_total"),
  listApplications: () => invoke<AppEntry[]>("list_applications"),
  appUpdates: () => invoke<string[]>("app_updates"),
  uninstallApps: (ids: string[]) =>
    invoke<AppActionReport>("uninstall_apps", { ids }),
  updateApps: (ids: string[]) =>
    invoke<AppActionReport>("update_apps", { ids }),
  homeCacheLoad: () => invoke<string | null>("home_cache_load"),
  homeCacheSave: (data: string) => invoke<void>("home_cache_save", { data }),
  appVersion: () => invoke<string>("app_version"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) =>
    invoke<Settings>("set_settings", { settings }),
  onLowSpace: (handler: (alert: LowSpaceAlert) => void): Promise<UnlistenFn> =>
    listen<LowSpaceAlert>("low-space", (event) => handler(event.payload)),
  // Task manager
  memStats: () => invoke<MemStats>("mem_stats"),
  processList: () => invoke<ProcInfo[]>("process_list"),
  killProcess: (pid: number, force: boolean) =>
    invoke<boolean>("kill_process", { pid, force }),
  restartProcess: (pid: number) => invoke<void>("restart_process", { pid }),
  panicKill: () => invoke<ProcInfo | null>("panic_kill"),
  onSummon: (handler: () => void): Promise<UnlistenFn> =>
    listen("summon-taskmgr", () => handler()),
};
