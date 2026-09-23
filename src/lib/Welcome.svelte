<script lang="ts">
  // First-run overlay (docs/06 §6). Shown once; the flag lives in settings.json (localStorage is only the legacy migration).
  import { servers } from "./state/servers.svelte";

  let { onDone }: { onDone: () => void } = $props();
  let importing = $state(false);
  let imported = $state<string | null>(null);

  async function importFavourites() {
    importing = true;
    const r = await servers.importOfficial();
    importing = false;
    imported = r ? `${r.imported} imported${r.already ? `, ${r.already} already here` : ""}` : (servers.error ?? "nothing to import");
  }

  // It declares `aria-modal="true"` and behaved like nothing of the sort: focus stayed
  // behind it, Tab walked the page underneath and Escape did nothing. The join dialog
  // has done this properly since D-184; this is the same treatment (D-198).
  let cardEl = $state<HTMLDivElement | null>(null);
  const FOCUSABLE = 'input:not([disabled]), select:not([disabled]), button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])';

  $effect(() => {
    if (!cardEl || cardEl.contains(document.activeElement)) return;
    (cardEl.querySelector<HTMLElement>(FOCUSABLE) ?? cardEl).focus();
  });

  function trap(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onDone();
      return;
    }
    if (e.key !== "Tab" || !cardEl) return;
    const items = [...cardEl.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null);
    if (items.length === 0) return;
    const first = items[0]!;
    const last = items[items.length - 1]!;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
</script>

<div class="backdrop" role="presentation">
  <div class="card" role="dialog" aria-modal="true" aria-labelledby="welcome-title" bind:this={cardEl} tabindex="-1" onkeydown={trap}>
    <h2 id="welcome-title">Welcome to DZSA CrayZ Launcher</h2>
    <ol>
      <li><strong>Steam stays in charge.</strong> The server list comes from Steam, mods download through the Workshop, and the game starts through BattlEye exactly like the official launcher. Keep Steam running.</li>
      <li><strong>Player counts are verified.</strong> More than half of the servers on Steam fake their population. Every number you see is checked directly with the server; fakes are hidden by default (toggle in the filter bar).</li>
      <li><strong>Join in one click.</strong> Pick a server, press Join. Missing mods download with progress and DayZ launches connected.</li>
    </ol>
    <div class="row">
      <button class="btn secondary" onclick={importFavourites} disabled={importing}>{importing ? "Importing…" : "Import favourites from the official launcher"}</button>
      {#if imported}<span class="muted">{imported}</span>{/if}
    </div>
    <footer>
      <span class="muted small">Keys: <kbd>/</kbd> search · <kbd>↑</kbd><kbd>↓</kbd> move · <kbd>Enter</kbd> join · <kbd>F</kbd> favourite</span>
      <button class="btn" onclick={onDone}>Start browsing</button>
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.5); display: flex; align-items: center; justify-content: center; z-index: 60; }
  .card { width: min(560px, calc(100vw - 32px)); background: var(--bg-elev); border: 1px solid var(--border); border-radius: 12px; padding: 22px 24px; display: flex; flex-direction: column; gap: 14px; box-shadow: 0 20px 60px rgba(0, 0, 0, 0.45); }
  h2 { margin: 0; font-size: 18px; font-weight: 600; }
  ol { margin: 0; padding-left: 20px; display: flex; flex-direction: column; gap: 8px; font-size: 13px; }
  .row { display: flex; align-items: center; gap: 10px; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-top: 4px; }
  .small { font-size: 11.5px; }
  kbd { padding: 1px 5px; border: 1px solid var(--border); border-radius: 4px; background: var(--bg-row); font-size: 11px; }
</style>
