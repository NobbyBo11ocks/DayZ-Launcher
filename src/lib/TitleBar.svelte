<script lang="ts">
  // Frameless window chrome (docs/06 §1). Needs core:window:allow-{minimize,
  // toggle-maximize,close,start-dragging} in capabilities/default.json.
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let { title = "DayZ Launcher", subtitle = "" }: { title?: string; subtitle?: string } = $props();
  const win = getCurrentWindow();
</script>

<header class="titlebar" data-tauri-drag-region>
  <span class="brand" data-tauri-drag-region>{title}</span>
  {#if subtitle}<span class="muted" data-tauri-drag-region>{subtitle}</span>{/if}
  <span class="spacer" data-tauri-drag-region></span>
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
  .titlebar { grid-area: top; display: flex; align-items: center; gap: 12px; height: 40px; padding: 0 0 0 14px; background: var(--bg-elev); border-bottom: 1px solid var(--border); user-select: none; }
  .brand { font-weight: 600; font-size: 14px; }
  .spacer { flex: 1; height: 100%; }
  .controls { display: flex; height: 100%; }
  .wbtn { all: unset; width: 46px; height: 100%; display: inline-flex; align-items: center; justify-content: center; color: var(--fg-muted); cursor: default; }
  .wbtn:hover { background: var(--bg-row); color: var(--fg); }
  .wbtn.close:hover { background: #e81123; color: #fff; }
  .wbtn:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
</style>
