<script lang="ts">
  // Join flow (docs/02 §6, docs/06 §6): plan → sync missing mods with progress → launch.
  // A full server can be waited for here (D-074): the dialog polls A2S_INFO every
  // 10 s and starts the game the moment the server reports a free slot.
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { invokeLogged as invoke } from "./log";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
  import { fmtBytes, type ItemProgress, type JoinPlan, type LaunchExited, type Launched, type LaunchProfile, type ServerSlots, type Settings, type SyncDone, type SyncProgress } from "./types";

  let { serverId, onClose }: { serverId: string; onClose: () => void } = $props();

  type Phase = "planning" | "ready" | "syncing" | "waiting" | "launching" | "running" | "exited" | "error";
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

  // Saved launch profiles (D-088): one can be picked for this launch only.
  let profiles = $state<LaunchProfile[]>([]);
  let profile = $state("");
  $effect(() => {
    invoke<Settings>("settings_get")
      .then((s) => (profiles = s.launchProfiles ?? []))
      .catch(() => {});
  });

  // Live slot count, refreshed on open and while waiting.
  const POLL_MS = 10_000;
  let slots = $state<ServerSlots | null>(null);
  let waitForSlot = $state(false);
  let checks = $state(0);
  let waitedSecs = $state(0);
  let pollTimer: ReturnType<typeof setInterval> | undefined;
  let clockTimer: ReturnType<typeof setInterval> | undefined;

  const full = $derived(!!slots && slots.maxPlayers > 0 && slots.players >= slots.maxPlayers);
  const toSync = $derived(plan ? plan.mods.filter((m) => !m.installed || m.needsUpdate) : []);
  const canLaunch = $derived(!!plan && plan.gameFound && plan.steamRunning && plan.battleyePresent && toSync.length === 0 && (!plan.passwordRequired || password.length > 0));
  const canSync = $derived(!!plan && plan.steamRunning && toSync.length > 0);
  const downloadedBytes = $derived([...progress.values()].reduce((a, p) => a + (p.state === "installed" ? p.total : p.downloaded), 0));
  const totalBytes = $derived([...progress.values()].reduce((a, p) => a + p.total, 0));
  // The median server in the cached list needs 25 mods and the worst needs 139, so
  // this is the longest wait in the product — and it showed a byte count creeping up,
  // no rate and no estimate (D-211). The window is wide because Steam reports an
  // item as a step change when it finishes, not as a smooth curve.
  const RATE_WINDOW_MS = 10_000;
  let rateSamples: { t: number; bytes: number }[] = [];
  let rate = $state(0);
  $effect(() => {
    if (phase !== "syncing") {
      rateSamples = [];
      rate = 0;
      return;
    }
    const bytes = downloadedBytes;
    const now = Date.now();
    rateSamples.push({ t: now, bytes });
    while (rateSamples.length > 2 && now - rateSamples[0]!.t > RATE_WINDOW_MS) rateSamples.shift();
    const first = rateSamples[0]!;
    const dt = (now - first.t) / 1000;
    // Under two seconds of history the number would jump around more than it informs.
    rate = dt >= 2 ? Math.max(0, (bytes - first.bytes) / dt) : 0;
  });
  const etaSecs = $derived(rate > 0 && totalBytes > downloadedBytes ? Math.round((totalBytes - downloadedBytes) / rate) : 0);
  const etaText = $derived(
    etaSecs <= 0 ? "" : etaSecs < 60 ? `${etaSecs} s` : etaSecs < 5400 ? `${Math.max(1, Math.round(etaSecs / 60))} min` : `${(etaSecs / 3600).toFixed(1)} h`,
  );
  // Only the launch itself is uninterruptible (D-151). A Workshop download used to lock
  // the dialog — and with it the window — until Steam finished or the backend's 45-minute
  // timeout fired. Closing now just stops watching: Steam keeps downloading and the Mods
  // page shows the progress.
  const busy = $derived(phase === "launching");

  /** Consecutive failed probes, so a server that dies mid-wait says so (D-194). */
  let slotMisses = $state(0);

  async function refreshSlots(): Promise<ServerSlots | null> {
    try {
      slots = await invoke<ServerSlots>("server_slots", { id: serverId });
      slotMisses = 0;
      return slots;
    } catch {
      // Returning the last snapshot made every failure look like "still full": the
      // wait loop's guard is `!s`, which was never null once one probe had worked, so
      // a server that had gone away counted up forever.
      slotMisses += 1;
      return null;
    }
  }

  $effect(() => {
    // Subscribed before anything is awaited, and cleaned up through the promises
    // rather than their results: every argument used to be awaited before `push`
    // ran, so the array was empty until all three resolved and an unmount inside
    // the IPC round trip unsubscribed nothing (D-222).
    const pending: Promise<UnlistenFn>[] = [
      listen<SyncProgress>("mods:progress", (ev) => {
        if (ev.payload.job !== job) return;
        syncInfo = ev.payload;
        progress = new Map(ev.payload.items.map((i) => [i.id, i]));
      }),
      listen<SyncDone>("mods:done", (ev) => {
        if (ev.payload.job !== job) return;
        progress = new Map(ev.payload.items.map((i) => [i.id, i]));
        if (ev.payload.ok) {
          if (plan) plan = { ...plan, mods: plan.mods.map((m) => ({ ...m, installed: true, needsUpdate: false })), missing: 0, updates: 0 };
          phase = "ready";
          if (autoLaunch) void joinNow();
        } else {
          error = ev.payload.error ?? "Mod download failed";
          phase = "error";
        }
      }),
      listen<LaunchExited>("launch:exited", (ev) => {
        if (launched && ev.payload.pid === launched.pid) {
          exit = ev.payload;
          phase = "exited";
        }
      }),
    ];
    (async () => {
      void refreshSlots();
      try {
        plan = await invoke<JoinPlan>("join_plan", { id: serverId });
        phase = "ready";
      } catch (e) {
        error = String(e);
        phase = "error";
      }
    })();
    return () => {
      pending.forEach((p) => void p.then((u) => u()));
      stopWaiting();
    };
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

  /** Join: either straight away (DayZ queues in-game if needed) or after a slot frees up. */
  async function joinNow() {
    if (waitForSlot && full) startWaiting();
    else await launch();
  }

  function startWaiting() {
    stopWaiting();
    phase = "waiting";
    error = null;
    checks = 0;
    waitedSecs = 0;
    const started = Date.now();
    clockTimer = setInterval(() => (waitedSecs = Math.floor((Date.now() - started) / 1000)), 1000);
    const poll = async () => {
      const s = await refreshSlots();
      checks += 1;
      if (phase !== "waiting" || !s) return;
      if (s.players < s.maxPlayers) {
        stopWaiting();
        try {
          await getCurrentWindow().requestUserAttention(UserAttentionType.Informational);
        } catch {
          /* attention request not permitted; the launch is the signal */
        }
        await launch();
      }
    };
    pollTimer = setInterval(() => void poll(), POLL_MS);
    void poll();
  }

  function stopWaiting() {
    clearInterval(pollTimer);
    clearInterval(clockTimer);
    pollTimer = clockTimer = undefined;
  }

  function cancelWaiting() {
    stopWaiting();
    phase = "ready";
  }

  async function launch() {
    if (!plan) return;
    error = null;
    phase = "launching";
    try {
      launched = await invoke<Launched>("launch_game", { id: serverId, password: password || null, profile: profile || null });
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

  const mmss = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  /** Whatever had focus when the dialog opened, so closing does not drop the
   *  keyboard user back to `<body>` and lose the row they came from (D-198). */
  const opener = typeof document !== "undefined" ? (document.activeElement as HTMLElement | null) : null;

  function close() {
    stopWaiting();
    onClose();
    // After the dialog is gone: focusing an element that is about to be hidden does
    // nothing useful.
    queueMicrotask(() => {
      if (opener?.isConnected) opener.focus();
    });
  }

  /** The dialog element, focused on open so the page behind it stops seeing keys. */
  let dialogEl = $state<HTMLElement | null>(null);
  $effect(() => {
    // The first focusable control, or the dialog itself: either way the grid behind
    // no longer has focus, and Tab starts inside the dialog.
    //
    // Depends on the phase deliberately. The footer sits inside
    // `{#if phase === "waiting"}`, so pressing Join destroys the button that had
    // focus and drops it to `<body>` — where `trap`, which is bound to the dialog,
    // never sees another key. One Shift+Tab then reached the grid behind the modal,
    // which is the D-184 failure all over again (D-197).
    void phase;
    if (dialogEl?.contains(document.activeElement)) return;
    const first = dialogEl?.querySelector<HTMLElement>(
      'input:not([disabled]), select:not([disabled]), button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
    );
    (first ?? dialogEl)?.focus();
  });

  /** Keeps Tab inside the dialog while it is open. */
  function trap(e: KeyboardEvent) {
    if (e.key !== "Tab" || !dialogEl) return;
    const items = [...dialogEl.querySelectorAll<HTMLElement>('input:not([disabled]), select:not([disabled]), button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])')].filter(
      (el) => el.offsetParent !== null,
    );
    if (items.length === 0) return;
    const first = items[0]!;
    const last = items[items.length - 1]!;
    const active = document.activeElement as HTMLElement | null;
    if (e.shiftKey && (active === first || active === dialogEl)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !busy) close();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && !busy && close()}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="join-title" tabindex="-1" bind:this={dialogEl} onkeydown={trap}>
    <header>
      <h2 id="join-title">{plan?.name ?? "Join server"}</h2>
      <span class="muted">
        {#if plan}{plan.ip}:{plan.gamePort} · v{plan.serverVersion}{/if}
        {#if slots}
          · <span class:warn={full}>{slots.players}/{slots.maxPlayers}{#if slots.queue} · {slots.queue} in queue{/if}</span>
        {/if}
      </span>
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
            {#each plan.mods as m, i (`${m.id}#${i}`)}
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
          <input type="password" bind:value={password} autocomplete="off" disabled={busy || phase === "waiting" || phase === "running"} />
        </label>
      {/if}

      {#if full && (phase === "ready" || phase === "error")}
        <label class="wait">
          <input type="checkbox" bind:checked={waitForSlot} />
          <span>
            <strong>The server is full.</strong> Wait here for a free slot and join automatically (checked every 10 s). Leave this off to join now and stand in DayZ's own login queue.
          </span>
        </label>
      {/if}

      <p class="muted small">
        Profile name: <strong>{plan.profileName || "(Steam persona)"}</strong> · change it in Settings
        {#if profiles.length}
          · launch with
          <select class="pick" bind:value={profile} disabled={busy || phase === "waiting" || phase === "running"} aria-label="Launch profile">
            <option value="">current settings</option>
            {#each profiles as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
          </select>
        {/if}
      </p>

      {#if phase === "syncing" && syncInfo}
        <p class="status" role="status" aria-live="polite">Downloading via Steam… {syncInfo.installed}/{syncInfo.total} installed{#if totalBytes > 0} · {fmtBytes(downloadedBytes)} of {fmtBytes(totalBytes)}{/if}{#if rate > 0} · {fmtBytes(rate)}/s{#if etaText} · about {etaText} left{/if}{/if}</p>
      {:else if phase === "waiting"}
        <p class="status" role="status" aria-live="polite">
          {#if slotMisses >= 3}
            The server has stopped answering — {slotMisses} checks in a row. Still trying · {mmss(waitedSecs)}
          {:else}
            Waiting for a free slot… {#if slots}{slots.players}/{slots.maxPlayers}{#if slots.queue} · {slots.queue} in queue{/if} · {/if}{checks} check{checks === 1 ? "" : "s"} · {mmss(waitedSecs)}
          {/if}
        </p>
        <p class="muted small">DayZ starts as soon as the server reports a free slot; the window will flash in the taskbar.</p>
      {:else if phase === "launching"}
        <p class="status" role="status" aria-live="polite">Starting DayZ through BattlEye…</p>
      {:else if phase === "running" && launched}
        <p class="status ok">DayZ is running (pid {launched.pid}). You can close this window.</p>
        <details class="cmd"><summary class="muted small">Command line</summary><code>{launched.commandLine}</code></details>
      {:else if phase === "exited" && exit}
        <p class="status" role="status" aria-live="polite" class:warn={exit.code !== 0}>DayZ exited{exit.code != null ? ` with code ${exit.code}` : ""}.</p>
      {/if}
    {/if}

    {#if error}<p class="error">{error}</p>{/if}

    <footer>
      {#if phase === "waiting"}
        <button class="btn secondary" onclick={cancelWaiting}>Stop waiting</button>
        <button class="btn" onclick={() => { stopWaiting(); void launch(); }} disabled={!canLaunch}>Join now anyway</button>
      {:else}
        <button class="btn secondary" onclick={close} disabled={busy} title={phase === "syncing" ? "Steam keeps downloading in the background; watch it on the Mods page" : undefined}>
          {phase === "running" || phase === "exited" ? "Close" : phase === "syncing" ? "Close (keeps downloading)" : "Cancel"}
        </button>
        {#if phase === "ready" || phase === "error" || phase === "exited"}
          {#if toSync.length}
            <button class="btn" onclick={() => sync(true)} disabled={!canSync || (plan?.passwordRequired && !password)}>Download {toSync.length} mod{toSync.length === 1 ? "" : "s"} and join</button>
            <button class="btn secondary" onclick={() => sync(false)} disabled={!canSync}>Download only</button>
          {:else}
            <button class="btn" onclick={joinNow} disabled={!canLaunch}>{waitForSlot && full ? "Wait and join" : "Join"}</button>
          {/if}
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
  .wait { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: var(--radius); border: 1px solid color-mix(in srgb, var(--warn) 50%, var(--border)); font-size: 12.5px; color: var(--fg-muted); cursor: pointer; }
  .wait input { margin-top: 2px; }
  .wait strong { color: var(--fg); }
  .small { font-size: 12px; margin: 0; }
  .pick { padding: 2px 6px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); font-size: 12px; }
  .status { margin: 0; font-size: 12.5px; }
  .cmd code { display: block; font-size: 11px; white-space: pre-wrap; word-break: break-all; color: var(--fg-muted); margin-top: 4px; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .error { color: var(--danger); margin: 0; font-size: 12.5px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
  .btn { all: unset; cursor: pointer; padding: 7px 16px; border-radius: var(--radius); background: var(--accent); color: var(--accent-fg); font-weight: 600; }
  .btn:disabled { opacity: 0.45; cursor: default; }
</style>
