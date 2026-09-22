<script lang="ts">
  // Server browser view (docs/06, D-108): one compact header block (a primary
  // filter row with Refresh at the right, a row of quick filters, a thin status
  // line), then the virtualised table and the details pane. Direct connect lives
  // in a small popover next to Refresh.
  import DetailsPane from "./DetailsPane.svelte";
  import FilterBar from "./FilterBar.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { servers } from "./state/servers.svelte";
  import { invoke } from "@tauri-apps/api/core";

  /** Per mount; the backend keeps only the first mark it ever receives. */
  let firstPaintMarked = false;

  let filterBar = $state<FilterBar | null>(null);
  let direct = $state("");
  let connecting = $state(false);
  let connectOpen = $state(false);
  let connectInput = $state<HTMLInputElement | null>(null);
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

  // Start-up timing for Diagnostics (D-078): the first frame that shows rows.
  $effect(() => {
    if (firstPaintMarked || servers.list.length === 0) return;
    firstPaintMarked = true;
    requestAnimationFrame(() => void invoke("perf_first_paint").catch(() => {}));
  });

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    const typing = target && (target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA");
    if (e.key === "/" && !typing) {
      e.preventDefault();
      filterBar?.focusSearch();
    } else if (e.key === "Escape" && connectOpen) {
      connectOpen = false;
    }
  }

  const busy = $derived(!servers.steam?.initialized || servers.steam?.refreshing === true);

  const status = $derived.by(() => {
    const s = servers.steam;
    const d = servers.done;
    const v = servers.verifySummary;
    const parts: string[] = [];
    parts.push(`${fmt.format(servers.list.length)} of ${fmt.format(servers.rows.size)} shown`);
    if (s?.refreshing) parts.push("refreshing from Steam…");
    else if (servers.dzsaLoading) parts.push("downloading the DZSA list…");
    else if (d) parts.push(`${fmt.format(d.responded)} from ${d.source === "dzsa" ? "DZSA" : d.source === "lan" ? "the LAN" : "Steam"} in ${(d.elapsedMs / 1000).toFixed(0)} s`);
    if (servers.verifying && !v) parts.push("verifying player counts…");
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
          <button class="iconbtn" class:on={connectOpen} onclick={toggleConnect} aria-expanded={connectOpen} aria-label="Direct connect" title="Direct connect to an address">
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
        <button class="btn" onclick={() => servers.refresh(true, false)} disabled={busy} title="Servers with players (about 40 s)">
          {servers.steam?.refreshing ? "Refreshing…" : "Refresh"}
        </button>
        <button class="btn secondary" onclick={() => servers.refresh(true, true)} disabled={busy} title="Also fetch empty servers, map by map (several minutes)">Full</button>
      </div>
    </div>

    <div class="row">
      <FilterBar part="chips" />
    </div>

    <div class="statusline">
      <span class="status">{status}</span>
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
      onVisible={(ids) => servers.verifyVisible(ids)}
      onActivate={(id) => (servers.joiningId = id)}
      onFavourite={(id) => servers.toggleFavourite(id)}
      friendsOn={servers.friendsOn}
    />
    {#if servers.selected}
      <DetailsPane row={servers.selected} localVersion={servers.localVersion} />
    {/if}
  </div>
</div>

<style>
  .servers { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .top { display: flex; flex-direction: column; gap: 6px; padding: 8px 16px 6px; border-bottom: 1px solid var(--border); background: var(--bg-elev); }
  /* The filter block wraps inside itself; the buttons at the right stay on the first line. */
  .row { display: flex; align-items: flex-start; flex-wrap: nowrap; gap: 8px; }
  .row > :global(.filters) { flex: 1 1 0; min-width: 0; }
  .actions { display: flex; align-items: center; gap: 6px; margin-left: auto; flex: none; }

  .btn { all: unset; cursor: pointer; box-sizing: border-box; height: 28px; padding: 0 14px; display: inline-flex; align-items: center; border-radius: var(--radius); background: var(--accent); color: var(--accent-fg); font-weight: 600; font-size: 12.5px; white-space: nowrap; }
  .btn.secondary { background: var(--bg-row); color: var(--fg); border: 1px solid var(--border); font-weight: 500; }
  .btn.secondary:hover { border-color: var(--accent); }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn:focus-visible { outline: 2px solid var(--fg); }

  .iconbtn { all: unset; cursor: pointer; box-sizing: border-box; width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg-muted); }
  .iconbtn svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; }
  .iconbtn:hover, .iconbtn.on { color: var(--fg); border-color: var(--accent); }
  .iconbtn:focus-visible { outline: 2px solid var(--accent); }

  .connect { position: relative; }
  .pop { position: absolute; top: 34px; right: 0; z-index: 20; display: flex; align-items: center; gap: 6px; padding: 8px; border-radius: 10px; background: var(--bg-elev); border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border)); box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45); }
  .poptitle { font-size: 11.5px; color: var(--fg-muted); white-space: nowrap; padding-right: 2px; }
  .pop input { box-sizing: border-box; width: 190px; height: 28px; padding: 0 10px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  .pop input:focus-visible { outline: 2px solid var(--accent); }

  .statusline { display: flex; align-items: center; gap: 10px; min-height: 16px; font-size: 11px; color: var(--fg-muted); }
  .status { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-variant-numeric: tabular-nums; }
  .link { all: unset; cursor: pointer; color: var(--accent); }
  .link:hover { text-decoration: underline; }
  .link:disabled { opacity: 0.6; cursor: default; }

  /* The details pane only exists while a row is selected; the table takes the full width otherwise. */
  .main { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr; }
  .main.with-pane { grid-template-columns: 1fr 360px; }
  .warn { color: var(--warn); }
  .error { color: var(--danger); }
  @media (max-width: 1000px) {
    .main.with-pane { grid-template-columns: 1fr; }
    .main > :global(aside) { display: none; }
  }
</style>
