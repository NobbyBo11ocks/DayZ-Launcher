<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { Diagnostics } from "./types";

  let data = $state<Diagnostics | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let exported = $state<string | null>(null);
  let copied = $state(false);

  async function exportReport() {
    exported = null;
    try {
      const r = await invoke<{ path: string; bytes: number }>("diagnostics_export");
      exported = `${r.path} (${Math.round(r.bytes / 1024)} kB)`;
    } catch (e) {
      error = String(e);
    }
  }

  async function copyReport() {
    if (!data) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(data, null, 2));
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      error = String(e);
    }
  }

  async function load() {
    loading = true;
    error = null;
    try {
      data = await invoke<Diagnostics>("diagnostics");
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void load();
  });

  const fmtBytes = (n: number) =>
    n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : `${(n / 1024).toFixed(0)} kB`;
  const fmtTime = (unix: number) => (unix ? new Date(unix * 1000).toLocaleString() : "–");
</script>

<section class="diag">
  <header class="row">
    <h1>Diagnostics</h1>
    <button class="btn" onclick={load} disabled={loading}>{loading ? "Scanning…" : "Rescan"}</button>
    <button class="btn" onclick={copyReport} disabled={!data}>{copied ? "Copied" : "Copy report"}</button>
    <button class="btn" onclick={exportReport} disabled={!data} title="Writes a JSON support report into the app's data folder">Save report</button>
    {#if data}<span class="muted">{data.timingMs} ms</span>{/if}
    {#if exported}<span class="muted mono">{exported}</span>{/if}
  </header>

  {#if error}
    <p class="error">{error}</p>
  {:else if !data}
    <p class="muted">Scanning Steam…</p>
  {:else}
    {#if data.warnings.length}
      <ul class="warnings">
        {#each data.warnings as w (w)}<li>{w}</li>{/each}
      </ul>
    {/if}

    <h2>Steam</h2>
    <dl class="kv">
      <dt>Path</dt><dd>{data.steam.path ?? "not found"} <span class="muted">({data.steam.source})</span></dd>
      <dt>Executable</dt><dd>{data.steam.exe ?? "–"}</dd>
      <dt>Running</dt>
      <dd class={data.steam.running ? "ok" : "warn"}>
        {data.steam.running ? `yes (pid ${data.steam.pid})` : "no"}
        {#if data.steam.registryPid && data.steam.registryPid !== data.steam.pid}
          <span class="muted"> · registry says pid {data.steam.registryPid} (stale)</span>
        {/if}
      </dd>
      <dt>Logged in</dt><dd class={data.steam.activeUser ? "ok" : "warn"}>{data.steam.activeUser ? "yes" : "no"}</dd>
    </dl>

    <h2>Libraries</h2>
    <table>
      <thead><tr><th>Path</th><th>Label</th><th class="num">Apps</th><th>DayZ</th></tr></thead>
      <tbody>
        {#each data.libraries as lib (lib.path)}
          <tr><td>{lib.path}</td><td>{lib.label || "–"}</td><td class="num">{lib.appCount}</td><td>{lib.hasDayz ? "✓" : ""}</td></tr>
        {/each}
      </tbody>
    </table>

    <h2>DayZ</h2>
    {#if data.dayz}
      <dl class="kv">
        <dt>Folder</dt><dd>{data.dayz.folder}</dd>
        <dt>Executable</dt><dd>{data.dayz.exe}</dd>
        <dt>Version</dt><dd>{data.dayz.gameVersion ?? "?"} <span class="muted">(exe {data.dayz.exeVersion ?? "?"})</span></dd>
        <dt>Build</dt><dd>{data.dayz.buildId} <span class="muted">updated {fmtTime(data.dayz.lastUpdated)}</span></dd>
        <dt>Size</dt><dd>{fmtBytes(data.dayz.sizeOnDisk)}</dd>
        <dt>State flags</dt><dd>{data.dayz.stateFlags} <span class="muted">{data.dayz.stateFlags === 4 ? "(fully installed)" : ""}</span></dd>
        <dt>BattlEye launcher</dt><dd class={data.dayz.hasBattleyeExe ? "ok" : "warn"}>{data.dayz.hasBattleyeExe ? "DayZ_BE.exe present" : "DayZ_BE.exe missing"}</dd>
        <dt>Official launcher</dt><dd>{data.dayz.hasOfficialLauncher ? "present" : "absent"}</dd>
      </dl>
    {:else}
      <p class="warn">DayZ (app 221100) not found in any library.</p>
    {/if}

    <h2>Workshop</h2>
    {#if data.workshop}
      <dl class="kv">
        <dt>Manifest</dt><dd>{data.workshop.acfPath}</dd>
        <dt>Status</dt>
        <dd>
          {data.workshop.items.length} items, {fmtBytes(data.workshop.sizeOnDisk)}
          {#if data.workshop.needsUpdate}<span class="warn"> · needs update</span>{/if}
          {#if data.workshop.needsDownload}<span class="warn"> · needs download</span>{/if}
        </dd>
      </dl>
      <table>
        <thead><tr><th>Workshop ID</th><th>meta.cpp name</th><th>mod.cpp name</th><th class="num">Size</th><th>Updated</th><th>Folder</th></tr></thead>
        <tbody>
          {#each data.workshop.items as it (it.id)}
            <tr class:stale={it.needsUpdate}>
              <td class="mono">{it.id}</td>
              <td>{it.metaName ?? "–"}</td>
              <td>{it.modName ?? "–"}</td>
              <td class="num">{fmtBytes(it.size)}</td>
              <td>{fmtTime(it.timeUpdated)}{it.needsUpdate ? " ⚠" : ""}</td>
              <td class={it.folder ? "" : "warn"}>{it.folder ? "present" : "missing"}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <p class="muted">No appworkshop_221100.acf found.</p>
    {/if}

    <h2>!Workshop junctions ({data.junctions.length})</h2>
    <table>
      <thead><tr><th>Name</th><th>Target</th><th>Workshop ID</th><th>Target</th></tr></thead>
      <tbody>
        {#each data.junctions as j (j.name)}
          <tr>
            <td>{j.name}</td>
            <td class="mono">{j.target ?? "–"}</td>
            <td class="mono">{j.workshopId ?? "–"}</td>
            <td class={j.targetExists ? "ok" : "warn"}>{j.targetExists ? "exists" : "dangling"}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .diag { display: flex; flex-direction: column; gap: 8px; }
  .row { display: flex; align-items: center; gap: 12px; }
  h2 { font-size: 14px; font-weight: 600; margin: 12px 0 4px; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  th, td { text-align: left; padding: 4px 8px; border-bottom: 1px solid var(--border); vertical-align: top; }
  th { color: var(--fg-muted); font-weight: 500; }
  .num { text-align: right; }
  .mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .stale td { color: var(--warn); }
  .warnings { margin: 0; padding-left: 18px; color: var(--warn); }
  .btn { all: unset; cursor: pointer; padding: 4px 10px; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border); }
  .btn:hover { border-color: var(--accent); }
  .btn:disabled { opacity: 0.6; cursor: default; }
  .btn:focus-visible { outline: 2px solid var(--accent); }
</style>
