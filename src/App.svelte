<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Diagnostics from "./lib/Diagnostics.svelte";
  import Favourites from "./lib/Favourites.svelte";
  import Friends from "./lib/Friends.svelte";
  import JoinDialog from "./lib/JoinDialog.svelte";
  import Lan from "./lib/Lan.svelte";
  import Mods from "./lib/Mods.svelte";
  import Recent from "./lib/Recent.svelte";
  import Servers from "./lib/Servers.svelte";
  import Settings from "./lib/Settings.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import Toasts from "./lib/Toasts.svelte";
  import Welcome from "./lib/Welcome.svelte";
  import { prefs } from "./lib/state/prefs.svelte";
  import { servers } from "./lib/state/servers.svelte";
  import { uiPrefs } from "./lib/state/uiprefs.svelte";
  import { updates } from "./lib/state/updates.svelte";

  // The server store lives for the whole session: its listeners must not depend on
  // which section is open (D-084).
  $effect(() => {
    void servers.start();
    void updates.autoCheck();
  });

  // The welcome overlay shows until the settings file says onboarded (D-070). A flag
  // left in localStorage by earlier builds counts and is migrated silently.
  const ONBOARDED_KEY = "dayz-launcher.onboarded.v1";
  let showWelcome = $state(false);
  $effect(() => {
    void uiPrefs.ready.then((u) => {
      if (u.onboarded) return;
      let legacy = false;
      try {
        legacy = localStorage.getItem(ONBOARDED_KEY) != null;
      } catch {
        /* storage unavailable */
      }
      if (legacy) uiPrefs.patch({ onboarded: true });
      else showWelcome = true;
    });
  });
  function finishWelcome() {
    showWelcome = false;
    uiPrefs.patch({ onboarded: true });
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
    { id: "lan", label: "LAN", glyph: "⌂" },
    { id: "favourites", label: "Favourites", glyph: "★" },
    { id: "friends", label: "Friends", glyph: "☺" },
    { id: "recent", label: "Recent", glyph: "↺" },
    { id: "mods", label: "Mods", glyph: "▦" },
    { id: "settings", label: "Settings", glyph: "⚙" },
    { id: "diagnostics", label: "Diagnostics", glyph: "✚" },
  ] as const;
  type Section = (typeof sections)[number]["id"];
  let active = $state<Section>("servers");
  /** List views manage their own edges; the rest get padding and must fit the viewport (D-094). */
  const listViews: ReadonlySet<Section> = new Set<Section>(["servers", "lan", "favourites", "friends"]);

  // A view can ask for a section switch (Mods → Servers with a mod filter, D-080).
  $effect(() => {
    const want = servers.navigate;
    if (want && sections.some((s) => s.id === want)) {
      active = want as Section;
      servers.navigate = null;
    }
  });
</script>

<div class="shell">
  <TitleBar notice={updates.state === "available" ? `Update ${updates.version} available in Settings` : ""} />

  <nav class="rail" aria-label="Sections">
    {#each sections as s (s.id)}
      <button class="rail-item" class:active={active === s.id} aria-current={active === s.id ? "page" : undefined} onclick={() => (active = s.id)} title={s.label}>
        <span class="glyph" aria-hidden="true">{s.glyph}</span>
        <span class="text">{s.label}</span>
      </button>
    {/each}
  </nav>

  <main class="content" class:padded={!listViews.has(active)}>
    {#if active === "servers"}
      <Servers />
    {:else if active === "lan"}
      <Lan />
    {:else if active === "favourites"}
      <Favourites />
    {:else if active === "friends"}
      <Friends />
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
<Toasts />
{#if showWelcome}
  <Welcome onDone={finishWelcome} />
{/if}
