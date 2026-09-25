<script lang="ts">
  // Server browser view (docs/06, D-108, D-249, D-250): a one-row header (the search,
  // Direct connect beside it in a small popover, Refresh at the right, and between them
  // any notice that needs the user), then the virtualised table and the details pane.
  // The filters live in the left rail under the sections (FilterPanel, mounted by
  // App.svelte while this page is open).
  import { untrack } from "svelte";
  import DetailsPane from "./DetailsPane.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { searchBox } from "./search";
  import { servers } from "./state/servers.svelte";

  // The shared search filter, written on a pause rather than on every keystroke: every
  // write re-filters and re-sorts up to 20 000 rows (D-152, D-222). `typed` keeps the
  // field responsive while the filter lags behind it.
  const box = searchBox();
  let typed = $state(servers.filters.search);
  let searchEl = $state<HTMLInputElement | null>(null);
  $effect(() => box.dispose);
  // A reset from elsewhere (the Reset button in the rail) has to show up in the field
  // and cancel a pending write, or the list stays filtered by an invisible term
  // (D-159). Only the store is tracked: with `typed` tracked too, every keystroke
  // blanked the field while the write still went through (D-230).
  $effect(() => {
    const stored = servers.filters.search;
    untrack(() => {
      if (stored === "" && typed !== "") {
        box.dispose();
        typed = "";
      }
    });
  });
  function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  let direct = $state("");
  let connecting = $state(false);
  let connectOpen = $state(false);
  let connectInput = $state<HTMLInputElement | null>(null);
  let connectBtn = $state<HTMLButtonElement | null>(null);

  /** Closing destroys the focused input, so focus has to be handed back (D-224). */
  function closeConnect() {
    connectOpen = false;
    connectBtn?.focus();
  }
  const fmt = new Intl.NumberFormat();

  function toggleConnect() {
    connectOpen = !connectOpen;
    if (connectOpen) requestAnimationFrame(() => connectInput?.focus());
  }

  async function connectDirect() {
    const address = direct.trim();
    if (!address) return;
    connecting = true;
    const row = await servers.directConnect(address);
    connecting = false;
    if (row) {
      direct = "";
      // Through closeConnect, like Escape: closing destroys the focused field, and
      // focus fell to <body>, where the grid's keys stop working (D-224, D-256).
      closeConnect();
    }
  }

  // The store is started once by App.svelte and never stopped (D-084).


  /** Inputs that take no text: a key pressed on one of these is not typing. */
  const NON_TEXT_INPUTS = new Set(["checkbox", "radio", "button", "submit", "reset", "range", "color", "file"]);

  function onKey(e: KeyboardEvent) {
    // The join dialog is modal; the page behind it must not act on keys (D-184).
    if (servers.joiningId) return;
    const target = e.target;
    // Only a field that takes text keeps "/" and Escape. The Status filters became
    // checkboxes in D-249, and a click on one left both keys dead while it had focus
    // (D-256).
    const typing =
      target instanceof HTMLTextAreaElement ||
      target instanceof HTMLSelectElement ||
      (target instanceof HTMLInputElement && !NON_TEXT_INPUTS.has(target.type));
    if (e.key === "/" && !typing) {
      e.preventDefault();
      focusSearch();
    } else if (e.key === "Escape" && connectOpen) {
      closeConnect();
    } else if (e.key === "Escape" && !typing && servers.selectedId) {
      // The grid closes the pane on Escape only while it has focus; after a click on
      // the pane's own Join, star or a mod link, or on a filter, the key did nothing
      // and the pane stayed (D-247). A text field keeps its own Escape.
      servers.select(null);
    }
  }

  const busy = $derived(!servers.steam?.initialized || servers.steam?.refreshing === true);
  // Only a selection the grid lists: one made on Favourites or LAN that the filters
  // here hide, or flipped untrusted by a verification, left the grid highlighting
  // nothing while Enter and F still acted on it (D-240 fixed this on the other two
  // pages, D-245 here). The pane keeps following the selection itself.
  const shownSelectedId = $derived(
    servers.selectedId && servers.list.some((r) => r.id === servers.selectedId) ? servers.selectedId : null,
  );

  /**
   * What an empty grid should say. The three cases are genuinely different: a filter
   * that matches nothing, a list that is on its way, and a list that cannot arrive.
   * Before D-184 the first frame claimed Steam had not answered — `steam` is simply
   * null until the first status event — and then told the user to press a Refresh
   * button that is disabled precisely because the refresh is already running.
   */
  const emptyText = $derived.by(() => {
    void servers.rowsTick;
    if (servers.rows.size > 0)
      return servers.hasEmptyServers
        ? "No server matches these filters. Clear the search, or widen the filters on the left."
        : "No server matches these filters — and servers with nobody on them have not been fetched. Press Refresh to include them; it takes a few minutes.";
    const s = servers.steam;
    if (s?.refreshing || servers.dzsaLoading) return "Fetching the list…";
    if (s == null) return "Starting up…";
    if (!s.initialized) return "Steam has not answered, so there is no list yet. Start Steam, or load the DZSA list above.";
    return "No servers yet. Press Refresh to ask Steam for the list; it takes about forty seconds.";
  });

  // What the hidden live region says. Only the outcomes, and only when they land:
  // the visible status line changes every 350 ms during a refresh, which is exactly
  // what must not be announced (D-224).
  const announcement = $derived.by(() => {
    if (servers.error) return servers.error;
    const v = servers.verifySummary;
    if (v?.skipped) return "Verification deferred; a pass is already running.";
    if (v) return `${fmt.format(v.verified)} verified, ${fmt.format(v.inflated + v.unverifiable + v.synthetic)} fake, ${v.offline} offline.`;
    const d = servers.done;
    if (d && !d.rejected) return `${fmt.format(d.responded)} servers listed.`;
    return "";
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="servers">
  <div class="top">
    <div class="row">
      <label class="searchwrap">
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" /></svg>
        <input
          class="search"
          type="search"
          placeholder="Search name, map or IP"
          value={typed}
          bind:this={searchEl}
          aria-label="Search servers (press / to focus)"
          spellcheck="false"
          oninput={(e) => ((typed = e.currentTarget.value), box.set(typed))}
          onkeydown={(e) => {
            if (e.key === "Escape") {
              typed = "";
              box.set("");
              e.currentTarget.blur();
            }
          }}
        />
        <kbd aria-hidden="true">/</kbd>
      </label>
      <!-- Beside the search, where an address would be typed anyway (user request,
           D-250); the popover opens to the right, over the list. -->
      <div class="connect">
        <button class="iconbtn" bind:this={connectBtn} class:on={connectOpen} onclick={toggleConnect} aria-expanded={connectOpen} aria-label="Direct connect" title="Direct connect to an address">
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 1.5v3M8 11.5v3M1.5 8h3M11.5 8h3" /><circle cx="8" cy="8" r="3" /></svg>
        </button>
        {#if connectOpen}
          <form class="pop" onsubmit={(e) => { e.preventDefault(); void connectDirect(); }}>
            <span class="poptitle">Direct connect</span>
            <input type="text" placeholder="ip:port" bind:value={direct} bind:this={connectInput} aria-label="Direct connect address" spellcheck="false" />
            <button class="btn" type="submit" disabled={connecting || !direct.trim()}>{connecting ? "…" : "Add"}</button>
          </form>
        {/if}
      </div>

      <!-- The status line under the search is gone (user request, D-250): the counts,
           the pass progress and the cache time it carried every second are not shown.
           What needs the user — Steam not running, with the DZSA fallback, and an
           error — shows here in the header row, and only while it is true. -->
      <div class="notices">
        {#if servers.steam && !servers.steam.initialized}
          <span class="warn" title={servers.steam.error ?? "Steam is not running; the launcher connects as soon as it starts."}>Steam is not running; the launcher connects as soon as it starts.</span>
          <button class="link" onclick={() => servers.loadDzsa()} disabled={servers.dzsaLoading} title="Download the DZSA Launcher's public server list (about 24 MB) instead">
            {servers.dzsaLoading ? "Downloading…" : "Load list from DZSA"}
          </button>
        {/if}
        {#if servers.error}<span class="error" title={servers.error}>{servers.error}</span>{/if}
      </div>

      <!-- One button (D-141): the full partition list starts with the populated
           servers, so the rows people care about arrive in about 40 s and the empty
           ones keep filling in behind them. -->
      <button class="btn" onclick={() => servers.refresh(true, true)} disabled={busy} title="Fetch from Steam: servers with players first (about 40 s), then the empty ones (a few minutes)">
        {servers.steam?.refreshing ? "Refreshing…" : "Refresh"}
      </button>
    </div>

    <!-- Screen readers still hear the outcomes, and only when they land: the old visible
         line read `rowsTick`, which bumps every 350 ms, so it was never a live region
         itself (D-224). -->
    <span class="sr-only" role="status" aria-live="polite">{announcement}</span>
  </div>

  <div class="main" class:with-pane={servers.selected != null}>
    <ServerTable
      rows={servers.list}
      selectedId={shownSelectedId}
      sort={servers.sort}
      localVersion={servers.localVersion}
      favourites={servers.favourites}
      onSelect={(id) => servers.select(id)}
      onSort={(k) => servers.setSort(k)}
      filterKey={servers.filterKey}
      onVisible={(ids) => servers.verifyVisible(ids)}
      onActivate={(id) => (servers.joiningId = id)}
      onFavourite={(id) => servers.toggleFavourite(id)}
      friendsOn={servers.friendsOn}
      modsByServer={servers.modsByServer}
      inert={servers.joiningId !== null}
      empty={emptyText}
    />
    {#if servers.selected}
      <DetailsPane row={servers.selected} localVersion={servers.localVersion} />
    {/if}
  </div>
</div>

<style>
  /* Read by assistive technology, never drawn. */
  .sr-only { position: absolute; width: 1px; height: 1px; margin: -1px; padding: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0; }
  .servers { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .top { padding: 8px 16px; border-bottom: 1px solid var(--border); background: var(--bg-elev); }
  /* One row: search, Direct connect beside it, any notice in the free middle, Refresh
     at the right. A notice shortens to its tooltip rather than wrap the header. */
  .row { display: flex; align-items: center; gap: 8px; }
  .notices { flex: 1 1 0; min-width: 0; display: flex; align-items: center; gap: 10px; padding-left: 6px; font-size: 12px; }
  .notices .warn, .notices .error { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .notices .link { flex: none; }

  .searchwrap { position: relative; display: inline-flex; align-items: center; flex: 0 1 340px; min-width: 180px; }
  .searchwrap .icon { position: absolute; left: 9px; width: 14px; height: 14px; fill: none; stroke: var(--fg-muted); stroke-width: 1.5; stroke-linecap: round; pointer-events: none; }
  .search { width: 100%; box-sizing: border-box; height: 28px; padding: 0 28px 0 28px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-size: 12.5px; }
  .search:focus-visible { outline: 2px solid var(--accent-ink); }
  .searchwrap kbd { position: absolute; right: 7px; padding: 0 5px; border: 1px solid var(--border); border-radius: 4px; font-size: 10.5px; line-height: 15px; color: var(--fg-muted); background: var(--bg-elev); pointer-events: none; }

  .iconbtn { all: unset; cursor: pointer; box-sizing: border-box; width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg-muted); }
  .iconbtn svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; }
  .iconbtn:hover, .iconbtn.on { color: var(--fg); border-color: var(--accent-ink); }
  .iconbtn:focus-visible { outline: 2px solid var(--accent-ink); }

  .connect { position: relative; flex: none; }
  .pop { position: absolute; top: 34px; left: 0; z-index: 20; display: flex; align-items: center; gap: 6px; padding: 8px; border-radius: 10px; background: var(--bg-elev); border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border)); box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45); }
  .poptitle { font-size: 11.5px; color: var(--fg-muted); white-space: nowrap; padding-right: 2px; }
  .pop input { box-sizing: border-box; width: 190px; height: 28px; padding: 0 10px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  .pop input:focus-visible { outline: 2px solid var(--accent-ink); }

  .link { all: unset; cursor: pointer; color: var(--accent-ink); text-decoration: underline; }
  .link:hover { filter: brightness(1.15); }
  .link:disabled { opacity: 0.6; cursor: default; text-decoration: none; }
  .link:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 2px; }

  /* The details pane only exists while a row is selected; the table takes the full width otherwise. */
  .main { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr; }
  .main.with-pane { grid-template-columns: 1fr 360px; }
  .warn { color: var(--warn); }
  .error { color: var(--danger); }
  /* Below this the table and a 360 px pane cannot both fit: the table's min-content
     width is 700 px, so the row needs 1060 px of content and the pane was clipped off
     the right edge between 1001 and 1239 px — at 1101 px about 40 % of it, including
     the Join button. It floats over the list now instead of being clipped or hidden,
     which is D-153's open recommendation: at the 960 px minimum a row click used to
     select the row and visibly do nothing (D-189). 1300 is the 240 px rail plus
     those 1060 (D-249); with the 180 px rail it was 1240. */
  @media (max-width: 1300px) {
    .main { position: relative; }
    .main.with-pane { grid-template-columns: minmax(0, 1fr); }
    /* The empty-list message centres in the part of the list the pane leaves visible;
       centred on the whole width, its end was under the pane (D-256). */
    .main.with-pane :global(.no-rows) { right: min(360px, 100%); }
    .main > :global(aside) {
      position: absolute;
      inset: 0 0 0 auto;
      width: min(360px, 100%);
      z-index: 15;
      background: var(--bg-elev);
      border-left: 1px solid var(--border);
      box-shadow: -12px 0 32px rgb(0 0 0 / 0.35);
    }
  }
</style>
