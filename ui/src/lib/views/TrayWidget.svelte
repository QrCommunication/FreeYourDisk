<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { Window, getCurrentWindow } from "@tauri-apps/api/window";
  import { ArrowRight, CircleNotch } from "phosphor-svelte";
  import { api, type MountUsage } from "../api";
  import DiskDonut from "../components/DiskDonut.svelte";

  let mount = $state<MountUsage | null>(null);
  let loading = $state(true);

  const GIB = 1024 ** 3;

  onMount(async () => {
    try {
      const list = await api.diskUsage();
      const real = list.filter(
        (m) =>
          m.total > GIB &&
          !m.mount.startsWith("/snap") &&
          !m.mount.startsWith("/boot"),
      );
      mount =
        real.find((m) => m.mount === "/") ??
        real.sort((a, b) => b.total - a.total)[0] ??
        null;
    } finally {
      loading = false;
    }
  });

  async function openMain() {
    const main = await Window.getByLabel("main");
    if (main) {
      await main.show();
      await main.unminimize();
      await main.setFocus();
    }
    await getCurrentWindow().hide();
  }
</script>

<div class="bg-base border-line flex h-screen flex-col border p-4">
  <header class="mb-1 flex items-center gap-2.5">
    <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
      <circle
        cx="12"
        cy="12"
        r="8"
        fill="none"
        stroke="var(--c-line)"
        stroke-width="3.5"
      />
      <circle
        cx="12"
        cy="12"
        r="8"
        fill="none"
        stroke="var(--c-accent)"
        stroke-width="3.5"
        stroke-dasharray="36 50"
        stroke-linecap="round"
        transform="rotate(-90 12 12)"
      />
    </svg>
    <span class="text-ink text-sm font-semibold tracking-tight"
      >{$_("app.name")}</span
    >
  </header>

  <div class="flex flex-1 flex-col justify-center">
    {#if loading}
      <div class="flex justify-center py-12">
        <CircleNotch size={26} class="text-accent animate-spin" />
      </div>
    {:else if mount}
      <div
        class="text-faint mb-1 text-center text-[11px] tracking-wider uppercase"
      >
        {$_("tray.occupied")}
      </div>
      <DiskDonut {mount} />
    {:else}
      <p class="text-muted py-10 text-center text-sm">
        {$_("state.error_title")}
      </p>
    {/if}
  </div>

  <button
    class="bg-accent text-accent-ink mt-3 flex cursor-pointer items-center justify-center gap-2 rounded-lg py-2.5 text-sm font-medium transition-all active:translate-y-px"
    onclick={openMain}
  >
    {$_("tray.open")}
    <ArrowRight size={16} weight="bold" />
  </button>
</div>
