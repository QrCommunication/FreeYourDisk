<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { ScanItem } from "../api";
  import { humanizeBytes } from "../format";

  let {
    items,
    selected,
    newIds,
    ontoggle,
    onselectall,
    max = 200,
  }: {
    items: ScanItem[];
    selected: Set<string>;
    newIds?: Set<string>;
    ontoggle: (id: string) => void;
    onselectall: (ids: string[], on: boolean) => void;
    max?: number;
  } = $props();

  const allSelected = $derived(
    items.length > 0 && items.every((i) => selected.has(i.id)),
  );
</script>

<div class="flex items-center justify-end px-1 pb-2">
  <button
    class="text-accent hover:bg-accent-soft cursor-pointer rounded-md px-2 py-1 text-xs font-medium transition"
    onclick={() =>
      onselectall(
        items.map((i) => i.id),
        !allSelected,
      )}
  >
    {allSelected ? $_("home.deselect_all") : $_("home.select_all")}
  </button>
</div>
<ul
  class="divide-line border-line bg-base max-h-[60vh] divide-y overflow-y-auto rounded-lg border"
>
  {#each items.slice(0, max) as item (item.id)}
    <li>
      <label
        class="hover:bg-elevated flex cursor-pointer items-center gap-3 px-3 py-2"
      >
        <input
          type="checkbox"
          class="accent-freed h-4 w-4"
          checked={selected.has(item.id)}
          onchange={() => ontoggle(item.id)}
        />
        <span class="text-ink flex-1 truncate text-sm" title={item.path}>
          {item.path}
        </span>
        {#if newIds?.has(item.id)}
          <span
            class="bg-savings-soft text-savings rounded px-1.5 py-0.5 text-[10px] font-medium uppercase"
            >{$_("home.new_badge")}</span
          >
        {/if}
        {#if item.requires_root}
          <span
            class="bg-danger-soft text-danger rounded px-1.5 py-0.5 text-[10px] font-medium"
            >{$_("table.root_badge")}</span
          >
        {/if}
        <span class="nums text-muted w-20 text-right text-xs">
          {humanizeBytes(item.size_bytes)}
        </span>
      </label>
    </li>
  {/each}
  {#if items.length > max}
    <li class="text-faint px-3 py-2 text-center text-xs">
      + {items.length - max}
    </li>
  {/if}
</ul>
