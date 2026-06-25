import { writable, get } from "svelte/store";
import { locale } from "svelte-i18n";
import { api, type LangPref, type Settings, type ThemePref } from "./api";

const DEFAULTS: Settings = {
  theme: "system",
  language: "system",
  autostart: false,
  monitor_enabled: true,
  monitor_threshold: 5,
  shortcut: "Ctrl+Alt+Delete",
};

/** The persisted settings (optimistically updated). */
export const settings = writable<Settings>(DEFAULTS);

/** The theme actually applied right now ("light" | "dark") — charts read this. */
export const resolvedTheme = writable<"light" | "dark">("dark");

const media = window.matchMedia("(prefers-color-scheme: dark)");

function systemTheme(): "light" | "dark" {
  return media.matches ? "dark" : "light";
}

function systemLang(): "fr" | "en" {
  return navigator.language.toLowerCase().startsWith("fr") ? "fr" : "en";
}

function applyTheme(pref: ThemePref): void {
  const resolved = pref === "system" ? systemTheme() : pref;
  document.documentElement.classList.toggle("dark", resolved === "dark");
  resolvedTheme.set(resolved);
}

function applyLanguage(pref: LangPref): void {
  locale.set(pref === "system" ? systemLang() : pref);
}

/** Load settings from the backend and apply theme + language. */
export async function initSettings(): Promise<void> {
  let loaded = DEFAULTS;
  try {
    loaded = await api.getSettings();
  } catch {
    /* fall back to defaults */
  }
  settings.set(loaded);
  applyTheme(loaded.theme);
  applyLanguage(loaded.language);

  // Follow OS theme changes while in "system" mode.
  media.addEventListener("change", () => {
    if (get(settings).theme === "system") applyTheme("system");
  });
}

/** Patch settings, apply side effects immediately, then persist. */
export async function updateSettings(patch: Partial<Settings>): Promise<void> {
  const next = { ...get(settings), ...patch };
  settings.set(next);
  if (patch.theme !== undefined) applyTheme(next.theme);
  if (patch.language !== undefined) applyLanguage(next.language);
  try {
    await api.setSettings(next);
  } catch {
    /* keep optimistic value */
  }
}
