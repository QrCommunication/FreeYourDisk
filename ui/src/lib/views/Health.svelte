<script lang="ts">
  import { _ } from "svelte-i18n";
  import { onMount, onDestroy } from "svelte";
  import { fade } from "svelte/transition";
  import {
    HardDrives,
    Clock,
    Pulse,
    ShieldCheck,
    ShieldWarning,
    Thermometer,
    CircleNotch,
    Lightning,
  } from "phosphor-svelte";
  import Chart from "../components/Chart.svelte";
  import { api, type DiskInfo, type SmartInfo } from "../api";
  import { humanizeBytes, humanizeUptime, humanizeRate } from "../format";
  import { cssColor } from "../theme";
  import { resolvedTheme } from "../settings";

  const WINDOW = 40; // samples kept per disk (~48s at 1.2s)
  const INTERVAL = 1200;

  let uptime = $state(0);
  let disks = $state<DiskInfo[]>([]);
  let smart = $state<Record<string, SmartInfo>>({});
  let smartLoading = $state(false);

  // Rolling throughput series per device (B/s).
  let series = $state<Record<string, { read: number[]; write: number[] }>>({});
  let last: Record<string, { read: number; write: number; t: number }> = {};
  let timer: ReturnType<typeof setInterval> | undefined;

  function sample(now: DiskInfo[]) {
    const t = performance.now();
    const nextSeries = { ...series };
    for (const disk of now) {
      const prev = last[disk.device];
      const dt = prev ? (t - prev.t) / 1000 : 0;
      const rRate =
        prev && dt > 0 ? Math.max(0, (disk.read_bytes - prev.read) / dt) : 0;
      const wRate =
        prev && dt > 0 ? Math.max(0, (disk.write_bytes - prev.write) / dt) : 0;
      const cur = nextSeries[disk.device] ?? { read: [], write: [] };
      const read = [...cur.read, rRate].slice(-WINDOW);
      const write = [...cur.write, wRate].slice(-WINDOW);
      nextSeries[disk.device] = { read, write };
      last[disk.device] = { read: disk.read_bytes, write: disk.write_bytes, t };
    }
    series = nextSeries;
  }

  async function tick() {
    try {
      const overview = await api.healthOverview();
      uptime = overview.uptime_secs;
      disks = overview.disks;
      sample(overview.disks);
    } catch {
      /* transient */
    }
  }

  async function loadSmart() {
    smartLoading = true;
    try {
      const list = await api.diskSmart();
      const map: Record<string, SmartInfo> = {};
      for (const info of list) map[info.device] = info;
      smart = map;
    } catch {
      /* cancelled / unavailable */
    } finally {
      smartLoading = false;
    }
  }

  function chartOption(device: string) {
    void $resolvedTheme;
    const data = series[device] ?? { read: [], write: [] };
    const x = data.read.map((_, i) => i);
    const readColor = cssColor("--c-accent", "#2dd4bf");
    const writeColor = cssColor("--c-savings", "#fbbf24");
    const axis = cssColor("--c-faint", "#5b6573");
    const area = (hex: string) => ({
      type: "line" as const,
      smooth: true,
      symbol: "none",
      lineStyle: { width: 2, color: hex },
      areaStyle: { color: hex, opacity: 0.14 },
      animationDuration: 300,
    });
    return {
      grid: { left: 8, right: 8, top: 8, bottom: 8 },
      xAxis: {
        type: "category" as const,
        data: x,
        show: false,
        boundaryGap: false,
      },
      yAxis: {
        type: "value" as const,
        show: false,
        min: 0,
        axisLabel: { color: axis },
      },
      tooltip: {
        trigger: "axis" as const,
        formatter: (p: any) =>
          p
            .map((s: any) => `${s.seriesName}: ${humanizeRate(s.value)}`)
            .join("<br/>"),
      },
      series: [
        { name: $_("health.read"), data: data.read, ...area(readColor) },
        { name: $_("health.write"), data: data.write, ...area(writeColor) },
      ],
    };
  }

  function latest(device: string, key: "read" | "write"): number {
    const arr = series[device]?.[key];
    return arr && arr.length ? (arr[arr.length - 1] ?? 0) : 0;
  }

  onMount(() => {
    tick();
    timer = setInterval(tick, INTERVAL);
  });
  onDestroy(() => clearInterval(timer));
</script>

