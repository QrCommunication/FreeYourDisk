<script lang="ts">
  import { _ } from "svelte-i18n";
  import {
    CircleNotch,
    Sparkle,
    WarningOctagon,
    ArrowClockwise,
  } from "phosphor-svelte";

  let {
    kind,
    title = "",
    desc = "",
    onretry,
  }: {
    kind: "loading" | "empty" | "error";
    title?: string;
    desc?: string;
    onretry?: () => void;
  } = $props();
</script>

<div
  class="flex flex-col items-center justify-center gap-3 px-6 py-20 text-center"
>
  {#if kind === "loading"}
    <CircleNotch size={30} class="text-accent animate-spin" />
    <p class="text-ink text-sm font-medium">{$_("state.loading")}</p>
    <p class="text-faint max-w-xs text-xs">{$_("state.loading_hint")}</p>
  {:else if kind === "empty"}
    <div
      class="bg-accent-soft text-accent grid h-12 w-12 place-items-center rounded-full"
    >
      <Sparkle size={22} weight="fill" />
    </div>
    <p class="text-ink text-sm font-medium">
      {title || $_("state.empty_title")}
    </p>
    <p class="text-muted max-w-xs text-sm">{desc || $_("state.empty_desc")}</p>
  {:else}
    <div
      class="bg-danger-soft text-danger grid h-12 w-12 place-items-center rounded-full"
    >
      <WarningOctagon size={22} weight="fill" />
    </div>
    <p class="text-ink text-sm font-medium">
      {title || $_("state.error_title")}
    </p>
    {#if desc}<p class="text-muted max-w-md text-sm">{desc}</p>{/if}
    {#if onretry}
      <button
        class="border-line text-ink hover:border-faint mt-1 flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-1.5 text-sm transition-colors active:translate-y-px"
        onclick={onretry}
      >
        <ArrowClockwise size={15} />
        {$_("state.retry")}
      </button>
    {/if}
  {/if}
</div>
