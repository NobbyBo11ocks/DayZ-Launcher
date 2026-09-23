<script lang="ts">
  // Frameless window chrome (docs/06 §1). Needs core:window:allow-{minimize,
  // toggle-maximize,close,start-dragging} in capabilities/default.json.
  // Slim and unlabelled (D-091): the bar is a drag handle plus the three controls;
  // it carries accent-coloured live counts with icons (servers with players, friends
  // in DayZ; D-103, D-105, D-106) and an update notice when one is pending.
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let {
    servers = null,
    friends = null,
    notice = "",
    greeting = "",
  }: { servers?: number | null; friends?: number | null; notice?: string; greeting?: string } = $props();
  const win = getCurrentWindow();
  const fmt = new Intl.NumberFormat();
</script>

<header class="titlebar" data-tauri-drag-region>
  {#if servers != null}
    <span class="stat" data-tauri-drag-region title="Servers with a verified player count above zero">
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.4" /><path d="M1.6 8h12.8M8 1.6c2.6 2.6 2.6 10.2 0 12.8M8 1.6C5.4 4.2 5.4 11.8 8 14.4" /></svg>
      {fmt.format(servers)} servers with players
    </span>
  {/if}
  {#if friends != null}
    <span class="stat" data-tauri-drag-region title="Steam friends playing DayZ right now">
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="5" r="3" /><path d="M2.6 14.4c0-3 2.4-5 5.4-5s5.4 2 5.4 5" /></svg>
      {friends} friend{friends === 1 ? "" : "s"} in DayZ
    </span>
  {/if}
  {#if notice}<span class="notice" data-tauri-drag-region>{notice}</span>{/if}
  <span class="spacer" data-tauri-drag-region></span>
  {#if greeting}<span class="greeting" data-tauri-drag-region>{greeting}</span>{/if}
  <div class="controls">
    <button class="wbtn" aria-label="Minimise" onclick={() => win.minimize()}>
      <svg viewBox="0 0 10 10" width="10" height="10"><path d="M0 5h10" stroke="currentColor" stroke-width="1" /></svg>
    </button>
    <button class="wbtn" aria-label="Maximise or restore" onclick={() => win.toggleMaximize()}>
      <svg viewBox="0 0 10 10" width="10" height="10"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1" /></svg>
    </button>
    <button class="wbtn close" aria-label="Close" onclick={() => win.close()}>
      <svg viewBox="0 0 10 10" width="10" height="10"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" stroke-width="1.1" /></svg>
    </button>
  </div>
</header>

<style>
  /* Above every overlay (D-151): modal backdrops are fixed and used to paint over the
     bar, which left minimise, maximise, close and the drag region dead while a dialog
     was open — with no native frame to fall back on. */
  .titlebar { grid-area: top; position: relative; z-index: 80; display: flex; align-items: center; gap: 16px; height: 30px; padding: 0 0 0 14px; background: var(--bg-elev); border-bottom: 1px solid var(--border); user-select: none; }
  /* Accent-coloured so the counts read as part of the theme (user request, D-103). */
  .stat { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 500; color: var(--accent-ink); white-space: nowrap; }
  .stat + .stat { margin-left: 16px; }
  .icon { width: 13px; height: 13px; fill: none; stroke: currentColor; stroke-width: 1.3; stroke-linecap: round; stroke-linejoin: round; flex: none; }
  .notice { font-size: 12px; color: var(--accent-ink); white-space: nowrap; }
  /* The welcome line lives here now (D-173), in the accent like the counts beside it. */
  .greeting { min-width: 0; margin-right: 12px; font-size: 12px; font-weight: 500; color: var(--accent-ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .spacer { flex: 1; height: 100%; }
  .controls { display: flex; height: 100%; }
  .wbtn { all: unset; width: 40px; height: 100%; display: inline-flex; align-items: center; justify-content: center; color: var(--fg-muted); cursor: default; }
  .wbtn:hover { background: var(--bg-row); color: var(--fg); }
  .wbtn.close:hover { background: #e81123; color: #fff; }
  .wbtn:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: -2px; }
</style>
