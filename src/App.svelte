<script lang="ts">
  import { describe, installErrorHooks, logError } from "./lib/log";
  import Favourites from "./lib/Favourites.svelte";
  import Friends from "./lib/Friends.svelte";
  import JoinDialog from "./lib/JoinDialog.svelte";
  import Lan from "./lib/Lan.svelte";
  import Logs from "./lib/Logs.svelte";
  import Mods from "./lib/Mods.svelte";
  import News from "./lib/News.svelte";
  import Recent from "./lib/Recent.svelte";
  import Servers from "./lib/Servers.svelte";
  import Settings from "./lib/Settings.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import Toasts from "./lib/Toasts.svelte";
  import Welcome from "./lib/Welcome.svelte";
  import { news } from "./lib/state/news.svelte";
  import { prefs } from "./lib/state/prefs.svelte";
  import { servers } from "./lib/state/servers.svelte";
  import { uiPrefs } from "./lib/state/uiprefs.svelte";
  import { updates } from "./lib/state/updates.svelte";

  // The server store lives for the whole session: its listeners must not depend on
  // which section is open (D-084).
  // Uncaught errors and rejected promises reach the log before anything else runs (D-158).
  installErrorHooks();


  $effect(() => {
    void servers.start();
    void updates.autoCheck();
    // Nothing is fetched when the page is off, and switching it off mid-session
    // stops the 30-minute refresh rather than leaving it armed (D-174, D-185).
    if (prefs.news) void news.start();
    else news.stop();
  });

  // The welcome overlay shows until the settings file says onboarded (D-070). A flag
  // left in localStorage by earlier builds counts and is migrated silently.
  const ONBOARDED_KEY = "dayz-launcher.onboarded.v1";
  let showWelcome = $state(false);
  $effect(() => {
    void uiPrefs.ready.then((u) => {
      // Only a settings file that was actually read and says "not onboarded" opens
      // the overlay; an unreadable backend must not look like a first run (D-112).
      if (u.onboarded || !uiPrefs.readOk) return;
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

  // Theme/accent to <html> and storage whenever they change.
  $effect(() => {
    void prefs.theme;
    void prefs.accent;
    prefs.apply();
  });

  const allSections = [
    { id: "news", label: "News", glyph: "▤" },
    { id: "servers", label: "Servers", glyph: "≡" },
    { id: "lan", label: "LAN", glyph: "⌂" },
    { id: "favourites", label: "Favourites", glyph: "★" },
    { id: "friends", label: "Friends", glyph: "☺" },
    { id: "recent", label: "Recent", glyph: "↺" },
    { id: "mods", label: "Mods", glyph: "▦" },
    { id: "settings", label: "Settings", glyph: "⚙" },
    { id: "logs", label: "Logs", glyph: "✚" },
  ] as const;
  type Section = (typeof allSections)[number]["id"];
  /** News can be switched off entirely (D-174); the sidebar then starts at Servers. */
  const sections = $derived(prefs.news ? allSections : allSections.filter((s) => s.id !== "news"));
  // The welcome line belongs to the window, not the home page (D-173).
  const hour = new Date().getHours();
  const timeOfDay = hour < 5 ? "Still up" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
  const persona = $derived(servers.steam?.persona ?? null);
  const greeting = $derived(persona ? `${timeOfDay}, ${persona}` : "");

  /** News is the landing page (D-100); the server list is one click away. */
  let active = $state<Section>("news");
  $effect(() => {
    if (!prefs.news && active === "news") active = "servers";
  });
  /** List views manage their own edges; the rest get padding and must fit the viewport (D-094). */
  const listViews: ReadonlySet<Section> = new Set<Section>(["servers", "lan", "favourites", "friends"]);

  // A view can ask for a section switch (Mods → Servers with a mod filter, D-080).
  $effect(() => {
    const want = servers.navigate;
    if (want && sections.some((s: { id: string }) => s.id === want)) {
      active = want as Section;
      servers.navigate = null;
    }
  });
</script>

<div class="shell">
  <!-- Title-bar counts (D-103, D-105, D-106): servers with players live from the row cache, friends in DayZ. -->
  <TitleBar servers={servers.rowsTick >= 0 && servers.rows.size > 0 ? servers.populatedCount : null} friends={servers.friendsInDayz} notice={updates.state === "available" ? `Update ${updates.version} available in Settings` : ""} {greeting} />

  <nav class="rail" aria-label="Sections">
    {#each sections as s (s.id)}
      <button class="rail-item" class:active={active === s.id} aria-current={active === s.id ? "page" : undefined} onclick={() => (active = s.id)} title={s.label}>
        <span class="glyph" aria-hidden="true">{s.glyph}</span>
        <span class="text">{s.label}</span>
        {#if s.id === "news" && news.unread > 0}<span class="badge" aria-label="{news.unread} new posts">{news.unread > 99 ? "99+" : news.unread}</span>{/if}
      </button>
    {/each}
  </nav>

  <main class="content" class:padded={!listViews.has(active)}>
    <!-- One page failing must not blank the whole window (D-160): the boundary keeps
         the title bar and the sidebar alive so the user can switch away, and the
         error reaches launcher.log like any other. -->
    <svelte:boundary onerror={(e) => logError("view", `${active} failed to render: ${describe(e)}`)}>
    {#if active === "news" && prefs.news}
      <News />
    {:else if active === "servers"}
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
    {:else if active === "logs"}
      <Logs />
    {/if}
      {#snippet failed(error, reset)}
        <div class="crashed">
          <p>This page stopped working.</p>
          <p class="crashed-why">{describe(error)}</p>
          <button class="crashed-retry" onclick={reset}>Try again</button>
        </div>
      {/snippet}
    </svelte:boundary>
  </main>
</div>

{#if servers.joiningId}
  <!-- Keyed on the server (D-159): the dialog plans once, after an await, so without
       this a new joiningId left the old plan, password and mod list on screen while
       the buttons acted on the new server. Now it is rebuilt from scratch. -->
  {#key servers.joiningId}
    <JoinDialog serverId={servers.joiningId} onClose={() => (servers.joiningId = null)} />
  {/key}
{/if}
<Toasts />
{#if showWelcome}
  <Welcome onDone={finishWelcome} />
{/if}
