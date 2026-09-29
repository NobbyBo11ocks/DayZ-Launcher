<script lang="ts">
  import { untrack } from "svelte";
  import { mapLabel } from "./maps";
  // Virtualised server table (docs/05 §5, docs/06 §2): fixed 36 px rows, renders only
  // the viewport plus overscan, reports visible ids for verification, keyboard nav.
  import Flag from "./Flag.svelte";
  import { clock, countryName, isInflated, isUnchecked, isUntrusted, playersShown, queueOf, type ServerRow, versionDiffers } from "./types";
  import { pingUnmeasured, type SortKey } from "./state/servers.svelte";

  let {
    rows,
    selectedId,
    sort,
    localVersion,
    favourites,
    onSelect,
    onSort,
    onVisible,
    onActivate,
    onFavourite,
    friendsOn,
    modsByServer,
    empty,
    filterKey = "",
    inert = false,
    label = "Servers",
    onSearch,
  }: {
    rows: ServerRow[];
    selectedId: string | null;
    sort: { key: SortKey; dir: 1 | -1 };
    localVersion: string | null;
    favourites: Set<string>;
    /** A modal is open, so the grid answers no keys — it can still hold focus
     *  behind one, and ArrowDown + Enter there used to rebuild the join dialog and
     *  discard its plan (D-197). */
    inert?: boolean;
    /** null clears the selection (click on the selected row again, or Escape). */
    onSelect: (id: string | null) => void;
    onSort: (key: SortKey) => void;
    onVisible: (ids: string[]) => void;
    onActivate: (id: string) => void;
    onFavourite: (id: string) => void;
    /** Friend names by server id (D-128): rows get a marker with the names in its tooltip. */
    friendsOn?: Map<string, string[]>;
    /** What to say when there are no rows at all, instead of an empty grid (D-165). */
    empty?: string;
    /** Scanned mod lists by server id (D-146): the Mods column counts them. */
    modsByServer?: Map<string, number[]>;
    /** Changes only when the filters change, so a filter that swaps the rows under an
     *  unchanged viewport still re-arms on-demand verification (D-209). */
    filterKey?: string;
    /** The grid's name: Favourites and LAN use this table too, and every one of them was
     *  announced as "Servers" (D-291). */
    label?: string;
    /** "/" from the list moves to the page's search. Only while the list has focus: a
     *  single-character key that works everywhere cannot be switched off (WCAG 2.1.4,
     *  D-291). */
    onSearch?: () => void;
  } = $props();

  const uid = $props.id();
  const fmt = new Intl.NumberFormat();
  const ROW = 36;
  const OVERSCAN = 8;

  /** What the list says without being asked. A sort or a favourite from the keyboard
   *  changed nothing a screen reader hears: `aria-sort` sits on headers that never take
   *  focus (D-291). Always in the document, so a change is announced (D-224). */
  let note = $state("");
  /** Set by the keys that sort, so a sort from a header click stays quiet. */
  let keyedSort = false;
  $effect(() => {
    const { key, dir } = sort;
    if (!keyedSort) return;
    keyedSort = false;
    note = `Sorted by ${columns.find((c) => c.key === key)?.label ?? key}, ${dir === 1 ? "ascending" : "descending"}.`;
    // The selected row keeps its place on screen: after a sort it could be thousands
    // of rows away, with `aria-activedescendant` naming a row that was not rendered.
    untrack(() => {
      if (selectedId) ensureVisible(selectedId);
    });
  });
  // How many rows a filter or search leaves, a second after it settles: D-250 removed the
  // count from the screen, and nothing was said at all. Keyed on the filters only, so the
  // batches of a refresh stay silent; an empty result is said by the message below.
  let countTimer: ReturnType<typeof setTimeout> | undefined;
  let filtersSeen = false;
  $effect(() => {
    void filterKey;
    if (!filtersSeen) {
      filtersSeen = true;
      return;
    }
    clearTimeout(countTimer);
    countTimer = setTimeout(() => {
      const n = untrack(() => rows.length);
      note = n > 0 ? `${fmt.format(n)} server${n === 1 ? "" : "s"}.` : "";
    }, 1000);
    return () => clearTimeout(countTimer);
  });

  /** A row as a screen reader hears it. Focus stays on the grid and the rows are named
   *  from their content, which began with the star's label, ran the numbers together
   *  with no column names, and left out every tooltip: the verdict, "not verified yet",
   *  the version mismatch, the friends' names and the ping band (D-291). */
  function rowLabel(r: ServerRow, mods: number | undefined): string {
    const untrusted = isUntrusted(r);
    const unchecked = isUnchecked(r);
    const queue = queueOf(r);
    const friends = friendsOn?.get(r.id);
    const mismatch = versionDiffers(r.version, localVersion);
    return [
      r.name,
      mapLabel(r.map),
      `${playersShown(r)} of ${r.maxPlayers} players` +
        (untrusted ? ", count not trusted" : unchecked ? ", not verified yet" : "") +
        (queue ? `, ${queue} in the queue` : ""),
      pingUnmeasured(r) ? "ping not measured" : `ping ${r.pingMs} ms, ${pingWord(r.pingMs)}`,
      mods === undefined ? (r.tags.modded ? "modded, mod list not scanned yet" : "mod list not scanned yet") : mods === 0 ? "no mods" : `${mods} mod${mods === 1 ? "" : "s"}`,
      r.tags.timeMinutes != null ? `time ${clock(r.tags.timeMinutes)}` : "",
      r.version ? `version ${r.version}${mismatch ? `, not your version ${localVersion}` : ""}` : "version unknown",
      r.password ? "password" : "",
      r.tags.firstPersonOnly ? "first person only" : "",
      r.tags.dlc ? "needs DLC" : "",
      friends?.length ? `friends here: ${friends.join(", ")}` : "",
      favourites.has(r.id) ? "favourite" : "",
      r.country ? countryName(r.country) : "",
    ]
      .filter(Boolean)
      .join(", ");
  }

  let body = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let height = $state(600);

  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  const slice = $derived(rows.slice(start, end));

  $effect(() => {
    if (!body) return;
    const el = body;
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height;
      if (h) height = h;
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  let raf = 0;
  function onScroll() {
    if (raf || !body) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      if (body) scrollTop = body.scrollTop;
    });
  }

  // Report visible ids when the viewport window itself changes (scroll, resize,
  // filter/sort) and on a fixed 60 s cadence, skipped while the page is hidden: WebView2
  // says so only when the window is hidden, not when it is minimised (wry keeps the
  // webview visible then, docs/05 §1; row 27). Data updates alone must not retrigger
  // this (D-060).
  let visTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    // Only the viewport window, never the data: `rows.length` changes on most
    // batches during a refresh, so it re-armed the timer before it could fire and
    // on-demand verification fell back to the 60 s cadence below (D-288).
    // The sort belongs in the key. It never changes `rows.length`, so both bounds
    // came out equal, Svelte stopped the propagation and the rows now on screen were
    // not reported — they waited up to 60 s for the tick below. It is already a prop,
    // so this costs no new dependency on the data (D-197).
    // The filters belong in the key for the same reason the sort does: a search term
    // typed at the top of the list changes every row without moving either bound
    // (D-209).
    const key = `${start}:${end}:${sort.key}:${sort.dir}:${filterKey}`;
    void key;
    clearTimeout(visTimer);
    visTimer = setTimeout(() => onVisible(rows.slice(start, end).map((r) => r.id)), 400);
    return () => clearTimeout(visTimer);
  });
  $effect(() => {
    const iv = setInterval(() => {
      if (document.hidden) return;
      onVisible(rows.slice(start, end).map((r) => r.id));
    }, 60_000);
    return () => clearInterval(iv);
  });

  function ensureVisible(id: string) {
    const idx = rows.findIndex((r) => r.id === id);
    if (idx < 0 || !body) return;
    const top = idx * ROW;
    if (top < body.scrollTop) body.scrollTop = top;
    else if (top + ROW > body.scrollTop + height) body.scrollTop = top + ROW - height;
  }

  function onKey(e: KeyboardEvent) {
    // Belt and braces with D-197's focus fix: while a modal is open the grid answers
    // no keys, the same guard `Servers.svelte` already applies to its own handler.
    if (inert) return;
    if (e.key === "Escape") {
      if (selectedId) {
        e.preventDefault();
        onSelect(null);
      }
      return;
    }
    if (e.key === "/" && onSearch) {
      e.preventDefault();
      onSearch();
      return;
    }
    // A bare F only: Ctrl+F and Alt+F used to toggle the favourite too (D-248).
    if ((e.key === "f" || e.key === "F") && !e.ctrlKey && !e.altKey && !e.metaKey) {
      if (selectedId) {
        e.preventDefault();
        const name = rows.find((r) => r.id === selectedId)?.name ?? "The server";
        note = `${name} ${favourites.has(selectedId) ? "removed from" : "added to"} favourites.`;
        onFavourite(selectedId);
      }
      return;
    }
    // Sorting from the keyboard. The column headers are deliberately outside the tab
    // order — seven extra stops in front of a 20 000-row grid is worse — so the grid
    // carries the keys instead: ←/→ move the sort column, Space flips its direction.
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      const i = columns.findIndex((c) => c.key === sort.key);
      const next = columns[(i + (e.key === "ArrowRight" ? 1 : columns.length - 1)) % columns.length];
      if (next) {
        keyedSort = true;
        onSort(next.key);
      }
      return;
    }
    if (e.key === " ") {
      e.preventDefault();
      keyedSort = true;
      onSort(sort.key);
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Enter" && e.key !== "Home" && e.key !== "End" && e.key !== "PageDown" && e.key !== "PageUp") return;
    e.preventDefault();
    if (rows.length === 0) return;
    const idx = selectedId ? rows.findIndex((r) => r.id === selectedId) : -1;
    let next = idx;
    // A page is what fits, less one row for continuity; the scroll box is a
    // descendant of the focused grid, so the browser's own PageDown did nothing (D-248).
    const page = Math.max(1, Math.floor(height / ROW) - 1);
    if (e.key === "ArrowDown") next = Math.min(rows.length - 1, idx + 1);
    if (e.key === "ArrowUp") next = Math.max(0, idx - 1);
    if (e.key === "PageDown") next = Math.min(rows.length - 1, Math.max(0, idx) + page);
    if (e.key === "PageUp") next = Math.max(0, idx - page);
    if (e.key === "Home") next = 0;
    if (e.key === "End") next = rows.length - 1;
    if (e.key === "Enter") {
      if (selectedId) onActivate(selectedId);
      return;
    }
    const id = rows[next]?.id;
    if (id) {
      onSelect(id);
      ensureVisible(id);
    }
  }

  const columns: { key: SortKey; label: string; cls: string }[] = [
    { key: "name", label: "Server", cls: "c-name" },
    { key: "map", label: "Map", cls: "c-map" },
    { key: "mods", label: "Mods", cls: "c-num" },
    { key: "players", label: "Players", cls: "c-num" },
    { key: "ping", label: "Ping", cls: "c-num" },
    { key: "time", label: "Time", cls: "c-time" },
    { key: "version", label: "Version", cls: "c-ver" },
  ];

  const sortMark = (key: SortKey) => (sort.key === key ? (sort.dir === 1 ? " ▲" : " ▼") : "");
  /** "1.29.163709" shows as "1.29" (user request, D-251): the build number said nothing
   *  at a glance and cut the column to "1.29.163…". Only the text is shortened: the
   *  mismatch colour and "My version" compare the full build, and the tooltip and the
   *  details pane still name it. Anything not shaped like a version is shown as it is. */
  const shortVersion = (v: string) => /^\d+\.\d+/.exec(v)?.[0] ?? v;
  const pingClass = (ms: number) => (ms < 60 ? "ok" : ms >= 120 ? "warn" : "");
  /** Ping quality in words: the colour alone said it, which is not available to a
   *  screen reader and not distinguishable to everyone else (D-198). */
  const pingWord = (ms: number) => (ms < 60 ? "good" : ms >= 120 ? "high" : "usable");
  const pingTitle = (ms: number) => `${ms} ms — ${pingWord(ms)}`;
