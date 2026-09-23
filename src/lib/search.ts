import { servers } from "./state/servers.svelte";

/**
 * Debounced writer for the shared search filter.
 *
 * `servers.filters.search` drives `list`, `favouriteRows`, `lanRows` and `filterKey`,
 * so a synchronous write per keystroke re-filters and re-sorts every row and re-arms
 * the table’s visibility effect. FilterBar has debounced this since D-152; the
 * Favourites and LAN boxes bound straight through and did not (D-222).
 *
 * Clearing the field applies at once — waiting to see the full list again reads as lag.
 */
export function searchBox(delayMs = 180) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    set(value: string) {
      clearTimeout(timer);
      if (value === "") {
        servers.filters.search = "";
        servers.saveFilters();
        return;
      }
      timer = setTimeout(() => {
        servers.filters.search = value;
        servers.saveFilters();
      }, delayMs);
    },
    dispose() {
      clearTimeout(timer);
    },
  };
}
