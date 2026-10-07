<script lang="ts">
  import { onMount } from "svelte";
  import { isLoading } from "svelte-i18n";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { nav, goTo, toasts } from "./lib/stores";
  import { initSettings } from "./lib/settings";
  import { restoreHomeCache, runAllScans } from "./lib/scan";
  import { api, type ServiceId } from "./lib/api";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import LowSpaceModal from "./lib/components/LowSpaceModal.svelte";
  import Home from "./lib/views/Home.svelte";
  import Health from "./lib/views/Health.svelte";
  import Settings from "./lib/views/Settings.svelte";
  import ServiceView from "./lib/views/ServiceView.svelte";
  import Applications from "./lib/views/Applications.svelte";
  import TaskManager from "./lib/views/TaskManager.svelte";
  import TrayWidget from "./lib/views/TrayWidget.svelte";

  let label = "main";
  try {
    label = getCurrentWindow().label;
  } catch {
    // Not running under Tauri (browser preview) — default to main.
  }
  const isTray = label === "tray";

  let startupIssue = $state("");

  function reportStartupIssue(message: string): void {
    startupIssue = message;
    if (!isTray) toasts.error(message);
  }

  async function startMainWindow(): Promise<void> {
    try {
      await initSettings();
    } catch {
      reportStartupIssue(
        "Les préférences n’ont pas pu être chargées. Les réglages par défaut sont utilisés.",
      );
    }

    if (isTray) return;

    // A shortcut failure must not affect navigation or the rest of the boot.
    try {
      await api.onSummon(() => goTo("taskmgr"));
    } catch {
      reportStartupIssue(
        "Le raccourci global est indisponible pour cette session.",
      );
    }

    // Cached results are optional; scans still begin when the cache is absent.
    let hadCache = false;
    try {
      hadCache = await restoreHomeCache();
    } catch {
      reportStartupIssue(
        "Les résultats précédents n’ont pas pu être restaurés.",
      );
    }

    void runAllScans(hadCache).catch(() => {
      reportStartupIssue(
        "L’analyse initiale a rencontré un problème. Vous pouvez continuer à utiliser l’application.",
      );
    });
  }

  onMount(() => {
    void startMainWindow();
  });
</script>

{#if $isLoading}
  <div
    class="bg-base text-ink flex h-screen flex-col items-center justify-center gap-3 px-6 text-center"
  >
    <div
      class="border-accent h-8 w-8 animate-spin rounded-full border-4 border-t-transparent"
    ></div>
    <p class="text-sm font-medium">Chargement de FreeYourDisk…</p>
    {#if startupIssue}
      <p class="text-muted max-w-md text-sm" role="alert">{startupIssue}</p>
    {/if}
  </div>
{:else if isTray}
  <TrayWidget />
{:else}
  <div class="flex h-screen overflow-hidden">
    <Sidebar />
    <main class="flex-1 overflow-y-auto">
      {#if $nav === "home"}
        <Home />
      {:else if $nav === "applications"}
        <Applications />
      {:else if $nav === "taskmgr"}
        <TaskManager />
      {:else if $nav === "health"}
        <Health />
      {:else if $nav === "settings"}
        <Settings />
      {:else}
        {#key $nav}
          <ServiceView service={$nav as ServiceId} />
        {/key}
      {/if}
    </main>
  </div>
  <Toasts />
  <LowSpaceModal />
{/if}
