<script lang="ts">
  // Favourites view: same table and details pane, favourite rows only, plus import.
  import DetailsPane from "./DetailsPane.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { servers } from "./state/servers.svelte";

  import { untrack } from "svelte";
  import { searchBox } from "./search";

  // The shared search filter, written on a pause rather than on every keystroke
  // (D-222). `typed` keeps the field responsive while the filter lags behind it.
  const box = searchBox();
  let typed = $state(servers.filters.search);
  $effect(() => box.dispose);
  // A reset from elsewhere has to show up in the field and cancel a pending write
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
  import type { ImportResult } from "./types";

  let importing = $state(false);
  let imported = $state<ImportResult | null>(null);
  /** The pane follows the selection only while it is one of these rows — and so do the
   *  grid's highlight and keys: given the global one, Enter and F acted on a server from
   *  the Servers page this grid does not show (D-240). */
  const selected = $derived(servers.favouriteRows.find((r) => r.id === servers.selectedId) ?? null);

  async function importOfficial() {
    importing = true;
    imported = await servers.importOfficial();
    importing = false;
  }
</script>

<div class="favs">
  <div class="top">
    <div class="bar">
      <input class="search" type="search" placeholder="Search favourites…" value={typed} oninput={(e) => (typed = e.currentTarget.value, box.set(typed))} aria-label="Search favourites" />
      <button class="btn secondary" onclick={importOfficial} disabled={importing} title="Reads %LOCALAPPDATA%\DayZ Launcher\FavouriteServers.xml">
        {importing ? "Importing…" : "Import from the official launcher"}
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
    <div class="main" class:with-pane={selected != null}>
      <ServerTable
        rows={servers.favouriteRows}
        selectedId={selected?.id ?? null}
        sort={servers.sort}
        localVersion={servers.localVersion}
        favourites={servers.favourites}
        onSelect={(id) => servers.select(id)}
        onSort={(k) => servers.setSort(k)}
        filterKey={servers.filterKey}
        inert={servers.joiningId !== null}
      onVisible={(ids) => servers.verifyVisible(ids)}
        onActivate={(id) => (servers.joiningId = id)}
        onFavourite={(id) => servers.toggleFavourite(id)}
        friendsOn={servers.friendsOn}
        modsByServer={servers.modsByServer}
        empty={servers.filters.search.trim()
          ? `No favourite matches "${servers.filters.search.trim()}". The search box is shared with the Servers page.`
          : "These favourites are not in the list yet. They appear after the next refresh, or once Steam answers."}
      />
      {#if selected}
        <DetailsPane row={selected} localVersion={servers.localVersion} />
      {/if}
    </div>
  {/if}
</div>

<style>
  .favs { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .top { padding: 8px 16px; border-bottom: 1px solid var(--border); }
  .bar { display: flex; align-items: center; gap: 10px; font-size: 12px; }
  .search { flex: 0 0 260px; padding: 6px 10px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); }
  .search:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 2px; }
  .empty { margin: auto; text-align: center; max-width: 520px; }
  .empty p { margin: 4px 0; }
  kbd { padding: 1px 5px; border: 1px solid var(--border); border-radius: 4px; background: var(--bg-row); font-size: 11px; }
  /* The details pane only exists while a row is selected; the table takes the full width otherwise. */
  .main { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr; }
  .main.with-pane { grid-template-columns: 1fr 360px; }
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
