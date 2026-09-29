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
  // The Mods column reads the stored mod lists, which start-up no longer loads (D-284):
  // once the list is in, not beside it, where it held the cache lock for 85–99 ms at
  // the same moment (row 23).
  $effect(() => {
    if (servers.listLoaded && !servers.modsIndexLoaded) void servers.loadModsIndex();
  });
  // Favourites that could not be read at start are asked for again here (row 23).
  $effect(() => {
    untrack(() => void servers.retryFavourites());
  });
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
  import { NOTHING_TO_IMPORT, type ImportResult } from "./types";

  let importing = $state(false);
  let imported = $state<ImportResult | null>(null);
  /** The import's outcome for screen readers: the visible line also carries the count,
   *  which changes with every star (D-291). */
  let importNote = $state("");
  let searchEl = $state<HTMLInputElement | null>(null);
  let emptyEl = $state<HTMLElement | null>(null);
  /** The pane follows the selection only while it is one of these rows — and so do the
   *  grid's highlight and keys: given the global one, Enter and F acted on a server from
   *  the Servers page this grid does not show (D-240). */
  const selected = $derived(servers.favouriteRows.find((r) => r.id === servers.selectedId) ?? null);

  /** Why the last import failed, shown on this page only (row 14, F7). */
  let importError = $state<string | null>(null);

  async function importOfficial() {
    importing = true;
    const r = await servers.importOfficial();
    importing = false;
    if (typeof r === "string") {
      importError = importNote = r;
      return;
    }
    importError = null;
    imported = r;
    importNote = r.missing
      ? NOTHING_TO_IMPORT
      : `Imported ${r.imported}, ${r.already} already there${r.unreachable ? `, ${r.unreachable} offline right now` : ""}${r.skipped ? `, ${r.skipped} skipped with no usable address` : ""}.`;
  }

  // F on the last favourite takes the grid, and the focus in it, away with it: focus
  // goes to the message that replaces it rather than to nowhere (D-291).
  $effect(() => {
    if (emptyEl && document.activeElement === document.body) emptyEl.focus();
  });

  /** Escape closes the pane from anywhere on the page, as it does on Servers: the
   *  pane's own close button says "Close (Esc)" (D-247, D-291). */
  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || servers.joiningId || !selected) return;
    const t = e.target;
    if (t instanceof HTMLInputElement || t instanceof HTMLSelectElement || t instanceof HTMLTextAreaElement) return;
    // Handled, so the notices leave it alone (row 27).
    e.preventDefault();
    servers.select(null);
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="favs">
  <h1 class="sr-only">Favourites</h1>
  <span class="sr-only" role="status">{importNote}</span>
  <div class="top">
    <div class="bar">
      <input class="search" type="search" placeholder="Search favourites…" value={typed} bind:this={searchEl} oninput={(e) => (typed = e.currentTarget.value, box.set(typed))} aria-label="Search favourites" />
      <button class="btn secondary" onclick={importOfficial} disabled={importing} title="Reads %LOCALAPPDATA%\DayZ Launcher\FavouriteServers.xml">
        {importing ? "Importing…" : "Import from the official launcher"}
      </button>
      <span class="muted">
        {servers.favouriteRows.length}{#if servers.favouriteRows.length !== servers.favouriteTotal}{" "}of {servers.favouriteTotal}{/if} favourite{servers.favouriteTotal === 1 ? "" : "s"}
        {#if imported?.missing}· {NOTHING_TO_IMPORT}{:else if imported}· imported {imported.imported}, {imported.already} already there{#if imported.unreachable}, {imported.unreachable} offline right now{/if}{#if imported.skipped}, {imported.skipped} skipped with no usable address{/if}{/if}
      </span>
      <!-- Some favourites unread while others show: the page said nothing (row 23, approved). -->
      {#if servers.favourites.size > 0 && (servers.cache?.inMemory || servers.favouritesUnread)}<span class="warn">Your favourites could not be read this session.</span>{/if}
      {#if importError}<span class="error" role="alert">{importError}</span>{/if}
      {#if servers.error}<span class="error" role="alert">{servers.error}</span>{/if}
    </div>
  </div>
  {#if servers.favourites.size === 0 && !(servers.favouritesLoaded || servers.favouritesUnread || servers.cache?.inMemory)}
    <!-- Not read yet: nothing to say "none" about, and no focus to move (row 23). -->
    <div class="empty"></div>
  {:else if servers.favourites.size === 0}
    <div class="empty">
      <!-- A cache the host could not use this session, or a read that failed at start, is
           not "none yet" (row 14, F14, approved). -->
      <p tabindex="-1" bind:this={emptyEl}>{servers.cache?.inMemory || servers.favouritesUnread ? "Your favourites could not be read this session." : "No favourites yet."}</p>
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
        onActivate={(id) => servers.requestJoin(id)}
        onFavourite={(id) => servers.toggleFavourite(id)}
        friendsOn={servers.friendsOn}
        modsByServer={servers.modsByServer}
        empty={servers.filters.search.trim()
          ? `No favourite matches “${servers.filters.search.trim()}”. The search box is shared with the Servers page.`
          : "These favourites are not in the list yet. They appear after the next refresh, or once Steam answers."}
        label="Favourites"
        onSearch={() => {
          searchEl?.focus();
          searchEl?.select();
        }}
      />
      {#if selected}
        <DetailsPane row={selected} localVersion={servers.localVersion} shows={(id) => servers.favouriteRows.some((r) => r.id === id)} />
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
  .empty p:focus { outline: none; }
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
