<script lang="ts">
  import { _ } from "svelte-i18n";
  import { fade, slide } from "svelte/transition";
  import {
    CircleNotch,
    Sparkle,
    ArrowClockwise,
    Broom,
    CaretRight,
  } from "phosphor-svelte";
  import Donut3D from "../components/Donut3D.svelte";
  import ItemList from "../components/ItemList.svelte";
  import TypeBar from "../components/TypeBar.svelte";
  import ConfirmDrawer from "../components/ConfirmDrawer.svelte";
  import Report from "../components/Report.svelte";
  import { serviceIcon } from "../icons";
  import { humanizeBytes } from "../format";
  import { cssColor } from "../theme";
  import { resolvedTheme } from "../settings";
  import { toasts } from "../stores";
  import {
    api,
    type Destination,
    type DeletionPlan,
    type ExecutionReport,
    type ServiceId,
  } from "../api";
  import {
    scanStatus,
    scanProgress,
    refreshing,
    disjoint,
    fileTypes,
    homeTotal,
    systemTotal,
    selection,
    selectedCount,
    reclaimableBytes,
    selectedBytes,
    primaryUsage,
    runAllScans,
    toggle,
    setMany,
    type UnifiedItem,
  } from "../scan";

  let confirming = $state(false);
  let busy = $state(false);
  let report = $state<ExecutionReport | null>(null);
  // Single-open accordion: opening one category collapses any other.
  let openGroup = $state<ServiceId | null>(null);

  const usage = $derived($primaryUsage);
  const total = $derived(usage?.total ?? 0);
  const used = $derived(usage?.used ?? 0);
  const free = $derived(Math.max(0, total - used));
  // Clamp to `used` — reclaimable can never exceed what's actually on disk.
  const reclaimable = $derived(Math.min($reclaimableBytes, used));
  const selected = $derived(Math.min($selectedBytes, reclaimable));

  const segments = $derived.by(() => {
    void $resolvedTheme;
    const usedSolid = Math.max(0, used - reclaimable);
    const savings = Math.max(0, reclaimable - selected);
    return [
      { value: usedSolid, color: cssColor("--c-accent", "#2dd4bf") },
      { value: savings, color: cssColor("--c-savings", "#fbbf24") },
      { value: selected, color: cssColor("--c-freed", "#34d399") },
      { value: free, color: cssColor("--c-line", "#1f2630") },
    ];
  });

  const newIds = $derived(
    new Set($disjoint.filter((i) => i.isNew).map((i) => i.id)),
  );

  const grouped = $derived.by(() => {
    const map = new Map<ServiceId, UnifiedItem[]>();
    for (const item of $disjoint) {
      const list = map.get(item.service);
      if (list) list.push(item);
      else map.set(item.service, [item]);
    }
    return [...map.entries()]
      .map(([service, list]) => ({
        service,
        list: list.slice().sort((a, b) => b.size_bytes - a.size_bytes),
        // Items are already mutually-exclusive, so a plain sum is exact.
        total: list.reduce((s, i) => s + i.size_bytes, 0),
        selectedCount: list.filter((i) => $selection.has(i.id)).length,
      }))
      .sort((a, b) => b.total - a.total);
  });

  function toggleGroup(service: ServiceId) {
    openGroup = openGroup === service ? null : service;
  }

  // The type bar represents the whole disk without double-counting:
  //   file types  = classified loose files in home (the type walk)
  //   caches&deps  = everything in home the walk skipped (node_modules, .cache,
  //                  .git, vendor, Applications…) = homeTotal − fileTypesSum
  //   system       = MEASURED OS footprint (/usr, /var, /opt, swap…)
  //   reserved     = ext4 reserved blocks + root-only dirs we can't stat
  //                  = used − home − system (no longer inflates "system")
  const barBuckets = $derived.by(() => {
    const ft = $fileTypes.map((b) => ({ ...b }));
    const ftSum = ft.reduce((s, b) => s + b.bytes, 0);
    const home = $homeTotal;
    const system = $systemTotal;
    if (home <= 0) {
      const fallback = Math.max(0, used - ftSum);
      return [
        ...ft,
        { category: "system", bytes: fallback, count: 0, top: [] },
      ].filter((b) => b.bytes > 0);
    }
    const skipped = Math.max(0, home - ftSum); // caches & dependencies in home
    const reserved = Math.max(0, used - home - system); // fs reserve + protected
    return [
      ...ft,
      { category: "dev_caches", bytes: skipped, count: 0, top: [] },
      { category: "system", bytes: system, count: 0, top: [] },
      { category: "reserved", bytes: reserved, count: 0, top: [] },
    ].filter((b) => b.bytes > 0);
  });

  function freePercent(): number {
    return total > 0 ? (free / total) * 100 : 0;
  }

  function buildPlan(): DeletionPlan {
    const chosen = $disjoint.filter((i) => $selection.has(i.id));
    return {
      items: chosen,
      destination: "trash",
      total_bytes: chosen.reduce((s, i) => s + i.size_bytes, 0),
      requires_root: chosen.some((i) => i.requires_root),
    };
  }

  async function doExecute(destination: Destination) {
    busy = true;
    try {
      const result = await api.execute({ ...buildPlan(), destination });
      report = result;
      confirming = false;
      if (result.errors.length > 0) {
        toasts.error(
          $_("toast.clean_partial", {
            values: {
              size: humanizeBytes(result.freed_bytes),
              errors: result.errors.length,
            },
          }),
        );
      } else {
        toasts.success(
          $_("toast.clean_success", {
            values: { size: humanizeBytes(result.freed_bytes) },
          }),
        );
      }
    } catch {
      toasts.error($_("toast.clean_error"));
    } finally {
      busy = false;
    }
  }

  function afterReport() {
    report = null;
    runAllScans();
  }
