// Workshop updates (D-191). Steam's `appworkshop_221100.acf` records what it knew the
// last time it checked, so a mod its author updated can sit stale in that file
// indefinitely; `ISteamUGC::GetItemState` answers from the running client, and only for
// items the user is subscribed to — Steam does not maintain the rest, so an update
// offered for one of those is an update that will never arrive. The live answer wins
// when there is one; the file is what is left when Steam cannot be asked. The sidebar
// reads the count, so a pending update is visible without opening the page.
// Every command through the logging wrapper: a failure is recorded with its
// command name before it is rethrown (D-158).
import { invokeLogged as invoke } from "../log";
import { SvelteSet } from "svelte/reactivity";
import type { Diagnostics } from "../types";

/** How often the installed set is re-checked while the launcher is open. */
const RECHECK_MS = 15 * 60_000;
/** `diagnostics` walks the Workshop folders and every junction, so the first check
 *  waits for the window and the cached list to be on screen (docs/05 §6). */
const FIRST_CHECK_MS = 5_000;

class ModUpdates {
  /** Workshop ids with a newer version waiting, from either source. */
  readonly stale = new SvelteSet<number>();
  /** The delay before the first check, then the repeating one — both plain numbers
   *  in the WebView, so one field and one `clear` covers either. */
  #timer: ReturnType<typeof setTimeout> | undefined;

  get count(): number {
    return this.stale.size;
  }

  /**
   * Reads the installed set, then asks Steam which of them are out of date.
   * Pass a `Diagnostics` already in hand — the Mods page has one — to save the
   * second collect.
   */
  async check(known?: Diagnostics | null) {
    let diag = known;
    if (!diag) {
      try {
        diag = await invoke<Diagnostics>("diagnostics");
      } catch {
        return; // no Steam, no game, or the ACF is missing: nothing to say
      }
    }
    const items = diag.workshop?.items ?? [];
    // The file's own view, which is right whenever Steam has checked recently and is
    // all there is when it is not running.
    let next = new Set(items.filter((i) => i.needsUpdate).map((i) => i.id));
    try {
      const live = await invoke<number[] | null>("mods_stale", { ids: items.map((i) => i.id) });
      // `null` is "could not ask", which is not "nothing is stale".
      if (live) next = new Set(live);
    } catch {
      /* keep the file's answer */
    }
    // One pass, so a re-check with the same answer produces no reactive churn.
    for (const id of this.stale) if (!next.has(id)) this.stale.delete(id);
    for (const id of next) this.stale.add(id);
  }

  start() {
    if (this.#timer !== undefined) return;
    this.#timer = setTimeout(() => {
      void this.check();
      this.#timer = setInterval(() => void this.check(), RECHECK_MS);
    }, FIRST_CHECK_MS);
  }

  stop() {
    if (this.#timer === undefined) return;
    clearTimeout(this.#timer);
    clearInterval(this.#timer);
    this.#timer = undefined;
  }
}

export const modUpdates = new ModUpdates();
