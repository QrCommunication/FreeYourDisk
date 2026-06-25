<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { api, SERVICES, type MountUsage } from "../api";
  import { goService, toasts } from "../stores";
  import { humanizeBytes, usedPercent } from "../format";
  import DiskDonut from "../components/DiskDonut.svelte";
  import ServiceCard from "../components/ServiceCard.svelte";
  import ScheduleToggle from "../components/ScheduleToggle.svelte";
  import StateBlock from "../components/StateBlock.svelte";

  let status = $state<"loading" | "error" | "data">("loading");
  let mounts = $state<MountUsage[]>([]);

  const GIB = 1024 ** 3;

  function relevant(list: MountUsage[]): MountUsage[] {
    return list
      .filter(
        (m) =>
          m.total > GIB &&
          !m.mount.startsWith("/snap") &&
          !m.mount.startsWith("/boot"),
      )
      .sort((a, b) => b.total - a.total);
  }

  const primary = $derived(mounts.find((m) => m.mount === "/") ?? mounts[0]);
  const others = $derived(mounts.filter((m) => m !== primary).slice(0, 5));

  async function load() {
    status = "loading";
    try {
      mounts = relevant(await api.diskUsage());
      status = "data";
    } catch {
      status = "error";
      toasts.error($_("toast.scan_error"));
    }
  }

  onMount(load);
</script>

<div class="mx-auto max-w-5xl px-8 py-8">
  <header class="mb-7">
    <h1 class="text-ink text-xl font-semibold tracking-tight">
      {$_("nav.dashboard")}
    </h1>
    <p class="text-muted text-sm">{$_("app.tagline")}</p>
  </header>

  {#if status === "loading"}
    <StateBlock kind="loading" />
  {:else if status === "error"}
    <StateBlock kind="error" onretry={load} />
  {:else}
    <div class="grid grid-cols-1 gap-5 lg:grid-cols-[340px_1fr]">
      <section class="border-line bg-surface rounded-xl border p-5">
        <h2 class="text-faint mb-2 text-xs tracking-wider uppercase">
          {$_("dashboard.disk_usage")}
        </h2>
        {#if primary}
          <DiskDonut mount={primary} />
        {/if}
      </section>

      <section
        class="border-line bg-surface flex flex-col rounded-xl border p-5"
      >
        <h2 class="text-faint mb-3 text-xs tracking-wider uppercase">
          Volumes
        </h2>
        <div class="flex flex-col gap-3.5">
          {#each others as m (m.mount)}
            {@const pct = usedPercent(m.used, m.total)}
            <div>
              <div class="mb-1 flex items-center justify-between text-xs">
                <span class="text-ink font-mono">{m.mount}</span>
                <span class="text-muted nums"
                  >{humanizeBytes(m.used)} / {humanizeBytes(m.total)}</span
                >
              </div>
              <div class="bg-line h-1.5 overflow-hidden rounded-full">
                <div
                  class="h-full rounded-full transition-all"
                  class:bg-accent={pct < 85}
                  class:bg-danger={pct >= 85}
                  style="width: {pct}%"
                ></div>
              </div>
            </div>
          {:else}
            <p class="text-faint text-sm">—</p>
          {/each}
        </div>
      </section>
    </div>

    <h2 class="text-faint mt-8 mb-3 text-xs tracking-wider uppercase">
      Services
    </h2>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {#each SERVICES as id (id)}
        <ServiceCard {id} onopen={() => goService(id)} />
      {/each}
    </div>

    <div class="mt-3">
      <ScheduleToggle />
    </div>
  {/if}
</div>
