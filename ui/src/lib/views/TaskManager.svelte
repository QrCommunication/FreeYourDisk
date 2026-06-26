<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import {
    Lightning,
    Prohibit,
    ArrowsClockwise,
    Warning,
    CaretUp,
    CaretDown,
  } from "phosphor-svelte";
  import Chart from "../components/Chart.svelte";
  import { api, type MemStats, type ProcInfo } from "../api";
  import { humanizeBytes } from "../format";
  import { cssColor } from "../theme";
  import { resolvedTheme } from "../settings";
  import { toasts } from "../stores";

  const WINDOW = 60; // seconds of history

  let stats = $state<MemStats | null>(null);
  let ramHist = $state<number[]>([]);
  let swapHist = $state<number[]>([]);
  let cpuHist = $state<number[]>([]);
  let procs = $state<ProcInfo[]>([]);
  let search = $state("");
  let selected = $state<number | null>(null);
  let busy = $state(false);

  type SortKey = "name" | "user" | "cpu" | "mem_pct" | "mem_bytes" | "pid";
  let sortKey = $state<SortKey>("mem_bytes");
  let sortDir = $state<"asc" | "desc">("desc");

  const memPct = $derived(
    stats && stats.mem_total > 0 ? (stats.mem_used / stats.mem_total) * 100 : 0,
  );
  const swapPct = $derived(
    stats && stats.swap_total > 0
      ? (stats.swap_used / stats.swap_total) * 100
      : 0,
  );

  const filtered = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const list = q
      ? procs.filter(
          (p) =>
            p.name.toLowerCase().includes(q) ||
            p.user.toLowerCase().includes(q) ||
            p.cmd.toLowerCase().includes(q),
        )
      : procs;
    const dir = sortDir === "asc" ? 1 : -1;
    return list.slice().sort((a, b) => {
      const k = sortKey;
      if (k === "name" || k === "user") return a[k].localeCompare(b[k]) * dir;
      return (a[k] - b[k]) * dir;
    });
  });

  function setSort(key: SortKey) {
    if (sortKey === key) sortDir = sortDir === "asc" ? "desc" : "asc";
    else {
      sortKey = key;
      sortDir = key === "name" || key === "user" ? "asc" : "desc";
    }
  }

  async function pollStats() {
    try {
      const s = await api.memStats();
      stats = s;
      const mp = s.mem_total > 0 ? (s.mem_used / s.mem_total) * 100 : 0;
      const sp = s.swap_total > 0 ? (s.swap_used / s.swap_total) * 100 : 0;
      ramHist = [...ramHist, mp].slice(-WINDOW);
      swapHist = [...swapHist, sp].slice(-WINDOW);
      cpuHist = [...cpuHist, s.cpu_total].slice(-WINDOW);
    } catch {
      /* transient */
    }
  }

  // Heatmap colour for a core: green (idle) → red (saturated).
  function coreColor(usage: number): string {
    const hue = 120 - (Math.min(100, Math.max(0, usage)) / 100) * 120;
    const light = 28 + (usage / 100) * 14;
    return `hsl(${hue}, 60%, ${light}%)`;
  }

  async function pollProcs() {
    try {
      procs = await api.processList();
    } catch {
      /* transient */
    }
  }

  async function act(kind: "term" | "kill" | "restart") {
    if (selected == null) return;
    const pid = selected;
    busy = true;
    try {
      if (kind === "restart") {
        await api.restartProcess(pid);
        toasts.success($_("taskmgr.restarted", { values: { pid } }));
      } else {
        await api.killProcess(pid, kind === "kill");
        toasts.success($_("taskmgr.killed", { values: { pid } }));
      }
      selected = null;
      await pollProcs();
    } catch {
      toasts.error($_("taskmgr.action_failed"));
    } finally {
      busy = false;
    }
  }

  async function panic() {
    busy = true;
    try {
      const victim = await api.panicKill();
      if (victim) {
        toasts.success(
          $_("taskmgr.panic_done", {
            values: {
              name: victim.name,
              size: humanizeBytes(victim.mem_bytes),
            },
          }),
        );
      } else {
        toasts.error($_("taskmgr.panic_none"));
      }
      await pollProcs();
    } catch {
      toasts.error($_("taskmgr.action_failed"));
    } finally {
      busy = false;
    }
  }

  const chartOption = $derived.by(() => {
    void $resolvedTheme;
    const ramColor = cssColor("--c-accent", "#2dd4bf");
    const swapColor = cssColor("--c-savings", "#fbbf24");
    const cpuColor = "#60a5fa";
    const axis = cssColor("--c-faint", "#5b6573");
    const idx = ramHist.map((_, i) => i);
    return {
      animation: false,
      grid: { left: 40, right: 14, top: 14, bottom: 20 },
      legend: {
        data: ["CPU", "RAM", "Swap"],
        top: 0,
        right: 8,
        textStyle: { color: axis },
        itemWidth: 14,
        itemHeight: 8,
      },
      tooltip: {
        trigger: "axis" as const,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        formatter: (p: any) =>
          p
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            .map((s: any) => `${s.seriesName}: ${s.value.toFixed(0)}%`)
            .join("<br/>"),
      },
      xAxis: {
        type: "category" as const,
        data: idx,
        boundaryGap: false,
        axisLabel: { show: false },
        axisLine: { lineStyle: { color: axis } },
        axisTick: { show: false },
      },
      yAxis: {
        type: "value" as const,
        min: 0,
        max: 100,
        axisLabel: { formatter: "{value}%", color: axis },
        splitLine: { lineStyle: { color: axis, opacity: 0.15 } },
      },
      series: [
        {
          name: "CPU",
          type: "line" as const,
          smooth: true,
          showSymbol: false,
          data: cpuHist,
          lineStyle: { color: cpuColor, width: 2 },
          areaStyle: { color: cpuColor, opacity: 0.14 },
        },
        {
          name: "RAM",
          type: "line" as const,
          smooth: true,
          showSymbol: false,
          data: ramHist,
          lineStyle: { color: ramColor, width: 2 },
          areaStyle: { color: ramColor, opacity: 0.18 },
        },
        {
          name: "Swap",
          type: "line" as const,
          smooth: true,
          showSymbol: false,
          data: swapHist,
          lineStyle: { color: swapColor, width: 2 },
          areaStyle: { color: swapColor, opacity: 0.14 },
        },
      ],
    };
  });

  onMount(() => {
    pollStats();
    pollProcs();
    const t1 = setInterval(pollStats, 1000);
    const t2 = setInterval(pollProcs, 2500);
    return () => {
      clearInterval(t1);
      clearInterval(t2);
    };
  });
