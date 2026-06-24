<script lang="ts">
  import type { EChartsOption } from "echarts";
  import Chart from "./Chart.svelte";
  import type { MountUsage } from "../api";
  import { humanizeBytes, usedPercent } from "../format";
  import { C } from "../theme";

  let { mount }: { mount: MountUsage } = $props();

  const pct = $derived(Math.round(usedPercent(mount.used, mount.total)));
  const free = $derived(Math.max(0, mount.total - mount.used));

  const option = $derived<EChartsOption>({
    animationDuration: 600,
    series: [
      {
        type: "pie",
        radius: ["74%", "92%"],
        silent: true,
        avoidLabelOverlap: false,
        label: { show: false },
        labelLine: { show: false },
        data: [
          { value: mount.used, name: "used", itemStyle: { color: C.accent } },
          { value: free, name: "free", itemStyle: { color: C.line } },
        ],
      },
    ],
  });
</script>

<div class="relative">
  <Chart {option} height={240} />
  <div
    class="pointer-events-none absolute inset-0 flex flex-col items-center justify-center"
  >
    <span class="nums text-3xl font-semibold text-ink"
      >{pct}<span class="text-muted text-xl">%</span></span
    >
    <span class="text-faint mt-1 text-xs tracking-wide uppercase"
      >{mount.mount}</span
    >
  </div>
</div>

<div class="mt-3 flex items-center justify-between text-sm">
  <span class="text-muted"
    >{humanizeBytes(mount.used)}
    <span class="text-faint">/ {humanizeBytes(mount.total)}</span></span
  >
  <span class="text-accent nums">{humanizeBytes(free)}</span>
</div>
