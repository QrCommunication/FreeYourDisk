<script lang="ts">
  import { fly } from "svelte/transition";
  import { CheckCircle, WarningCircle, X } from "phosphor-svelte";
  import { toasts } from "../stores";
</script>

<div
  class="pointer-events-none fixed right-4 bottom-4 z-50 flex flex-col gap-2"
>
  {#each $toasts as toast (toast.id)}
    <div
      in:fly={{ y: 12, duration: 200 }}
      out:fly={{ y: 12, duration: 150 }}
      class="border-line bg-elevated pointer-events-auto flex items-center gap-2.5 rounded-xl border px-3.5 py-2.5 shadow-lg"
    >
      {#if toast.type === "success"}
        <CheckCircle size={18} weight="fill" class="text-accent shrink-0" />
      {:else}
        <WarningCircle size={18} weight="fill" class="text-danger shrink-0" />
      {/if}
      <span class="text-ink text-sm">{toast.message}</span>
      <button
        class="text-faint hover:text-ink ml-1 cursor-pointer transition-colors"
        aria-label="Dismiss"
        onclick={() => toasts.dismiss(toast.id)}
      >
        <X size={14} />
      </button>
    </div>
  {/each}
</div>
