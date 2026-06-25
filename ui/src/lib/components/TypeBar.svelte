<script lang="ts">
  import { _ } from "svelte-i18n";
  import { slide } from "svelte/transition";
  import type { TypeBucket } from "../api";
  import { humanizeBytes } from "../format";

  let { buckets, total }: { buckets: TypeBucket[]; total: number } = $props();

  // Distinct categorical hues (data viz, not brand accent).
  const COLORS: Record<string, string> = {
    images: "#38bdf8",
    videos: "#fb923c",
    audio: "#f472b6",
    archives: "#fbbf24",
    disk_images: "#22d3ee",
    applications: "#a78bfa",
    executables: "#f43f5e",
    documents: "#34d399",
    dev_caches: "#2dd4bf",
    system: "#64748b",
    reserved: "#475569",
    other: "#cbd5e1",
  };

  let open = $state<string | null>(null);

  const shown = $derived(buckets.filter((b) => b.bytes > 0));
  const sumBytes = $derived(shown.reduce((s, b) => s + b.bytes, 0));
  const denom = $derived(Math.max(total, sumBytes, 1));
  const openBucket = $derived(shown.find((b) => b.category === open) ?? null);

  const color = (cat: string) => COLORS[cat] ?? "#94a3b8";
  const pct = (bytes: number) => (bytes / denom) * 100;
</script>

<section class="border-line bg-surface rounded-2xl border p-5">
  <h2 class="mb-3 text-sm font-semibold">{$_("filetype.title")}</h2>

  <!-- Stacked distribution bar -->
  <div class="bg-base flex h-5 w-full overflow-hidden rounded-full">
    {#each shown as b (b.category)}
      <button
        class="h-full cursor-pointer transition-opacity hover:opacity-80"
        style="width:{pct(b.bytes)}%; background:{color(b.category)}"
        title="{$_(`filetype.${b.category}`)} — {humanizeBytes(b.bytes)}"
        aria-label={$_(`filetype.${b.category}`)}
        onclick={() => (open = open === b.category ? null : b.category)}
      ></button>
    {/each}
  </div>

  <!-- Legend (clickable) -->
  <div class="mt-4 grid grid-cols-2 gap-x-6 gap-y-1.5 sm:grid-cols-3">
    {#each shown as b (b.category)}
      <button
        class="flex cursor-pointer items-center gap-2 rounded-md py-1 text-left transition-colors"
        class:text-ink={open === b.category}
        class:text-muted={open !== b.category}
        onclick={() => (open = open === b.category ? null : b.category)}
      >
        <span
          class="h-2.5 w-2.5 shrink-0 rounded-full"
          style="background:{color(b.category)}"
        ></span>
        <span class="flex-1 truncate text-sm"
          >{$_(`filetype.${b.category}`)}</span
        >
        <span class="nums text-xs">{humanizeBytes(b.bytes)}</span>
        <span class="nums text-faint w-9 text-right text-xs"
          >{pct(b.bytes).toFixed(0)}%</span
        >
      </button>
    {/each}
  </div>

  <!-- Top files of the opened category -->
  {#if openBucket && openBucket.top.length > 0}
    <div class="mt-4" transition:slide={{ duration: 200 }}>
      <div
        class="text-faint mb-1 flex items-center justify-between px-1 text-xs"
      >
        <span>{$_(`filetype.${openBucket.category}`)}</span>
        <span class="nums">{openBucket.count} {$_("filetype.files")}</span>
      </div>
      <ul
        class="divide-line border-line bg-base max-h-[28rem] divide-y overflow-y-auto rounded-lg border"
      >
        {#each openBucket.top as f (f.path)}
          <li class="flex items-center gap-3 px-3 py-2">
            <span class="text-ink flex-1 truncate text-sm" title={f.path}
              >{f.path}</span
            >
            <span class="nums text-muted w-20 text-right text-xs"
              >{humanizeBytes(f.size_bytes)}</span
            >
          </li>
        {/each}
      </ul>
      <p class="text-faint mt-1.5 px-1 text-[11px]">{$_("filetype.hint")}</p>
    </div>
  {/if}
</section>
