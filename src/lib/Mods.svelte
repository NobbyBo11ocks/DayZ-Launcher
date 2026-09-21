<script lang="ts">
  // Installed Workshop items and their !Workshop junctions (docs/02 §3–4), with
  // mod management (D-075): update what is stale, unsubscribe what is unwanted.
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { fmtBytes, type Diagnostics, type SyncDone, type SyncProgress, type UnsubscribeResult } from "./types";

  let data = $state<Diagnostics | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  /** Item whose Unsubscribe button is awaiting confirmation. */
  let confirming = $state<number | null>(null);
  let busyIds = $state<Set<number>>(new Set());
  let updating = $state<SyncProgress | null>(null);
  let updateJob = 0;
  type SortKey = "name" | "size" | "updated";
  let sort = $state<{ key: SortKey; dir: 1 | -1 }>({ key: "size", dir: -1 });

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

  const items = $derived.by(() => {
    const list = [...(data?.workshop?.items ?? [])];
    const { key, dir } = sort;
    const name = (i: (typeof list)[number]) => (i.modName ?? i.metaName ?? String(i.id)).toLowerCase();
    list.sort((a, b) => {
      const c = key === "size" ? a.size - b.size : key === "updated" ? a.timeUpdated - b.timeUpdated : name(a).localeCompare(name(b));
      return dir * (c || a.id - b.id);
    });
    return list;
  });
  const junctionsById = $derived(new Map((data?.junctions ?? []).filter((j) => j.workshopId != null).map((j) => [j.workshopId as number, j])));
  const dangling = $derived((data?.junctions ?? []).filter((j) => !j.targetExists).length);
  const total = $derived(items.reduce((a, i) => a + i.size, 0));
  const stale = $derived(items.filter((i) => i.needsUpdate));

  function setSort(key: SortKey) {
    sort = sort.key === key ? { key, dir: sort.dir === 1 ? -1 : 1 } : { key, dir: key === "name" ? 1 : -1 };
  }
  const mark = (key: SortKey) => (sort.key === key ? (sort.dir === 1 ? " ▲" : " ▼") : "");

  async function updateAll() {
    if (!stale.length) return;
    updateJob = Date.now();
    notice = null;
    error = null;
    try {
      await invoke("mods_sync", { job: updateJob, ids: stale.map((i) => i.id) });
    } catch (e) {
      error = String(e);
    }
  }

  async function unsubscribe(id: number, name: string) {
    confirming = null;
    busyIds = new Set([...busyIds, id]);
    notice = null;
    error = null;
    try {
      const [r] = await invoke<UnsubscribeResult[]>("mods_unsubscribe", { ids: [id] });
      if (r?.ok) notice = `Unsubscribed from ${name}. Steam removes the files when DayZ is not running; the junction stays for the official launcher.`;
      else error = `Could not unsubscribe from ${name}: ${r?.error ?? "no answer from Steam"}`;
    } catch (e) {
      error = String(e);
    } finally {
      busyIds = new Set([...busyIds].filter((x) => x !== id));
      void load();
    }
  }
</script>

<section class="mods">
  <header class="row">
    <h1>Mods</h1>
    <button class="btn" onclick={load}>Rescan</button>
    {#if stale.length}
      <button class="btn accent" onclick={updateAll} disabled={!!updating}>{updating ? `Updating… ${updating.installed}/${updating.total}` : `Update ${stale.length} mod${stale.length === 1 ? "" : "s"}`}</button>
    {/if}
    {#if data?.workshop}<span class="muted">{items.length} installed · {fmtBytes(total)}{#if dangling} · {dangling} stale junction{dangling === 1 ? "" : "s"}{/if}</span>{/if}
  </header>
  {#if error}<p class="error">{error}</p>{/if}
  {#if notice}<p class="ok small">{notice}</p>{/if}
  {#if data && !data.workshop}<p class="muted">No Workshop items installed for DayZ.</p>{/if}
  {#if items.length}
    <table>
      <thead>
        <tr>
          <th><button class="th" onclick={() => setSort("name")}>Mod{mark("name")}</button></th>
          <th>Workshop title</th>
          <th class="num"><button class="th" onclick={() => setSort("size")}>Size{mark("size")}</button></th>
          <th><button class="th" onclick={() => setSort("updated")}>Updated{mark("updated")}</button></th>
          <th>Junction</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each items as it (it.id)}
          {@const j = junctionsById.get(it.id)}
          {@const name = it.modName ?? it.metaName ?? String(it.id)}
          <tr class:stale={it.needsUpdate}>
            <td>{name}</td>
            <td class="muted">{it.metaName ?? "–"} <a class="mid" href="https://steamcommunity.com/sharedfiles/filedetails/?id={it.id}" target="_blank" rel="noreferrer">{it.id}</a></td>
            <td class="num">{fmtBytes(it.size)}</td>
            <td>{new Date(it.timeUpdated * 1000).toLocaleDateString()}{it.needsUpdate ? " ⚠ update available" : ""}</td>
            <td class={j ? "ok" : "muted"}>{j ? j.name : "none (created on first join)"}</td>
            <td class="act">
              {#if busyIds.has(it.id)}
                <span class="muted">…</span>
              {:else if confirming === it.id}
                <span class="muted">Unsubscribe?</span>
                <button class="btn danger" onclick={() => unsubscribe(it.id, name)}>Yes</button>
                <button class="btn" onclick={() => (confirming = null)}>No</button>
              {:else}
                <button class="btn" onclick={() => (confirming = it.id)} title="Unsubscribe on Steam; the files are removed by Steam">Unsubscribe</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <p class="muted small">Mods are downloaded by Steam when you join a server that needs them. Junctions in <code>!Workshop</code> are shared with the official launcher and never deleted.</p>
</section>

<style>
  .mods { display: flex; flex-direction: column; gap: 10px; }
  .row { display: flex; align-items: center; gap: 12px; }
  .btn { all: unset; cursor: pointer; padding: 4px 10px; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border); font-size: 12.5px; }
  .btn:hover { border-color: var(--accent); }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn.accent { background: var(--accent); color: #111; font-weight: 600; border-color: transparent; }
  .btn.danger { border-color: var(--danger); color: var(--danger); }
  .th { all: unset; cursor: pointer; }
  .th:hover { color: var(--fg); }
  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  th, td { text-align: left; padding: 5px 8px; border-bottom: 1px solid var(--border); vertical-align: top; }
  th { color: var(--fg-muted); font-weight: 500; }
  .num { text-align: right; }
  .act { text-align: right; white-space: nowrap; }
  .act .btn + .btn { margin-left: 4px; }
  .mid { font-size: 11px; color: var(--fg-muted); text-decoration: none; margin-left: 4px; }
  .mid:hover { text-decoration: underline; }
  .ok { color: var(--ok); }
  .stale td { color: var(--warn); }
  .small { font-size: 12px; margin: 0; }
  .error { color: var(--danger); margin: 0; }
  code { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; }
</style>
