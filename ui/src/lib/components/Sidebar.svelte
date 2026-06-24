<script lang="ts">
  import { _ } from "svelte-i18n";
  import { House } from "phosphor-svelte";
  import { view, goDashboard, goService } from "../stores";
  import { SERVICES } from "../api";
  import { serviceIcon } from "../icons";

  const current = $derived($view);
</script>

<aside class="bg-surface border-line flex w-60 shrink-0 flex-col border-r">
  <div class="flex items-center gap-2.5 px-5 py-5">
    <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
      <circle
        cx="12"
        cy="12"
        r="8"
        fill="none"
        stroke="#1f2630"
        stroke-width="3.5"
      />
      <circle
        cx="12"
        cy="12"
        r="8"
        fill="none"
        stroke="#2dd4bf"
        stroke-width="3.5"
        stroke-dasharray="36 50"
        stroke-linecap="round"
        transform="rotate(-90 12 12)"
      />
    </svg>
    <div class="leading-tight">
      <div class="text-ink text-sm font-semibold tracking-tight">
        {$_("app.name")}
      </div>
      <div class="text-faint text-[11px]">{$_("app.tagline")}</div>
    </div>
  </div>

  <nav class="flex flex-col gap-0.5 px-3 py-2">
    <button
      class="group flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors"
      class:bg-accent-soft={current.kind === "dashboard"}
      class:text-accent={current.kind === "dashboard"}
      class:text-muted={current.kind !== "dashboard"}
      onclick={goDashboard}
    >
      <House
        size={18}
        weight={current.kind === "dashboard" ? "fill" : "regular"}
      />
      {$_("nav.dashboard")}
    </button>

    <div class="text-faint px-3 pt-4 pb-1 text-[11px] tracking-wider uppercase">
      Services
    </div>

    {#each SERVICES as id (id)}
      {@const Icon = serviceIcon[id]}
      {@const active = current.kind === "service" && current.id === id}
      <button
        class="group flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors"
        class:bg-accent-soft={active}
        class:text-accent={active}
        class:text-muted={!active}
        onclick={() => goService(id)}
      >
        <Icon size={18} weight={active ? "fill" : "regular"} />
        {$_(`service.${id}`)}
      </button>
    {/each}
  </nav>

  <div class="text-faint mt-auto px-5 py-4 text-[11px]">v0.1.0 · GPL-3.0</div>
</aside>