</script>

<div class="flex h-full w-full flex-col px-10 py-8">
  <header class="mb-5 flex items-end justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">{$_("nav.taskmgr")}</h1>
      <p class="text-muted text-sm">{$_("taskmgr.subtitle")}</p>
    </div>
    <button
      class="bg-danger inline-flex items-center gap-2 rounded-xl px-4 py-2.5 text-sm font-semibold text-white transition active:scale-95 disabled:opacity-50"
      disabled={busy}
      onclick={panic}
      title={$_("taskmgr.panic_hint")}
    >
      <Warning size={18} weight="fill" />
      {$_("taskmgr.panic")}
    </button>
  </header>

  <!-- Live graph + gauges -->
  <section class="border-line bg-surface mb-5 rounded-2xl border p-5">
    <div class="mb-3 grid grid-cols-2 gap-4 sm:grid-cols-5">
      <div>
        <div class="text-faint text-xs uppercase">{$_("taskmgr.cpu")}</div>
        <div class="nums text-lg font-semibold">
          {stats ? stats.cpu_total.toFixed(0) : "—"}<span
            class="text-faint text-sm font-normal">%</span
          >
        </div>
      </div>
      <div>
        <div class="text-faint text-xs uppercase">{$_("taskmgr.temp")}</div>
        <div class="nums text-lg font-semibold">
          {stats && stats.cpu_temp != null
            ? `${stats.cpu_temp.toFixed(0)}°C`
            : "—"}
        </div>
      </div>
      <div>
        <div class="text-faint text-xs uppercase">{$_("taskmgr.ram")}</div>
        <div class="nums text-lg font-semibold">
          {stats ? humanizeBytes(stats.mem_used) : "—"}
          <span class="text-faint text-sm font-normal">
            / {stats ? humanizeBytes(stats.mem_total) : "—"} · {memPct.toFixed(
              0,
            )}%
          </span>
        </div>
      </div>
      <div>
        <div class="text-faint text-xs uppercase">{$_("taskmgr.swap")}</div>
        <div class="nums text-lg font-semibold">
          {stats ? humanizeBytes(stats.swap_used) : "—"}
          <span class="text-faint text-sm font-normal">
            / {stats ? humanizeBytes(stats.swap_total) : "—"} · {swapPct.toFixed(
              0,
            )}%
          </span>
        </div>
      </div>
      <div>
        <div class="text-faint text-xs uppercase">{$_("taskmgr.load")}</div>
        <div class="nums text-lg font-semibold">
          {stats ? stats.load1.toFixed(2) : "—"}
          <span class="text-faint text-sm font-normal">
            {stats
              ? `${stats.load5.toFixed(2)} ${stats.load15.toFixed(2)}`
              : ""}
          </span>
        </div>
      </div>
    </div>
    <Chart option={chartOption} height={180} />

    {#if stats && stats.cpus.length > 0}
      <div class="mt-4">
        <div class="text-faint mb-2 text-xs uppercase">
          {$_("taskmgr.cores")} · {stats.cpus.length}
        </div>
        <div
          class="grid gap-1"
          style="grid-template-columns: repeat(auto-fill, minmax(40px, 1fr))"
        >
          {#each stats.cpus as usage, i (i)}
            <div
              class="rounded py-1.5 text-center text-[10px] font-semibold text-white tabular-nums"
              style="background: {coreColor(usage)}"
              title="Core {i}: {usage.toFixed(0)}%"
            >
              {usage.toFixed(0)}
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </section>

  <!-- Action bar -->
  <div class="mb-3 flex items-center gap-2">
    <input
      class="border-line bg-base focus:border-accent w-64 rounded-lg border px-3 py-1.5 text-sm outline-none"
      placeholder={$_("taskmgr.filter")}
      bind:value={search}
    />
    <div class="flex-1"></div>
    <button
      class="border-line text-muted hover:text-ink inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm transition active:scale-95 disabled:opacity-40"
      disabled={selected == null || busy}
      onclick={() => act("restart")}
    >
      <ArrowsClockwise size={15} />{$_("taskmgr.restart")}
    </button>
    <button
      class="border-line text-muted hover:text-ink inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm transition active:scale-95 disabled:opacity-40"
      disabled={selected == null || busy}
      onclick={() => act("term")}
    >
      <Lightning size={15} />{$_("taskmgr.terminate")}
    </button>
    <button
      class="bg-danger inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-medium text-white transition active:scale-95 disabled:opacity-40"
      disabled={selected == null || busy}
      onclick={() => act("kill")}
    >
      <Prohibit size={15} weight="bold" />{$_("taskmgr.force")}
    </button>
  </div>

  <!-- Process table -->
  <div class="border-line bg-surface flex-1 overflow-hidden rounded-xl border">
    <div class="h-full overflow-y-auto">
      <table class="w-full text-sm">
        <thead
          class="bg-elevated text-faint sticky top-0 z-10 text-left text-xs"
        >
          <tr>
            {#each [["name", $_("taskmgr.col_name")], ["user", $_("taskmgr.col_user")], ["cpu", "CPU %"], ["mem_pct", "RAM %"], ["mem_bytes", $_("taskmgr.col_ram")], ["pid", "PID"]] as [key, label] (key)}
              <th
                class="hover:text-ink cursor-pointer px-3 py-2 font-medium select-none"
                class:text-right={key !== "name" && key !== "user"}
                onclick={() => setSort(key as SortKey)}
              >
                <span
                  class="inline-flex items-center gap-1"
                  class:text-accent={sortKey === key}
                >
                  {label}
                  {#if sortKey === key}
                    {#if sortDir === "asc"}<CaretUp
                        size={10}
                      />{:else}<CaretDown size={10} />{/if}
                  {/if}
                </span>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody class="divide-line divide-y">
          {#each filtered as p (p.pid)}
            <tr
              class="hover:bg-elevated cursor-pointer"
              class:bg-accent-soft={selected === p.pid}
              onclick={() => (selected = selected === p.pid ? null : p.pid)}
            >
              <td class="text-ink max-w-0 truncate px-3 py-1.5" title={p.cmd}>
                {p.name}
              </td>
              <td class="text-muted px-3 py-1.5">{p.user}</td>
              <td
                class="nums px-3 py-1.5 text-right"
                class:text-savings={p.cpu > 50}
              >
                {p.cpu.toFixed(0)}
              </td>
              <td
                class="nums px-3 py-1.5 text-right"
                class:text-danger={p.mem_pct > 20}
              >
                {p.mem_pct.toFixed(1)}
              </td>
              <td class="nums text-muted px-3 py-1.5 text-right">
                {humanizeBytes(p.mem_bytes)}
              </td>
              <td class="nums text-faint px-3 py-1.5 text-right">{p.pid}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
</div>
