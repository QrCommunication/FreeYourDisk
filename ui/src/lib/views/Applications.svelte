<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { fade } from "svelte/transition";
  import {
    CircleNotch,
    Trash,
    ArrowsClockwise,
    Lightning,
    Package,
    Lock,
    Funnel,
  } from "phosphor-svelte";
  import { api, type AppEntry } from "../api";
  import { humanizeBytes } from "../format";
  import { toasts } from "../stores";
  import StateBlock from "../components/StateBlock.svelte";

  let status = $state<"loading" | "done" | "error">("loading");
  let apps = $state<AppEntry[]>([]);
  let updateIds = $state<Set<string>>(new Set());
  let selection = $state<Set<string>>(new Set());
  let checking = $state(false);
  let busy = $state<null | "uninstall" | "update">(null);
  let confirmUninstall = $state(false);
  let onlyUpdates = $state(false);

  const SOURCE_COLOR: Record<string, string> = {
    apt: "#d70a53",
    flatpak: "#4a90d9",
    snap: "#f5732b",
    appimage: "#f7a800",
  };

  const protectedSet = $derived(
    new Set(apps.filter((a) => a.protected).map((a) => a.id)),
  );
  const displayed = $derived(
    onlyUpdates ? apps.filter((a) => updateIds.has(a.id)) : apps,
  );
  // Uninstall never touches protected system apps; update may.
  const selectedUninstallable = $derived(
    [...selection].filter((id) => !protectedSet.has(id)),
  );
  const selectedUpdatable = $derived(
    [...selection].filter((id) => updateIds.has(id)),
  );

  async function load() {
    status = "loading";
    selection = new Set();
    try {
      apps = await api.listApplications();
      status = "done";
    } catch {
      status = "error";
    }
  }

  async function checkUpdates(announce = true) {
    checking = true;
    try {
      updateIds = new Set(await api.appUpdates());
      if (announce || updateIds.size > 0) {
        toasts.success(
          $_("applications.updates_found", {
            values: { count: updateIds.size },
          }),
        );
      }
    } catch {
      if (announce) toasts.error($_("applications.updates_failed"));
    } finally {
      checking = false;
    }
  }

  function toggle(id: string) {
    const next = new Set(selection);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selection = next;
  }

  async function doUninstall() {
    confirmUninstall = false;
    busy = "uninstall";
    try {
      const report = await api.uninstallApps(selectedUninstallable);
      report.errors.length > 0
        ? toasts.error(
            $_("applications.action_partial", {
              values: {
                ok: report.succeeded.length,
                err: report.errors.length,
              },
            }),
          )
        : toasts.success(
            $_("applications.uninstalled", {
              values: { count: report.succeeded.length },
            }),
          );
      await load();
    } catch {
      toasts.error($_("applications.action_failed"));
    } finally {
      busy = null;
    }
  }

  async function doUpdate() {
    busy = "update";
    try {
      const report = await api.updateApps(selectedUpdatable);
      report.errors.length > 0
        ? toasts.error(
            $_("applications.action_partial", {
              values: {
                ok: report.succeeded.length,
                err: report.errors.length,
              },
            }),
          )
        : toasts.success(
            $_("applications.updated", {
              values: { count: report.succeeded.length },
            }),
          );
      updateIds = new Set();
      await load();
    } catch {
      toasts.error($_("applications.action_failed"));
    } finally {
      busy = null;
    }
  }

  onMount(async () => {
    await load();
    // Automatically surface available updates on open (quiet unless any exist).
    void checkUpdates(false);
  });
</script>

