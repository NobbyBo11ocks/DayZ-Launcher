<script lang="ts">
  // The app's own log (D-158, D-168): the last entries from both halves of the app,
  // newest first, with the file one click away. This replaced the Diagnostics page,
  // whose inventory tables repeated what the Mods page and the server browser already
  // show and whose sampling cost more than it told anyone.
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { invokeLogged as invoke } from "./log";
  import type { Settings } from "./types";

  type LogEntry = { at: number; level: "info" | "warn" | "error"; target: string; message: string };

  let logs = $state<LogEntry[]>([]);
  let logPath = $state<string | null>(null);
  let onlyProblems = $state(false);
  let error = $state<string | null>(null);
  let copied = $state(false);

  // Recording can be switched off entirely (D-169): nothing is written to the file
  // and nothing is kept in memory. The setting lives with the rest of them, so it
  // survives a restart.
  let recording = $state(true);
  // Read by the chips’ `disabled` now, so it has to be reactive (D-197).
  let settings = $state<Settings | null>(null);
  async function loadSettings() {
    try {
      settings = await invoke<Settings>("settings_get");
      recording = settings.logging;
      muted = settings.logMuted ?? [];
    } catch (e) {
      // The chips guard on `settings` and would otherwise click with no effect and
      // no message, which reads as "logging is off" when it is not (D-197).
      error = `Log settings could not be read (${String(e)}); recording cannot be changed.`;
    }
  }
  /**
   * The areas an entry can belong to, in the order they matter to someone trying to
   * work out what went wrong (D-172). An entry's area is its target up to the first
   * `:`, so "ui" covers every `ui:…` target. Muting is applied in the host, so a
   * muted area is never written to the file either.
   */
  const AREAS: { id: string; label: string; hint: string }[] = [
    { id: "join", label: "Joining", hint: "Join plans, direct connect and what a server needs" },
    { id: "launch", label: "Launching", hint: "The game starting and how it ended" },
    { id: "mods", label: "Mods", hint: "Downloads, junctions, unsubscribes and mod scans" },
    { id: "steam", label: "Steam", hint: "The Steam session and server-list refreshes" },
    { id: "verify", label: "Verification", hint: "Player-count checks against the servers" },
    { id: "cache", label: "Cache", hint: "The local database: writes, prunes and failures" },
    { id: "settings", label: "Settings", hint: "Reading and writing the settings file" },
    { id: "app", label: "App", hint: "Start-up and anything the interface reports" },
  ];
  let muted = $state<string[]>([]);
  const isMuted = (id: string) => muted.includes(id);
  async function toggleArea(id: string) {
    // The guard comes first. Mutating `muted` before it left the chip struck through
    // while logging carried on exactly as before, with nothing said (D-194).
    if (!settings) return;
    muted = isMuted(id) ? muted.filter((m) => m !== id) : [...muted, id];
    settings = { ...settings, logMuted: muted };
    try {
      await invoke("settings_set", { settings });
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function setRecording(on: boolean) {
    if (!settings) return;
    recording = on;
    settings = { ...settings, logging: on };
    try {
      await invoke("settings_set", { settings });
      if (!on) logs = [];
      else await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function load() {
    try {
      const [entries, path] = await Promise.all([invoke<LogEntry[]>("logs_recent", { limit: 400 }), invoke<string | null>("logs_path")]);
      logs = entries.reverse();
      logPath = path;
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  const area = (t: string) => {
    const head = t.split(":")[0] ?? t;
    return head === "ui" ? "app" : head;
  };
  const shown = $derived(onlyProblems ? logs.filter((l) => l.level === "warn" || l.level === "error") : logs);
  const counts = $derived.by(() => {
    const m = new Map<string, number>();
    for (const l of logs) m.set(area(l.target), (m.get(area(l.target)) ?? 0) + 1);
    return m;
  });
  const problems = $derived(logs.filter((l) => l.level === "warn" || l.level === "error").length);
  const time = (ms: number) => new Date(ms).toLocaleTimeString();

  async function reveal() {
    if (!logPath) return;
    try {
      await revealItemInDir(logPath);
    } catch (e) {
      error = String(e);
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(shown.map((l) => `${time(l.at)} ${l.level.toUpperCase()} ${l.target} ${l.message}`).join("\n"));
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      error = String(e);
    }
  }

  // Live while the page is open: one read every few seconds is cheaper than a
  // subscription and the list is only 400 entries.
  $effect(() => {
    void loadSettings();
    void load();
    const t = setInterval(() => {
      if (recording) void load();
    }, 5000);
    return () => clearInterval(t);
  });
</script>

<section class="logs-view">
  <header class="head">
    <div class="title">
      <h1>Logs</h1>
      <span class="sub">
        {recording ? "What the launcher has been doing, both halves of it. Nothing leaves this machine." : "Recording is off: nothing is written to disk or kept in memory."}
        {#if recording && problems}&nbsp;· {problems} problem{problems === 1 ? "" : "s"}{/if}
      </span>
    </div>
    <div class="acts">
      <button class="chip" class:on={recording} onclick={() => void setRecording(!recording)} title="Keep a local record of what the launcher does">
        {recording ? "Recording" : "Paused"}
      </button>
      <button class="chip" class:on={onlyProblems} onclick={() => (onlyProblems = !onlyProblems)} disabled={!recording || !settings}>
        {onlyProblems ? "Problems only" : "Everything"}
      </button>
      <button class="chip" onclick={copy} disabled={!shown.length}>{copied ? "Copied" : "Copy"}</button>
      <button class="chip" onclick={reveal} disabled={!logPath} title={logPath ?? ""}>Show the file</button>
    </div>
  </header>

  {#if error}<p class="note bad">{error}</p>{/if}

  <!-- One chip per area; switching one off stops it being recorded at all, in the
       file as well as here (D-172). -->
  <div class="areas" role="group" aria-label="What to record">
    {#each AREAS as a (a.id)}
      <button class="chip sm" aria-pressed={!isMuted(a.id)} class:off={isMuted(a.id)} onclick={() => void toggleArea(a.id)} title={isMuted(a.id) ? `Not recording: ${a.hint}` : a.hint} disabled={!recording || !settings}>
        {a.label}
        {#if !isMuted(a.id) && counts.get(a.id)}<span class="n">{counts.get(a.id)}</span>{/if}
      </button>
    {/each}
  </div>

  <div class="list">
    {#if shown.length}
      <!-- Unkeyed on purpose (D-176): newest first means one new entry shifts every
           index, so a key made Svelte destroy and rebuild all 400 rows on each poll.
           Unkeyed reconciliation updates the text in place. -->
      {#each shown as l}
        <div class="line {l.level}">
          <span class="t">{time(l.at)}</span>
          <span class="lvl">{l.level}</span>
          <span class="tgt">{l.target}</span>
          <span class="msg">{l.message}</span>
        </div>
      {/each}
    {:else}
      <p class="empty">{!recording ? "Recording is off. Turn it back on to collect a log." : logs.length ? "No warnings or errors." : "Nothing logged yet."}</p>
    {/if}
  </div>
</section>

<style>
  .logs-view { display: flex; flex-direction: column; gap: 8px; min-height: 0; height: 100%; }

  .head { display: flex; align-items: flex-end; gap: 16px; }
  .title { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  h1 { margin: 0; font-size: 19px; line-height: 1.1; }
  .sub { color: var(--fg-muted); font-size: 12px; }
  .acts { margin-left: auto; display: flex; gap: 6px; flex-shrink: 0; }

  .chip { all: unset; box-sizing: border-box; cursor: pointer; display: inline-flex; align-items: center; height: 24px; padding: 0 10px; border-radius: 999px; background: var(--bg-row); border: 1px solid var(--border-control); font-size: 11.5px; white-space: nowrap; }
  .chip:hover { border-color: var(--accent-ink); }
  .chip:disabled { opacity: 0.5; cursor: default; }
  .chip:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 1px; }
  .chip.on { border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); color: var(--fg); }

  .areas { display: flex; flex-wrap: wrap; gap: 5px; }
  .chip.sm { height: 21px; padding: 0 8px; font-size: 11px; }
  .chip.off { opacity: 0.7; text-decoration: line-through; }
  .n { margin-left: 5px; color: var(--fg-muted); font-variant-numeric: tabular-nums; }

  .note { margin: 0; font-size: 12px; }
  .note.bad { color: var(--danger); }

  .list { flex: 1; min-height: 0; overflow: auto; border: 1px solid var(--border); border-radius: 10px; background: var(--bg-elev); padding: 4px 0; }
  .line { display: grid; grid-template-columns: 70px 46px 96px minmax(0, 1fr); gap: 10px; padding: 1px 10px; font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; line-height: 1.55; }
  .line:hover { background: var(--bg-row); }
  .t { color: var(--fg-muted); }
  .lvl { color: var(--fg-muted); text-transform: uppercase; font-size: 10px; letter-spacing: 0.05em; align-self: center; }
  .tgt { color: var(--accent-ink); overflow: hidden; text-overflow: ellipsis; }
  .msg { color: var(--fg); overflow-wrap: anywhere; }
  .line.warn .lvl, .line.warn .msg { color: var(--warn); }
  .line.error .lvl, .line.error .msg { color: var(--danger); }
  .empty { margin: 12px auto; color: var(--fg-muted); font-size: 12.5px; text-align: center; }
</style>
