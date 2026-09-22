<script lang="ts">
  // Virtualised server table (docs/05 §5, docs/06 §2): fixed 36 px rows, renders only
  // the viewport plus overscan, reports visible ids for verification, keyboard nav.
  import Flag from "./Flag.svelte";
  import { clock, isInflated, isUntrusted, trustedPlayers, type ServerRow } from "./types";
  import type { SortKey } from "./state/servers.svelte";

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
    alerts,
    onAlert,
    friendsOn,
  }: {
    rows: ServerRow[];
    selectedId: string | null;
    sort: { key: SortKey; dir: 1 | -1 };
    localVersion: string | null;
    favourites: Set<string>;
    /** null clears the selection (click on the selected row again, or Escape). */
    onSelect: (id: string | null) => void;
    onSort: (key: SortKey) => void;
    onVisible: (ids: string[]) => void;
    onActivate: (id: string) => void;
    onFavourite: (id: string) => void;
    /** Watched servers and the toggle; only the Favourites view passes them (D-083). */
    alerts?: Set<string>;
    onAlert?: (id: string) => void;
    /** Friend names by server id (D-128): rows get a marker with the names in its tooltip. */
    friendsOn?: Map<string, string[]>;
  } = $props();

  const ROW = 36;
  const OVERSCAN = 8;

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
  // filter/sort) and on a fixed 60 s cadence while the document is visible. Data
  // updates alone must not retrigger this (D-060).
  let visTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const key = `${start}:${end}:${rows.length}`;
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
    if (e.key === "Escape") {
      if (selectedId) {
        e.preventDefault();
        onSelect(null);
      }
      return;
    }
    if (e.key === "f" || e.key === "F") {
      if (selectedId) {
        e.preventDefault();
        onFavourite(selectedId);
      }
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Enter" && e.key !== "Home" && e.key !== "End") return;
    e.preventDefault();
    if (rows.length === 0) return;
    const idx = selectedId ? rows.findIndex((r) => r.id === selectedId) : -1;
    let next = idx;
    if (e.key === "ArrowDown") next = Math.min(rows.length - 1, idx + 1);
    if (e.key === "ArrowUp") next = Math.max(0, idx - 1);
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
    { key: "players", label: "Players", cls: "c-num" },
    { key: "ping", label: "Ping", cls: "c-num" },
    { key: "time", label: "Time", cls: "c-time" },
    { key: "version", label: "Version", cls: "c-ver" },
  ];

  const sortMark = (key: SortKey) => (sort.key === key ? (sort.dir === 1 ? " ▲" : " ▼") : "");
  const pingClass = (ms: number) => (ms < 60 ? "ok" : ms >= 120 ? "warn" : "");
</script>

