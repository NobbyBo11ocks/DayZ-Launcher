<script lang="ts">
  // Installed Workshop items and their !Workshop junctions (docs/02 §3–4).
  import { invoke } from "@tauri-apps/api/core";
  import { fmtBytes, type Diagnostics } from "./types";

  let data = $state<Diagnostics | null>(null);
  let error = $state<string | null>(null);

  async function load() {
    try {
      data = await invoke<Diagnostics>("diagnostics");
    } catch (e) {
      error = String(e);
    }
  }
  $effect(() => {
    void load();
  });

  const items = $derived(data?.workshop?.items ?? []);
  const junctionsById = $derived(new Map((data?.junctions ?? []).filter((j) => j.workshopId != null).map((j) => [j.workshopId as number, j])));
  const dangling = $derived((data?.junctions ?? []).filter((j) => !j.targetExists).length);
  const total = $derived(items.reduce((a, i) => a + i.size, 0));
</script>

<section class="mods">
  <header class="row">
    <h1>Mods</h1>
    <button class="btn" onclick={load}>Rescan</button>
    {#if data?.workshop}<span class="muted">{items.length} installed · {fmtBytes(total)}{#if dangling} · {dangling} stale junction{dangling === 1 ? "" : "s"}{/if}</span>{/if}
  </header>
  {#if error}<p class="error">{error}</p>{/if}
  {#if data && !data.workshop}<p class="muted">No Workshop items installed for DayZ.</p>{/if}
  {#if items.length}
    <table>
      <thead><tr><th>Mod</th><th>Workshop title</th><th class="num">Size</th><th>Updated</th><th>Junction</th></tr></thead>
      <tbody>
        {#each items as it (it.id)}
          {@const j = junctionsById.get(it.id)}
          <tr class:stale={it.needsUpdate}>
            <td>{it.modName ?? it.metaName ?? it.id}</td>
            <td class="muted">{it.metaName ?? "–"} <a class="mid" href="https://steamcommunity.com/sharedfiles/filedetails/?id={it.id}" target="_blank" rel="noreferrer">{it.id}</a></td>
            <td class="num">{fmtBytes(it.size)}</td>
            <td>{new Date(it.timeUpdated * 1000).toLocaleDateString()}{it.needsUpdate ? " ⚠ update available" : ""}</td>
            <td class={j ? "ok" : "muted"}>{j ? j.name : "none (created on first join)"}</td>
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
  .btn { all: unset; cursor: pointer; padding: 4px 10px; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border); }
  .btn:hover { border-color: var(--accent); }
  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  th, td { text-align: left; padding: 5px 8px; border-bottom: 1px solid var(--border); vertical-align: top; }
  th { color: var(--fg-muted); font-weight: 500; }
  .num { text-align: right; }
  .mid { font-size: 11px; color: var(--fg-muted); text-decoration: none; margin-left: 4px; }
  .mid:hover { text-decoration: underline; }
  .ok { color: var(--ok); }
  .stale td { color: var(--warn); }
  .small { font-size: 12px; }
  code { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; }
</style>