<div class="flex h-full w-full flex-col px-10 py-8">
  <header class="mb-6 flex items-end justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">
        {$_("nav.applications")}
      </h1>
      <p class="text-muted text-sm">{$_("applications.subtitle")}</p>
    </div>
    <div class="flex items-center gap-2">
      {#if updateIds.size > 0}
        <button
          class="inline-flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs transition active:scale-95"
          class:border-savings={onlyUpdates}
          class:text-savings={onlyUpdates}
          class:border-line={!onlyUpdates}
          class:text-muted={!onlyUpdates}
          onclick={() => (onlyUpdates = !onlyUpdates)}
        >
          <Funnel size={14} weight={onlyUpdates ? "fill" : "regular"} />
          {onlyUpdates
            ? $_("applications.show_all")
            : $_("applications.only_updates")}
        </button>
      {/if}
      <button
        class="border-line text-muted hover:text-ink inline-flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs transition active:scale-95"
        onclick={() => checkUpdates()}
        disabled={checking || status !== "done"}
      >
        {#if checking}
          <CircleNotch size={14} class="animate-spin" />{$_(
            "applications.checking",
          )}
        {:else}
          <Lightning size={14} weight="fill" />{$_(
            "applications.check_updates",
          )}
        {/if}
      </button>
    </div>
  </header>

  {#if status === "loading"}
    <StateBlock kind="loading" />
  {:else if status === "error"}
    <StateBlock kind="error" onretry={load} />
  {:else if apps.length === 0}
    <StateBlock kind="empty" title={$_("applications.none")} desc="" />
  {:else}
    <ul
      class="divide-line border-line bg-surface flex-1 divide-y overflow-y-auto rounded-xl border"
    >
      {#each displayed as app (app.id)}
        {@const upd = updateIds.has(app.id)}
        <li>
          <label
            class="hover:bg-elevated flex cursor-pointer items-center gap-3 px-4 py-2.5"
            class:opacity-55={app.protected}
          >
            <input
              type="checkbox"
              class="accent-accent h-4 w-4"
              checked={selection.has(app.id)}
              onchange={() => toggle(app.id)}
            />
            <Package size={18} class="text-faint shrink-0" />
            <div class="min-w-0 flex-1">
              <p class="text-ink truncate text-sm font-medium">{app.name}</p>
              <p class="nums text-faint text-xs">
                {app.version ?? ""}
              </p>
            </div>
            {#if app.protected}
              <span
                class="text-faint inline-flex items-center gap-1 text-[10px] font-medium"
                title={$_("applications.protected_hint")}
              >
                <Lock size={11} weight="fill" />{$_("applications.protected")}
              </span>
            {/if}
            <span
              class="rounded px-1.5 py-0.5 text-[10px] font-medium uppercase"
              style="color:{SOURCE_COLOR[
                app.source
              ]}; background:color-mix(in oklab, {SOURCE_COLOR[
                app.source
              ]} 16%, transparent)"
            >
              {app.source}
            </span>
            {#if upd}
              <span
                class="bg-savings-soft text-savings rounded px-1.5 py-0.5 text-[10px] font-medium"
              >
                {$_("applications.update_badge")}
              </span>
            {/if}
            <span class="nums text-muted w-20 text-right text-xs">
              {humanizeBytes(app.size_bytes)}
            </span>
          </label>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<!-- Action bar -->
{#if selection.size > 0}
  <div
    class="border-line bg-elevated fixed right-6 bottom-6 z-30 flex items-center gap-3 rounded-2xl border px-4 py-3 shadow-2xl"
    transition:fade={{ duration: 150 }}
  >
    <span class="text-muted text-sm">
      {$_("table.selected", { values: { count: selection.size } })}
    </span>
    {#if selectedUpdatable.length > 0}
      <button
        class="bg-savings inline-flex items-center gap-1.5 rounded-lg px-3 py-2 text-sm font-semibold text-white transition active:scale-95"
        disabled={busy !== null}
        onclick={doUpdate}
      >
        {#if busy === "update"}<CircleNotch
            size={15}
            class="animate-spin"
          />{:else}<ArrowsClockwise size={15} weight="bold" />{/if}
        {$_("applications.update_selected", {
          values: { count: selectedUpdatable.length },
        })}
      </button>
    {/if}
    {#if selectedUninstallable.length > 0}
      <button
        class="bg-danger inline-flex items-center gap-1.5 rounded-lg px-3 py-2 text-sm font-semibold text-white transition active:scale-95"
        disabled={busy !== null}
        onclick={() => (confirmUninstall = true)}
      >
        {#if busy === "uninstall"}<CircleNotch
            size={15}
            class="animate-spin"
          />{:else}<Trash size={15} weight="fill" />{/if}
        {$_("applications.uninstall_selected")}
        <span class="nums opacity-80">({selectedUninstallable.length})</span>
      </button>
    {/if}
  </div>
{/if}

<!-- Uninstall confirmation -->
{#if confirmUninstall}
  <div
    class="fixed inset-0 z-40 grid place-items-center p-6"
    transition:fade={{ duration: 120 }}
  >
    <button
      class="absolute inset-0 cursor-default bg-black/55"
      aria-label="Close"
      onclick={() => (confirmUninstall = false)}
    ></button>
    <div
      class="bg-elevated border-line relative w-full max-w-sm rounded-2xl border p-6 text-center shadow-2xl"
    >
      <div
        class="bg-danger-soft text-danger mx-auto grid h-12 w-12 place-items-center rounded-full"
      >
        <Trash size={22} weight="fill" />
      </div>
      <h2 class="mt-3 text-base font-semibold">
        {$_("applications.confirm_title")}
      </h2>
      <p class="text-muted mt-1 text-sm">
        {$_("applications.confirm_body", {
          values: { count: selectedUninstallable.length },
        })}
      </p>
      <div class="mt-5 flex justify-center gap-3">
        <button
          class="border-line text-muted hover:text-ink cursor-pointer rounded-lg border px-4 py-2 text-sm"
          onclick={() => (confirmUninstall = false)}
        >
          {$_("actions.cancel")}
        </button>
        <button
          class="bg-danger cursor-pointer rounded-lg px-4 py-2 text-sm font-semibold text-white"
          onclick={doUninstall}
        >
          {$_("applications.uninstall_selected")}
        </button>
      </div>
    </div>
  </div>
{/if}
