import { invoke } from "@tauri-apps/api/core";

// Mirror of the Rust `core-ipc` DTOs. Kept in lockstep with that crate.
export type ServiceId = "temp" | "big_files" | "git_repos" | "dev_cache";
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

export const SERVICES: ServiceId[] = [
  "temp",
  "big_files",
  "git_repos",
  "dev_cache",
];

export const api = {
  scan: (service: ServiceId) => invoke<ScanResult>("scan", { service }),
  preview: (service: ServiceId, selection: string[]) =>
    invoke<DeletionPlan>("preview", { service, selection }),
  execute: (plan: DeletionPlan) => invoke<ExecutionReport>("execute", { plan }),
  diskUsage: () => invoke<MountUsage[]>("disk_usage"),
};
