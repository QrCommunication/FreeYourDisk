<script lang="ts">
  import { _ } from "svelte-i18n";
  import { CaretRight } from "phosphor-svelte";
  import type { ServiceId } from "../api";
  import { serviceIcon } from "../icons";

  let { id, onopen }: { id: ServiceId; onopen: () => void } = $props();
  const Icon = $derived(serviceIcon[id]);
</script>

<button
  class="group border-line bg-surface hover:border-faint flex w-full cursor-pointer items-center gap-4 rounded-xl border p-4 text-left transition-colors active:translate-y-px"
  onclick={onopen}
>
  <div
    class="bg-accent-soft text-accent grid h-11 w-11 shrink-0 place-items-center rounded-lg"
  >
    <Icon size={22} />
  </div>
  <div class="min-w-0 flex-1">
    <div class="text-ink text-sm font-medium">{$_(`service.${id}`)}</div>
    <div class="text-muted truncate text-xs">{$_(`service.${id}_desc`)}</div>
  </div>
  <CaretRight
    size={16}
    class="text-faint group-hover:text-accent transition-colors"
  />
</button>
