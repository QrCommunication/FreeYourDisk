<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { ArrowLeft, Broom } from "phosphor-svelte";
  import {
    api,
    type Destination,
    type DeletionPlan,
    type ExecutionReport,
    type ScanResult,
    type ServiceId,
  } from "../api";
  import { goDashboard, toasts } from "../stores";
  import { humanizeBytes } from "../format";
  import { serviceIcon } from "../icons";
  import ItemsTable from "../components/ItemsTable.svelte";
  import ServiceChart from "../components/ServiceChart.svelte";
  import ConfirmDrawer from "../components/ConfirmDrawer.svelte";
  import Report from "../components/Report.svelte";
  import StateBlock from "../components/StateBlock.svelte";

  let { service }: { service: ServiceId } = $props();
  const Icon = $derived(serviceIcon[service]);

  let status = $state<"loading" | "error" | "data">("loading");
  let result = $state<ScanResult | null>(null);
  let selected = $state<Set<string>>(new Set());
  let plan = $state<DeletionPlan | null>(null);
  let report = $state<ExecutionReport | null>(null);
  let busy = $state(false);

  const selectedBytes = $derived(
    result
      ? result.items
          .filter((i) => selected.has(i.id))
          .reduce((s, i) => s + i.size_bytes, 0)
      : 0,
  );

  async function doScan() {
    status = "loading";
    result = null;
    selected = new Set();
    try {
      result = await api.scan(service);
      status = "data";
    } catch {
      status = "error";
      toasts.error($_("toast.scan_error"));
    }
  }

  function toggle(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  function toggleAll(ids: string[]) {
    const allSelected = ids.every((id) => selected.has(id));
    const next = new Set(selected);
    for (const id of ids) {
      if (allSelected) next.delete(id);
      else next.add(id);
    }
    selected = next;
  }

  async function openConfirm() {
    if (selected.size === 0) return;
    try {
      plan = await api.preview(service, [...selected]);
    } catch {
      toasts.error($_("toast.scan_error"));
    }
  }

  async function execute(destination: Destination) {
    if (!plan) return;
    busy = true;
    try {
      const r = await api.execute({ ...plan, destination });
      report = r;
      plan = null;
      const freed = humanizeBytes(r.freed_bytes);
      if (r.errors.length > 0) {
        toasts.error(
          $_("toast.clean_partial", {
            values: { size: freed, errors: r.errors.length },
          }),
        );
      } else {
        toasts.success($_("toast.clean_success", { values: { size: freed } }));
      }
      await doScan();
    } catch {
      toasts.error($_("toast.clean_error"));
    } finally {
      busy = false;
    }
  }

  onMount(doScan);
</script>

<div class="mx-auto max-w-5xl px-8 py-8">
  <header class="mb-6 flex items-center gap-4">
    <button
      class="border-line text-muted hover:text-ink grid h-9 w-9 shrink-0 cursor-pointer place-items-center rounded-lg border transition-colors active:translate-y-px"
      aria-label={$_("actions.back")}
      onclick={goDashboard}
    >
      <ArrowLeft size={16} />
    </button>
    <div
      class="bg-accent-soft text-accent grid h-10 w-10 shrink-0 place-items-center rounded-lg"
    >
      <Icon size={20} />
    </div>
    <div class="min-w-0 flex-1">
      <h1 class="text-ink text-lg font-semibold tracking-tight">
        {$_(`service.${service}`)}
      </h1>
      <p class="text-muted truncate text-sm">{$_(`service.${service}_desc`)}</p>
    </div>
    {#if result}
      <span class="nums text-accent shrink-0 text-lg font-semibold"
        >{humanizeBytes(result.total_bytes)}</span
      >
    {/if}
  </header>

  {#if status === "loading"}
    <StateBlock kind="loading" />
  {:else if status === "error"}
    <StateBlock kind="error" onretry={doScan} />
  {:else if result && result.items.length === 0}
    <StateBlock kind="empty" />
  {:else if result}
    <div class="border-line bg-surface mb-5 rounded-xl border p-4">
      <ServiceChart items={result.items} {service} />
    </div>

    <ItemsTable
      items={result.items}
      {selected}
      ontoggle={toggle}
      ontoggleAll={toggleAll}
    />

    <div
      class="bg-base/80 border-line sticky bottom-0 mt-4 flex items-center justify-between border-t py-3 backdrop-blur"
    >
      <span class="text-muted text-sm">
        {$_("table.selected", { values: { count: selected.size } })}
        {#if selected.size > 0}<span class="nums text-accent ml-1"
            >· {humanizeBytes(selectedBytes)}</span
          >{/if}
      </span>
      <button
        class="bg-accent text-accent-ink flex cursor-pointer items-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition-all active:translate-y-px disabled:cursor-not-allowed disabled:opacity-40"
        disabled={selected.size === 0}
        onclick={openConfirm}
      >
        <Broom size={16} />
        {$_("actions.preview")}
      </button>
    </div>
  {/if}
</div>

{#if plan}
  <ConfirmDrawer
    {plan}
    {busy}
    onconfirm={execute}
    oncancel={() => (plan = null)}
  />
{/if}

{#if report}
  <Report {report} ondone={() => (report = null)} />
{/if}