<!-- Roving selection on the grid: the grid takes focus, arrow keys move the selected row. -->
<div class="table" role="grid" aria-rowcount={rows.length} aria-label="Servers" tabindex="0" onkeydown={onKey}>
  <div class="head" role="row">
    {#each columns as c (c.key)}
      <button class="th {c.cls}" role="columnheader" tabindex="-1" aria-sort={sort.key === c.key ? (sort.dir === 1 ? "ascending" : "descending") : "none"} onclick={() => onSort(c.key)}>
        {c.label}{sortMark(c.key)}
      </button>
    {/each}
  </div>

  <div class="body" bind:this={body} onscroll={onScroll} role="rowgroup">
    <div class="spacer" style="height: {rows.length * ROW}px">
      <div class="window" style="transform: translateY({start * ROW}px)">
        {#each slice as r (r.id)}
          {@const pop = trustedPlayers(r)}
          {@const untrusted = isUntrusted(r)}
          <div
            class="row"
            class:selected={r.id === selectedId}
            class:untrusted
            role="row"
            tabindex="-1"
            aria-selected={r.id === selectedId}
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
            onkeydown={onKey}
          >
            <div class="cell c-name" title={r.description || r.name}>
              <button
                class="star"
                class:on={favourites.has(r.id)}
                aria-label={favourites.has(r.id) ? "Remove from favourites" : "Add to favourites"}
                aria-pressed={favourites.has(r.id)}
                tabindex="-1"
                onclick={(e) => {
                  e.stopPropagation();
                  onFavourite(r.id);
                }}
                ondblclick={(e) => e.stopPropagation()}>{favourites.has(r.id) ? "★" : "☆"}</button
              >
              {#if onAlert && alerts}
                <button
                  class="star bell"
                  class:on={alerts.has(r.id)}
                  aria-label={alerts.has(r.id) ? "Stop watching this server" : "Alert me when a slot frees up or it comes back online"}
                  aria-pressed={alerts.has(r.id)}
                  title={alerts.has(r.id) ? "Watching: stop alerts" : "Alert me when a slot frees up or it comes back online"}
                  tabindex="-1"
                  onclick={(e) => {
                    e.stopPropagation();
                    onAlert(r.id);
                  }}
                  ondblclick={(e) => e.stopPropagation()}>{alerts.has(r.id) ? "🔔" : "🔕"}</button
                >
              {/if}
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
            <div class="cell c-map">{r.map}</div>
            <div
              class="cell c-num players"
              title={isInflated(r)
                ? `Steam reports 0 authenticated players; the server claims ${r.players}`
                : r.verdict === "inflated"
                  ? `Inflated: server claims ${r.players}, ${r.verifiedPlayers ?? 0} actually connected`
                  : r.verdict === "unverifiable"
                    ? r.steamEmpty === false
                      ? "Server does not answer player queries; Steam confirms it has authenticated players (count is the server's own)"
                      : "Server refuses player queries while claiming players"
                    : r.verdict === "synthetic"
                      ? "Player list looks fabricated"
                      : r.verdict === "verified"
                        ? `Verified head-count (${clock(r.tags.timeMinutes)} in game)`
                        : "Reported by the server, not yet verified"}
            >
              <span class="txt" class:muted={r.verdict == null && !untrusted}>
                {#if untrusted}<span class="warn">⚠</span>{/if}
                {pop}/{r.maxPlayers}{#if r.tags.queue}<span class="muted"> +{r.tags.queue}</span>{/if}
              </span>
            </div>
            <div class="cell c-num {pingClass(r.pingMs)}">{r.pingMs}</div>
            <div class="cell c-time">
              {#if r.tags.timeMinutes != null}
                <span class="glyph" aria-hidden="true">{r.tags.timeMinutes >= 6 * 60 && r.tags.timeMinutes < 20 * 60 ? "☀" : "☾"}</span>
              {/if}
              {clock(r.tags.timeMinutes)}
            </div>
            <div class="cell c-ver" class:warn={localVersion != null && r.version !== localVersion}>{r.version}</div>
          </div>
        {/each}
      </div>
    </div>
  </div>
</div>

<style>
  .table { display: flex; flex-direction: column; min-height: 0; height: 100%; font-size: 12.5px; outline: none; }
  .table:focus-visible { box-shadow: inset 0 0 0 2px var(--accent); }
  .head, .row { display: grid; grid-template-columns: minmax(200px, 1fr) 130px 96px 56px 64px 92px; align-items: center; }
  .head { border-bottom: 1px solid var(--border); background: var(--bg); }
  .th { all: unset; cursor: pointer; padding: 0 8px; height: 30px; display: flex; align-items: center; color: var(--fg-muted); font-weight: 500; white-space: nowrap; }
  .th:hover { color: var(--fg); }
  .th:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .body { flex: 1; min-height: 0; overflow-y: auto; overflow-x: hidden; contain: strict; }
  .row { outline: none; }
  .spacer { position: relative; width: 100%; }
  /* No will-change: the compositor layer cost more GPU memory than the transform saved (Q17, D-056). */
  .window { position: absolute; left: 0; right: 0; top: 0; }
  .row { height: 36px; border-bottom: 1px solid var(--border); cursor: default; }
  .row:hover { background: var(--bg-row); }
  .row.selected { background: color-mix(in srgb, var(--accent) 18%, var(--bg-row)); box-shadow: inset 3px 0 0 var(--accent); }
  .row.untrusted { color: var(--fg-muted); }
  .cell { padding: 0 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .c-name { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .star { all: unset; cursor: pointer; flex: none; width: 18px; text-align: center; color: var(--fg-muted); opacity: 0.55; font-size: 13px; }
  .star:hover, .star.on { opacity: 1; color: var(--accent); }
  .bell { font-size: 11px; }
  .flags { display: inline-flex; gap: 4px; flex: none; }
  .flag { font-size: 11px; }
  .pill { font-size: 10px; line-height: 14px; padding: 0 5px; border-radius: 4px; background: var(--bg-row); color: var(--fg-muted); border: 1px solid var(--border); }
  .pill.friends { display: inline-flex; align-items: center; gap: 3px; color: var(--accent); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); background: color-mix(in srgb, var(--accent) 14%, var(--bg-row)); font-weight: 600; }
  .pill.friends svg { width: 10px; height: 10px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .name { overflow: hidden; text-overflow: ellipsis; }
  .c-num { text-align: right; }
  .c-time .glyph { color: var(--fg-muted); margin-right: 3px; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
</style>
