<script lang="ts">
  import { _ } from "svelte-i18n";
  import { Desktop, Sun, Moon } from "phosphor-svelte";
  import Switch from "../components/Switch.svelte";
  import ScheduleToggle from "../components/ScheduleToggle.svelte";
  import { settings, updateSettings } from "../settings";
  import type { LangPref, ThemePref } from "../api";

  const themeOptions: { value: ThemePref; label: string; icon: typeof Sun }[] =
    [
      { value: "system", label: "settings.theme_system", icon: Desktop },
      { value: "light", label: "settings.theme_light", icon: Sun },
      { value: "dark", label: "settings.theme_dark", icon: Moon },
    ];
  const langOptions: { value: LangPref; label: string }[] = [
    { value: "system", label: "settings.lang_system" },
    { value: "fr", label: "settings.lang_fr" },
    { value: "en", label: "settings.lang_en" },
  ];
</script>

<div class="mx-auto max-w-3xl px-10 py-8">
  <header class="mb-8">
    <h1 class="text-2xl font-semibold tracking-tight">{$_("nav.settings")}</h1>
    <p class="text-muted text-sm">{$_("settings.subtitle")}</p>
  </header>

  <!-- Appearance -->
  <section class="mb-8">
    <h2 class="text-faint mb-3 text-xs font-semibold uppercase tracking-wide">
      {$_("settings.appearance")}
    </h2>
    <div class="border-line bg-surface divide-line divide-y rounded-xl border">
      <div class="flex items-center justify-between gap-4 p-4">
        <span class="text-sm font-medium">{$_("settings.theme")}</span>
        <div class="bg-base flex gap-1 rounded-lg p-1">
          {#each themeOptions as opt (opt.value)}
            {@const Icon = opt.icon}
            <button
              class="flex cursor-pointer items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition"
              class:bg-accent={$settings.theme === opt.value}
              class:text-accent-ink={$settings.theme === opt.value}
              class:text-muted={$settings.theme !== opt.value}
              onclick={() => updateSettings({ theme: opt.value })}
            >
              <Icon
                size={15}
                weight={$settings.theme === opt.value ? "fill" : "regular"}
              />
              {$_(opt.label)}
            </button>
          {/each}
        </div>
      </div>
      <div class="flex items-center justify-between gap-4 p-4">
        <span class="text-sm font-medium">{$_("settings.language")}</span>
        <div class="bg-base flex gap-1 rounded-lg p-1">
          {#each langOptions as opt (opt.value)}
            <button
              class="cursor-pointer rounded-md px-3 py-1.5 text-xs font-medium transition"
              class:bg-accent={$settings.language === opt.value}
              class:text-accent-ink={$settings.language === opt.value}
              class:text-muted={$settings.language !== opt.value}
              onclick={() => updateSettings({ language: opt.value })}
            >
              {$_(opt.label)}
            </button>
          {/each}
        </div>
      </div>
    </div>
  </section>

  <!-- Startup -->
  <section class="mb-8">
    <h2 class="text-faint mb-3 text-xs font-semibold uppercase tracking-wide">
      {$_("settings.startup")}
    </h2>
    <div
      class="border-line bg-surface flex items-center gap-4 rounded-xl border p-4"
    >
      <div class="min-w-0 flex-1">
        <p class="text-sm font-medium">{$_("settings.autostart")}</p>
        <p class="text-muted text-xs">{$_("settings.autostart_desc")}</p>
      </div>
      <Switch
        checked={$settings.autostart}
        label={$_("settings.autostart")}
        onchange={(v) => updateSettings({ autostart: v })}
      />
    </div>
  </section>

  <!-- Monitoring -->
  <section class="mb-8">
    <h2 class="text-faint mb-3 text-xs font-semibold uppercase tracking-wide">
      {$_("settings.monitoring")}
    </h2>
    <div class="border-line bg-surface divide-line divide-y rounded-xl border">
      <div class="flex items-center gap-4 p-4">
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium">{$_("settings.monitor")}</p>
          <p class="text-muted text-xs">{$_("settings.monitor_desc")}</p>
        </div>
        <Switch
          checked={$settings.monitor_enabled}
          label={$_("settings.monitor")}
          onchange={(v) => updateSettings({ monitor_enabled: v })}
        />
      </div>
      {#if $settings.monitor_enabled}
        <div class="flex items-center justify-between gap-4 p-4">
          <span class="text-sm">{$_("settings.threshold")}</span>
          <div class="flex items-center gap-3">
            <input
              type="range"
              min="1"
              max="20"
              class="accent-accent w-40"
              value={$settings.monitor_threshold}
              oninput={(e) =>
                updateSettings({
                  monitor_threshold: Number(e.currentTarget.value),
                })}
            />
            <span class="nums text-accent w-8 text-right text-sm font-medium"
              >{$settings.monitor_threshold}%</span
            >
          </div>
        </div>
      {/if}
    </div>
  </section>

  <!-- Scheduled cleanup -->
  <section>
    <h2 class="text-faint mb-3 text-xs font-semibold uppercase tracking-wide">
      {$_("settings.scheduled_clean")}
    </h2>
    <ScheduleToggle />
  </section>
</div>
