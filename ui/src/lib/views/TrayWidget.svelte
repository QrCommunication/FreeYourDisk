<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { Window, getCurrentWindow } from "@tauri-apps/api/window";
  import {
    ArrowRight,
    Broom,
    CircleNotch,
    Package,
    ArrowsClockwise,
    Trash,
    GitBranch,
    Sparkle,
  } from "phosphor-svelte";
  import type { EChartsOption } from "echarts";
  import { api, type MountUsage, type AppUpdate } from "../api";
  import Chart from "../components/Chart.svelte";
  import { humanizeBytes, usedPercent } from "../format";
  import { C } from "../theme";

  let mount = $state<MountUsage | null>(null);
  let reclaimable = $state(0);
  let updateCount = $state(0);
  let loading = $state(true);
  let cleaning = $state(false);

  const GIB = 1024 ** 3;

  /** Donut: used vs free vs reclaimable overlay */
  const chartOption = $derived<EChartsOption>(() => {
    if (!mount) return {};
    const free = Math.max(0, mount.total - mount.used);
    const safeReclaim = Math.min(reclaimable, free);
    return {
      animationDuration: 500,
      series: [
        {
          type: "pie",
          radius: ["68%", "88%"],
          silent: true,
          avoidLabelOverlap: false,
          label: { show: false },
          labelLine: { show: false },
          data: [
            { value: mount.used, name: "used", itemStyle: { color: C.accent } },
            {
              value: Math.max(0, free - safeReclaim),
              name: "free",
              itemStyle: { color: C.line },
            },
            {
              value: safeReclaim,
              name: "reclaimable",
              itemStyle: { color: C.freed },
            },
          ],
        },
      ],
    };
  });

  const pct = $derived(mount ? Math.round(usedPercent(mount.used, mount.total)) : 0);
  const freeBytes = $derived(mount ? Math.max(0, mount.total - mount.used) : 0);

  async function loadData() {
    loading = true;
    try {
      const [diskList, scanResults] = await Promise.all([
        api.diskUsage(),
        Promise.allSettled(
          ["temp", "app_cache", "dev_cache"] as const,
        ).then(() => [] as number[]),
      ]);

      // Pick primary mount
      const real = diskList.filter(
        (m) =>
          m.total > GIB &&
          !m.mount.startsWith("/snap") &&
          !m.mount.startsWith("/boot"),
      );
      mount =
        real.find((m) => m.mount === "/") ??
        real.sort((a, b) => b.total - a.total)[0] ??
        null;

      // Quick parallel scan of lightweight services for reclaimable estimate
      const scans = await Promise.allSettled([
        api.scan("temp"),
        api.scan("app_cache"),
        api.scan("dev_cache"),
      ]);
      let total = 0;
      for (const s of scans) {
        if (s.status === "fulfilled") total += s.value.result.total_bytes;
      }
      reclaimable = total;

      // App updates count
      try {
        const upd = await api.appUpdates();
        updateCount = upd.entries.filter((u: AppUpdate) => u.can_update).length;
      } catch {
        updateCount = 0;
      }
    } finally {
      loading = false;
    }
  }

  async function quickClean() {
    if (cleaning) return;
    cleaning = true;
    try {
      // Scan temp + app_cache, select all, execute trash
      const [tempResp, cacheResp] = await Promise.allSettled([
        api.scan("temp"),
        api.scan("app_cache"),
      ]);
      const items = [];
      if (tempResp.status === "fulfilled")
        items.push(...tempResp.value.result.items);
      if (cacheResp.status === "fulfilled")
        items.push(...cacheResp.value.result.items);
      if (items.length === 0) return;
      const plan = {
        items,
        destination: "trash" as const,
        total_bytes: items.reduce((s, i) => s + i.size_bytes, 0),
        requires_root: items.some((i) => i.requires_root),
      };
      await api.execute(plan);
      await loadData();
    } finally {
      cleaning = false;
    }
  }

  async function openMain() {
    const main = await Window.getByLabel("main");
    if (main) {
      await main.show();
      await main.unminimize();
      await main.setFocus();
    }
    await getCurrentWindow().hide();
  }

  onMount(loadData);
</script>

