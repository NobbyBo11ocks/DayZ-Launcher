<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Diagnostics from "./lib/Diagnostics.svelte";
  import Favourites from "./lib/Favourites.svelte";
  import JoinDialog from "./lib/JoinDialog.svelte";
  import Mods from "./lib/Mods.svelte";
  import Recent from "./lib/Recent.svelte";
  import Servers from "./lib/Servers.svelte";
  import Settings from "./lib/Settings.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import Welcome from "./lib/Welcome.svelte";
  import { prefs } from "./lib/state/prefs.svelte";
  import { servers } from "./lib/state/servers.svelte";
  import { updates } from "./lib/state/updates.svelte";

  $effect(() => {
    void updates.autoCheck();
  });

  const ONBOARDED_KEY = "dayz-launcher.onboarded.v1";
  let showWelcome = $state(false);
  try {
    showWelcome = localStorage.getItem(ONBOARDED_KEY) == null;
  } catch {
    showWelcome = false;
  }
  function finishWelcome() {
    showWelcome = false;
    try {
      localStorage.setItem(ONBOARDED_KEY, String(Date.now()));
    } catch {
      /* ignore */
    }
  }

  type AppInfo = { name: string; version: string; tauri: string; os: string };
  let info = $state<AppInfo | null>(null);

  $effect(() => {
    invoke<AppInfo>("app_info")
      .then((i) => (info = i))
      .catch(() => {});
  });

  // Theme/accent to <html> and storage whenever they change.
  $effect(() => {
    void prefs.theme;
    void prefs.accent;
    prefs.apply();
  });

  const sections = [
    { id: "servers", label: "Servers", glyph: "≡" },
    { id: "favourites", label: "Favourites", glyph: "★" },
    { id: "recent", label: "Recent", glyph: "↺" },
    { id: "mods", label: "Mods", glyph: "▦" },
    { id: "settings", label: "Settings", glyph: "⚙" },
    { id: "diagnostics", label: "Diagnostics", glyph: "✚" },
  ] as const;
  type Section = (typeof sections)[number]["id"];
  let active = $state<Section>("servers");
</script>

<div class="shell">
  <TitleBar subtitle={updates.state === "available" ? `v${info?.version ?? ""} · update ${updates.version} available (Settings)` : info ? `v${info.version}` : ""} />

  <nav class="rail" aria-label="Sections">
    {#each sections as s (s.id)}
      <button class="rail-item" class:active={active === s.id} aria-current={active === s.id ? "page" : undefined} onclick={() => (active = s.id)} title={s.label}>
        <span class="glyph" aria-hidden="true">{s.glyph}</span>
        <span class="text">{s.label}</span>
      </button>
    {/each}
  </nav>

  <main class="content" class:padded={active !== "servers" && active !== "favourites"}>
    {#if active === "servers"}
      <Servers />
    {:else if active === "favourites"}
      <Favourites />
    {:else if active === "recent"}
      <Recent />
    {:else if active === "mods"}
      <Mods />
    {:else if active === "settings"}
      <Settings />
    {:else if active === "diagnostics"}
      <Diagnostics />
    {/if}
  </main>
</div>

{#if servers.joiningId}
  <JoinDialog serverId={servers.joiningId} onClose={() => (servers.joiningId = null)} />
{/if}
{#if showWelcome}
  <Welcome onDone={finishWelcome} />
{/if}