</script>

<!-- Roving selection on the grid: the grid takes focus, arrow keys move the selected row. -->
<!-- `aria-activedescendant` is what makes arrow-key movement audible: focus never
     leaves the container, so without it the selection moved silently (D-224).
     `aria-rowcount` counts the header, which is row 1, so an empty grid has one row: it
     said -1, "unknown", about a list that was known to be empty (D-291). -->
<!-- One element for the host's grid. The table and its empty-state message were two, and
     with the details pane open the message took the pane's column and pushed the pane
     into a second row under the table (D-256). -->
<div class="wrap">
<div
  class="table"
  role="grid"
  aria-rowcount={rows.length + 1}
  aria-label={label}
  aria-describedby={rows.length === 0 && empty ? `${uid}-keys ${uid}-empty` : `${uid}-keys`}
  aria-activedescendant={selectedId ? `row-${selectedId}` : undefined}
  tabindex="0"
  onkeydown={onKey}
  onfocusin={(e) => {
    // Focus lives on the grid (D-224). A click focused the row itself (tabindex -1),
    // and once that row scrolled past the overscan the virtual window removed it:
    // focus fell to <body> and the arrows, Enter and F stopped working (D-240).
    if (e.target !== e.currentTarget) (e.currentTarget as HTMLElement).focus({ preventScroll: true });
  }}
