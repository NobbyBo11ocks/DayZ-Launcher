<script lang="ts">
  // Favourites view: same table and details pane, favourite rows only, plus import.
  import DetailsPane from "./DetailsPane.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { servers } from "./state/servers.svelte";
  import type { ImportResult } from "./types";

  let importing = $state(false);
  let imported = $state<ImportResult | null>(null);

  async function importOfficial() {
    importing = true;
    imported = await servers.importOfficial();
    importing = false;
  }
</script>

<div class="favs">
  <div class="top">
    <div class="bar">
      <input class="search" type="search" placeholder="Search favourites…" bind:value={servers.filters.search} aria-label="Search favourites" />
      <button class="btn secondary" onclick={importOfficial} disabled={importing} title="Reads %LOCALAPPDATA%\DayZ Launcher\FavouriteServers.xml">
        {importing ? "Importing…" : "Import from DayZ Launcher"}
      </button>
      <span class="muted">
        {servers.favouriteRows.length} favourite{servers.favouriteRows.length === 1 ? "" : "s"}
        {#if imported}· imported {imported.imported}, {imported.already} already there{#if imported.unreachable}, {imported.unreachable} offline right now{/if}{/if}
      </span>
      {#if servers.error}<span class="error">{servers.error}</span>{/if}
    </div>
  </div>
  {#if servers.favourites.size === 0}
    <div class="empty">
      <p>No favourites yet.</p>
      <p class="muted">Press <kbd>F</kbd> on a server or click its star. The official launcher's favourites can be imported with the button above.</p>
    </div>
  {:else}
    <div class="main" class:with-pane={servers.selected != null}>
      <ServerTable
        rows={servers.favouriteRows}
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
      {#if servers.selected}
        <DetailsPane row={servers.selected} localVersion={servers.localVersion} />
      {/if}
    </div>
  {/if}
</div>

<style>
  .favs { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .top { padding: 8px 16px; border-bottom: 1px solid var(--border); }
  .bar { display: flex; align-items: center; gap: 10px; font-size: 12px; }
  .search { flex: 0 0 260px; padding: 6px 10px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); }
  .btn { all: unset; cursor: pointer; padding: 6px 12px; border-radius: var(--radius); background: var(--bg-row); color: var(--fg); border: 1px solid var(--border); }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn:focus-visible { outline: 2px solid var(--accent); }
  .empty { margin: auto; text-align: center; }
  .empty p { margin: 4px 0; }
  kbd { padding: 1px 5px; border: 1px solid var(--border); border-radius: 4px; background: var(--bg-row); font-size: 11px; }
  /* The details pane only exists while a row is selected; the table takes the full width otherwise. */
  .main { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr; }
  .main.with-pane { grid-template-columns: 1fr 360px; }
  .error { color: var(--danger); }
  @media (max-width: 1000px) {
    .main.with-pane { grid-template-columns: 1fr; }
    .main > :global(aside) { display: none; }
  }
</style>
