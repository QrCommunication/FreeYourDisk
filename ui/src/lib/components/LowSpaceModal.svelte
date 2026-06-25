<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { _ } from "svelte-i18n";
  import { fly } from "svelte/transition";
  import { WarningCircle, Broom, X } from "phosphor-svelte";
  import { api, type LowSpaceAlert } from "../api";
  import { goTo } from "../stores";
  import { runAllScans } from "../scan";

  let alert = $state<LowSpaceAlert | null>(null);
  let unlisten: (() => void) | undefined;

  onMount(async () => {
    try {
      unlisten = await api.onLowSpace((a) => (alert = a));
    } catch {
      /* not under Tauri */
    }
  });
  onDestroy(() => unlisten?.());

  function cleanNow() {
    alert = null;
    goTo("home");
    runAllScans();
  }
</script>

{#if alert}
  <div
    class="fixed right-6 bottom-6 z-50 w-80"
    transition:fly={{ y: 24, duration: 250 }}
  >
    <div class="border-danger/40 bg-elevated rounded-2xl border p-4 shadow-2xl">
      <div class="flex items-start gap-3">
        <span
          class="bg-danger-soft text-danger grid h-10 w-10 shrink-0 place-items-center rounded-xl"
        >
          <WarningCircle size={22} weight="fill" />
        </span>
        <div class="min-w-0 flex-1">
          <p class="text-sm font-semibold">{$_("lowspace.title")}</p>
          <p class="text-muted mt-0.5 text-xs">
            {$_("lowspace.body", {
              values: {
                mount: alert.mount,
                percent: alert.free_percent.toFixed(0),
              },
            })}
          </p>
        </div>
        <button
          class="text-faint hover:text-ink -mt-1 cursor-pointer"
          aria-label={$_("lowspace.dismiss")}
          onclick={() => (alert = null)}
        >
          <X size={16} />
        </button>
      </div>
      <div class="mt-3 flex justify-end gap-2">
        <button
          class="text-muted hover:text-ink cursor-pointer rounded-lg px-3 py-1.5 text-xs"
          onclick={() => (alert = null)}
        >
          {$_("lowspace.dismiss")}
        </button>
        <button
          class="bg-danger inline-flex cursor-pointer items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold text-white transition active:scale-95"
          onclick={cleanNow}
        >
          <Broom size={14} weight="fill" />
          {$_("lowspace.cta")}
        </button>
      </div>
    </div>
  </div>
{/if}
