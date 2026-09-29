<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { tick, untrack } from "svelte";
  import { type Release, whatsNewPlan } from "./lib/changes";
  import { describe, invokeLogged, logError } from "./lib/log";
  import Favourites from "./lib/Favourites.svelte";
  import FilterPanel from "./lib/FilterPanel.svelte";
  import Friends from "./lib/Friends.svelte";
  import JoinDialog from "./lib/JoinDialog.svelte";
  import Lan from "./lib/Lan.svelte";
  import Logs from "./lib/Logs.svelte";
  import Mods from "./lib/Mods.svelte";
  import News from "./lib/News.svelte";
  import RailIcon from "./lib/RailIcon.svelte";
  import Recent from "./lib/Recent.svelte";
  import Servers from "./lib/Servers.svelte";
  import Settings from "./lib/Settings.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import Toasts from "./lib/Toasts.svelte";
  import Welcome from "./lib/Welcome.svelte";
  import WhatsNew from "./lib/WhatsNew.svelte";
  import { answerCloseRequests } from "./lib/state/closing";
  import { modUpdates } from "./lib/state/mods.svelte";
  import { news } from "./lib/state/news.svelte";
  import { notices } from "./lib/state/notices.svelte";
  import { prefs } from "./lib/state/prefs.svelte";
  import { servers } from "./lib/state/servers.svelte";
  import { uiPrefs } from "./lib/state/uiprefs.svelte";
  import { updates } from "./lib/state/updates.svelte";

  // Uncaught errors and rejected promises reach the log before anything else runs
  // (D-158): installed by main.ts's first import, ahead of the stores (row 25).

  // Every close of the window waits for the page's unsaved work, for a bounded time
  // (row 27; the host's close handler asks).
  $effect(() => answerCloseRequests());

  // Workshop updates are checked at start and every fifteen minutes, so a mod its
  // author updated shows up without opening the Mods page (D-191).
  $effect(() => {
    modUpdates.start();
    const off = listen("mods:done", () => void modUpdates.check());
    return () => {
      modUpdates.stop();
      void off.then((f) => f());
    };
  });

  // Nothing is fetched when the page is off, and switching it off mid-session
  // stops the 30-minute refresh rather than leaving it armed (D-174, D-185). The
  // landing page's stored posts are asked for as `news.start()` begins, ahead of the
  // server list, whose read holds the cache lock for 65–117 ms at 40 000–71 000 rows;
  // D-284 put this effect first for that, but the ask waited for the settings read
  // until row 24.
  $effect(() => {
    if (prefs.news) void news.start();
    else news.stop();
  });
  // The server store lives for the whole session: its listeners must not depend on
  // which section is open (D-084).
  // Once per session: read inside the News effect above, the News setting re-ran these
  // on every change, a second update check included (D-281).
  $effect(() => {
    untrack(() => {
      void servers.start();
      void updates.autoCheck();
    });
  });

  // The browser's reload keys are on in WebView2 unless something turns them off: F5,
  // pressed out of habit to refresh the list, reloaded the whole page, back to News with
  // an open join dialog — a slot wait, a typed password — gone, and the stores started
  // over beside a host that had not (row 14, F9).
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "F5" || ((e.ctrlKey || e.metaKey) && (e.key === "r" || e.key === "R"))) e.preventDefault();
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  });

  // Coming back to the window checks for a release again once an hour has passed, so
  // a launcher left open learns of one without a restart (D-280, D-300). A start always
  // checks.
  $effect(() => {
    const onFocus = () => updates.focusCheck();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  // The welcome overlay shows until the settings file says onboarded (D-070). A flag
  // left in localStorage by earlier builds counts and is migrated silently.
  const ONBOARDED_KEY = "dayz-launcher.onboarded.v1";
  let showWelcome = $state(false);
  $effect(() => {
    void uiPrefs.ready.then((u) => {
      // A settings file the host could not read, or had to set aside, is said once: the
      // defaults stood in for it with nothing on screen (row 14, F8/H4, approved).
      const h = uiPrefs.health;
      // After a reset it says what came through: the page keeps its own theme, filters and
      // News choice and writes them into the new file (row 15, F1, approved).
      if (h.reset) {
        const kept = h.keptAs ? ` The damaged file was kept as ${h.keptAs}.` : "";
        notices.push("Settings", `The settings file could not be read, so your launch options and profiles are back to their defaults; your theme, filters and News choice were kept.${kept}`);
      } else if (h.unreadable) {
        notices.push("Settings", "The settings file could not be read, so the defaults are in use.");
      }
      // Only a settings file that was actually read and says "not onboarded" opens
      // the overlay; an unreadable backend must not look like a first run (D-112).
      if (!uiPrefs.readOk) return;
      let onboarded = u.onboarded;
      if (!onboarded) {
        let legacy = false;
        try {
          legacy = localStorage.getItem(ONBOARDED_KEY) != null;
        } catch {
          /* storage unavailable */
        }
        if (legacy) uiPrefs.patch({ onboarded: true });
        else {
          showWelcome = true;
          // The list behind the welcome, where "Start browsing" goes: News mounted under it
          // marked its five first-run highlights seen before anyone saw them (row 17).
          active = "servers";
        }
        onboarded = legacy;
      }
      void checkWhatsNew(u.lastSeenVersion ?? "", onboarded);
      // The start-up copy of the start page is usually the file's; when it was not (storage
      // cleared), the file's choice applies unless another page was opened meanwhile.
      if (active === openedOn && prefs.openOn !== openedOn) active = prefs.openOn;
    });
  });
  function finishWelcome() {
    showWelcome = false;
    // "Start browsing" opens the server list, not the page behind the welcome (row 16).
    active = "servers";
    uiPrefs.patch({ onboarded: true });
    try {
      localStorage.setItem(ONBOARDED_KEY, String(Date.now()));
    } catch {
      /* ignore */
    }
    // The overlay took the focused button with it, and the next Tab started at the
    // title bar's window buttons: the list takes the focus instead, or the open section
    // before the list is drawn (D-291).
    void tick().then(() => (mainEl?.querySelector<HTMLElement>('[role="grid"]') ?? document.querySelector<HTMLElement>('.rail-item[aria-current="page"]'))?.focus());
  }

  // What changed, once after each update (user request, D-301). A first run has the
  // welcome instead and only records the version; a settings file from before 0.1.78
  // has none, which is an update to this one and gets its notes. The version counts as
  // seen when the window is closed, not when it opens.
  const SEEN_KEY = "dayz-launcher.whats-new-seen";
  let whatsNew = $state<{ version: string; releases: Release[] } | null>(null);
  async function checkWhatsNew(fileSeen: string, onboarded: boolean) {
    let version: string;
    try {
      version = (await invokeLogged<{ version: string }>("app_info")).version;
    } catch {
      return;
    }
    // The rules are `whatsNewPlan`'s (changes.ts), where they are tested (row 24).
    let kept = "";
    try {
      kept = localStorage.getItem(SEEN_KEY) ?? "";
    } catch {
      /* storage unavailable */
    }
    const plan = whatsNewPlan(fileSeen, kept, version, onboarded);
    if (plan.releases.length > 0) whatsNew = { version, releases: plan.releases };
    else if (plan.markSeen) markSeen(version);
  }
  function markSeen(version: string) {
    uiPrefs.patch({ lastSeenVersion: version });
    try {
      localStorage.setItem(SEEN_KEY, version);
    } catch {
      /* the file has it */
    }
  }
  function finishWhatsNew() {
    if (whatsNew) markSeen(whatsNew.version);
    whatsNew = null;
    // As after the welcome: focus to the open section, not the window buttons (D-291).
    void tick().then(() => document.querySelector<HTMLElement>('.rail-item[aria-current="page"]')?.focus());
  }

  // Theme/accent to <html> and storage whenever they change.
  $effect(() => {
    void prefs.theme;
    void prefs.accent;
    prefs.apply();
  });

  // Each section draws the icon of the same name (RailIcon, D-250).
  const allSections = [
    { id: "news", label: "News" },
    { id: "servers", label: "Servers" },
    { id: "lan", label: "LAN" },
    { id: "favourites", label: "Favourites" },
    { id: "friends", label: "Friends" },
    { id: "recent", label: "Recent" },
    { id: "mods", label: "Mods" },
    { id: "settings", label: "Settings" },
    { id: "logs", label: "Logs" },
  ] as const;
  type Section = (typeof allSections)[number]["id"];
  /** News can be switched off entirely (D-174); the sidebar then starts at Servers. */
  const sections = $derived(prefs.news ? allSections : allSections.filter((s) => s.id !== "news"));
  // The welcome line belongs to the window, not the home page (D-173). The hour is
  // re-read every five minutes: read once, a launcher opened in the morning said "Good
  // morning" all evening (D-256).
  let hour = $state(new Date().getHours());
  $effect(() => {
    const t = setInterval(() => (hour = new Date().getHours()), 5 * 60_000);
    return () => clearInterval(t);
  });
  const timeOfDay = $derived(hour < 5 ? "Still up" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening");
  /** Only while the Steam session is open (the idle release keeps it open): the last name
   *  stayed with Steam closed or nobody signed in (row 24, approved). */
  const persona = $derived(servers.steam?.initialized ? (servers.steam.persona ?? null) : null);
  const greeting = $derived(persona ? `${timeOfDay}, ${persona}` : "");

  /** The page the player picked to open on, News unless changed (D-100, row 16). */
  const openedOn: Section = prefs.openOn;
  let active = $state<Section>(openedOn);
  $effect(() => {
    if (!prefs.news && active === "news") active = "servers";
  });
  /** List views manage their own edges; the rest get padding and must fit the viewport (D-094). */
  const listViews: ReadonlySet<Section> = new Set<Section>(["servers", "lan", "favourites", "friends"]);

  // A view can ask for a section switch (Mods → Servers with a mod filter, D-080).
  // The switch takes that view, and the focus in it, away: focus goes to the new page's
  // list, or to its section in the rail, rather than to <body> (D-291).
  let mainEl = $state<HTMLElement | null>(null);
  $effect(() => {
    const want = servers.navigate;
    if (want && sections.some((s: { id: string }) => s.id === want)) {
      const hadFocus = untrack(() => mainEl)?.contains(document.activeElement) ?? false;
      active = want as Section;
      servers.navigate = null;
      if (hadFocus)
        void tick().then(() =>
          (untrack(() => mainEl)?.querySelector<HTMLElement>('[role="grid"]') ?? document.querySelector<HTMLElement>('.rail-item[aria-current="page"]'))?.focus(),
        );
    }
  });
</script>

<!-- `filters` keeps the rail at full width below 1100 px while it carries them (app.css). -->
<div class="shell" class:filters={active === "servers"}>
  <!-- Title-bar counts (D-103, D-105, D-106): servers with players live from the row cache, friends in DayZ. -->
  <TitleBar servers={servers.rowsTick >= 0 && servers.rows.size > 0 ? servers.populatedCount : null} friends={servers.friendsInDayz} friendsIdleAt={servers.steam?.idle ? servers.friendsAt : null} {greeting} />

  <div class="rail">
    <nav class="sections" aria-label="Sections">
      {#each sections as s (s.id)}
        <button class="rail-item" class:active={active === s.id} aria-current={active === s.id ? "page" : undefined} onclick={() => (active = s.id)} title={s.label}>
          <RailIcon name={s.id} />
          <span class="text">{s.label}</span>
          {#if s.id === "news" && news.unread > 0}<span class="badge" aria-label="{news.unread} new post{news.unread === 1 ? "" : "s"}">{news.unread > 99 ? "99+" : news.unread}</span>{/if}
          {#if s.id === "mods" && modUpdates.count > 0}<span class="badge" aria-label="{modUpdates.count} mod{modUpdates.count === 1 ? " has" : "s have"} an update waiting" title="{modUpdates.count} mod{modUpdates.count === 1 ? "" : "s"} can be updated">{modUpdates.count > 99 ? "99+" : modUpdates.count}</span>{/if}
        </button>
      {/each}
    </nav>

    <!-- The server filters sit under the sections while the Servers page is open, the
         one page they apply to (user's sketch, D-249). They are outside the page's
         boundary below, so they get one of their own: a failure here must not take
         the rail and the window with it (D-165). -->
    {#if active === "servers"}
      <svelte:boundary onerror={(e) => logError("view", `filters failed to render: ${describe(e)}`)}>
        <FilterPanel />
        {#snippet failed(_, reset)}
          <div class="rail-crashed">
            <p>The filters stopped working. The details are on the Logs page.</p>
            <button class="crashed-retry" onclick={reset}>Try again</button>
          </div>
        {/snippet}
      </svelte:boundary>
    {/if}

    <!-- The update is applied in Settings, so this is a way there rather than a
         label: a notice in the title bar could only be read, and the title bar is
         for what is true right now (counts, who you are) rather than for something
         to act on (D-216). `margin-top: auto` in app.css puts it at the foot of the
         rail, under the filters when they are showing (D-249). -->
    {#if updates.state === "available"}
      <button
        class="rail-item update"
        onclick={() => (active = "settings")}
        title="Version {updates.version} is ready to install — opens Settings"
        aria-label="Update available: version {updates.version}, opens Settings"
      >
        <RailIcon name="update" />
        <span class="text">Update available</span>
      </button>
    {/if}
  </div>

  <main class="content" class:padded={!listViews.has(active)} bind:this={mainEl}>
    <!-- One page failing must not blank the whole window (D-165): the boundary keeps
         the title bar and the sidebar alive so the user can switch away, and the
         error reaches launcher.log like any other. Keyed on the section, because a
         boundary that has failed stays failed: switching away kept "This page stopped
         working" on screen for every other page until Try again (D-240). -->
    {#key active}
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
      {#snippet failed(_, reset)}
        <div class="crashed">
          <p>This page stopped working. The details are on the Logs page.</p>
          <button class="crashed-retry" onclick={reset}>Try again</button>
        </div>
      {/snippet}
    </svelte:boundary>
    {/key}
  </main>
</div>

{#if servers.joiningId}
  <!-- Keyed on the server (D-159): the dialog plans once, after an await, so without
       this a new joiningId left the old plan, password and mod list on screen while
       the buttons acted on the new server. Now it is rebuilt from scratch. -->
  {#key servers.joiningId}
    <!-- A join window that fails to draw left its backdrop over the app with nothing on
         it (D-222); now it says so and can be closed (row 14, F18, approved). -->
    <svelte:boundary onerror={(e) => logError("view", `the join window failed to render: ${describe(e)}`)}>
      <JoinDialog serverId={servers.joiningId} onClose={() => (servers.joiningId = null)} />
      {#snippet failed()}
        <div class="join-crashed" role="presentation">
          <!-- Escape closes it and Tab stays on its one button, as in every other modal:
               the key did nothing, and Tab walked out to the rail behind it (row 27). -->
          <div
            class="crashed"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="join-crashed-text"
            tabindex="-1"
            onkeydown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                servers.joiningId = null;
              } else if (e.key === "Tab") {
                e.preventDefault();
                e.currentTarget.querySelector("button")?.focus();
              }
            }}
          >
            <p id="join-crashed-text">The join window stopped working.</p>
            <!-- svelte-ignore a11y_autofocus -->
            <button class="btn" autofocus onclick={() => (servers.joiningId = null)}>Close</button>
          </div>
        </div>
      {/snippet}
    </svelte:boundary>
  {/key}
{/if}
<Toasts />
{#if showWelcome}
  <Welcome onDone={finishWelcome} />
{:else if whatsNew}
  <WhatsNew version={whatsNew.version} releases={whatsNew.releases} onDone={finishWhatsNew} />
{/if}
