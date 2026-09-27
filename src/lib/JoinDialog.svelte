<script lang="ts">
  // Join flow (docs/02 §6, docs/06 §6): plan → sync missing mods with progress → launch.
  // A full server can be waited for here (D-074): the dialog polls A2S_INFO every
  // 10 s and starts the game the moment the server reports a free slot.
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { tick, untrack } from "svelte";
  import { invoke as invokeQuiet } from "@tauri-apps/api/core";
  import { invokeLogged as invoke, logInfo, logWarn } from "./log";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
  import { servers } from "./state/servers.svelte";
  import { fmtBytes, isUntrusted, type ItemProgress, type JoinPlan, type LaunchExited, type Launched, type LaunchProfile, type ServerSlots, type Settings, type SyncDone, type SyncProgress } from "./types";

  let { serverId, onClose }: { serverId: string; onClose: () => void } = $props();
  /** The server this dialog is for, read once. The prop is a live read of the store's
   *  `joiningId`, and the launch read it after awaiting a re-plan: a dialog taken down
   *  meanwhile launched null, or the next dialog's server with this one's password and
   *  profile (D-295). */
  const sid = untrack(() => serverId);

  type Phase = "planning" | "ready" | "syncing" | "waiting" | "launching" | "running" | "exited" | "error";
  let phase = $state<Phase>("planning");
  let plan = $state<JoinPlan | null>(null);
  let error = $state<string | null>(null);
  /** What happened that is not a failure: a download replaced by a newer one (D-277). */
  let note = $state<string | null>(null);
  let password = $state("");
  let progress = $state<Map<number, ItemProgress>>(new Map());
  let syncInfo = $state<SyncProgress | null>(null);
  let launched = $state<Launched | null>(null);
  let exit = $state<LaunchExited | null>(null);
  /** What the dialog says out loud, in one region that is always there. Each phase had
   *  a `role="status"` paragraph of its own, created together with its text, which is
   *  the pattern that often goes unannounced (D-224); a download that finished without
   *  launching said nothing at all (D-291). */
  let announce = $state("");
  /** Says `text` from that region, even when it is what the region already holds: an
   *  unchanged text is not read again, so a retried step went unsaid. `""` clears it, so
   *  an error or a stop leaves no stale step behind (D-295). */
  function say(text: string) {
    announce = "";
    if (text) void tick().then(() => (announce = text));
  }
  let autoLaunch = false;
  const job = Date.now();
  const exited = new Map<number, LaunchExited>();
  /** When the plan was made. A plan outlives a long wait for a slot, and a server that
   *  restarts meanwhile can add or update a mod (D-240). */
  let plannedAt = 0;
  const PLAN_MAX_AGE_MS = 2 * 60_000;

  /** Asks the host for a fresh plan; false when it could not answer. */
  async function replan(): Promise<boolean> {
    try {
      plan = await invoke<JoinPlan>("join_plan", { id: sid });
      plannedAt = Date.now();
      return true;
    } catch {
      return false;
    }
  }

  // "Steam is not running" is true of the moment the plan was made. Once the session is
  // back the plan is made again, so Join and Download come back without Cancel and a
  // second Join; once each time Steam comes back (row 14, F11).
  let replannedForSteam = false;
  $effect(() => {
    const up = servers.steam?.initialized ?? false;
    if (!up) replannedForSteam = false;
    else if (phase === "ready" && plan && !plan.steamRunning && !replannedForSteam) {
      replannedForSteam = true;
      void replan();
    }
  });

  // Saved launch profiles (D-088): one can be picked for this launch only.
  let profiles = $state<LaunchProfile[]>([]);
  let profile = $state("");
  /** The name DayZ will be started with: the picked profile's, not the settings' (D-240). */
  const shownProfileName = $derived.by(() => {
    const picked = profile ? profiles.find((p) => p.name === profile) : undefined;
    return picked ? picked.profileName.trim() : (plan?.profileName ?? "");
  });
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

  /** The slot count is the server's own INFO claim, which the list does not take on trust
   *  (D-038). A server it has flagged as untrusted is never called full: a faker claiming
   *  60/60 could be waited on for ever. And the queue beside the count follows the list's
   *  rule (`queueOf`), so a junk queue tag no longer read "7/60 · 172 in queue" (D-296). */
  const untrusted = $derived.by(() => {
    const row = servers.rowsTick >= 0 ? servers.rows.get(sid) : undefined;
    return !!row && isUntrusted(row);
  });
  const claimsFull = $derived(!!slots && slots.maxPlayers > 0 && slots.players >= slots.maxPlayers);
  const full = $derived(claimsFull && !untrusted);
  const queue = $derived(!untrusted && slots?.queue && slots.players >= slots.maxPlayers - 2 ? slots.queue : 0);
  const toSync = $derived(plan ? plan.mods.filter((m) => !m.installed || m.needsUpdate) : []);
  const canLaunch = $derived(!!plan && plan.gameFound && plan.steamRunning && plan.battleyePresent && toSync.length === 0 && (!plan.passwordRequired || password.length > 0));
  const canSync = $derived(!!plan && plan.steamRunning && toSync.length > 0);
  /** The warnings, as the description of the primary button (D-291). Every footer button
   *  carried them, so each Tab read the whole list again (D-295). */
  const warned = $derived(plan?.warnings.length ? "join-warnings" : undefined);
  const downloadedBytes = $derived([...progress.values()].reduce((a, p) => a + (p.state === "installed" ? p.total : p.downloaded), 0));
  const totalBytes = $derived([...progress.values()].reduce((a, p) => a + p.total, 0));
  // The median server in the cached list needs 25 mods and the worst needs 139, so
  // this is the longest wait in the product — and it showed a byte count creeping up,
  // no rate and no estimate (D-211). The window is wide because Steam reports an
  // item as a step change when it finishes, not as a smooth curve.
  const RATE_WINDOW_MS = 10_000;
  let rateSamples: { t: number; bytes: number }[] = [];
  let rate = $state(0);
  /** A sample a second whether or not the bytes moved: the effect below re-ran only on
   *  a change, so a download Steam had stopped kept its last speed and time left on
   *  screen for up to 15 minutes (row 14, F4). */
  let rateTick = $state(0);
  $effect(() => {
    if (phase !== "syncing") return;
    const t = setInterval(() => rateTick++, 1000);
    return () => clearInterval(t);
  });
  $effect(() => {
    if (phase !== "syncing") {
      rateSamples = [];
      rate = 0;
      return;
    }
    void rateTick;
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
      // Not through the logging wrapper: a wait on a server that stopped answering
      // wrote an error line every 10 s, 360 an hour in the Logs page's "Problems only".
      // The third miss in a row is logged once, and the recovery (D-295).
      slots = await invokeQuiet<ServerSlots>("server_slots", { id: sid });
      if (slotMisses >= 3) logInfo("join", `server_slots answers again for ${sid} after ${slotMisses} misses`);
      slotMisses = 0;
      return slots;
    } catch (e) {
      // Returning the last snapshot made every failure look like "still full": the
      // wait loop's guard is `!s`, which was never null once one probe had worked, so
      // a server that had gone away counted up forever.
      slotMisses += 1;
      if (slotMisses === 3) logWarn("join", `server_slots failed three times in a row for ${sid}: ${String(e)}`);
      return null;
    }
  }

  /** Mods a download reports installed, marked so in the plan with the bytes left. Only
   *  a complete download updated the plan, so after a failed one the rows that finished
   *  showed ✓ under "5 to download" and a button offering all five again (D-295). */
  function markInstalled(items: ItemProgress[]) {
    if (!plan) return;
    const done = new Set(items.filter((i) => i.state === "installed").map((i) => i.id));
    if (done.size === 0) return;
    const mods = plan.mods.map((m) => (done.has(m.id) ? { ...m, installed: true, needsUpdate: false } : m));
    plan = {
      ...plan,
      mods,
      missing: mods.filter((m) => !m.installed).length,
      updates: mods.filter((m) => m.installed && m.needsUpdate).length,
      downloadBytes: mods.filter((m) => !m.installed || m.needsUpdate).reduce((a, m) => a + (m.size ?? 0), 0),
    };
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
          else say("All mods are installed.");
        } else if (ev.payload.superseded) {
          markInstalled(ev.payload.items);
          // Not a failure: a newer download took over, and this one can be started
          // again from here (D-277).
          note = "Replaced by a newer download.";
          say(note);
          phase = "ready";
        } else {
          markInstalled(ev.payload.items);
          // Led by the mod it failed on (D-277). The alert says it; the region keeps no
          // "Downloading" behind it (D-295).
          const why = ev.payload.error ?? "Mod download failed";
          const m = plan?.mods.find((x) => x.id === ev.payload.failedId);
          error = m ? `${m.title ?? m.name}: ${why}` : why;
          phase = "error";
          say("");
        }
      }),
      listen<LaunchExited>("launch:exited", (ev) => {
        // Kept whoever it is for: an instant BattlEye exit (D-165) can be emitted
        // before `launch_game` has replied, while `launched` is still unset (D-240).
        exited.set(ev.payload.pid, ev.payload);
        if (launched && ev.payload.pid === launched.pid) {
          exit = ev.payload;
          phase = "exited";
          say(exitText(ev.payload));
        }
      }),
    ];
    (async () => {
      void refreshSlots();
      try {
        const p = await invoke<JoinPlan>("join_plan", { id: sid });
        plan = p;
        plannedAt = Date.now();
        phase = "ready";
        // The server first: the dialog was still titled "Join server" when focus landed
        // on it, and nothing said the name after (D-295). What stands in the way next
        // (D-291) — unless focus lands on the primary button, whose description is the
        // same warnings, which were then read twice (D-295) — then what the join will do.
        const toGet = p.mods.filter((m) => !m.installed || m.needsUpdate).length;
        const primaryReadsThem = !p.passwordRequired && (toGet ? canSync : canLaunch);
        say(
          [
            `${p.name}.`,
            ...(primaryReadsThem ? [] : p.warnings),
            toGet ? `${toGet} mod${toGet === 1 ? "" : "s"} to download.` : p.mods.length ? "All mods are installed." : "",
          ].join(" "),
        );
      } catch (e) {
        error = String(e);
        phase = "error";
      }
    })();
    return () => {
      // Taken down without `close()` — a prune cleared the server under it — so the
      // focus it held goes back to where the dialog was opened from (D-291).
      const wasClosed = closed;
      closed = true;
      pending.forEach((p) => void p.then((u) => u()));
      stopWaiting();
      if (!wasClosed)
        queueMicrotask(() => {
          if (!document.activeElement || document.activeElement === document.body) returnFocus();
        });
    };
  });

  const exitText = (e: LaunchExited) => `DayZ exited${e.code != null ? ` with code ${e.code}` : ""}.`;
  // The server stopped answering while the dialog waited: said once, when it happens.
  $effect(() => {
    if (phase === "waiting" && slotMisses === 3) say("The server has stopped answering. Still trying.");
  });

  async function sync(thenLaunch: boolean) {
    if (!plan) return;
    autoLaunch = thenLaunch;
    error = null;
    note = null;
    phase = "syncing";
    const n = toSync.length;
    try {
      await invoke("mods_sync", { job, ids: toSync.map((m) => m.id) });
      // Said once Steam has taken the job, which it refuses at once when it is not
      // running, and not over a finish that came first (D-295).
      if (phase === "syncing") say(`Downloading ${n} mod${n === 1 ? "" : "s"} through Steam.`);
    } catch (e) {
      error = String(e);
      phase = "error";
      say("");
    }
  }

  /** Join: either straight away (DayZ queues in-game if needed) or after a slot frees up. */
  async function joinNow() {
    if (waitForSlot && full) startWaiting();
    else await launch();
  }

  /** Moved on by every start and end of a wait the player makes, so a poll from an
   *  earlier wait, or one still inside its attention request, does nothing: the phase
   *  stayed "waiting" through that request, and "Stop waiting" then still started DayZ,
   *  and "Join now anyway" started it twice (D-295). */
  let waitGen = 0;

  function startWaiting() {
    stopWaiting();
    const gen = ++waitGen;
    phase = "waiting";
    say("Waiting for a free slot. DayZ starts as soon as one opens.");
    error = null;
    note = null;
    checks = 0;
    waitedSecs = 0;
    // A wait started again began at the last one's count, and at 0:00 read "stopped
    // answering · 5 checks in a row" (D-295).
    slotMisses = 0;
    const started = Date.now();
    clockTimer = setInterval(() => (waitedSecs = Math.floor((Date.now() - started) / 1000)), 1000);
    // `closed` as well as the phase: closing never changed the phase, so a poll already
    // in flight when the player closed the dialog still found "waiting", and a slot
    // freeing up then started DayZ from a dialog that was gone (D-265). And this wait's
    // own generation, checked after every await (D-295).
    const current = () => !closed && gen === waitGen && phase === "waiting";
    const poll = async () => {
      const s = await refreshSlots();
      if (!current()) return;
      checks += 1;
      if (!s) return;
      if (s.players < s.maxPlayers) {
        stopWaiting();
        try {
          await getCurrentWindow().requestUserAttention(UserAttentionType.Informational);
        } catch {
          /* attention request not permitted; the launch is the signal */
        }
        if (!current()) return;
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
    waitGen++;
    stopWaiting();
    phase = "ready";
    // "Still trying" stayed in the region after the wait had stopped (D-295).
    say("");
  }

  function joinAnyway() {
    waitGen++;
    stopWaiting();
    void launch();
  }

  async function launch() {
    // One at a time: a slot poll and "Join now anyway" could both get here (D-295).
    if (!plan || phase === "launching") return;
    error = null;
    note = null;
    phase = "launching";
    // The plan was never made again, so a mod the server added or updated while this
    // dialog waited for a slot sent the launch into "not installed; sync mods first",
    // with only Join — the same failure — on offer (D-240).
    if (Date.now() - plannedAt > PLAN_MAX_AGE_MS && (await replan()) && toSync.length > 0) {
      error = "Some of this server's mods need downloading since its list was read. Download them to join.";
      phase = "ready";
      say("");
      return;
    }
    // Closed during the re-plan; or the fresh plan asks for a password it did not ask
    // for before, or the field was emptied during a download that then launched by
    // itself without one. The dialog shows what is missing (D-295).
    if (closed) return;
    if (!canLaunch) {
      phase = "ready";
      say("");
      return;
    }
    // Said once the re-plan is behind it: it was said, and shown, before a re-plan that
    // could end with nothing started (D-295).
    say("Starting DayZ through BattlEye.");
    try {
      // Exits recorded for an earlier launch from this dialog: Windows reuses process
      // ids, and an old entry under the new pid read as "DayZ exited" (D-265).
      exited.clear();
      launched = await invoke<Launched>("launch_game", { id: sid, password: password || null, profile: profile || null });
      const early = exited.get(launched.pid);
      if (early) {
        exit = early;
        phase = "exited";
        say(exitText(early));
      } else {
        phase = "running";
        say("DayZ is running. You can close this window.");
      }
    } catch (e) {
      error = String(e);
      phase = "error";
      say("");
      // So the footer offers what the failure asks for (a download) rather than the
      // same Join again.
      void replan();
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

  /** Set once the dialog is closed or destroyed; a wait already in flight checks it (D-265). */
  let closed = false;

  /** Back to where the dialog was opened from. When that is gone — Friends' 30 s reload
   *  drops the Join of a friend who has left the server — focus fell to `<body>`: the
   *  list takes it, or else the page's section in the rail (D-295). */
  function returnFocus() {
    const back = opener?.isConnected
      ? opener
      : (document.querySelector<HTMLElement>('main [role="grid"]') ?? document.querySelector<HTMLElement>('.rail-item[aria-current="page"]'));
    back?.focus();
  }

  function close() {
    closed = true;
    stopWaiting();
    onClose();
    // After the dialog is gone: focusing an element that is about to be hidden does
    // nothing useful.
    queueMicrotask(returnFocus);
  }

  /** The dialog element, focused on open so the page behind it stops seeing keys. */
  let dialogEl = $state<HTMLElement | null>(null);

  /** Where focus lands: the password field when one is needed, else the primary button,
   *  else the first enabled control. The mod list and the command line are Tab stops
   *  (`trap`) but not landing places: since D-291 they matched here, so a launch put
   *  focus on the list and kept it there through an error, and a finished vanilla
   *  launch landed on the command line rather than Close (D-295). */
  function landing(): HTMLElement | null {
    return (
      dialogEl?.querySelector<HTMLElement>('input[type="password"]:not([disabled])') ??
      dialogEl?.querySelector<HTMLElement>("button.btn:not(.secondary):not([disabled])") ??
      dialogEl?.querySelector<HTMLElement>("input:not([disabled]), select:not([disabled]), button:not([disabled]), [href]") ??
      null
    );
  }

  // Focus that leaves the modal is brought back (D-295). Minimise and Maximise sit above
  // the backdrop by design (D-151) and may keep it; anything else — Tab on from the title
  // bar, or a control that went disabled while focused — reached the rail and the page
  // behind, where a Join replaced this dialog.
  $effect(() => {
    const onFocusIn = (e: FocusEvent) => {
      const t = e.target;
      if (!dialogEl || !(t instanceof Element) || dialogEl.contains(t) || t.closest(".titlebar")) return;
      (landing() ?? dialogEl).focus();
    };
    document.addEventListener("focusin", onFocusIn);
    return () => document.removeEventListener("focusin", onFocusIn);
  });

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
    // A control the user is already on keeps focus; the dialog itself does not count.
    // While the plan loads, Cancel is the only button, so focusing "the first control"
    // then put focus on Cancel, and this early return kept it there once the plan
    // arrived: Enter on a row and Enter again still cancelled, whatever D-248 said. The
    // dialog holds focus until there is something worth landing on (D-256).
    const active = document.activeElement;
    if (active !== dialogEl && dialogEl?.contains(active)) return;
    // Waiting too: its primary button is "Join now anyway", so a second Enter after
    // "Wait and join" skipped the wait (D-295).
    if (phase === "planning" || phase === "waiting") {
      dialogEl?.focus();
      return;
    }
    // The password field when one is needed, else the primary button: in DOM order
    // the first control was the footer's Cancel, so Enter on a row and Enter again —
    // the natural next keypress — cancelled the join, and a screen reader's first
    // word on a dialog named after a server was "Cancel" (D-248).
    (landing() ?? dialogEl)?.focus();
  });

  /** Keeps Tab inside the dialog while it is open. `summary` and the scrolling mod list
   *  are in it too: without them the command line could not be opened from the keyboard
   *  and a long mod list could not be scrolled (D-291). */
  function trap(e: KeyboardEvent) {
    if (e.key !== "Tab" || !dialogEl) return;
    const items = [...dialogEl.querySelectorAll<HTMLElement>('input:not([disabled]), select:not([disabled]), button:not([disabled]), summary, [href], [tabindex]:not([tabindex="-1"])')].filter(
      (el) => el.offsetParent !== null,
    );
    const active = document.activeElement as HTMLElement | null;
    // Nothing enabled to move to — a vanilla launch disables every button — or focus on
    // a control the list no longer holds: the key left the modal (D-295).
    if (items.length === 0 || (active !== dialogEl && !items.includes(active as HTMLElement))) {
      e.preventDefault();
      (items[0] ?? dialogEl).focus();
      return;
    }
    const first = items[0]!;
    const last = items[items.length - 1]!;
    if (e.shiftKey && (active === first || active === dialogEl)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !busy) {
      // The dialog's Escape alone: a page mounted after it registered its own listener
      // later, found no dialog open once this one had closed, and cleared the selection
      // with the same key (D-295).
      e.stopImmediatePropagation();
      close();
    }
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
          · <span class:warn={full}>{slots.players}/{slots.maxPlayers}{#if queue} · {queue} in queue{/if}</span>
        {/if}
      </span>
    </header>

    {#if phase === "planning"}
      <p class="muted">Reading the server's mod list and checking your Workshop…</p>
    {:else if plan}
      {#if plan.warnings.length}
        <!-- Read with the primary button, which names them as its description: a
             warning that disables Join was never said when focus landed there (D-291). -->
        <ul class="warnings" id="join-warnings">
          <!-- By position: two warnings with the same text threw on a duplicate key and
               left the backdrop with no dialog on it (row 14, F18). -->
          {#each plan.warnings as w, i (i)}<li>{w}</li>{/each}
        </ul>
      {/if}

      {#if plan.mods.length}
        <section class="mods">
          <h3>
            Mods ({plan.mods.length})
            {#if toSync.length}<span class="warn"> · {toSync.length} to download{#if plan.downloadBytes} ({fmtBytes(plan.downloadBytes)}){/if}</span>{:else}<span class="ok"> · all installed</span>{/if}
          </h3>
          <!-- Focusable, so a list longer than its box scrolls from the keyboard (D-291). -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <ul tabindex="0" aria-label="Mods">
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

      {#if claimsFull && untrusted && (phase === "ready" || phase === "error")}
        <p class="muted small">The server reports itself full, but its player count is not trusted here. Join now and DayZ's own queue decides.</p>
      {:else if full && (phase === "ready" || phase === "error")}
        <label class="wait">
          <input type="checkbox" bind:checked={waitForSlot} />
          <span>
            <strong>The server is full.</strong> Wait here for a free slot and join automatically (checked every 10 s). Leave this off to join now and stand in DayZ's own login queue.
          </span>
        </label>
      {/if}

      <p class="muted small">
        Profile name: <strong>{shownProfileName || "(Steam persona)"}</strong> · change it in Settings
        {#if profiles.length}
          · launch with
          <select class="pick" bind:value={profile} disabled={busy || phase === "waiting" || phase === "running"} aria-label="Launch with profile">
            <option value="">current settings</option>
            {#each profiles as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
          </select>
        {/if}
      </p>

      {#if phase === "syncing" && syncInfo}
        <!-- A progress bar a screen reader can ask about, rather than a hidden line: the
             overall figures were `aria-hidden` and nothing said how far a download of up
             to 139 mods had got (D-291). Not live; the region below says the steps. -->
        <p class="status">
          Downloading via Steam…
          <span
            role="progressbar"
            aria-label="Mod download"
            aria-valuemin={0}
            aria-valuemax={syncInfo.total}
            aria-valuenow={syncInfo.installed}
            aria-valuetext="{syncInfo.installed} of {syncInfo.total} installed{totalBytes > 0 ? `, ${fmtBytes(downloadedBytes)} of ${fmtBytes(totalBytes)}` : ''}{etaText ? `, about ${etaText} left` : ''}"
            >{syncInfo.installed}/{syncInfo.total} installed{#if totalBytes > 0} · {fmtBytes(downloadedBytes)} of {fmtBytes(totalBytes)}{/if}{#if rate >
              0} · {fmtBytes(rate)}/s{#if etaText} · about {etaText} left{/if}{/if}</span
          >
        </p>
      {:else if phase === "waiting"}
        <p class="status">
          {#if slotMisses >= 3}
            The server has stopped answering.
            <span aria-hidden="true">{slotMisses} checks in a row. Still trying · {mmss(waitedSecs)}</span>
          {:else}
            Waiting for a free slot…
            <span aria-hidden="true"
              >{#if slots}{slots.players}/{slots.maxPlayers}{#if queue} · {queue} in queue{/if} · {/if}{checks} check{checks === 1 ? "" : "s"} · {mmss(waitedSecs)}</span
            >
          {/if}
        </p>
        <p class="muted small">DayZ starts as soon as the server reports a free slot; the window will flash in the taskbar.</p>
      {:else if phase === "launching"}
        <p class="status">Starting DayZ through BattlEye…</p>
      {:else if phase === "running" && launched}
        <p class="status ok">DayZ is running. You can close this window.</p>
        <details class="cmd"><summary class="muted small">Command line · process {launched.pid}</summary><code>{launched.commandLine}</code></details>
      {:else if phase === "exited" && exit}
        <p class="status" class:warn={exit.code !== 0}>DayZ exited{exit.code != null ? ` with code ${exit.code}` : ""}.</p>
      {/if}
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{:else if note}<p class="muted small">{note}</p>{/if}
    <p class="sr-only" role="status">{announce}</p>

    <footer>
      {#if phase === "waiting"}
        <button class="btn secondary" onclick={cancelWaiting}>Stop waiting</button>
        <button class="btn" onclick={joinAnyway} disabled={!canLaunch}>Join now anyway</button>
      {:else}
        <button class="btn secondary" onclick={close} disabled={busy} title={phase === "syncing" ? "Steam keeps downloading in the background; watch it on the Mods page" : undefined}>
          {phase === "running" || phase === "exited" ? "Close" : phase === "syncing" ? "Close (keeps downloading)" : "Cancel"}
        </button>
        {#if phase === "ready" || phase === "error" || phase === "exited"}
          {#if toSync.length}
            <button class="btn" onclick={() => sync(true)} disabled={!canSync || (plan?.passwordRequired && !password)} aria-describedby={warned}>Download {toSync.length} mod{toSync.length === 1 ? "" : "s"} and join</button>
            <button class="btn secondary" onclick={() => sync(false)} disabled={!canSync}>Download only</button>
          {:else}
            <button class="btn" onclick={joinNow} disabled={!canLaunch} aria-describedby={warned}>{waitForSlot && full ? "Wait and join" : "Join"}</button>
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
  .mods ul:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 2px; border-radius: 4px; }
  .cmd summary:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 2px; border-radius: 3px; }
  .mods li { display: grid; grid-template-columns: 16px 1fr auto 110px; grid-template-areas: "tick name size state" "tick bar bar bar"; gap: 2px 8px; align-items: center; }
  .tick { grid-area: tick; color: var(--ok); }
  .missing .tick, .update .tick { color: var(--warn); }
  .mname { grid-area: name; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .msize { grid-area: size; font-size: 11.5px; }
  .mstate { grid-area: state; font-size: 11.5px; text-align: right; }
  .pbar { grid-area: bar; height: 3px; background: var(--bg-row); border-radius: 2px; overflow: hidden; }
  .pfill { display: block; height: 100%; background: var(--accent); transition: width 150ms; }
  .field { display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; color: var(--fg-muted); }
  .field input { padding: 6px 10px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); }
  .wait { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: var(--radius); border: 1px solid color-mix(in srgb, var(--warn) 50%, var(--border)); font-size: 12.5px; color: var(--fg-muted); cursor: pointer; }
  .wait input { margin-top: 2px; }
  .wait strong { color: var(--fg); }
  .small { font-size: 12px; margin: 0; }
  /* The control border, which D-226 gave every other control: `--border` measured 1.75
     and 1.86:1 against this surface, under 1.4.11's 3:1 (D-291). */
  .pick { padding: 2px 6px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-size: 12px; }
  .status { margin: 0; font-size: 12.5px; }
  .cmd code { display: block; font-size: 11px; white-space: pre-wrap; word-break: break-all; color: var(--fg-muted); margin-top: 4px; }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .error { color: var(--danger); margin: 0; font-size: 12.5px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
</style>
