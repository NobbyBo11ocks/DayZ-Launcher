<script lang="ts">
  // Mod manager (D-142): the installed Workshop items and their !Workshop junctions
  // (docs/02 §3–4) with search, filters, multi-select and bulk update or unsubscribe
  // on top of the single-item management from D-075. Fits the viewport; only the
  // table scrolls (D-094).
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158/D-160).
  import { invokeLogged as invoke } from "./log";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { SvelteSet } from "svelte/reactivity";
  import { external } from "./external";
  import { servers } from "./state/servers.svelte";
  import { fmtBytes, type Diagnostics, type SyncDone, type SyncProgress, type UnsubscribeResult, type WorkshopItemInfo } from "./types";

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
  let updateJob = 0;
  let search = $state("");
  /** "one" confirms a single unsubscribe, "bulk" the selection. */
  let confirming = $state<{ kind: "one"; id: number; name: string } | { kind: "bulk" } | null>(null);
  const selected = new SvelteSet<number>();

  type SortKey = "name" | "size" | "updated" | "servers";
  let sort = $state<{ key: SortKey; dir: 1 | -1 }>({ key: "size", dir: -1 });
  type View = "all" | "updates" | "unused" | "nojunction";
  let view = $state<View>("all");

  async function load() {
    try {
      data = await invoke<Diagnostics>("diagnostics");
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    void load();
    const unlisteners: UnlistenFn[] = [];
    (async () => {
      unlisteners.push(
        await listen<SyncProgress>("mods:progress", (ev) => {
          if (ev.payload.job === updateJob) updating = ev.payload;
        }),
        await listen<SyncDone>("mods:done", (ev) => {
          if (ev.payload.job !== updateJob) return;
          updating = null;
          notice = ev.payload.ok ? `Updated ${ev.payload.items.length} mod${ev.payload.items.length === 1 ? "" : "s"}.` : null;
          error = ev.payload.ok ? null : (ev.payload.error ?? "Update failed");
          void load();
        }),
      );
    })();
    return () => unlisteners.forEach((u) => u());
  });

  const all = $derived(data?.workshop?.items ?? []);
  const junctionsById = $derived(new Map((data?.junctions ?? []).filter((j) => j.workshopId != null).map((j) => [j.workshopId as number, j])));
  const dangling = $derived((data?.junctions ?? []).filter((j) => !j.targetExists).length);
  /** `mod.cpp` names are sometimes a localisation key (`$STR_nam_mod_terrain_name`);
   *  the Workshop's own name is the readable one in that case (D-143). */
  const nameOf = (i: WorkshopItemInfo) => {
    const n = i.modName?.trim();
    return (n && !n.startsWith("$") ? n : null) ?? i.metaName ?? n ?? String(i.id);
  };
  const serversFor = (id: number) => servers.modCatalog.get(id)?.servers ?? 0;

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
    updateJob = Date.now();
    notice = null;
    error = null;
    // Optimistic, so the buttons disable at once instead of waiting for the first
    // progress event (D-151); `mods:done` clears it.
    updating = { job: updateJob, items: [], installed: 0, total: ids.length, elapsedMs: 0 };
    try {
      await invoke("mods_sync", { job: updateJob, ids });
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
    {#if stale.length}
      <button class="btn accent" onclick={() => update(stale.map((i) => i.id))} disabled={!!updating}>
        {updating ? `Updating… ${updating.installed}/${updating.total}` : `Update all ${stale.length}`}
      </button>
    {/if}
    <button class="btn" onclick={load} disabled={!!updating}>Rescan</button>
  </header>

  {#if all.length}
    <div class="bar tools">
      <input class="search" type="search" placeholder="Search mods" bind:value={search} aria-label="Search mods" spellcheck="false" />
      <div class="chips" role="tablist" aria-label="Mod filter">
        {#each views as v (v.id)}
          {@const n = v.count()}
          <button class="chip" class:on={view === v.id} role="tab" aria-selected={view === v.id} onclick={() => (view = v.id)} disabled={n === 0 && v.id !== "all"}>
            {v.label} <span class="n">{n}</span>
          </button>
        {/each}
      </div>
      <span class="spacer"></span>
      {#if selected.size}
        <span class="picked">{selected.size} selected · {fmtBytes(pickedSize)}</span>
        {#if confirming?.kind === "bulk"}
          <span class="muted">Unsubscribe {selected.size}?</span>
          <button class="btn danger" onclick={() => unsubscribe(pickedItems.map((i) => ({ id: i.id, name: nameOf(i) })))}>Yes</button>
          <button class="btn" onclick={() => (confirming = null)}>No</button>
        {:else}
          {#if pickedStale.length}
            <button class="btn" onclick={() => update(pickedStale.map((i) => i.id))} disabled={!!updating}>Update {pickedStale.length}</button>
          {/if}
          <button class="btn danger" onclick={() => (confirming = { kind: "bulk" })}>Unsubscribe</button>
          <button class="btn ghost" onclick={() => selected.clear()}>Clear</button>
        {/if}
      {:else}
        <span class="muted small">{items.length} shown · {fmtBytes(shownSize)}</span>
      {/if}
    </div>
  {/if}

  {#if error}<p class="error">{error}</p>{/if}
  {#if notice}<p class="ok small">{notice}</p>{/if}

  {#if data && (!data.workshop || all.length === 0)}
    <div class="empty">
      <p>No Workshop mods installed.</p>
      <p class="muted">Join a modded server and the launcher subscribes to and downloads what it needs, then lists it here.</p>
      <button class="btn accent" onclick={() => (servers.navigate = "servers")}>Browse servers</button>
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
            <th class="num" title="Populated servers whose scanned mod list includes this item">
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
                {#if running}<button class="btn slim" onclick={() => showServers(it.id)} title="Show these servers">{running}</button>{:else}<span class="muted">0</span>{/if}
              </td>
              <td class="act">
                {#if busyIds.has(it.id)}
                  <span class="muted">…</span>
                {:else if confirming?.kind === "one" && confirming.id === it.id}
                  <span class="muted">Unsubscribe?</span>
                  <button class="btn danger" onclick={() => unsubscribe([{ id: it.id, name }])}>Yes</button>
                  <button class="btn" onclick={() => (confirming = null)}>No</button>
                {:else}
                  {#if it.needsUpdate}<button class="btn" onclick={() => update([it.id])} disabled={!!updating} title="Download the new version through Steam">Update</button>{/if}
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
    Steam downloads mods when you join a server that needs them. Junctions in <code>!Workshop</code> are shared with the official launcher and never deleted here on their own; dangling ones can be removed from Diagnostics.
  </p>
</section>

<style>
  /* Fits the viewport; only the table scrolls when the inventory is long (D-094). */
  .mods { display: flex; flex-direction: column; gap: 8px; min-height: 0; }
  .bar { display: flex; align-items: center; gap: 10px; flex: none; }
  .tools { gap: 8px; flex-wrap: nowrap; }
  .spacer { flex: 1; }
  h1 { margin: 0; }
  .scroll { flex: 1; min-height: 0; overflow: auto; }

  .search { flex: 0 1 240px; min-width: 120px; box-sizing: border-box; height: 26px; padding: 0 9px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); font-size: 12.5px; }
  .search:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }

  .chips { display: inline-flex; gap: 5px; }
  .chip { all: unset; cursor: pointer; box-sizing: border-box; height: 26px; padding: 0 10px; display: inline-flex; align-items: center; gap: 5px; border-radius: var(--radius); border: 1px solid var(--border); color: var(--fg-muted); font-size: 12.5px; white-space: nowrap; }
  .chip.on { background: color-mix(in srgb, var(--accent) 22%, var(--bg-row)); color: var(--fg); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); }
  .chip:disabled { opacity: 0.45; cursor: default; }
  .chip .n { color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .chip.on .n { color: var(--fg); }

  .picked { color: var(--fg); font-weight: 600; font-size: 12.5px; white-space: nowrap; }

  .btn { all: unset; cursor: pointer; box-sizing: border-box; height: 26px; padding: 0 10px; display: inline-flex; align-items: center; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border); font-size: 12.5px; white-space: nowrap; }
  .btn:hover { border-color: var(--accent); }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn.accent { background: var(--accent); color: var(--accent-fg); font-weight: 600; border-color: transparent; }
  .btn.ghost { background: transparent; color: var(--fg-muted); }
  .btn.ghost:hover { color: var(--fg); }
  .btn.danger { border-color: var(--danger); color: var(--danger); }
  .btn.slim { height: 22px; padding: 0 8px; }
  .th { all: unset; cursor: pointer; }
  .th:hover { color: var(--fg); }

  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  th, td { text-align: left; padding: 5px 8px; border-bottom: 1px solid var(--border); vertical-align: middle; }
  th { position: sticky; top: 0; background: var(--bg); color: var(--fg-muted); font-weight: 500; }
  .pick { width: 26px; padding-right: 0; }
  .pick input { accent-color: var(--accent); cursor: pointer; }
  .name { font-weight: 500; }
  .num { text-align: right; }
  .act { text-align: right; white-space: nowrap; }
  .act .btn + .btn { margin-left: 4px; }
  tr.picked td { background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .mid { font-size: 11px; color: var(--fg-muted); text-decoration: none; margin-left: 6px; }
  .mid:hover { text-decoration: underline; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .stale td:not(.pick) { color: var(--warn); }
  .small { font-size: 12px; }
  p.small { margin: 0; }
  .empty { margin: auto; text-align: center; max-width: 440px; display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .empty p { margin: 0; }
  .empty .btn { margin-top: 6px; }
  .error { color: var(--danger); margin: 0; }
  code { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; }
</style>