</script>

<div class="w-full px-10 py-8">
  <section class="grid items-center gap-10 lg:grid-cols-[440px_1fr]">
    <div class="grid justify-center">
      <Donut3D {segments} size={400}>
        {#if $scanStatus === "idle"}
          <button
            class="bg-accent text-accent-ink hover:bg-accent/90 grid h-28 w-28 cursor-pointer place-items-center rounded-full text-sm font-semibold shadow-lg transition active:scale-95"
            onclick={() => runAllScans()}
          >
            <span class="flex flex-col items-center gap-1">
              <Sparkle size={22} weight="fill" />
              {$_("home.scan_cta")}
            </span>
          </button>
        {:else if $scanStatus === "scanning"}
          <div class="flex flex-col items-center gap-2">
            <CircleNotch size={30} class="text-accent animate-spin" />
            <span class="nums text-2xl font-semibold"
              >{$scanProgress.done}/{$scanProgress.total}</span
            >
            <span class="text-muted text-xs">{$_("home.scanning")}</span>
          </div>
        {:else}
          <div class="flex flex-col items-center">
            <span class="nums text-3xl font-bold"
              >{freePercent().toFixed(0)}%</span
            >
            <span class="text-muted text-xs">{$_("home.free")}</span>
          </div>
        {/if}
      </Donut3D>
    </div>

    <div class="flex flex-col gap-1">
      <h1 class="text-2xl font-semibold tracking-tight">{$_("app.name")}</h1>
      <p class="text-muted mb-2 text-sm">{$_("home.subtitle")}</p>
      {#if $refreshing}
        <p class="text-faint mb-2 flex items-center gap-1.5 text-xs" in:fade>
          <CircleNotch size={12} class="animate-spin" />
          {$_("home.refreshing")}
        </p>
      {/if}

      <div class="divide-line border-line divide-y rounded-xl border">
        <div class="flex items-center justify-between px-4 py-3">
          <span class="text-muted flex items-center gap-2 text-sm">
            <span class="bg-accent h-2.5 w-2.5 rounded-full"></span>{$_(
              "home.used",
            )}</span
          >
          <span class="nums text-sm"
            >{humanizeBytes(used)} / {humanizeBytes(total)}</span
          >
        </div>
        <div class="flex items-center justify-between px-4 py-3">
          <span class="text-muted flex items-center gap-2 text-sm">
            <span class="bg-savings h-2.5 w-2.5 rounded-full"></span>{$_(
              "home.reclaimable",
            )}</span
          >
          <span class="nums text-savings text-sm font-medium"
            >{humanizeBytes(reclaimable)}</span
          >
        </div>
        <div class="flex items-center justify-between px-4 py-3">
          <span class="text-muted flex items-center gap-2 text-sm">
            <span class="bg-freed h-2.5 w-2.5 rounded-full"></span>{$_(
              "home.selected",
            )}</span
          >
          <span class="nums text-freed text-sm font-medium"
            >{humanizeBytes(selected)}</span
          >
        </div>
      </div>

      {#if $scanStatus === "done"}
        <div class="mt-4 flex gap-3" in:fade>
          <button
            class="bg-freed disabled:bg-line disabled:text-faint inline-flex items-center gap-2 rounded-xl px-5 py-2.5 text-sm font-semibold text-white transition active:scale-95 disabled:cursor-not-allowed"
            disabled={$selectedCount === 0 || busy}
            onclick={() => (confirming = true)}
          >
            <Broom size={18} weight="fill" />
            {$_("home.clean_selected")}
            {#if $selectedCount > 0}
              <span class="nums opacity-80">({humanizeBytes(selected)})</span>
            {/if}
          </button>
          <button
            class="border-line text-muted hover:text-ink inline-flex items-center gap-2 rounded-xl border px-4 py-2.5 text-sm transition active:scale-95"
            onclick={() => runAllScans()}
          >
            <ArrowClockwise size={16} />
            {$_("home.rescan")}
          </button>
        </div>
      {/if}
    </div>
  </section>

  {#if $scanStatus === "scanning"}
    <p class="text-faint mt-6 text-center text-xs">
      {$_("home.scanning_hint")}
    </p>
  {/if}

  {#if $scanStatus === "done" && barBuckets.length > 0}
    <div class="mt-8">
      <TypeBar buckets={barBuckets} {total} />
    </div>
  {/if}

  {#if $scanStatus === "done"}
    {#if $disjoint.length === 0}
      <div class="mt-10 text-center" in:fade>
        <h2 class="text-lg font-medium">{$_("home.nothing_title")}</h2>
        <p class="text-muted text-sm">{$_("home.nothing_desc")}</p>
      </div>
    {:else}
      <div class="mt-8 flex flex-col gap-3">
        <p class="text-faint text-xs">{$_("home.select_hint")}</p>
        {#each grouped as group (group.service)}
          {@const Icon = serviceIcon[group.service]}
          {@const open = openGroup === group.service}
          <section
            class="border-line bg-surface overflow-hidden rounded-xl border"
          >
            <button
              class="hover:bg-elevated flex w-full cursor-pointer items-center gap-3 px-4 py-3 text-left transition"
              onclick={() => toggleGroup(group.service)}
            >
              <CaretRight
                size={16}
                class="text-faint shrink-0 transition-transform {open
                  ? 'rotate-90'
                  : ''}"
              />
              <span
                class="bg-accent-soft text-accent grid h-9 w-9 shrink-0 place-items-center rounded-lg"
              >
                <Icon size={19} weight="duotone" />
              </span>
              <div class="min-w-0 flex-1">
                <p class="text-sm font-medium">
                  {$_(`service.${group.service}`)}
                </p>
                <p class="nums text-faint text-xs">
                  {group.list.length}{group.selectedCount > 0
                    ? ` · ${group.selectedCount} ${$_("home.selected").toLowerCase()}`
                    : ""}
                </p>
              </div>
              <span class="nums text-savings shrink-0 text-sm font-semibold">
                {humanizeBytes(group.total)}
              </span>
            </button>
            {#if open}
              <div class="px-4 pt-1 pb-4" transition:slide={{ duration: 200 }}>
                <ItemList
                  items={group.list}
                  selected={$selection}
                  {newIds}
                  ontoggle={toggle}
                  onselectall={setMany}
                />
              </div>
            {/if}
          </section>
        {/each}
      </div>
    {/if}
  {/if}
</div>

{#if confirming}
  <ConfirmDrawer
    plan={buildPlan()}
    {busy}
    onconfirm={doExecute}
    oncancel={() => (confirming = false)}
  />
{/if}

{#if report}
  <Report {report} ondone={afterReport} />
{/if}
