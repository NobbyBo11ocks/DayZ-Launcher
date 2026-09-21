<script lang="ts">
  // Favourite alerts (D-083): stacked in the top-right corner until dismissed.
  import { servers } from "./state/servers.svelte";

  function join(id: string, at: number) {
    servers.select(id);
    servers.joiningId = id;
    servers.dismissAlert(at);
  }
</script>

{#if servers.alerts.length}
  <div class="toasts" role="status" aria-live="polite">
    {#each servers.alerts as a (a.at)}
      <div class="toast">
        <div class="text">
          <strong>{a.name}</strong>
          <span class="muted">
            {#if a.kind === "slot"}has a free slot: {a.players}/{a.maxPlayers}{:else}is back online: {a.players}/{a.maxPlayers}{/if}
          </span>
        </div>
        <button class="btn" onclick={() => join(a.id, a.at)}>Join</button>
        <button class="close" onclick={() => servers.dismissAlert(a.at)} aria-label="Dismiss">✕</button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toasts { position: fixed; top: 44px; right: 16px; display: flex; flex-direction: column; gap: 8px; z-index: 60; width: min(360px, calc(100vw - 32px)); }
  .toast { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 10px; align-items: center; padding: 10px 12px; border-radius: var(--radius); background: var(--bg-elev); border: 1px solid color-mix(in srgb, var(--accent) 60%, var(--border)); box-shadow: 0 10px 30px rgba(0, 0, 0, 0.4); font-size: 12.5px; }
  .text { display: flex; flex-direction: column; gap: 2px; min-width: 0; overflow-wrap: anywhere; }
  .text strong { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .btn { all: unset; cursor: pointer; padding: 6px 12px; border-radius: var(--radius); background: var(--accent); color: #111; font-weight: 600; }
  .btn:focus-visible { outline: 2px solid var(--fg); }
  .close { all: unset; cursor: pointer; color: var(--fg-muted); padding: 4px; }
  .close:hover { color: var(--fg); }
</style>
