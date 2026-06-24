<script lang="ts">
  import type { EChartsOption } from "echarts";
  import Chart from "./Chart.svelte";
  import type { ScanItem, ServiceId } from "../api";
  import { C, SERIES } from "../theme";
  import { humanizeBytes } from "../format";

  let { items, service }: { items: ScanItem[]; service: ServiceId } = $props();

  const top = $derived(
    [...items].sort((a, b) => b.size_bytes - a.size_bytes).slice(0, 12),
  );

  function basename(p: string): string {
    const i = p.lastIndexOf("/");
    return i >= 0 ? p.slice(i + 1) || p : p;
  }

  function tip(p: unknown): string {
    const o = p as { name: string; value: number };
    return `${o.name}: ${humanizeBytes(o.value)}`;
  }
  function barLabel(p: unknown): string {
    return humanizeBytes((p as { value: number }).value);
  }

  const treemapOption = $derived<EChartsOption>({
    tooltip: {
      formatter: tip,
      backgroundColor: C.surface,
      borderColor: C.line,
      textStyle: { color: C.ink },
    },
    series: [
      {
        type: "treemap",
        roam: false,
        nodeClick: false,
        breadcrumb: { show: false },
        label: {
          show: true,
          formatter: "{b}",
          color: C.base,
          fontSize: 11,
          overflow: "truncate",
        },
        itemStyle: { borderColor: C.base, borderWidth: 2, gapWidth: 2 },
        levels: [{ color: SERIES }],
        data: top.map((it) => ({
          name: basename(it.path),
          value: it.size_bytes,
        })),
      },
    ],
  });

  const barOption = $derived<EChartsOption>({
    grid: { left: 4, right: 70, top: 6, bottom: 6, containLabel: true },
    tooltip: {
      trigger: "item",
      formatter: tip,
      backgroundColor: C.surface,
      borderColor: C.line,
      textStyle: { color: C.ink },
    },
    xAxis: {
      type: "value",
      axisLabel: { show: false },
      axisLine: { show: false },
      splitLine: { lineStyle: { color: C.line } },
    },
    yAxis: {
      type: "category",
      inverse: true,
      data: top.map((it) => basename(it.path)),
      axisLabel: { color: C.muted, fontSize: 11 },
      axisLine: { show: false },
      axisTick: { show: false },
    },
    series: [
      {
        type: "bar",
        data: top.map((it) => it.size_bytes),
        barWidth: "62%",
        itemStyle: { color: C.accent, borderRadius: [0, 4, 4, 0] },
        label: {
          show: true,
          position: "right",
          color: C.muted,
          fontSize: 10,
          formatter: barLabel,
        },
      },
    ],
  });

  const option = $derived(service === "big_files" ? treemapOption : barOption);
</script>

{#if top.length > 0}
  <Chart {option} height={280} />
{/if}
