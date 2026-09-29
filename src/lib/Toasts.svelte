<script lang="ts">
  // DayZ update posts (D-099) and one-off notices (D-303: a settings file that could
  // not be read, saved data moved, a join not added to Recent), stacked in the top-right
  // corner until dismissed. Favourite alerts used to share this rail (removed in D-182).
  import { MODAL, noticesTakeEscape } from "./escape";
  import { POST_NOT_OPENED } from "./external";
  import { news } from "./state/news.svelte";
  import { notices } from "./state/notices.svelte";

  const count = $derived(news.alerts.length + notices.items.length);

  /** Escape clears the stack. It stays until dismissed and sits over the top right of
   *  every page, where it hid the Logs page's header controls, Mods' Rescan and Update
   *  all, and Servers' Refresh — and the keyboard focus on them (WCAG 2.4.11, D-291).
   *  Not while a dialog is open or a field is being typed in: those keep their Escape;
   *  nor when anything else acted on the key (`noticesTakeEscape`, row 27). */
  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || count === 0) return;
    const modalOpen = document.querySelector(MODAL) !== null;
    // Once every other handler has had the key: the window's listeners run in the order
    // their components mounted, which a page switch reverses, so a look at once missed the
    // page's own (row 27).
    setTimeout(() => {
      if (!noticesTakeEscape(e, modalOpen)) return;
      for (const n of [...news.alerts]) news.dismissAlert(n.gid);
      notices.clear();
    });
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- The container is always in the DOM: wrapping it in the `{#if}` meant the live
     region was created together with its content, and a region that appears already
     populated is generally never announced (D-224). -->
<div class="toasts" role="status" aria-live="polite" class:empty={count === 0}>
  <!-- One-off notices (D-303): the same card, with only the dismiss button. -->
  {#each notices.items as n (n.id)}
    <div class="toast notice">
      <div class="text">
        <strong>{n.title}</strong>
        <span class="muted" id="notice-{n.id}">{n.text}</span>
      </div>
      <button class="close" onclick={() => notices.dismiss(n.id)} aria-label="Dismiss" aria-describedby="notice-{n.id}">✕</button>
    </div>
  {/each}
  {#if news.alerts.length}
      {#each news.alerts as n (n.gid)}
        <div class="toast">
          <div class="text">
            <strong>DayZ update</strong>
            <span class="muted" id="toast-{n.gid}">{n.title}</span>
            <!-- The toast stays when its post could not be opened, and says so, in News's
                 words (row 27, approved); Read tries again. -->
            {#if n.failed}<span class="error" role="alert">{POST_NOT_OPENED}</span>{/if}
          </div>
          <!-- Each pair says which post it is for: up to three toasts stack, with the same
               two buttons in each (D-291). -->
          <button class="btn" onclick={() => void news.readAlert(n.gid)} aria-describedby="toast-{n.gid}">Read</button>
          <button class="close" onclick={() => news.dismissAlert(n.gid)} aria-label="Dismiss" aria-describedby="toast-{n.gid}">✕</button>
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
  .toast.notice { grid-template-columns: minmax(0, 1fr) auto; }
  .text { display: flex; flex-direction: column; gap: 2px; min-width: 0; overflow-wrap: anywhere; }
  .text strong { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .close { all: unset; cursor: pointer; color: var(--fg-muted); padding: 4px; }
  .close:hover { color: var(--fg); }
  /* `all: unset` above outranks app.css's `button:focus-visible`, so the ring was never
     drawn here; this is the same ring (D-291). */
  .close:focus-visible { outline: 2px solid var(--accent-ink); border-radius: 4px; }
</style>
