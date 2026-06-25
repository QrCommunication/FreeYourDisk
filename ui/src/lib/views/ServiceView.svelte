<script lang="ts">
  import { _ } from "svelte-i18n";
  import { fade } from "svelte/transition";
  import { Broom } from "phosphor-svelte";
  import {
    api,
    type ScanItem,
    type ServiceId,
    type Destination,
    type DeletionPlan,
    type ExecutionReport,
  } from "../api";
  import { serviceIcon } from "../icons";
  import { humanizeBytes } from "../format";
  import { dedupSize } from "../reclaim";
  import { toasts } from "../stores";
  import ItemList from "../components/ItemList.svelte";
  import StateBlock from "../components/StateBlock.svelte";
  import ConfirmDrawer from "../components/ConfirmDrawer.svelte";
  import Report from "../components/Report.svelte";

  let { service }: { service: ServiceId } = $props();

  let status = $state<"loading" | "done" | "error">("loading");
  let list = $state<ScanItem[]>([]);
  let newIds = $state<Set<string>>(new Set());
  let selection = $state<Set<string>>(new Set());
  let busy = $state(false);
  let confirming = $state(false);
  let report = $state<ExecutionReport | null>(null);

  const Icon = $derived(serviceIcon[service]);
  const total = $derived(dedupSize(list));
  const selected = $derived(dedupSize(list.filter((i) => selection.has(i.id))));
  const selCount = $derived(selection.size);

  async function scan() {
    status = "loading";
    selection = new Set();
    try {
      const resp = await api.scan(service);
      list = resp.result.items
        .slice()
        .sort((a, b) => b.size_bytes - a.size_bytes);
      newIds = new Set(resp.first_scan ? [] : resp.new_ids);
      status = "done";
    } catch {
      status = "error";
    }
  }

  function toggle(id: string) {
    const next = new Set(selection);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selection = next;
  }
  function setMany(ids: string[], on: boolean) {
    const next = new Set(selection);
    for (const id of ids) {
      if (on) next.add(id);
      else next.delete(id);
    }
    selection = next;
  }

  function buildPlan(): DeletionPlan {
    const chosen = list.filter((i) => selection.has(i.id));
    return {
      items: chosen,
      destination: "trash",
      total_bytes: dedupSize(chosen),
      requires_root: chosen.some((i) => i.requires_root),
    };
  }

  async function doExecute(destination: Destination) {
    busy = true;
    try {
      const result = await api.execute({ ...buildPlan(), destination });
      report = result;
      confirming = false;
      if (result.errors.length > 0) {
        toasts.error(
          $_("toast.clean_partial", {
            values: {
              size: humanizeBytes(result.freed_bytes),
              errors: result.errors.length,
            },
          }),
        );
      } else {
        toasts.success(
          $_("toast.clean_success", {
            values: { size: humanizeBytes(result.freed_bytes) },
          }),
        );
      }
    } catch {
      toasts.error($_("toast.clean_error"));
    } finally {
      busy = false;
    }
  }

  function afterReport() {
    report = null;
    scan();
  }

  // (Re)scan whenever the active service changes.
  $effect(() => {
    void service;
    scan();
  });
</script>

<div class="w-full px-10 py-8">
  <header class="mb-6 flex items-center gap-4">
    <span
      class="bg-accent-soft text-accent grid h-12 w-12 place-items-center rounded-2xl"
    >
      <Icon size={26} weight="duotone" />
    </span>
    <div class="flex-1">
      <h1 class="text-2xl font-semibold tracking-tight">
        {$_(`service.${service}`)}
      </h1>
      <p class="text-muted text-sm">{$_(`service.${service}_desc`)}</p>
    </div>
    {#if status === "done" && list.length > 0}
      <div class="text-right">
        <p class="text-faint text-[11px] uppercase">{$_("home.reclaimable")}</p>
        <p class="nums text-savings text-xl font-semibold">
          {humanizeBytes(total)}
        </p>
      </div>
    {/if}
  </header>

  {#if status === "loading"}
    <StateBlock kind="loading" />
  {:else if status === "error"}
    <StateBlock kind="error" onretry={scan} />
  {:else if list.length === 0}
    <StateBlock kind="empty" />
  {:else}
    <ItemList
      items={list}
      selected={selection}
      {newIds}
      ontoggle={toggle}
      onselectall={setMany}
      max={300}
    />
    {#if selCount > 0}
      <div class="mt-5 flex items-center justify-between" in:fade>
        <span class="text-muted text-sm">
          {$_("table.selected", { values: { count: selCount } })} ·
          <span class="nums text-freed font-medium"
            >{humanizeBytes(selected)}</span
          >
        </span>
        <button
          class="bg-freed inline-flex cursor-pointer items-center gap-2 rounded-xl px-5 py-2.5 text-sm font-semibold text-white transition active:scale-95"
          disabled={busy}
          onclick={() => (confirming = true)}
        >
          <Broom size={18} weight="fill" />
          {$_("home.clean_selected")}
        </button>
      </div>
    {/if}
  {/if}
</div>

{#if confirming}
  <ConfirmDrawer
    plan={buildPlan()}
    {busy}
    onconfirm={doExecute}
    oncancel={() => (confirming = false)}
  />
{/if}

{#if report}
  <Report {report} ondone={afterReport} />
{/if}
