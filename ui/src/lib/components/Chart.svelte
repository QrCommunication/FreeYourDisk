<script lang="ts">
  import * as echarts from "echarts";
  import { onMount } from "svelte";

  let {
    option,
    height = 280,
  }: { option: echarts.EChartsOption; height?: number } = $props();

  let el: HTMLDivElement;
  let chart: echarts.ECharts | undefined;

  onMount(() => {
    chart = echarts.init(el, undefined, { renderer: "canvas" });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => {
      ro.disconnect();
      chart?.dispose();
    };
  });

  $effect(() => {
    chart?.setOption(option, true);
  });
</script>

<div bind:this={el} style="height: {height}px; width: 100%;"></div>
