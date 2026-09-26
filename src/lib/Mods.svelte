<script lang="ts">
  // Mod manager (D-142): the installed Workshop items and their !Workshop junctions
  // (docs/02 §3–4) with search, filters, multi-select and bulk update or unsubscribe
  // on top of the single-item management from D-075. Fits the viewport; only the
  // table scrolls (D-094).
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { invokeLogged as invoke } from "./log";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { SvelteSet } from "svelte/reactivity";
  import { external } from "./external";
  import { modUpdates } from "./state/mods.svelte";
  import { servers } from "./state/servers.svelte";
  import { fmtBytes, type Diagnostics, type JunctionCleanup, type JunctionInfo, type SyncDone, type SyncProgress, type UnsubscribeResult, type WorkshopItemInfo } from "./types";

  /** Jump to the server list filtered to servers running this mod (D-080). */
  function showServers(id: number) {
    servers.filters.mod = id;
    servers.saveFilters();
    servers.navigate = "servers";
  }

  let data = $state<Diagnostics | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let busyIds = $state<Set<number>>(new Set());
  let updating = $state<SyncProgress | null>(null);
  // The page's own update job id lives in the store: a per-page variable was 0 again
  // after leaving the page and coming back, so its own update read as "Downloading for
  // a join…" (D-276).
  /** The download in flight was started elsewhere (the join dialog), not by this page. */
  let fromJoin = $state(false);
  let search = $state("");
  /** "one" confirms a single unsubscribe, "bulk" the selection. */
  let confirming = $state<{ kind: "one"; id: number; name: string } | { kind: "bulk" } | null>(null);
  const selected = new SvelteSet<number>();

  type SortKey = "name" | "size" | "updated" | "servers";
  let sort = $state<{ key: SortKey; dir: 1 | -1 }>({ key: "size", dir: -1 });
  type View = "all" | "updates" | "unused" | "nojunction";
  let view = $state<View>("all");

  async function load() {
    // The server counts come from the server list's stored mod lists; a failed first
    // read left every mod at 0 for the rest of the run (D-277).
    if (!servers.modsIndexLoaded) void servers.loadModsIndex();
    try {
      data = await invoke<Diagnostics>("diagnostics");
      error = null;
      // The `.acf` is only as fresh as the last time Steam checked, so the live
      // answer is folded in here too — same source the sidebar badge counts (D-191).
      void modUpdates.check(data);
    } catch (e) {
      error = String(e);
    }
  }

  // Dangling-junction clean-up (D-093), which used to live on the Diagnostics page
  // and moved here with it (D-170). Junctions are shared with the official launcher,
  // so this is confirmed, and it only ever removes entries whose target folder is
  // gone — never one the launcher did not create and never a live one.
  let confirmClean = $state(false);
  let cleaning = $state(false);
  async function cleanJunctions() {
    confirmClean = false;
    cleaning = true;
    try {
      const r = await invoke<JunctionCleanup>("junctions_remove_dangling");
      const failed = r.failed.map((f) => `${f.name} (${f.error})`).join(", ");
      notice = `Removed ${r.removed.length} stale junction${r.removed.length === 1 ? "" : "s"}${failed ? `; could not remove ${failed}` : ""}.`;
      error = failed ? `Could not remove ${failed}` : null;
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      cleaning = false;
    }
  }

  $effect(() => {
    void load();
    // Every argument is awaited before `push` runs, so the array stayed empty until
    // all of them resolved and an early unmount unsubscribed nothing. Keep the
    // promises instead (D-222).
    const pending: Promise<UnlistenFn>[] = [];
    {
      pending.push(
        // Any job, not just this page's: a download started from the join dialog is
        // the one the user is most likely to come here to watch (D-184).
        listen<SyncProgress>("mods:progress", (ev) => {
          updating = ev.payload;
          fromJoin = ev.payload.job !== modUpdates.pageJob;
        }),
        listen<SyncDone>("mods:done", (ev) => {
          updating = null;
          fromJoin = false;
          const d = ev.payload;
          if (d.superseded) {
            // Not a failure: a newer download took over (D-277).
            notice = "Replaced by a newer download.";
            error = null;
          } else {
            // An update leaves out mods no longer subscribed (D-276), so it can finish
            // with nothing downloaded; "Downloaded 0 mods." said otherwise.
            const n = d.items.length;
            notice = d.ok && n ? `Downloaded ${n} mod${n === 1 ? "" : "s"}.` : null;
            error = d.ok ? null : failureText(d);
          }
          void load();
        }),
      );
    }
    return () => pending.forEach((p) => void p.then((u) => u()));
  });

  /** The `.acf`'s `needsUpdate` corrected by Steam's live, subscription-aware
   *  answer, so this page, the join dialog and the sidebar badge all agree (D-191). */
  const all = $derived(
    (data?.workshop?.items ?? []).map((i) => {
      const stale = modUpdates.stale.has(i.id);
      return stale === i.needsUpdate ? i : { ...i, needsUpdate: stale };
    }),
  );
  // A live junction wins over a stale one for the same item: in name order, a stale
  // `@Community Framework` left from an old library replaced a live `@CF` and the cell
  // showed "target gone" for a mod that was linked (D-276).
  const junctionsById = $derived.by(() => {
    const m = new Map<number, JunctionInfo>();
    for (const j of data?.junctions ?? []) {
      if (j.workshopId == null) continue;
      const have = m.get(j.workshopId);
      if (!have || (!have.targetExists && j.targetExists)) m.set(j.workshopId, j);
    }
    return m;
  });
  // What the clean-up will remove, counted by the host's own rule (D-276).
  const dangling = $derived((data?.junctions ?? []).filter((j) => j.removable).length);
  /** `mod.cpp` names are sometimes a localisation key (`$STR_nam_mod_terrain_name`);
   *  the Workshop's own name is the readable one in that case (D-143). */
  const nameOf = (i: WorkshopItemInfo) => {
    const n = i.modName?.trim();
    return (n && !n.startsWith("$") ? n : null) ?? i.metaName ?? n ?? String(i.id);
  };
  const serversFor = (id: number) => servers.modCatalog.get(id)?.servers ?? 0;
  /** A failed download's message, led by the mod it failed on (D-277). */
  function failureText(d: SyncDone): string {
    const why = d.error ?? "Download failed";
    if (d.failedId == null) return why;
    const it = data?.workshop?.items.find((i) => i.id === d.failedId);
    return `${it ? nameOf(it) : `Workshop item ${d.failedId}`}: ${why}`;
  }

  const stale = $derived(all.filter((i) => i.needsUpdate));
  const unused = $derived(all.filter((i) => serversFor(i.id) === 0));
  const noJunction = $derived(all.filter((i) => !junctionsById.has(i.id)));
  const totalSize = $derived(all.reduce((a, i) => a + i.size, 0));

  const views: { id: View; label: string; count: () => number }[] = [
    { id: "all", label: "All", count: () => all.length },
    { id: "updates", label: "Updates", count: () => stale.length },
    { id: "unused", label: "On no server", count: () => unused.length },
    { id: "nojunction", label: "No junction", count: () => noJunction.length },
  ];

  const items = $derived.by(() => {
    const q = search.trim().toLowerCase();
    let list = all.filter((i) => {
      if (view === "updates" && !i.needsUpdate) return false;
      if (view === "unused" && serversFor(i.id) !== 0) return false;
      if (view === "nojunction" && junctionsById.has(i.id)) return false;
      if (!q) return true;
      return nameOf(i).toLowerCase().includes(q) || (i.metaName ?? "").toLowerCase().includes(q) || String(i.id).includes(q);
    });
    const { key, dir } = sort;
    list = [...list].sort((a, b) => {
      const c =
        key === "size"
          ? a.size - b.size
          : key === "updated"
            ? a.timeUpdated - b.timeUpdated
            : key === "servers"
              ? serversFor(a.id) - serversFor(b.id)
              : nameOf(a).toLowerCase().localeCompare(nameOf(b).toLowerCase());
      return dir * (c || a.id - b.id);
    });
    return list;
  });

  const shownSize = $derived(items.reduce((a, i) => a + i.size, 0));
  const pickedItems = $derived(all.filter((i) => selected.has(i.id)));
  const pickedStale = $derived(pickedItems.filter((i) => i.needsUpdate));
  const pickedSize = $derived(pickedItems.reduce((a, i) => a + i.size, 0));
  const allShownPicked = $derived(items.length > 0 && items.every((i) => selected.has(i.id)));

  function toggleAllShown() {
    if (allShownPicked) for (const i of items) selected.delete(i.id);
    else for (const i of items) selected.add(i.id);
  }
  function toggle(id: number) {
    if (selected.has(id)) selected.delete(id);
    else selected.add(id);
  }

  function setSort(key: SortKey) {
    sort = sort.key === key ? { key, dir: sort.dir === 1 ? -1 : 1 } : { key, dir: key === "name" ? 1 : -1 };
  }
  const mark = (key: SortKey) => (sort.key === key ? (sort.dir === 1 ? " ▲" : " ▼") : "");

  async function update(ids: number[]) {
    if (!ids.length || updating) return;
    modUpdates.pageJob = Date.now();
    notice = null;
    error = null;
    // Optimistic, so the buttons disable at once instead of waiting for the first
    // progress event (D-151); `mods:done` clears it.
    updating = { job: modUpdates.pageJob, items: [], installed: 0, total: ids.length, elapsedMs: 0 };
    try {
      // Update only: never re-subscribe a mod that was unsubscribed since the badge
      // last heard from Steam (D-276).
      await invoke("mods_sync", { job: modUpdates.pageJob, ids, subscribe: false });
    } catch (e) {
      // The backend rejects synchronously when Steam is not connected, and then no
      // `mods:done` ever arrives — without this the toolbar stayed disabled reading
      // "Updating… 0/N" until the view was left and reopened (D-159).
      updating = null;
      error = String(e);
    }
  }

  async function unsubscribe(ids: { id: number; name: string }[]) {
    confirming = null;
    if (!ids.length) return;
    busyIds = new Set([...busyIds, ...ids.map((x) => x.id)]);
    notice = null;
    error = null;
    try {
      const res = await invoke<UnsubscribeResult[]>("mods_unsubscribe", { ids: ids.map((x) => x.id) });
      const ok = res.filter((r) => r.ok).length;
      const bad = res.filter((r) => !r.ok);
      if (ok) {
        notice =
          ids.length === 1 && ids[0]
            ? `Unsubscribed from ${ids[0].name}. Steam removes the files when DayZ is not running; the junction stays for the official launcher.`
            : `Unsubscribed from ${ok} mods. Steam removes the files when DayZ is not running; the junctions stay for the official launcher.`;
        for (const r of res) if (r.ok) selected.delete(r.id);
      }
      if (bad.length) error = `Could not unsubscribe from ${bad.length} mod${bad.length === 1 ? "" : "s"}: ${bad[0]?.error ?? "no answer from Steam"}`;
    } catch (e) {
      error = String(e);
    } finally {
      busyIds = new Set([...busyIds].filter((x) => !ids.some((i) => i.id === x)));
      void load();
    }
  }

  async function openFolder(folder: string) {
    try {
      await revealItemInDir(folder);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<section class="mods">
  <header class="bar">
    <h1>Mods</h1>
    {#if data?.workshop}
      <span class="muted">{all.length} installed · {fmtBytes(totalSize)}{#if dangling} · {dangling} stale junction{dangling === 1 ? "" : "s"}{/if}</span>
    {/if}
    <span class="spacer"></span>
    {#if dangling}
      {#if confirmClean}
        <span class="muted">Remove {dangling} stale junction{dangling === 1 ? "" : "s"}? Only ones whose target folder is gone.</span>
        <button class="btn danger" onclick={cleanJunctions}>Yes</button>
        <button class="btn secondary" onclick={() => (confirmClean = false)}>No</button>
      {:else}
        <button class="btn secondary" onclick={() => (confirmClean = true)} disabled={cleaning || !!updating} title="Delete the !Workshop junctions whose target folder no longer exists">
          {cleaning ? "Removing…" : `Clean ${dangling} stale`}
        </button>
      {/if}
    {/if}
    {#if updating && fromJoin}
      <span class="muted">Downloading for a join… {updating.installed}/{updating.total}</span>
    {/if}
    {#if stale.length}
      <button class="btn" onclick={() => update(stale.map((i) => i.id))} disabled={!!updating}>
        {updating && !fromJoin ? `Updating… ${updating.installed}/${updating.total}` : `Update all ${stale.length}`}
      </button>
    {/if}
    <button class="btn secondary" onclick={load} disabled={!!updating}>Rescan</button>
  </header>

  {#if all.length}
    <div class="bar tools">
      <input class="search" type="search" placeholder="Search mods" bind:value={search} aria-label="Search mods" spellcheck="false" />
      <div class="chips" role="group" aria-label="Mod filter">
        {#each views as v (v.id)}
          {@const n = v.count()}
          <!-- "On no server" needs the stored mod lists; before they load every count
               is 0, and this chip offered every mod for a bulk Unsubscribe (D-277). -->
          {@const known = v.id !== "unused" || servers.modsIndexLoaded}
          <button class="chip" class:on={view === v.id} aria-pressed={view === v.id} onclick={() => (view = v.id)} disabled={!known || (n === 0 && v.id !== "all")}>
            {v.label} <span class="n">{known ? n : "—"}</span>
          </button>
        {/each}
      </div>
      <span class="spacer"></span>
      {#if selected.size}
        <span class="picked">{selected.size} selected · {fmtBytes(pickedSize)}</span>
        {#if confirming?.kind === "bulk"}
          <span class="muted">Unsubscribe {selected.size}?</span>
          <button class="btn danger" onclick={() => unsubscribe(pickedItems.map((i) => ({ id: i.id, name: nameOf(i) })))}>Yes</button>
          <button class="btn secondary" onclick={() => (confirming = null)}>No</button>
        {:else}
          {#if pickedStale.length}
            <button class="btn secondary" onclick={() => update(pickedStale.map((i) => i.id))} disabled={!!updating}>Update {pickedStale.length}</button>
          {/if}
          <button class="btn danger" onclick={() => (confirming = { kind: "bulk" })}>Unsubscribe</button>
          <button class="btn ghost" onclick={() => selected.clear()}>Clear</button>
        {/if}
      {:else}
        <span class="muted small">{items.length} shown · {fmtBytes(shownSize)}</span>
      {/if}
    </div>
  {/if}

  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if notice}<p class="ok small" role="status" aria-live="polite">{notice}</p>{/if}

  {#if data && !data.workshop}
    <!-- The inventory could not be read: saying "no mods installed" here sent the user
         off to re-download what was already on disk (D-256). -->
    <div class="empty">
      <p>The installed mods could not be read.</p>
      {#each data.warnings as w, i (i)}<p class="muted">{w}</p>{/each}
      <p class="muted">Rescan once that is sorted out.</p>
    </div>
  {:else if data && all.length === 0}
    <div class="empty">
      <p>No Workshop mods installed.</p>
      <p class="muted">Join a modded server and the launcher subscribes to and downloads what it needs, then lists it here.</p>
      <button class="btn" onclick={() => (servers.navigate = "servers")}>Browse servers</button>
    </div>
  {:else if all.length && items.length === 0}
    <p class="muted">No mod matches this filter.</p>
  {/if}

  {#if items.length}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th class="pick"><input type="checkbox" checked={allShownPicked} onchange={toggleAllShown} aria-label="Select all shown" /></th>
            <th><button class="th" onclick={() => setSort("name")}>Mod{mark("name")}</button></th>
            <th class="num"><button class="th" onclick={() => setSort("size")}>Size{mark("size")}</button></th>
            <th><button class="th" onclick={() => setSort("updated")}>Updated{mark("updated")}</button></th>
            <th>Junction</th>
            <th class="num" title="Servers whose scanned mod list includes this item">
              <button class="th" onclick={() => setSort("servers")}>Servers{mark("servers")}</button>
            </th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each items as it (it.id)}
            {@const j = junctionsById.get(it.id)}
            {@const name = nameOf(it)}
            {@const running = serversFor(it.id)}
            <tr class:stale={it.needsUpdate} class:picked={selected.has(it.id)}>
              <td class="pick"><input type="checkbox" checked={selected.has(it.id)} onchange={() => toggle(it.id)} aria-label="Select {name}" /></td>
              <td>
                <span class="name">{name}</span>
                <a class="mid" href="https://steamcommunity.com/sharedfiles/filedetails/?id={it.id}" onclick={external} title="Open in the Steam Workshop">{it.id}</a>
                {#if it.metaName && it.metaName !== name}<span class="muted small"> · {it.metaName}</span>{/if}
              </td>
              <td class="num">{fmtBytes(it.size)}</td>
              <td>{new Date(it.timeUpdated * 1000).toLocaleDateString()}{it.needsUpdate ? " ⚠ update" : ""}</td>
              <td class={j ? (j.targetExists ? "ok" : "warn") : "muted"}>{j ? (j.targetExists ? j.name : `${j.name} (target gone)`) : "none (created on first join)"}</td>
              <td class="num">
                {#if !servers.modsIndexLoaded}<span class="muted" title="The server list's mod lists have not loaded yet">—</span>{:else if running}<button class="btn slim" onclick={() => showServers(it.id)} title="Show these servers">{running}</button>{:else}<span class="muted">0</span>{/if}
              </td>
              <td class="act">
                {#if busyIds.has(it.id)}
                  <span class="muted">…</span>
                {:else if confirming?.kind === "one" && confirming.id === it.id}
                  <span class="muted">Unsubscribe?</span>
                  <button class="btn danger" onclick={() => unsubscribe([{ id: it.id, name }])}>Yes</button>
                  <button class="btn secondary" onclick={() => (confirming = null)}>No</button>
                {:else}
                  {#if it.needsUpdate}<button class="btn secondary" onclick={() => update([it.id])} disabled={!!updating} title="Download the new version through Steam">Update</button>{/if}
                  {#if it.folder}<button class="btn ghost" onclick={() => openFolder(it.folder as string)} title="Show the mod folder">Folder</button>{/if}
                  <button class="btn ghost" onclick={() => (confirming = { kind: "one", id: it.id, name })} title="Unsubscribe on Steam; the files are removed by Steam">Unsubscribe</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <p class="muted small">
    Steam downloads mods when you join a server that needs them. Junctions in <code>!Workshop</code> are shared with the official launcher and never deleted here on their own; dangling ones can be cleaned up with the button above.
  </p>
</section>

<style>
  /* Fits the viewport; only the table scrolls when the inventory is long (D-094). */
  .mods { display: flex; flex-direction: column; gap: 8px; min-height: 0; }
  .bar { display: flex; align-items: center; gap: 10px; flex: none; }
  /* ~965 px of min-content inside 908 px of content width at the 960 px window
     minimum, and the page clips its overflow - so Unsubscribe and Clear were cut off.
     The selection group drops to a second line instead (D-226). */
  .tools { gap: 8px; flex-wrap: wrap; }
  .spacer { flex: 1; }
  h1 { margin: 0; }
  .scroll { flex: 1; min-height: 0; overflow: auto; }

  .search { flex: 0 1 240px; min-width: 120px; box-sizing: border-box; height: 26px; padding: 0 9px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-size: 12.5px; }
  .search:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: -2px; }

  .chips { display: inline-flex; gap: 5px; }
  .chip { all: unset; cursor: pointer; box-sizing: border-box; height: 26px; padding: 0 10px; display: inline-flex; align-items: center; gap: 5px; border-radius: var(--radius); border: 1px solid var(--border-control); color: var(--fg-muted); font-size: 12.5px; white-space: nowrap; }
  .chip.on { background: color-mix(in srgb, var(--accent) 22%, var(--bg-row)); color: var(--fg); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); }
  .chip:disabled { opacity: 0.45; cursor: default; }
  .chip .n { color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .chip.on .n { color: var(--fg); }

  .picked { color: var(--fg); font-weight: 600; font-size: 12.5px; white-space: nowrap; }

  .th { all: unset; cursor: pointer; }
  .th:hover { color: var(--fg); }

  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  th, td { text-align: left; padding: 5px 8px; border-bottom: 1px solid var(--border); vertical-align: middle; }
  th { position: sticky; top: 0; background: var(--bg); color: var(--fg-muted); font-weight: 500; }
  .pick { width: 26px; padding-right: 0; }
  .pick input { accent-color: var(--accent); cursor: pointer; }
  .name { font-weight: 500; }
  /* A size never splits from its unit: with the 240 px rail (D-249) the table meets
     its narrow widths sooner, and "1.58" over "GB" read as two values. */
  .num { text-align: right; white-space: nowrap; }
  .act { text-align: right; white-space: nowrap; }
  .act .btn + .btn { margin-left: 4px; }
  tr.picked td { background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .mid { font-size: 11px; color: var(--fg-muted); text-decoration: none; margin-left: 6px; }
  .mid:hover { text-decoration: underline; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .stale td:not(.pick) { color: var(--warn); }
  /* On a selected row the accent tint lifts the background under the amber and all
     twelve light accents fall under 4.5:1 (3.83 worst, rose). The Updated column
     already says the mod is stale, so the selected row keeps the ordinary ink - the
     same trade D-198 made for the selected server row (D-226). */
  tr.picked.stale td:not(.pick) { color: var(--fg); }
  .small { font-size: 12px; }
  p.small { margin: 0; }
  .empty { margin: auto; text-align: center; max-width: 440px; display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .empty p { margin: 0; }
  .empty .btn { margin-top: 6px; }
  .error { color: var(--danger); margin: 0; }
  code { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; }
</style>
