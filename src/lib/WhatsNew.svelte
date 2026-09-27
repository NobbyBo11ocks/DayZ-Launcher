<script lang="ts">
  // What changed, shown once after an update (user request, D-301): the releases the
  // player has not seen, newest first, each line under the words DayZ's own patch notes
  // use. App.svelte decides when it opens; closing it marks the version as seen.
  import emblemUrl from "../assets/emblem.png";
  import type { NoteKind, Release } from "./changes";

  let { version, releases, onDone }: { version: string; releases: Release[]; onDone: () => void } = $props();

  const KIND: Record<NoteKind, string> = { added: "Added", fixed: "Fixed", changed: "Changed", removed: "Removed" };

  // Modal the way the welcome and the join dialog are (D-184, D-198): focus starts on
  // the button, Tab stays inside and Escape closes.
  let cardEl = $state<HTMLDivElement | null>(null);
  let doneEl = $state<HTMLButtonElement | null>(null);
  let notesEl = $state<HTMLDivElement | null>(null);
  /** The notes scroll when a player several updates behind gets a long list: the list
   *  then takes the focus too, so the keys can scroll it (D-291). */
  let scrolls = $state(false);

  $effect(() => {
    if (doneEl && !cardEl?.contains(document.activeElement)) doneEl.focus();
  });
  $effect(() => {
    if (notesEl) scrolls = notesEl.scrollHeight > notesEl.clientHeight + 1;
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      // The pages listen for Escape on the window too: Servers would close its pane.
      e.stopPropagation();
      onDone();
      return;
    }
    if (e.key !== "Tab" || !cardEl) return;
    const items = [...cardEl.querySelectorAll<HTMLElement>('button, [tabindex="0"]')];
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return;
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
  <div class="card" role="dialog" aria-modal="true" aria-labelledby="whatsnew-title" aria-describedby="whatsnew-notes" tabindex="-1" bind:this={cardEl} onkeydown={onKey}>
    <header>
      <img class="mark" src={emblemUrl} alt="" width="27" height="30" draggable="false" />
      <div>
        <h2 id="whatsnew-title">What's new</h2>
        <span class="muted">Version {version}</span>
      </div>
    </header>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="notes" id="whatsnew-notes" bind:this={notesEl} tabindex={scrolls ? 0 : undefined}>
      {#each releases as r (r.version)}
        <section>
          {#if releases.length > 1}<h3>{r.version}</h3>{/if}
          <ul>
            {#each r.notes as n, i (i)}
              <li><span class="pill">{KIND[n.kind]}<span class="sr-only">:</span></span><span>{n.text}</span></li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
    <footer>
      <button class="btn" bind:this={doneEl} onclick={onDone}>Got it</button>
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.5); display: flex; align-items: center; justify-content: center; z-index: 60; }
  .card { width: min(520px, calc(100vw - 32px)); max-height: calc(100vh - 64px); background: var(--bg-elev); border: 1px solid var(--border); border-radius: 12px; padding: 20px 22px; display: flex; flex-direction: column; gap: 14px; box-shadow: 0 20px 60px rgba(0, 0, 0, 0.45); }
  header { display: flex; align-items: center; gap: 12px; }
  header div { display: flex; flex-direction: column; gap: 2px; }
  .mark { flex: none; }
  h2 { margin: 0; font-size: 16px; font-weight: 600; }
  header .muted { font-size: 12px; }
  .notes { min-height: 0; overflow: auto; display: flex; flex-direction: column; gap: 12px; }
  .notes:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 2px; border-radius: 4px; }
  /* The heading the join dialog gives its sections. */
  h3 { margin: 0 0 6px; font-size: 12px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; font-size: 13px; }
  li { display: grid; grid-template-columns: 72px 1fr; gap: 10px; align-items: baseline; }
  /* News's "Update" pill (D-186), so the labels read as the app's own. */
  .pill { justify-self: start; padding: 1px 7px; border-radius: 9px; font-size: 10.5px; font-weight: 600; letter-spacing: 0.02em; background: var(--bg-row); color: var(--accent-ink); border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border)); }
  footer { display: flex; justify-content: flex-end; }
</style>
