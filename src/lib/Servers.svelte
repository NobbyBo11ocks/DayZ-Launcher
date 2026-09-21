<script lang="ts">
  // Server browser view: filter bar, virtualised table, details pane (docs/06).
  import DetailsPane from "./DetailsPane.svelte";
  import FilterBar from "./FilterBar.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { servers } from "./state/servers.svelte";

  let filterBar = $state<FilterBar | null>(null);
  let direct = $state("");
  let connecting = $state(false);

  async function connectDirect() {
    const address = direct.trim();
    if (!address) return;
    connecting = true;
    const row = await servers.directConnect(address);
    connecting = false;
    if (row) direct = "";
  }

  $effect(() => {
    void servers.start();
    return () => servers.stop();
  });

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    const typing = target && (target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA");
    if (e.key === "/" && !typing) {
      e.preventDefault();
      filterBar?.focusSearch();
    }
  }

  const status = $derived.by(() => {
    const s = servers.steam;
    const d = servers.done;
    const v = servers.verifySummary;
    const parts: string[] = [];
    parts.push(`${servers.list.length} shown of ${servers.rows.size}`);
    if (s?.refreshing) parts.push("refreshing from Steam…");
    else if (d) parts.push(`${d.responded} servers from Steam in ${(d.elapsedMs / 1000).toFixed(0)} s`);
    if (servers.verifying && !v) parts.push("verifying player counts…");
    else if (v) parts.push(`${v.verified} verified · ${v.inflated + v.unverifiable + v.synthetic} fake · ${v.offline} offline`);
    if (servers.lastRefresh && !s?.refreshing && !d) parts.push(`cached ${new Date(servers.lastRefresh * 1000).toLocaleTimeString()}`);
    return parts.join(" · ");
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="servers">
  <div class="top">
    <FilterBar bind:this={filterBar} />
    <div class="bar">
      <button class="btn" onclick={() => servers.refresh(true, false)} disabled={!servers.steam?.initialized || servers.steam?.refreshing} title="Servers with players (~40 s)">
        {servers.steam?.refreshing ? "Refreshing…" : "Refresh"}
      </button>
      <button class="btn secondary" onclick={() => servers.refresh(true, true)} disabled={!servers.steam?.initialized || servers.steam?.refreshing} title="Also fetch empty servers per map (several minutes)">
        Full refresh
      </button>
      <form class="direct" onsubmit={(e) => { e.preventDefault(); void connectDirect(); }}>
        <input type="text" placeholder="Direct connect: ip:port" bind:value={direct} aria-label="Direct connect address" spellcheck="false" />
        <button class="btn secondary" type="submit" disabled={connecting || !direct.trim()}>{connecting ? "…" : "Add"}</button>
      </form>
      <span class="muted status">{status}</span>
      {#if servers.steam && !servers.steam.initialized}
        <span class="warn">Steam unavailable: {servers.steam.error ?? "not initialised"}. Cached data only.</span>
      {/if}
      {#if servers.error}<span class="error">{servers.error}</span>{/if}
    </div>
  </div>

  <div class="main">
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
    />
    <DetailsPane row={servers.selected} localVersion={servers.localVersion} />
  </div>
</div>

<style>
  .servers { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .top { padding: 0 16px; border-bottom: 1px solid var(--border); }
  .bar { display: flex; align-items: center; gap: 10px; padding: 0 0 8px; font-size: 12px; }
  .status { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .btn { all: unset; cursor: pointer; padding: 6px 12px; border-radius: var(--radius); background: var(--accent); color: #111; font-weight: 600; }
  .btn.secondary { background: var(--bg-row); color: var(--fg); border: 1px solid var(--border); font-weight: 500; }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn:focus-visible { outline: 2px solid var(--fg); }
  .direct { display: inline-flex; gap: 4px; margin-left: 8px; }
  .direct input { width: 190px; padding: 5px 8px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  .direct input:focus-visible { outline: 2px solid var(--accent); }
  .main { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr 360px; }
  .warn { color: var(--warn); }
  .error { color: var(--danger); }
  @media (max-width: 1000px) {
    .main { grid-template-columns: 1fr; }
    .main > :global(aside) { display: none; }
  }
</style>
