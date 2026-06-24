<script lang="ts">
  import { isLoading } from "svelte-i18n";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { view } from "./lib/stores";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import Dashboard from "./lib/views/Dashboard.svelte";
  import ServiceView from "./lib/views/ServiceView.svelte";
  import TrayWidget from "./lib/views/TrayWidget.svelte";

  let label = "main";
  try {
    label = getCurrentWindow().label;
  } catch {
    // Not running under Tauri (e.g. a plain browser preview) — default to main.
  }
  const isTray = label === "tray";
</script>

{#if !$isLoading}
  {#if isTray}
    <TrayWidget />
  {:else}
    <div class="flex h-screen overflow-hidden">
      <Sidebar />
      <main class="flex-1 overflow-y-auto">
        {#if $view.kind === "dashboard"}
          <Dashboard />
        {:else}
          {#key $view.id}
            <ServiceView service={$view.id} />
          {/key}
        {/if}
      </main>
    </div>
    <Toasts />
  {/if}
{/if}
