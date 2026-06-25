import { writable } from "svelte/store";
import type { ServiceId } from "./api";

export type Nav = "home" | ServiceId | "applications" | "health" | "settings";

/** Active top-level section. */
export const nav = writable<Nav>("home");

export function goTo(section: Nav): void {
  nav.set(section);
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
