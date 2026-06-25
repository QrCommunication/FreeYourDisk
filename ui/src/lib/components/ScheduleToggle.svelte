<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { Clock } from "phosphor-svelte";
  import { api } from "../api";
  import { toasts } from "../stores";

  let enabled = $state(false);
  let busy = $state(false);

  onMount(async () => {
    try {
      enabled = await api.scheduleEnabled();
    } catch {
      // systemd unavailable — leave disabled.
    }
  });

  async function toggle() {
    if (busy) return;
    busy = true;
    try {
      enabled = await api.setSchedule(!enabled);
    } catch {
      toasts.error($_("schedule.failed"));
    } finally {
      busy = false;
    }
  }
</script>

<div
  class="border-line bg-surface flex items-center gap-4 rounded-xl border p-4"
>
  <div
    class="bg-accent-soft text-accent grid h-10 w-10 shrink-0 place-items-center rounded-lg"
  >
    <Clock size={20} />
  </div>
  <div class="min-w-0 flex-1">
    <div class="text-ink text-sm font-medium">{$_("schedule.title")}</div>
    <div class="text-muted text-xs">{$_("schedule.desc")}</div>
  </div>
  <button
    role="switch"
    aria-checked={enabled}
    aria-label={$_("schedule.title")}
    disabled={busy}
    onclick={toggle}
    class="relative h-6 w-11 shrink-0 cursor-pointer rounded-full transition-colors disabled:opacity-50"
    class:bg-accent={enabled}
    class:bg-line={!enabled}
  >
    <span
      class="absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-white transition-transform"
      class:translate-x-5={enabled}
    ></span>
  </button>
</div>
