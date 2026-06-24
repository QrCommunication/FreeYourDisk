<script lang="ts">
  import { _ } from "svelte-i18n";
  import { fly, fade, scale } from "svelte/transition";
  import { CheckCircle, WarningCircle } from "phosphor-svelte";
  import type { ExecutionReport } from "../api";
  import { humanizeBytes } from "../format";

  let { report, ondone }: { report: ExecutionReport; ondone: () => void } =
    $props();
  const hasErrors = $derived(report.errors.length > 0);
</script>

<div
  class="fixed inset-0 z-40 grid place-items-center p-6"
  transition:fade={{ duration: 150 }}
>
  <button
    class="absolute inset-0 cursor-default bg-black/55"
    aria-label="Close"
    onclick={ondone}
  ></button>

  <div
    class="bg-elevated border-line relative w-full max-w-sm rounded-2xl border p-6 text-center shadow-2xl"
    transition:scale={{ start: 0.96, duration: 200 }}
  >
    <div
      class="mx-auto grid h-14 w-14 place-items-center rounded-full"
      class:bg-accent-soft={!hasErrors}
      class:bg-danger-soft={hasErrors}
    >
      {#if hasErrors}
        <WarningCircle size={28} weight="fill" class="text-danger" />
      {:else}
        <CheckCircle size={28} weight="fill" class="text-accent" />
      {/if}
    </div>

    <h2 class="text-ink mt-4 text-base font-semibold">{$_("report.title")}</h2>

    <div class="my-5">
      <div
        class="nums text-accent text-4xl font-semibold"
        in:fly={{ y: 8, duration: 300, delay: 80 }}
      >
        {humanizeBytes(report.freed_bytes)}
      </div>
      <div class="text-faint mt-1 text-xs tracking-wide uppercase">
        {$_("report.freed")}
      </div>
    </div>

    <p class="text-muted text-sm">
      {$_("report.removed", { values: { count: report.deleted_count } })}
    </p>
    {#if hasErrors}
      <p class="text-danger mt-1 text-sm">
        {$_("report.errors", { values: { count: report.errors.length } })}
      </p>
    {/if}

    <button
      class="bg-accent text-accent-ink mt-6 w-full cursor-pointer rounded-lg py-2.5 text-sm font-medium transition-all active:translate-y-px"
      onclick={ondone}
    >
      {$_("report.done")}
    </button>
  </div>
</div>
