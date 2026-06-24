<script lang="ts">
  import { _ } from "svelte-i18n";
  import { fly, fade } from "svelte/transition";
  import {
    Trash,
    Warning,
    ShieldWarning,
    X,
    CircleNotch,
  } from "phosphor-svelte";
  import type { DeletionPlan, Destination } from "../api";
  import { humanizeBytes } from "../format";

  let {
    plan,
    busy = false,
    onconfirm,
    oncancel,
  }: {
    plan: DeletionPlan;
    busy?: boolean;
    onconfirm: (destination: Destination) => void;
    oncancel: () => void;
  } = $props();

  let permanent = $state(false);
  let acknowledged = $state(false);

  const needsAck = $derived(permanent || plan.requires_root);
  const canConfirm = $derived(
    plan.items.length > 0 && (!needsAck || acknowledged) && !busy,
  );

  function confirm() {
    if (canConfirm) onconfirm(permanent ? "permanent" : "trash");
  }
</script>

<div
  class="fixed inset-0 z-40 flex justify-end"
  transition:fade={{ duration: 150 }}
>
  <button
    class="absolute inset-0 cursor-default bg-black/50"
    aria-label="Close"
    onclick={oncancel}
  ></button>

  <div
    class="bg-elevated border-line relative flex h-full w-full max-w-md flex-col border-l shadow-2xl"
    transition:fly={{ x: 320, duration: 220 }}
  >
    <div
      class="border-line flex items-center justify-between border-b px-5 py-4"
    >
      <h2 class="text-ink text-base font-semibold">{$_("confirm.title")}</h2>
      <button
        class="text-faint hover:text-ink cursor-pointer transition-colors"
        aria-label="Close"
        onclick={oncancel}
      >
        <X size={18} />
      </button>
    </div>

    <div class="flex-1 overflow-y-auto px-5 py-4">
      <div
        class="border-line bg-surface flex items-baseline justify-between rounded-xl border px-4 py-3"
      >
        <span class="text-muted text-sm"
          >{$_("confirm.summary", {
            values: { count: plan.items.length },
          })}</span
        >
        <span class="nums text-accent text-xl font-semibold"
          >{humanizeBytes(plan.total_bytes)}</span
        >
      </div>

      <div class="mt-5">
        <div class="text-faint mb-2 text-xs tracking-wider uppercase">
          {$_("confirm.destination")}
        </div>
        <label
          class="border-line flex cursor-pointer items-start gap-3 rounded-lg border p-3 transition-colors"
          class:border-accent={!permanent}
          class:bg-accent-soft={!permanent}
        >
          <input
            type="radio"
            class="accent-accent mt-0.5"
            checked={!permanent}
            onchange={() => (permanent = false)}
          />
          <span>
            <span class="text-ink flex items-center gap-1.5 text-sm font-medium"
              ><Trash size={15} />{$_("confirm.to_trash")}</span
            >
            <span class="text-muted text-xs">{$_("confirm.to_trash_hint")}</span
            >
          </span>
        </label>
        <label
          class="border-line mt-2 flex cursor-pointer items-start gap-3 rounded-lg border p-3 transition-colors"
          class:border-danger={permanent}
          class:bg-danger-soft={permanent}
        >
          <input
            type="radio"
            class="accent-danger mt-0.5"
            checked={permanent}
            onchange={() => (permanent = true)}
          />
          <span class="text-ink flex items-center gap-1.5 text-sm font-medium"
            ><Warning size={15} />{$_("confirm.permanent_toggle")}</span
          >
        </label>
      </div>

      {#if plan.requires_root}
        <div
          class="border-danger/40 bg-danger-soft/50 mt-4 flex items-start gap-2.5 rounded-lg border px-3 py-2.5"
        >
          <ShieldWarning
            size={18}
            weight="fill"
            class="text-danger mt-0.5 shrink-0"
          />
          <p class="text-danger/90 text-xs">{$_("confirm.root_warning")}</p>
        </div>
      {/if}

      {#if needsAck}
        <label class="mt-4 flex cursor-pointer items-center gap-2.5">
          <input
            type="checkbox"
            class="accent-danger"
            bind:checked={acknowledged}
          />
          <span class="text-muted text-sm">{$_("confirm.acknowledge")}</span>
        </label>
      {/if}
    </div>

    <div class="border-line flex items-center gap-3 border-t px-5 py-4">
      <button
        class="border-line text-muted hover:text-ink flex-1 cursor-pointer rounded-lg border py-2.5 text-sm transition-colors active:translate-y-px"
        onclick={oncancel}
      >
        {$_("actions.cancel")}
      </button>
      <button
        class="flex flex-1 cursor-pointer items-center justify-center gap-2 rounded-lg py-2.5 text-sm font-medium transition-all active:translate-y-px disabled:cursor-not-allowed disabled:opacity-40"
        class:bg-accent={!permanent && !plan.requires_root}
        class:text-accent-ink={!permanent && !plan.requires_root}
        class:bg-danger={permanent || plan.requires_root}
        class:text-base={permanent || plan.requires_root}
        disabled={!canConfirm}
        onclick={confirm}
      >
        {#if busy}<CircleNotch size={16} class="animate-spin" />{/if}
        {$_("actions.confirm")}
      </button>
    </div>
  </div>
</div>
