<script lang="ts">
  // Join flow (docs/02 §6, docs/06 §6): plan → sync missing mods with progress → launch.
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { fmtBytes, type ItemProgress, type JoinPlan, type LaunchExited, type Launched, type SyncDone, type SyncProgress } from "./types";

  let { serverId, onClose }: { serverId: string; onClose: () => void } = $props();

  type Phase = "planning" | "ready" | "syncing" | "launching" | "running" | "exited" | "error";
  let phase = $state<Phase>("planning");
  let plan = $state<JoinPlan | null>(null);
  let error = $state<string | null>(null);
  let password = $state("");
  let progress = $state<Map<number, ItemProgress>>(new Map());
  let syncInfo = $state<SyncProgress | null>(null);
  let launched = $state<Launched | null>(null);
  let exit = $state<LaunchExited | null>(null);
  let autoLaunch = false;
  const job = Date.now();

  const toSync = $derived(plan ? plan.mods.filter((m) => !m.installed || m.needsUpdate) : []);
  const canLaunch = $derived(!!plan && plan.gameFound && plan.steamRunning && plan.battleyePresent && toSync.length === 0 && (!plan.passwordRequired || password.length > 0));
  const canSync = $derived(!!plan && plan.steamRunning && toSync.length > 0);
  const downloadedBytes = $derived([...progress.values()].reduce((a, p) => a + (p.state === "installed" ? p.total : p.downloaded), 0));
  const totalBytes = $derived([...progress.values()].reduce((a, p) => a + p.total, 0));

  $effect(() => {
    const unlisteners: UnlistenFn[] = [];
    (async () => {
      unlisteners.push(
        await listen<SyncProgress>("mods:progress", (ev) => {
          if (ev.payload.job !== job) return;
          syncInfo = ev.payload;
          progress = new Map(ev.payload.items.map((i) => [i.id, i]));
        }),
        await listen<SyncDone>("mods:done", (ev) => {
          if (ev.payload.job !== job) return;
          progress = new Map(ev.payload.items.map((i) => [i.id, i]));
          if (ev.payload.ok) {
            if (plan) plan = { ...plan, mods: plan.mods.map((m) => ({ ...m, installed: true, needsUpdate: false })), missing: 0, updates: 0 };
            phase = "ready";
            if (autoLaunch) void launch();
          } else {
            error = ev.payload.error ?? "Mod download failed";
            phase = "error";
          }
        }),
        await listen<LaunchExited>("launch:exited", (ev) => {
          if (launched && ev.payload.pid === launched.pid) {
            exit = ev.payload;
            phase = "exited";
          }
        }),
      );
      try {
        plan = await invoke<JoinPlan>("join_plan", { id: serverId });
        phase = "ready";
      } catch (e) {
        error = String(e);
        phase = "error";
      }
    })();
    return () => unlisteners.forEach((u) => u());
  });

  async function sync(thenLaunch: boolean) {
    if (!plan) return;
    autoLaunch = thenLaunch;
    error = null;
    phase = "syncing";
    try {
      await invoke("mods_sync", { job, ids: toSync.map((m) => m.id) });
    } catch (e) {
      error = String(e);
      phase = "error";
    }
  }

  async function launch() {
    if (!plan) return;
    error = null;
    phase = "launching";
    try {
      launched = await invoke<Launched>("launch_game", { id: serverId, password: password || null });
      phase = "running";
    } catch (e) {
      error = String(e);
      phase = "error";
    }
  }

  const stateLabel: Record<ItemProgress["state"], string> = {
    subscribing: "Subscribing…",
    subscribed: "Queued",
    pending: "Queued",
    downloading: "Downloading",
    needs_update: "Update queued",
    installed: "Installed",
    failed: "Failed",
  };

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && phase !== "syncing" && phase !== "launching") onClose();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && phase !== "syncing" && onClose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="join-title">
    <header>
      <h2 id="join-title">{plan?.name ?? "Join server"}</h2>
      {#if plan}<span class="muted">{plan.ip}:{plan.gamePort} · v{plan.serverVersion}</span>{/if}
    </header>

    {#if phase === "planning"}
      <p class="muted">Reading the server's mod list and checking your Workshop…</p>
    {:else if plan}
      {#if plan.warnings.length}
        <ul class="warnings">
          {#each plan.warnings as w (w)}<li>{w}</li>{/each}
        </ul>
      {/if}

      {#if plan.mods.length}
        <section class="mods">
          <h3>
            Mods ({plan.mods.length})
            {#if toSync.length}<span class="warn"> · {toSync.length} to download{#if plan.downloadBytes} ({fmtBytes(plan.downloadBytes)}){/if}</span>{:else}<span class="ok"> · all installed</span>{/if}
          </h3>
          <ul>
            {#each plan.mods as m (m.id)}
              {@const p = progress.get(m.id)}
              <li class:missing={!m.installed} class:update={m.installed && m.needsUpdate}>
                <span class="tick" aria-hidden="true">{p ? (p.state === "installed" ? "✓" : p.state === "failed" ? "✕" : "…") : m.installed && !m.needsUpdate ? "✓" : "○"}</span>
                <span class="mname" title={m.title ?? m.name}>{m.title ?? m.name}</span>
                <span class="msize muted">{m.size != null ? fmtBytes(m.size) : ""}</span>
                <span class="mstate muted">
                  {#if p}
                    {stateLabel[p.state]}{#if p.state === "downloading" && p.total > 0} {Math.round((100 * p.downloaded) / p.total)}%{/if}
                  {:else if !m.installed}missing{:else if m.needsUpdate}update{:else}installed{/if}
                </span>
                {#if p && p.state !== "installed" && p.total > 0}
                  <span class="pbar"><span class="pfill" style="width: {Math.min(100, (100 * p.downloaded) / p.total)}%"></span></span>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {:else if plan.rulesOk}
        <p class="muted">Vanilla server, no mods required.</p>
      {/if}

      {#if plan.passwordRequired}
        <label class="field">
          Password
          <input type="password" bind:value={password} autocomplete="off" disabled={phase === "syncing" || phase === "launching" || phase === "running"} />
        </label>
      {/if}

      <p class="muted small">Profile name: <strong>{plan.profileName || "(Steam persona)"}</strong> · change it in Settings</p>

      {#if phase === "syncing" && syncInfo}
        <p class="status">Downloading via Steam… {syncInfo.installed}/{syncInfo.total} installed{#if totalBytes > 0} · {fmtBytes(downloadedBytes)} of {fmtBytes(totalBytes)}{/if}</p>
      {:else if phase === "launching"}
        <p class="status">Starting DayZ through BattlEye…</p>
      {:else if phase === "running" && launched}
        <p class="status ok">DayZ is running (pid {launched.pid}). You can close this window.</p>
        <details class="cmd"><summary class="muted small">Command line</summary><code>{launched.commandLine}</code></details>
      {:else if phase === "exited" && exit}
        <p class="status" class:warn={exit.code !== 0}>DayZ exited{exit.code != null ? ` with code ${exit.code}` : ""}.</p>
      {/if}
    {/if}

    {#if error}<p class="error">{error}</p>{/if}

    <footer>
      <button class="btn secondary" onclick={onClose} disabled={phase === "syncing" || phase === "launching"}>{phase === "running" || phase === "exited" ? "Close" : "Cancel"}</button>
      {#if phase === "ready" || phase === "error" || phase === "exited"}
        {#if toSync.length}
          <button class="btn" onclick={() => sync(true)} disabled={!canSync || (plan?.passwordRequired && !password)}>Download {toSync.length} mod{toSync.length === 1 ? "" : "s"} and join</button>
          <button class="btn secondary" onclick={() => sync(false)} disabled={!canSync}>Download only</button>
        {:else}
          <button class="btn" onclick={launch} disabled={!canLaunch}>Join</button>
        {/if}
      {/if}
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.45); display: flex; align-items: center; justify-content: center; z-index: 50; }
  .dialog { width: min(640px, calc(100vw - 32px)); max-height: calc(100vh - 64px); overflow: auto; background: var(--bg-elev); border: 1px solid var(--border); border-radius: 12px; padding: 18px 20px; display: flex; flex-direction: column; gap: 12px; box-shadow: 0 20px 60px rgba(0, 0, 0, 0.45); }
  header h2 { margin: 0; font-size: 16px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  h3 { margin: 0 0 6px; font-size: 12px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .warnings { margin: 0; padding-left: 18px; color: var(--warn); font-size: 12.5px; }
  .mods ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; max-height: 40vh; overflow: auto; font-size: 12.5px; }
  .mods li { display: grid; grid-template-columns: 16px 1fr auto 110px; grid-template-areas: "tick name size state" "tick bar bar bar"; gap: 2px 8px; align-items: center; }
  .tick { grid-area: tick; color: var(--ok); }
  .missing .tick, .update .tick { color: var(--warn); }
  .mname { grid-area: name; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .msize { grid-area: size; font-size: 11.5px; }
  .mstate { grid-area: state; font-size: 11.5px; text-align: right; }
  .pbar { grid-area: bar; height: 3px; background: var(--bg-row); border-radius: 2px; overflow: hidden; }
  .pfill { display: block; height: 100%; background: var(--accent); transition: width 200ms; }
  .field { display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; color: var(--fg-muted); }
  .field input { padding: 6px 10px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); }
  .small { font-size: 12px; margin: 0; }
  .status { margin: 0; font-size: 12.5px; }
  .cmd code { display: block; font-size: 11px; white-space: pre-wrap; word-break: break-all; color: var(--fg-muted); margin-top: 4px; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .error { color: var(--danger); margin: 0; font-size: 12.5px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
  .btn { all: unset; cursor: pointer; padding: 7px 16px; border-radius: var(--radius); background: var(--accent); color: #111; font-weight: 600; }
  .btn.secondary { background: var(--bg-row); color: var(--fg); border: 1px solid var(--border); font-weight: 500; }
  .btn:disabled { opacity: 0.45; cursor: default; }
  .btn:focus-visible { outline: 2px solid var(--fg); }
</style>
