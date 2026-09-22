<script lang="ts">
  // Steam / DayZ / Workshop inventory plus the Performance section (D-078), laid out
  // in two columns so the view fits the viewport without page scrolling (D-094).
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158/D-160).
  import { invokeLogged as invoke } from "./log";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import type { CacheStats, Diagnostics, JunctionCleanup, PerfSample } from "./types";

  // Cache row counts (D-115): a glance tells whether favourites, history or the
  // server list went missing (Q22).
  let cache = $state<CacheStats | null>(null);
  async function loadCache() {
    try {
      cache = await invoke<CacheStats>("cache_stats");
    } catch {
      cache = null;
    }
  }

  // The diagnostic log (D-158): the last entries from both halves of the app, newest
  // first here, with the file behind a "Show the file" button.
  type LogEntry = { at: number; level: "debug" | "info" | "warn" | "error"; target: string; message: string };
  let logs = $state<LogEntry[]>([]);
  let logPath = $state<string | null>(null);
  let logLevel = $state<"all" | "warn">("all");
  async function loadLogs() {
    try {
      const [entries, path] = await Promise.all([invoke<LogEntry[]>("logs_recent", { limit: 200 }), invoke<string | null>("logs_path")]);
      logs = entries.reverse();
      logPath = path;
    } catch (e) {
      error = String(e);
    }
  }
  const shownLogs = $derived(logLevel === "all" ? logs : logs.filter((l) => l.level === "warn" || l.level === "error"));
  const logTime = (ms: number) => new Date(ms).toLocaleTimeString();
  async function revealLog() {
    if (!logPath) return;
    try {
      await revealItemInDir(logPath);
    } catch (e) {
      error = String(e);
    }
  }
  async function copyLog() {
    try {
      await navigator.clipboard.writeText(logs.map((l) => `${logTime(l.at)} ${l.level.toUpperCase()} ${l.target} ${l.message}`).join("\n"));
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      error = String(e);
    }
  }

  // Release budgets from docs/05 §6 (D-078).
  const BUDGET = { startMs: 1000, hostBytes: 90 * 1024 * 1024, totalBytes: 330 * 1024 * 1024 };
  let perf = $state<PerfSample | null>(null);
  let perfAt = $state("");
  async function samplePerf() {
    try {
      perf = await invoke<PerfSample>("perf_sample");
      perfAt = new Date().toLocaleTimeString();
    } catch (e) {
      error = String(e);
    }
  }
  const fmtDur = (ms: number) => (ms >= 60_000 ? `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s` : `${(ms / 1000).toFixed(1)} s`);

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

  // Dangling-junction cleanup (D-093): explicit, confirmed, dangling entries only.
  let confirmClean = $state(false);
  let cleaning = $state(false);
  let cleanResult = $state<{ ok: boolean; text: string } | null>(null);
  const dangling = $derived(data?.junctions.filter((j) => !j.targetExists).length ?? 0);

  async function cleanJunctions() {
    confirmClean = false;
    cleaning = true;
    cleanResult = null;
    try {
      const r = await invoke<JunctionCleanup>("junctions_remove_dangling");
      const failed = r.failed.map((f) => `${f.name} (${f.error})`).join(", ");
      cleanResult = {
        ok: r.failed.length === 0,
        text: `Removed ${r.removed.length} junction${r.removed.length === 1 ? "" : "s"}${failed ? `; could not remove ${failed}` : ""}.`,
      };
      await load();
    } catch (e) {
      cleanResult = { ok: false, text: String(e) };
    } finally {
      cleaning = false;
    }
  }

  $effect(() => {
    void load();
    void samplePerf();
    void loadCache();
    void loadLogs();
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

  <!-- The error sits beside the report, not instead of it (D-151): a failed clipboard
       copy or perf sample used to replace the whole view until the next rescan. -->
  {#if error}<p class="error">{error}</p>{/if}
  {#if !data}
    <p class="muted">Scanning Steam…</p>
  {:else}
    {#if data.warnings.length}
      <ul class="warnings">
        {#each data.warnings as w (w)}<li>{w}</li>{/each}
      </ul>
    {/if}

    <div class="cols">
      <div class="col">
        <h2>Performance <button class="btn small" onclick={samplePerf}>Sample</button></h2>
        {#if perf}
          <dl class="kv">
            <dt>Start-up</dt>
            <dd class={perf.firstPaintMs == null ? "muted" : perf.firstPaintMs <= BUDGET.startMs ? "ok" : "warn"}>
              {perf.firstPaintMs == null ? "no frame painted yet" : `${perf.firstPaintMs} ms to the first frame`} <span class="muted">(budget {BUDGET.startMs} ms)</span>
            </dd>
            <dt>Uptime</dt><dd>{fmtDur(perf.uptimeMs)} · CPU {fmtDur(perf.hostCpuMs)} ({((100 * perf.hostCpuMs) / Math.max(1, perf.uptimeMs)).toFixed(1)}% of one core)</dd>
            <dt>Host memory</dt>
            <dd class={perf.hostPrivateBytes <= BUDGET.hostBytes ? "ok" : "warn"}>{fmtBytes(perf.hostPrivateBytes)} <span class="muted">(budget {fmtBytes(BUDGET.hostBytes)})</span></dd>
            <dt>WebView memory</dt><dd>{fmtBytes(perf.webviewPrivateBytes)} <span class="muted">across {perf.webviewProcesses} processes</span></dd>
            <dt>Total</dt>
            <dd class={perf.totalPrivateBytes <= BUDGET.totalBytes ? "ok" : "warn"}>{fmtBytes(perf.totalPrivateBytes)} <span class="muted">(budget {fmtBytes(BUDGET.totalBytes)}; sampled {perfAt})</span></dd>
          </dl>
        {:else}
          <p class="muted">Sampling…</p>
        {/if}

        <h2>Cache <button class="btn small" onclick={loadCache}>Recount</button></h2>
        {#if cache}
          <dl class="kv">
            <dt>Servers</dt><dd>{cache.servers.toLocaleString()} rows <span class="muted">· {cache.modLists.toLocaleString()} mod lists</span></dd>
            <dt>Yours</dt><dd>{cache.favourites} favourite{cache.favourites === 1 ? "" : "s"} · {cache.history} join{cache.history === 1 ? "" : "s"} in history · {cache.population.toLocaleString()} population samples</dd>
            <dt>Database</dt><dd>{fmtBytes(cache.dbBytes)} <span class="muted">+ {fmtBytes(cache.walBytes)} write-ahead log</span></dd>
          </dl>
        {:else}
          <p class="muted">Counting…</p>
        {/if}

        <!-- Diagnostic log (D-158): what the app has been doing, both halves in one list. -->
        <h2>
          Log
          <button class="btn small" onclick={loadLogs}>Refresh</button>
          <button class="btn small" class:on={logLevel === "warn"} onclick={() => (logLevel = logLevel === "all" ? "warn" : "all")} title="Show only warnings and errors">
            {logLevel === "warn" ? "Problems only" : "Everything"}
          </button>
          <button class="btn small" onclick={copyLog} disabled={!logs.length}>Copy</button>
          <button class="btn small" onclick={revealLog} disabled={!logPath} title={logPath ?? ""}>Show the file</button>
        </h2>
        {#if shownLogs.length}
          <div class="logs">
            {#each shownLogs as l, i (`${l.at}#${i}`)}
              <div class="logline {l.level}">
                <span class="logtime">{logTime(l.at)}</span>
                <span class="logtarget">{l.target}</span>
                <span class="logmsg">{l.message}</span>
              </div>
            {/each}
          </div>
        {:else}
          <p class="muted">{logs.length ? "No warnings or errors." : "Nothing logged yet."}</p>
        {/if}

        <h2>Steam</h2>
        <dl class="kv">
          <dt>Path</dt><dd class="wrap">{data.steam.path ?? "not found"} <span class="muted">({data.steam.source})</span></dd>
          <dt>Executable</dt><dd class="wrap">{data.steam.exe ?? "–"}</dd>
          <dt>Running</dt>
          <dd class={data.steam.running ? "ok" : "warn"}>
            {data.steam.running ? `yes (pid ${data.steam.pid})` : "no"}
            {#if data.steam.registryPid && data.steam.registryPid !== data.steam.pid}
              <span class="muted"> · registry says pid {data.steam.registryPid} (stale)</span>
            {/if}
          </dd>
          <dt>Logged in</dt><dd class={data.steam.activeUser ? "ok" : "warn"}>{data.steam.activeUser ? "yes" : "no"}</dd>
        </dl>

        <h2>DayZ</h2>
        {#if data.dayz}
          <dl class="kv">
            <dt>Folder</dt><dd class="wrap">{data.dayz.folder}</dd>
            <dt>Executable</dt><dd class="wrap">{data.dayz.exe}</dd>
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
      </div>

      <div class="col">
        <h2>Libraries</h2>
        <table>
          <thead><tr><th>Path</th><th>Label</th><th class="num">Apps</th><th>DayZ</th></tr></thead>
          <tbody>
            {#each data.libraries as lib (lib.path)}
              <tr><td>{lib.path}</td><td>{lib.label || "–"}</td><td class="num">{lib.appCount}</td><td>{lib.hasDayz ? "✓" : ""}</td></tr>
            {/each}
          </tbody>
        </table>

        <h2>Workshop</h2>
        {#if data.workshop}
          <dl class="kv">
            <dt>Manifest</dt><dd class="wrap">{data.workshop.acfPath}</dd>
            <dt>Status</dt>
            <dd>
              {data.workshop.items.length} items, {fmtBytes(data.workshop.sizeOnDisk)}
              {#if data.workshop.needsUpdate}<span class="warn"> · needs update</span>{/if}
              {#if data.workshop.needsDownload}<span class="warn"> · needs download</span>{/if}
            </dd>
          </dl>
          {#if data.workshop.items.length}
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
          {/if}
        {:else}
          <p class="muted">No appworkshop_221100.acf found.</p>
        {/if}

        <h2>
          <span>!Workshop junctions ({data.junctions.length})</span>
          {#if dangling}
            {#if confirmClean}
              <span class="inline">
                Remove {dangling} dangling junction{dangling === 1 ? "" : "s"}? Only entries whose target folder is gone are removed.
                <button class="btn small danger" onclick={cleanJunctions}>Yes</button>
                <button class="btn small" onclick={() => (confirmClean = false)}>No</button>
              </span>
            {:else}
              <button class="btn small" onclick={() => (confirmClean = true)} disabled={cleaning} title="Delete the junctions in !Workshop whose target folder no longer exists">
                {cleaning ? "Removing…" : `Remove ${dangling} dangling`}
              </button>
            {/if}
          {/if}
        </h2>
        {#if cleanResult}<p class={cleanResult.ok ? "ok small" : "warn small"}>{cleanResult.text}</p>{/if}
        <table>
          <thead><tr><th>Name</th><th>Target</th><th>Workshop ID</th><th>Status</th></tr></thead>
          <tbody>
            {#each data.junctions as j (j.name)}
              <tr>
                <td>{j.name}</td>
                <td class="mono path" title={j.target ?? ""}>{j.target ?? "–"}</td>
                <td class="mono">{j.workshopId ?? "–"}</td>
                <td class={j.targetExists ? "ok" : "warn"}>{j.targetExists ? "exists" : "dangling"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {/if}
</section>

<style>
  .diag { display: flex; flex-direction: column; gap: 6px; min-height: 0; }
  .row { display: flex; align-items: center; gap: 12px; }
  h1 { margin: 0; }
  /* Two columns that share the height; a column scrolls on its own only when the
     window is smaller than its content. */
  .cols { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 3fr); gap: 0 28px; }
  .col { min-height: 0; min-width: 0; overflow: auto; }
  h2 { display: flex; align-items: center; gap: 10px; font-size: 12px; font-weight: 600; margin: 10px 0 4px; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .col > h2:first-child { margin-top: 0; }
  .inline { display: inline-flex; align-items: center; gap: 6px; flex-wrap: wrap; text-transform: none; letter-spacing: 0; font-weight: 400; color: var(--fg); }
  .kv { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 2px 12px; margin: 0; font-size: 12.5px; }
  .kv dt { color: var(--fg-muted); }
  .kv dd { margin: 0; min-width: 0; }
  .kv dd.wrap { overflow-wrap: anywhere; }
  table { border-collapse: collapse; width: 100%; font-size: 12px; }
  th, td { text-align: left; padding: 3px 8px; border-bottom: 1px solid var(--border); vertical-align: top; }
  th { color: var(--fg-muted); font-weight: 500; }
  td.path { max-width: 0; width: 46%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .num { text-align: right; }
  .mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .stale td { color: var(--warn); }
  .warnings { margin: 0; padding-left: 18px; color: var(--warn); font-size: 12px; }
  .small { font-size: 12px; margin: 0; }
  .btn { all: unset; cursor: pointer; padding: 4px 10px; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border); }
  .btn:hover { border-color: var(--accent); }
  .btn:disabled { opacity: 0.6; cursor: default; }
  .btn:focus-visible { outline: 2px solid var(--accent); }
  .btn.small { font-size: 11px; padding: 2px 8px; text-transform: none; letter-spacing: 0; font-weight: 500; }
  .btn.small.on { border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); color: var(--fg); }

  /* Log (D-158): monospace, newest first, its own scroll so the page still fits. */
  .logs { max-height: 240px; overflow: auto; border: 1px solid var(--border); border-radius: var(--radius); background: var(--bg); padding: 4px 0; }
  .logline { display: grid; grid-template-columns: 74px 92px minmax(0, 1fr); gap: 8px; padding: 2px 8px; font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; line-height: 1.45; }
  .logline:hover { background: var(--bg-row); }
  .logtime { color: var(--fg-muted); }
  .logtarget { color: var(--accent); overflow: hidden; text-overflow: ellipsis; }
  .logmsg { color: var(--fg); overflow-wrap: anywhere; }
  .logline.warn .logmsg { color: var(--warn); }
  .logline.error .logmsg { color: var(--danger); }
  .logline.debug .logmsg { color: var(--fg-muted); }
  .btn.danger { border-color: var(--danger); color: var(--danger); }
</style>
