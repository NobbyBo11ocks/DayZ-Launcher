<script lang="ts">
  // DayZ update posts (D-099): stacked in the top-right corner until dismissed.
  // Favourite alerts used to share this rail; they were removed in D-182.
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { news } from "./state/news.svelte";

  function read(gid: string, url: string) {
    void openUrl(url).catch(() => {});
    news.dismissAlert(gid);
  }
</script>

<!-- The container is always in the DOM: wrapping it in the `{#if}` meant the live
     region was created together with its content, and a region that appears already
     populated is generally never announced (D-224). -->
<div class="toasts" role="status" aria-live="polite" class:empty={news.alerts.length === 0}>
  {#if news.alerts.length}
      {#each news.alerts as n (n.gid)}
        <div class="toast">
          <div class="text">
            <strong>DayZ update</strong>
            <span class="muted">{n.title}</span>
          </div>
          <button class="btn" onclick={() => read(n.gid, n.url)}>Read</button>
          <button class="close" onclick={() => news.dismissAlert(n.gid)} aria-label="Dismiss">✕</button>
        </div>
      {/each}
  {/if}
</div>

<style>
  .toasts.empty { pointer-events: none; }
  /* Below the join dialog (D-159): a toast on top of the modal could be clicked, and
     its Join then pointed the open dialog at a different server. */
  .toasts { position: fixed; top: 44px; right: 16px; display: flex; flex-direction: column; gap: 8px; z-index: 40; width: min(360px, calc(100vw - 32px)); }
  .toast { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 10px; align-items: center; padding: 10px 12px; border-radius: var(--radius); background: var(--bg-elev); border: 1px solid color-mix(in srgb, var(--accent) 60%, var(--border)); box-shadow: 0 10px 30px rgba(0, 0, 0, 0.4); font-size: 12.5px; }
  .text { display: flex; flex-direction: column; gap: 2px; min-width: 0; overflow-wrap: anywhere; }
  .text strong { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .close { all: unset; cursor: pointer; color: var(--fg-muted); padding: 4px; }
  .close:hover { color: var(--fg); }
</style>
