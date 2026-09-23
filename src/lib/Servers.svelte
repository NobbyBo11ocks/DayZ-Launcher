<script lang="ts">
  // Server browser view (docs/06, D-108): one compact header block (a primary
  // filter row with Refresh at the right, a row of quick filters, a thin status
  // line), then the virtualised table and the details pane. Direct connect lives
  // in a small popover next to Refresh.
  import DetailsPane from "./DetailsPane.svelte";
  import FilterBar from "./FilterBar.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { servers } from "./state/servers.svelte";
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { invokeLogged as invoke } from "./log";

  /** Per mount; the backend keeps only the first mark it ever receives. */

  let filterBar = $state<FilterBar | null>(null);
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
      connectOpen = false;
    }
  }

  // The store is started once by App.svelte and never stopped (D-084).


  function onKey(e: KeyboardEvent) {
    // The join dialog is modal; the page behind it must not act on keys (D-184).
    if (servers.joiningId) return;
    const target = e.target as HTMLElement | null;
    const typing = target && (target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA");
    if (e.key === "/" && !typing) {
      e.preventDefault();
      filterBar?.focusSearch();
    } else if (e.key === "Escape" && connectOpen) {
      closeConnect();
    }
  }

  const busy = $derived(!servers.steam?.initialized || servers.steam?.refreshing === true);

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
        ? "No server matches these filters. Clear the search, or widen the filters above."
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
  const status = $derived.by(() => {
    const s = servers.steam;
    const d = servers.done;
    const v = servers.verifySummary;
    const parts: string[] = [];
    void servers.rowsTick;
    parts.push(`${fmt.format(servers.list.length)} of ${fmt.format(servers.rows.size)} shown`);
    if (s?.refreshing) parts.push("refreshing from Steam…");
    else if (servers.dzsaLoading) parts.push("downloading the DZSA list…");
    else if (d) parts.push(`${fmt.format(d.responded)} from ${d.source === "dzsa" ? "DZSA" : d.source === "lan" ? "the LAN" : "Steam"} in ${(d.elapsedMs / 1000).toFixed(0)} s`);
    if (servers.verifying && !v) parts.push("verifying player counts…");
    // A skipped pass is not a result: all zeroes would read as "nothing found" (D-208).
    else if (v?.skipped) parts.push("verification deferred — a pass is already running");
    else if (v) parts.push(`${fmt.format(v.verified)} verified · ${fmt.format(v.inflated + v.unverifiable + v.synthetic)} fake · ${v.offline} offline`);
    if (servers.modScanning) parts.push(`scanning mod lists (${fmt.format(servers.modScan?.total ?? 0)})…`);
    if (servers.filters.mod && servers.unscannedModded > 0) parts.push(`${fmt.format(servers.unscannedModded)} modded servers not scanned yet`);
    if (servers.lastRefresh && !s?.refreshing && !d) parts.push(`cached ${new Date(servers.lastRefresh * 1000).toLocaleTimeString()}`);
    return parts.join(" · ");
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="servers">
  <div class="top">
    <div class="row">
      <FilterBar bind:this={filterBar} part="primary" />
      <div class="actions">
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
        <!-- One button (D-141): the full partition list starts with the populated
             servers, so the rows people care about arrive in about 40 s and the empty
             ones keep filling in behind them. -->
        <button class="btn" onclick={() => servers.refresh(true, true)} disabled={busy} title="Fetch from Steam: servers with players first (about 40 s), then the empty ones (a few minutes)">
          {servers.steam?.refreshing ? "Refreshing…" : "Refresh"}
        </button>
      </div>
    </div>

    <div class="row">
      <FilterBar part="chips" />
    </div>

    <div class="statusline">
      <!-- Deliberately not a live region. `polite` defers, it does not coalesce, and
           this sentence reads `rowsTick`, which bumps every 350 ms - so a refresh had
           a screen reader reciting it about three times a second for forty seconds
           with no way to interrupt. The announcement below carries the outcomes
           instead, and only when they land (D-224). -->
      <span class="status" title={status}>{status}</span>
      <span class="sr-only" role="status" aria-live="polite">{announcement}</span>
      <!-- The scan runs itself after a refresh, but until now there was no way to ask
           for it: a mod filter with unscanned servers was a dead end (D-160). -->
      {#if servers.unscannedModded > 0 && !servers.modScanning}
        <button class="link" onclick={() => void servers.scanMods(false)} title="Read the mod list of every populated modded server that has not been scanned">
          Scan {fmt.format(servers.unscannedModded)} mod list{servers.unscannedModded === 1 ? "" : "s"}
        </button>
      {/if}
      {#if servers.steam && !servers.steam.initialized}
        <span class="warn" title={servers.steam.error ?? ""}>Steam is not running; the launcher connects as soon as it starts.</span>
        <button class="link" onclick={() => servers.loadDzsa()} disabled={servers.dzsaLoading} title="Download the DZSA Launcher's public server list (about 24 MB) instead">
          {servers.dzsaLoading ? "Downloading…" : "Load list from DZSA"}
        </button>
      {/if}
      {#if servers.error}<span class="error">{servers.error}</span>{/if}
    </div>
  </div>

  <div class="main" class:with-pane={servers.selected != null}>
    <ServerTable
      rows={servers.list}
      selectedId={servers.selectedId}
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
  .top { display: flex; flex-direction: column; gap: 6px; padding: 8px 16px 6px; border-bottom: 1px solid var(--border); background: var(--bg-elev); }
  /* The filter block wraps inside itself; the buttons at the right stay on the first line. */
  .row { display: flex; align-items: flex-start; flex-wrap: nowrap; gap: 8px; }
  .row > :global(.filters) { flex: 1 1 0; min-width: 0; }
  .actions { display: flex; align-items: center; gap: 6px; margin-left: auto; flex: none; }


  .iconbtn { all: unset; cursor: pointer; box-sizing: border-box; width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg-muted); }
  .iconbtn svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; }
  .iconbtn:hover, .iconbtn.on { color: var(--fg); border-color: var(--accent-ink); }
  .iconbtn:focus-visible { outline: 2px solid var(--accent-ink); }

  .connect { position: relative; }
  .pop { position: absolute; top: 34px; right: 0; z-index: 20; display: flex; align-items: center; gap: 6px; padding: 8px; border-radius: 10px; background: var(--bg-elev); border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border)); box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45); }
  .poptitle { font-size: 11.5px; color: var(--fg-muted); white-space: nowrap; padding-right: 2px; }
  .pop input { box-sizing: border-box; width: 190px; height: 28px; padding: 0 10px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  .pop input:focus-visible { outline: 2px solid var(--accent-ink); }

  .statusline { display: flex; align-items: center; gap: 10px; min-height: 16px; font-size: 11px; color: var(--fg-muted); }
  .status { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-variant-numeric: tabular-nums; }
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
     select the row and visibly do nothing (D-189). */
  @media (max-width: 1240px) {
    .main { position: relative; }
    .main.with-pane { grid-template-columns: minmax(0, 1fr); }
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
