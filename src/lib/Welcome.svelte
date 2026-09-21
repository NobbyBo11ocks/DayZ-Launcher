<script lang="ts">
  // First-run overlay (docs/06 §6). Shown once; the flag lives in localStorage.
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
</script>

<div class="backdrop" role="presentation">
  <div class="card" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
    <h2 id="welcome-title">Welcome to DayZ Launcher</h2>
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
  .btn { all: unset; cursor: pointer; padding: 7px 16px; border-radius: var(--radius); background: var(--accent); color: #111; font-weight: 600; }
  .btn.secondary { background: var(--bg-row); color: var(--fg); border: 1px solid var(--border); font-weight: 500; }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn:focus-visible { outline: 2px solid var(--fg); }
</style>
