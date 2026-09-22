<script lang="ts">
  // LAN tab (D-087): rows with a local-network address, filled by Steam's LAN
  // discovery. Same table and details pane as the other list views.
  import DetailsPane from "./DetailsPane.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { servers } from "./state/servers.svelte";

  let scanning = $state(false);
  let scannedAt = $state("");

  async function scan() {
    scanning = true;
    const started = await servers.refreshLan();
    scanning = false;
    if (started) scannedAt = new Date().toLocaleTimeString();
  }

  const busy = $derived(scanning || servers.steam?.refreshing === true);
  const lastScan = $derived(servers.done?.source === "lan" ? servers.done : null);
  const selected = $derived(servers.lanRows.find((r) => r.id === servers.selectedId) ?? null);
</script>

<div class="lan">
  <div class="top">
    <div class="bar">
      <button class="btn" onclick={scan} disabled={!servers.steam?.initialized || busy} title="Ask Steam's LAN discovery for DayZ servers on your network">
        {busy ? "Scanning…" : "Scan LAN"}
      </button>
      <input class="search" type="search" placeholder="Search…" bind:value={servers.filters.search} aria-label="Search LAN servers" />
      <span class="muted">
        {servers.lanRows.length} server{servers.lanRows.length === 1 ? "" : "s"} on your network
        {#if lastScan}· scan answered {lastScan.responded} in {(lastScan.elapsedMs / 1000).toFixed(1)} s{#if scannedAt} at {scannedAt}{/if}{/if}
      </span>
      {#if servers.steam && !servers.steam.initialized}<span class="warn">Steam is not running; LAN discovery needs it and works once it is up.</span>{/if}
      {#if servers.error}<span class="error">{servers.error}</span>{/if}
    </div>
  </div>
  {#if servers.lanRows.length === 0}
    <div class="empty">
      <p>No LAN servers yet.</p>
      <p class="muted">Scan LAN asks Steam for DayZ servers on this network: a server on this PC or behind the same router. Servers found stay in this list until the cache expires. For a known address elsewhere, use Direct connect in Servers.</p>
    </div>
  {:else}
    <div class="main" class:with-pane={selected != null}>
      <ServerTable
        rows={servers.lanRows}
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
        modsByServer={servers.modsByServer}
      />
      {#if selected}
        <DetailsPane row={selected} localVersion={servers.localVersion} />
      {/if}
    </div>
  {/if}
</div>

<style>
  .lan { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .top { padding: 8px 16px; border-bottom: 1px solid var(--border); }
  .bar { display: flex; align-items: center; gap: 10px; font-size: 12px; }
  .search { flex: 0 0 220px; padding: 6px 10px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); }
  .search:focus-visible { outline: 2px solid var(--accent); }
  .empty { margin: auto; text-align: center; max-width: 520px; }
  .empty p { margin: 4px 0; }
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
