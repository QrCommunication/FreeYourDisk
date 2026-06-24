<script lang="ts">
  import { _ } from "svelte-i18n";
  import { CaretDown, CaretUp } from "phosphor-svelte";
  import type { ScanItem } from "../api";
  import { humanizeBytes, humanizeDate } from "../format";

  let {
    items,
    selected,
    ontoggle,
    ontoggleAll,
  }: {
    items: ScanItem[];
    selected: Set<string>;
    ontoggle: (id: string) => void;
    ontoggleAll: (ids: string[]) => void;
  } = $props();

  const MAX_ROWS = 500;
  let sortDesc = $state(true);

  const sorted = $derived(
    [...items].sort((a, b) =>
      sortDesc ? b.size_bytes - a.size_bytes : a.size_bytes - b.size_bytes,
    ),
  );
  const shown = $derived(sorted.slice(0, MAX_ROWS));
  const allShownSelected = $derived(
    shown.length > 0 && shown.every((i) => selected.has(i.id)),
  );

  function splitPath(p: string): { dir: string; base: string } {
    const i = p.lastIndexOf("/");
    return i >= 0
      ? { dir: p.slice(0, i + 1), base: p.slice(i + 1) }
      : { dir: "", base: p };
  }
</script>

<div class="border-line overflow-hidden rounded-xl border">
  <table class="w-full border-collapse text-sm">
    <thead>
      <tr class="bg-surface text-faint border-line border-b text-xs">
        <th class="w-10 py-2.5 pl-3">
          <input
            type="checkbox"
            class="accent-accent cursor-pointer"
            checked={allShownSelected}
            aria-label={$_("table.path")}
            onchange={() => ontoggleAll(shown.map((i) => i.id))}
          />
        </th>
        <th class="py-2.5 text-left font-medium">{$_("table.path")}</th>
        <th class="py-2.5 text-left font-medium">{$_("table.last_access")}</th>
        <th class="py-2.5 pr-4 text-right font-medium">
          <button
            class="hover:text-ink ml-auto inline-flex cursor-pointer items-center gap-1 transition-colors"
            onclick={() => (sortDesc = !sortDesc)}
          >
            {$_("table.size")}
            {#if sortDesc}<CaretDown size={12} />{:else}<CaretUp
                size={12}
              />{/if}
          </button>
        </th>
      </tr>
    </thead>
    <tbody>
      {#each shown as item (item.id)}
        {@const p = splitPath(item.path)}
        {@const checked = selected.has(item.id)}
        <tr
          class="border-line/60 hover:bg-surface/60 cursor-pointer border-b transition-colors last:border-0"
          class:bg-accent-soft={checked}
          onclick={() => ontoggle(item.id)}
        >
          <td class="py-2 pl-3">
            <input
              type="checkbox"
              class="accent-accent pointer-events-none cursor-pointer"
              {checked}
              tabindex="-1"
            />
          </td>
          <td class="max-w-0 py-2">
            <div class="flex items-center gap-2 truncate">
              <span class="truncate font-mono text-xs" title={item.path}>
                <span class="text-faint">{p.dir}</span><span class="text-ink"
                  >{p.base}</span
                >
              </span>
              {#if item.requires_root}
                <span
                  class="bg-danger-soft text-danger shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium tracking-wide uppercase"
                >
                  {$_("table.root_badge")}
                </span>
              {/if}
            </div>
          </td>
          <td class="text-muted py-2 text-xs"
            >{humanizeDate(item.last_access) ?? $_("table.never")}</td
          >
          <td class="nums text-ink py-2 pr-4 text-right text-xs"
            >{humanizeBytes(item.size_bytes)}</td
          >
        </tr>
      {/each}
    </tbody>
  </table>
  {#if items.length > MAX_ROWS}
    <div
      class="text-faint bg-surface border-line border-t px-3 py-2 text-center text-xs"
    >
      {shown.length} / {items.length}
    </div>
  {/if}
</div>