<div class="w-full px-10 py-8">
  <header class="mb-6 flex items-end justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">{$_("nav.health")}</h1>
      <p class="text-muted text-sm">{$_("health.subtitle")}</p>
    </div>
    <div class="text-right">
      <p
        class="text-faint flex items-center justify-end gap-1.5 text-xs uppercase"
      >
        <Clock size={13} />
        {$_("health.system_uptime")}
      </p>
      <p class="nums text-lg font-semibold">{humanizeUptime(uptime)}</p>
    </div>
  </header>

  {#if disks.length === 0}
    <div class="text-muted py-16 text-center text-sm">
      {$_("health.no_disks")}
    </div>
  {:else}
    <div class="mb-5 flex justify-end">
      <button
        class="border-line text-muted hover:text-ink inline-flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs transition active:scale-95"
        onclick={loadSmart}
        disabled={smartLoading}
      >
        {#if smartLoading}
          <CircleNotch size={14} class="animate-spin" />{$_(
            "health.loading_smart",
          )}
        {:else}
          <Lightning size={14} weight="fill" />{$_("health.load_smart")}
        {/if}
      </button>
    </div>

    <div class="flex flex-col gap-5">
      {#each disks as disk (disk.device)}
        {@const s = smart[disk.device]}
        <section
          class="border-line bg-surface overflow-hidden rounded-2xl border"
        >
          <header
            class="border-line flex flex-wrap items-center justify-between gap-3 border-b px-5 py-4"
          >
            <div class="flex items-center gap-3">
              <span
                class="bg-accent-soft text-accent grid h-10 w-10 place-items-center rounded-xl"
              >
                <HardDrives size={22} weight="duotone" />
              </span>
              <div>
                <p class="font-medium">{disk.model ?? disk.device}</p>
                <p class="nums text-faint text-xs">/dev/{disk.device}</p>
              </div>
            </div>
            <div class="flex items-center gap-5 text-sm">
              <div class="text-right">
                <p class="text-faint text-[11px] uppercase">
                  {$_("health.capacity")}
                </p>
                <p class="nums font-medium">{humanizeBytes(disk.size_bytes)}</p>
              </div>
              <div class="text-right">
                <p class="text-faint text-[11px] uppercase">
                  {$_("health.type")}
                </p>
                <p class="font-medium">
                  {disk.rotational ? $_("health.hdd") : $_("health.ssd")}
                </p>
              </div>
            </div>
          </header>

          <!-- Live throughput -->
          <div class="px-3 pt-3">
            <div
              class="text-faint flex items-center justify-between px-2 text-xs"
            >
              <span class="flex items-center gap-1.5"
                ><Pulse size={13} />{$_("health.throughput")}</span
              >
              <span class="nums flex gap-4">
                <span class="text-accent"
                  >↓ {humanizeRate(latest(disk.device, "read"))}</span
                >
                <span class="text-savings"
                  >↑ {humanizeRate(latest(disk.device, "write"))}</span
                >
              </span>
            </div>
            <Chart option={chartOption(disk.device)} height={120} />
          </div>

          <!-- SMART -->
          {#if s}
            <div
              class="border-line grid grid-cols-3 gap-3 border-t px-5 py-4"
              in:fade
            >
              {#if s.available}
                <div>
                  <p class="text-faint text-[11px] uppercase">
                    {$_("health.smart_title")}
                  </p>
                  {#if s.passed === false}
                    <p
                      class="text-danger flex items-center gap-1.5 text-sm font-medium"
                    >
                      <ShieldWarning size={16} weight="fill" />{$_(
                        "health.failing",
                      )}
                    </p>
                  {:else}
                    <p
                      class="text-freed flex items-center gap-1.5 text-sm font-medium"
                    >
                      <ShieldCheck size={16} weight="fill" />{$_(
                        "health.healthy",
                      )}
                    </p>
                  {/if}
                </div>
                <div>
                  <p class="text-faint text-[11px] uppercase">
                    {$_("health.power_on")}
                  </p>
                  <p class="nums text-sm font-medium">
                    {s.power_on_hours != null
                      ? $_("health.hours", {
                          values: { count: s.power_on_hours },
                        })
                      : "—"}
                  </p>
                </div>
                <div>
                  <p
                    class="text-faint flex items-center gap-1 text-[11px] uppercase"
                  >
                    <Thermometer size={12} />{$_("health.temperature")}
                  </p>
                  <p class="nums text-sm font-medium">
                    {s.temperature_c != null ? `${s.temperature_c} °C` : "—"}
                  </p>
                </div>
              {:else}
                <p class="text-faint col-span-3 text-xs">
                  {$_("health.smart_unavailable")}
                </p>
              {/if}
            </div>
          {/if}
        </section>
      {/each}
    </div>
  {/if}
</div>
