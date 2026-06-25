<script lang="ts">
  import { onMount } from "svelte";
  import { isLoading } from "svelte-i18n";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { nav } from "./lib/stores";
  import { initSettings } from "./lib/settings";
  import { restoreHomeCache, runAllScans } from "./lib/scan";
  import type { ServiceId } from "./lib/api";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import LowSpaceModal from "./lib/components/LowSpaceModal.svelte";
  import Home from "./lib/views/Home.svelte";
  import Health from "./lib/views/Health.svelte";
  import Settings from "./lib/views/Settings.svelte";
  import ServiceView from "./lib/views/ServiceView.svelte";
  import Applications from "./lib/views/Applications.svelte";
  import TrayWidget from "./lib/views/TrayWidget.svelte";

  let label = "main";
  try {
    label = getCurrentWindow().label;
  } catch {
    // Not running under Tauri (browser preview) — default to main.
  }
  const isTray = label === "tray";

  onMount(async () => {
    await initSettings();
    if (isTray) return;
    // Show last results instantly, then refresh in the background on startup.
    const hadCache = await restoreHomeCache();
    void runAllScans(hadCache);
  });
</script>

{#if !$isLoading}
  {#if isTray}
    <TrayWidget />
  {:else}
    <div class="flex h-screen overflow-hidden">
      <Sidebar />
      <main class="flex-1 overflow-y-auto">
        {#if $nav === "home"}
          <Home />
        {:else if $nav === "applications"}
          <Applications />
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
{/if}
