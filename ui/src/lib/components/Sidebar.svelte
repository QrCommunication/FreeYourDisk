<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { House, HardDrives, GearSix, Package } from "phosphor-svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { nav, goTo } from "../stores";
  import { SERVICES } from "../api";
  import { serviceIcon } from "../icons";

  // Single source of truth: the version comes from Cargo.toml via Tauri,
  // never hardcoded in the UI.
  let version = $state("");
  onMount(async () => {
    try {
      version = await getVersion();
    } catch {
      /* not running under Tauri (e.g. browser preview) */
    }
  });
</script>

<aside class="bg-surface border-line flex w-60 shrink-0 flex-col border-r">
  <div class="flex items-center gap-2.5 px-5 py-5">
    <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
      <circle
        cx="12"
        cy="12"
        r="8"
        fill="none"
        stroke="var(--c-line)"
        stroke-width="3.5"
      />
      <circle
        cx="12"
        cy="12"
        r="8"
        fill="none"
        stroke="var(--c-accent)"
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

  <nav class="flex flex-1 flex-col gap-0.5 overflow-y-auto px-3 py-2">
    <button
      class="group flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors"
      class:bg-accent-soft={$nav === "home"}
      class:text-accent={$nav === "home"}
      class:text-muted={$nav !== "home"}
      class:hover:text-ink={$nav !== "home"}
      onclick={() => goTo("home")}
    >
      <House size={18} weight={$nav === "home" ? "fill" : "regular"} />
      {$_("nav.home")}
    </button>

    <div class="text-faint px-3 pt-4 pb-1 text-[11px] tracking-wider uppercase">
      Services
    </div>

    {#each SERVICES as id (id)}
      {@const Icon = serviceIcon[id]}
      {@const active = $nav === id}
      <button
        class="group flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors"
        class:bg-accent-soft={active}
        class:text-accent={active}
        class:text-muted={!active}
        class:hover:text-ink={!active}
        onclick={() => goTo(id)}
      >
        <Icon size={18} weight={active ? "fill" : "regular"} />
        {$_(`service.${id}`)}
      </button>
    {/each}

    <div class="text-faint px-3 pt-4 pb-1 text-[11px] tracking-wider uppercase">
      Système
    </div>

    <button
      class="group flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors"
      class:bg-accent-soft={$nav === "applications"}
      class:text-accent={$nav === "applications"}
      class:text-muted={$nav !== "applications"}
      class:hover:text-ink={$nav !== "applications"}
      onclick={() => goTo("applications")}
    >
      <Package
        size={18}
        weight={$nav === "applications" ? "fill" : "regular"}
      />
      {$_("nav.applications")}
    </button>
    <button
      class="group flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors"
      class:bg-accent-soft={$nav === "health"}
      class:text-accent={$nav === "health"}
      class:text-muted={$nav !== "health"}
      class:hover:text-ink={$nav !== "health"}
      onclick={() => goTo("health")}
    >
      <HardDrives size={18} weight={$nav === "health" ? "fill" : "regular"} />
      {$_("nav.health")}
    </button>
    <button
      class="group flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors"
      class:bg-accent-soft={$nav === "settings"}
      class:text-accent={$nav === "settings"}
      class:text-muted={$nav !== "settings"}
      class:hover:text-ink={$nav !== "settings"}
      onclick={() => goTo("settings")}
    >
      <GearSix size={18} weight={$nav === "settings" ? "fill" : "regular"} />
      {$_("nav.settings")}
    </button>
  </nav>

  <div class="text-faint px-5 py-4 text-[11px]">
    {version ? `v${version} · ` : ""}GPL-3.0
  </div>
</aside>