>
  <!-- A column header holding a button: a button cannot take the columnheader role and
       keep being a button (D-291). Still out of the tab order; the grid sorts from the
       keyboard (D-198). -->
  <div class="head" role="row" aria-rowindex="1">
    {#each columns as c (c.key)}
      <div class="th {c.cls}" role="columnheader" aria-sort={sort.key === c.key ? (sort.dir === 1 ? "ascending" : "descending") : "none"}>
        <button tabindex="-1" onclick={() => onSort(c.key)}>{c.label}<span aria-hidden="true">{sortMark(c.key)}</span></button>
      </div>
    {/each}
  </div>



  <div class="body" bind:this={body} onscroll={onScroll} role="rowgroup">
    <!-- `presentation` on both wrappers: they exist to size and offset the window, and
         a generic element between a rowgroup and its rows breaks ownership (D-189). -->
    <div class="spacer" role="presentation" style="height: {rows.length * ROW}px">
      <div class="window" role="presentation" style="transform: translateY({start * ROW}px)">
        {#each slice as r, i (r.id)}
          {@const untrusted = isUntrusted(r)}
          {@const unchecked = isUnchecked(r)}
          {@const mods = modsByServer?.get(r.id)?.length}
          {@const versionOff = versionDiffers(r.version, localVersion)}
          <!-- Keyboard handling belongs to the grid container, not each row: one handler
               there sees every key, while a second on the row made Svelte's delegated
               keydown fire twice (arrows skipped rows, F cancelled itself out, D-151). -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="row"
            class:selected={r.id === selectedId}
            class:untrusted
            role="row"
            tabindex="-1"
            id="row-{r.id}"
            aria-rowindex={start + i + 2}
            aria-selected={r.id === selectedId}
            aria-label={rowLabel(r, mods)}
            onclick={(e) => {
              // A click on the selected row clears the selection and collapses the
              // details pane. The second click of a double-click (detail 2) is ignored
              // so that double-click keeps the row selected and activates it.
              if (e.detail > 1) return;
              onSelect(r.id === selectedId ? null : r.id);
            }}
            ondblclick={() => {
              onSelect(r.id);
              onActivate(r.id);
            }}
          >
            <div role="gridcell" class="cell c-name" title={r.description ? [r.name, r.description].join(String.fromCharCode(10, 10)) : r.name}>
              <button
                class="star"
                class:on={favourites.has(r.id)}
                aria-label="Favourite"
                aria-pressed={favourites.has(r.id)}
                tabindex="-1"
                onclick={(e) => {
                  e.stopPropagation();
                  onFavourite(r.id);
                }}
                ondblclick={(e) => e.stopPropagation()}
                ><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3.7L14.29 9.54L20.56 9.92L15.71 13.91L17.29 19.98L12 16.6L6.71 19.98L8.29 13.91L3.44 9.92L9.71 9.54Z" /></svg></button
              >
              <Flag code={r.country} />
              <span class="flags">
                {#if r.password}<span class="flag" title="Password protected">🔒</span>{/if}
                {#if r.tags.firstPersonOnly}<span class="pill" title="First person only">1PP</span>{/if}
                {#if r.tags.modded}<span class="pill" title="Modded">MOD</span>{/if}
                {#if r.tags.dlc}<span class="pill" title="Requires DLC">DLC</span>{/if}
                {#if friendsOn?.has(r.id)}
                  {@const names = friendsOn.get(r.id) ?? []}
                  <span class="pill friends" title="Friends here: {names.join(', ')}">
                    <svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="5" r="3" /><path d="M2.6 14.4c0-3 2.4-5 5.4-5s5.4 2 5.4 5" /></svg>{names.length}
                  </span>
                {/if}
              </span>
              <span class="name">{r.name}</span>
            </div>
            <!-- The name players use, not the folder A2S reports: Livonia is
                 `enoch` and Chernarus is `chernarusplus` (D-195). The raw id stays
                 in the tooltip, since server owners quote it in their rules. -->
            <div role="gridcell" class="cell c-map" title={r.map}>{mapLabel(r.map)}</div>
            <!-- Mod count from the last scan (D-146); a dash means this server has not
                 been scanned yet, which is not the same as "no mods". -->
            <div role="gridcell" class="cell c-num mods" class:muted={mods === undefined} title={mods === undefined ? "Mod list not scanned yet" : mods === 0 ? "Vanilla" : `${mods} mod${mods === 1 ? "" : "s"}`}>
              {mods ?? "–"}
            </div>
            <div
              role="gridcell"
              class="cell c-num players"
              title={r.clone
                ? "Same name as a server verified on another address; this copy's own check has not verified it"
                : isInflated(r)
                  ? `Steam sees nobody on it; the server claims ${r.players}`
                : r.players > 127
                  ? "Claims more players than any DayZ server this launcher has counted"
                : r.verifiedPlayers == null && (r.bots ?? 0) > 0 && r.bots === r.players
                  ? "Its listing carries the mark of a faked count, and it has never been counted"
                : r.verdict === "inflated"
                  ? `Inflated: the server claims ${r.players}, ${r.verifiedPlayers ?? 0} actually connected`
                  : r.verdict === "unverifiable"
                    ? r.steamEmpty === false
                      ? r.verifiedPlayers != null
                        ? "The server has stopped sharing its player list; showing its last count"
                        : "The server does not share its player list; Steam sees a session, but the number is the server's own, unconfirmed"
                      : "The server claims players but does not share its player list"
                    : r.verdict === "synthetic"
                      ? "The player list looks fake"
                      : r.verdict === "offline"
                        ? "The server is not answering; this is the last number it reported"
                        : r.verdict === "verified"
                          ? "Counted by the launcher"
                          : "The server's own number, not checked yet"}
            >
              <!-- An unchecked count is dimmed and marked "?" (D-288). Steam vouching
                   for a server keeps it in the list, but the number is still the
                   server's own, and showing it like a verified one is the gap a
                   server that answers INFO and firewalls PLAYER relies on. -->
              <span class="txt" class:muted={unchecked && !untrusted}>
                {#if untrusted}<span class="warn">⚠</span>{:else if unchecked}<span class="unchecked">?</span>{/if}
                {playersShown(r)}/{r.maxPlayers}{#if queueOf(r)}<span class="muted"> +{queueOf(r)}</span>{/if}
              </span>
            </div>
            <!-- 0 on a non-LAN row is "not measured" (DZSA list, an unreachable favourite),
                 which used to read as a green "0 ms — good" (D-242). -->
            {#if pingUnmeasured(r)}
              <div role="gridcell" class="cell c-num muted" title="Ping not measured yet">—</div>
            {:else}
              <div role="gridcell" class="cell c-num {pingClass(r.pingMs)}" title={pingTitle(r.pingMs)}>{r.pingMs}</div>
            {/if}
            <div role="gridcell" class="cell c-time">
              {#if r.tags.timeMinutes != null}
                <span class="glyph" aria-hidden="true">{r.tags.timeMinutes >= 6 * 60 && r.tags.timeMinutes < 20 * 60 ? "☀" : "☾"}</span>
              {/if}
              {clock(r.tags.timeMinutes)}
            </div>
            <!-- An unknown version is no difference: it was amber, with an empty "Server
                 runs ;" (row 26, approved). -->
            <div
              role="gridcell"
              class="cell c-ver"
              class:warn={versionOff}
              class:muted={!r.version}
              title={!r.version ? "Version unknown" : versionOff ? `Server runs ${r.version}; your DayZ is ${localVersion}` : `Server version ${r.version}`}
            >{r.version ? shortVersion(r.version) : "unknown"}</div>
          </div>
        {/each}
      </div>
    </div>
  </div>
</div>

<!-- A sibling of the grid, not a child of it: `role="grid"` owns rows and rowgroups
     only, so a bare paragraph inside was liable to be pruned from the accessibility
     tree - and then filtering everything away left a screen-reader user with an empty
     grid and no explanation, which is the failure D-189 set out to fix (D-224). -->
<!-- Always in the document and filled only while the grid is empty: created together
     with its text, a status region is often not announced at all, which is the pattern
     D-224 removed from the toasts (D-291). Empty, it has no box. -->
<p class="no-rows" id="{uid}-empty" role="status">{rows.length === 0 && empty ? empty : ""}</p>
<span class="sr-only" role="status">{note}</span>
<span class="sr-only" id="{uid}-keys">Up and Down Arrow choose a server, Enter joins it, F adds or removes a favourite, Left and Right Arrow change the sort column, Space reverses it{onSearch ? ", slash moves to the search" : ""}, Escape closes the details.</span>
</div>

<style>
  .wrap { position: relative; min-width: 0; min-height: 0; height: 100%; }
  .table { display: flex; flex-direction: column; min-height: 0; height: 100%; font-size: 12.5px; outline: none; }
  .table:focus-visible { box-shadow: inset 0 0 0 2px var(--accent-ink); }
  /* Time was 64 px and "☀ 19:42" needs about that before the sun or moon, which comes from
     whichever font has it, draws wider: it read "19…". It took 16 px from Version, which
     shows "1.29" now (D-251), so the fixed columns are still 490 px and the table's
     700 px floor behind the D-189 and D-249 breakpoints does not move (D-252). */
  .head, .row { display: grid; grid-template-columns: minmax(200px, 1fr) 130px 52px 96px 56px 80px 76px; align-items: center; }
  .head { border-bottom: 1px solid var(--border); background: var(--bg); }
  /* Accent, like the title-bar counts (user request, D-177). */
  /* The cell is the column header and the button inside fills it, so the whole cell
     still sorts on a click and looks as it did when the button was the cell (D-291). */
  .th { display: flex; min-width: 0; }
  .th button { all: unset; box-sizing: border-box; flex: 1; min-width: 0; cursor: pointer; padding: 0 8px; height: 30px; display: flex; align-items: center; color: var(--accent-ink); font-weight: 500; white-space: nowrap; }
  .th button:hover { filter: brightness(1.15); }
  .th button:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: -2px; }
  /* A header is a flex box, so the column's `text-align` never reached it: Mods,
     Players and Ping sat at the left over numbers aligned right (D-256). */
  .th.c-num button { justify-content: flex-end; }
  /* The header is a separate grid from the body, so the body always reserves the
     scrollbar and the header matches it with padding. `scrollbar-gutter` on the header
     did nothing — it only applies to scroll containers — which left every column from
     Map rightwards 10 px out in both states (D-151, corrected in D-159). */
  .body { flex: 1; min-height: 0; overflow-y: auto; overflow-x: hidden; scrollbar-gutter: stable; contain: strict; }
  .head { padding-right: 10px; }
  .row { outline: none; }
  .spacer { position: relative; width: 100%; }
  /* No will-change: the compositor layer cost more GPU memory than the transform saved (Q17, D-056). */
  .window { position: absolute; left: 0; right: 0; top: 0; }
  .row { height: 36px; border-bottom: 1px solid var(--border); cursor: default; }
  /* Under the header, over the empty body: in the wrapper it no longer takes a grid cell. */
  .no-rows { position: absolute; top: 31px; left: 0; right: 0; margin: 28px auto 0; max-width: 46ch; text-align: center; color: var(--fg-muted); font-size: 13px; line-height: 1.5; }
  .row:hover { background: var(--bg-row); }
  /* The tint was 18 %, and on it `--fg-muted`, `.ok` and `.warn` measured 3.41, 3.58
     and 3.43 at worst across all 24 theme × accent combinations — under 4.5 in every
     one of them. Two changes: a lighter tint, and the selected row drops the dim and
     quality colours altogether for full `--fg` (10.02 dark / 12.00 light at worst).
     Nothing is lost — the ping's quality and the version mismatch are in their
     tooltips in words now, and the row is already marked by the bar (D-198). */
  .row.selected { background: color-mix(in srgb, var(--accent) 12%, var(--bg-row)); box-shadow: inset 3px 0 0 var(--accent-ink); }
  .row.selected,
  .row.selected.untrusted,
  .row.selected .cell,
  .row.selected .ok,
  .row.selected .warn,
  .row.selected .muted { color: var(--fg); }
  .row.untrusted { color: var(--fg-muted); }
  .cell { padding: 0 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .c-name { display: flex; align-items: center; gap: 6px; min-width: 0; }
  /* .55 measured 2.68:1 (dark) / 2.29 (light) on the page; .8 gives 3.9 / 3.3 (D-248). */
  .star { all: unset; cursor: pointer; flex: none; display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; border-radius: 6px; color: var(--fg-muted); opacity: 0.8; }
  .star:hover, .star.on { opacity: 1; color: var(--accent-ink); }
  /* Drawn, and a size up (user request, D-254): the ☆ glyph at 13 px came out small and
     thin in whichever symbol font had it. The rail's Favourites star, outlined until the
     server is a favourite and filled after. */
  .star svg { width: 18px; height: 18px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linejoin: round; }
  .star.on svg { fill: currentColor; }
  .flags { display: inline-flex; gap: 4px; flex: none; }
  .flag { font-size: 11px; }
  .pill { font-size: 10px; line-height: 14px; padding: 0 5px; border-radius: 4px; background: var(--bg-row); color: var(--fg-muted); border: 1px solid var(--border); }
  .pill.friends { display: inline-flex; align-items: center; gap: 3px; color: var(--accent-ink); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); background: var(--bg-row); font-weight: 600; }
  .pill.friends svg { width: 10px; height: 10px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .name { overflow: hidden; text-overflow: ellipsis; }
  .c-num { text-align: right; }
  .c-time .glyph { color: var(--fg-muted); margin-right: 3px; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .unchecked { color: var(--fg-muted); font-weight: 600; }
</style>