<div class="bg-base border-line flex h-screen flex-col border p-4">
  <!-- Header -->
  <header class="mb-3 flex items-center gap-2.5">
    <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
      <circle
        cx="12" cy="12" r="8" fill="none"
        stroke="var(--c-line)" stroke-width="3.5"
      />
      <circle
        cx="12" cy="12" r="8" fill="none"
        stroke="var(--c-accent)" stroke-width="3.5"
        stroke-dasharray="36 50" stroke-linecap="round"
        transform="rotate(-90 12 12)"
      />
    </svg>
    <span class="text-ink text-sm font-semibold tracking-tight">
      {$_("app.name")}
    </span>
  </header>

  {#if loading}
    <div class="flex flex-1 items-center justify-center">
      <CircleNotch size={28} class="text-accent animate-spin" />
    </div>
  {:else if mount}
    <!-- Donut chart: used / free / reclaimable -->
    <div class="relative mx-auto w-full max-w-[200px]">
      <Chart option={chartOption()} height={180} />
      <div class="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
        <span class="nums text-2xl font-semibold text-ink">
          {pct}<span class="text-muted text-lg">%</span>
        </span>
        <span class="text-faint mt-0.5 text-[10px] uppercase tracking-wide">
          {$_("tray.used")}
        </span>
      </div>
    </div>

    <!-- Stats row -->
    <div class="mt-2 flex items-center justify-between text-xs">
      <span class="text-muted">
        {humanizeBytes(mount.used)}
        <span class="text-faint"> / {humanizeBytes(mount.total)}</span>
      </span>
      <span class="text-freed nums font-medium">
        {humanizeBytes(freeBytes)} {$_("tray.free_label")}
      </span>
    </div>

    <!-- Reclaimable badge + quick clean button -->
    <div class="mt-3 flex items-center gap-2">
      <div class="bg-freed/10 text-freed flex flex-1 items-center gap-2 rounded-lg px-3 py-2 text-xs font-medium">
        <Sparkle size={14} weight="fill" />
        <span class="nums">{humanizeBytes(reclaimable)}</span>
        <span class="text-freed/70">{$_("tray.reclaimable")}</span>
      </div>
      <button
        class="bg-freed text-base inline-flex cursor-pointer items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition active:scale-95 disabled:opacity-50"
        onclick={quickClean}
        disabled={cleaning || reclaimable === 0}
      >
        {#if cleaning}
          <CircleNotch size={14} class="animate-spin" />
        {:else}
          <Broom size={14} weight="fill" />
        {/if}
        {$_("tray.quick_clean")}
      </button>
    </div>

    <!-- Cards row -->
    <div class="mt-3 grid grid-cols-2 gap-2">
      <!-- App updates card -->
      <button
        class="bg-surface border-line hover:border-accent/40 flex cursor-pointer flex-col items-start gap-1 rounded-xl border p-3 text-left transition"
        onclick={openMain}
      >
        <div class="flex items-center gap-1.5">
          {#if updateCount > 0}
            <ArrowsClockwise size={16} class="text-accent" weight="bold" />
          {:else}
            <Package size={16} class="text-muted" />
          {/if}
          <span class="text-ink text-xs font-semibold">
            {$_("tray.apps_title")}
          </span>
        </div>
        {#if updateCount > 0}
          <span class="text-accent text-[11px] font-medium">
            {$_("tray.updates_available", { values: { count: updateCount } })}
          </span>
        {:else}
          <span class="text-faint text-[11px]">
            {$_("tray.apps_up_to_date")}
          </span>
        {/if}
      </button>

      <!-- Git worktrees card -->
      <button
        class="bg-surface border-line hover:border-accent/40 flex cursor-pointer flex-col items-start gap-1 rounded-xl border p-3 text-left transition"
        onclick={openMain}
      >
        <div class="flex items-center gap-1.5">
          <GitBranch size={16} class="text-muted" />
          <span class="text-ink text-xs font-semibold">
            {$_("service.git_repos")}
          </span>
        </div>
        <span class="text-faint text-[11px]">
          {$_("tray.git_hint")}
        </span>
      </button>
    </div>

    <!-- Open main button -->
    <button
      class="bg-accent text-accent-ink mt-auto flex cursor-pointer items-center justify-center gap-2 rounded-lg py-2.5 text-sm font-medium transition-all active:translate-y-px"
      onclick={openMain}
    >
      {$_("tray.open")}
      <ArrowRight size={16} weight="bold" />
    </button>
  {:else}
    <p class="text-muted flex flex-1 items-center justify-center text-sm">
      {$_("state.error_title")}
    </p>
    <button
      class="bg-accent text-accent-ink mt-3 flex cursor-pointer items-center justify-center gap-2 rounded-lg py-2.5 text-sm font-medium transition-all active:translate-y-px"
      onclick={openMain}
    >
      {$_("tray.open")}
      <ArrowRight size={16} weight="bold" />
    </button>
  {/if}
</div>