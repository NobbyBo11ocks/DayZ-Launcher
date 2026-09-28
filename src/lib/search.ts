import { servers } from "./state/servers.svelte";

/**
 * Debounced writer for the shared search filter.
 *
 * `servers.filters.search` drives `list`, `favouriteRows`, `lanRows` and `filterKey`,
 * so a synchronous write per keystroke re-filters and re-sorts every row and re-arms
 * the table’s visibility effect. The Servers box has debounced this since D-152; the
 * Favourites and LAN boxes bound straight through and did not (D-222). All three use
 * this now: the Servers box moved out of FilterBar when the filters went to the rail
 * (D-249).
 *
 * Clearing the field applies at once — waiting to see the full list again reads as lag.
 */
export function searchBox(delayMs = 180) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    set(value: string) {
      clearTimeout(timer);
      // The search is never saved, so it saves nothing: through `saveFilters` it marked
      // the filters as the player's own, and a term typed before the settings file had
      // been read replaced the file's saved filters with the defaults (row 15, F4).
      if (value === "") {
        servers.filters.search = "";
        return;
      }
      timer = setTimeout(() => {
        servers.filters.search = value;
      }, delayMs);
    },
    dispose() {
      clearTimeout(timer);
    },
  };
}
