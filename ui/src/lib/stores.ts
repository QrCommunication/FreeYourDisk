import { writable } from "svelte/store";
import type { ServiceId } from "./api";

export type View = { kind: "dashboard" } | { kind: "service"; id: ServiceId };

/** Current top-level view. */
export const view = writable<View>({ kind: "dashboard" });

export function goDashboard(): void {
  view.set({ kind: "dashboard" });
}
export function goService(id: ServiceId): void {
  view.set({ kind: "service", id });
}

export interface Toast {
  id: number;
  type: "success" | "error";
  message: string;
}

function createToasts() {
  const { subscribe, update } = writable<Toast[]>([]);
  let nextId = 1;

  function push(type: Toast["type"], message: string) {
    const id = nextId++;
    update((list) => [...list, { id, type, message }]);
    setTimeout(() => update((list) => list.filter((t) => t.id !== id)), 4500);
  }

  return {
    subscribe,
    success: (message: string) => push("success", message),
    error: (message: string) => push("error", message),
    dismiss: (id: number) => update((list) => list.filter((t) => t.id !== id)),
  };
}

export const toasts = createToasts();
