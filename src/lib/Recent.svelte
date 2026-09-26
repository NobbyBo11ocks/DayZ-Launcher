<script lang="ts">
  // Recent joins from the launch history table. Fits the viewport; the table
  // scrolls on its own when the history is longer than the window (D-094).
  import { servers } from "./state/servers.svelte";
  import type { HistoryEntry } from "./types";

  $effect(() => {
    void servers.loadHistory();
  });

  /**
   * A server that has dropped out of the list is found by probing its ports, which
   * takes up to ~15 s for one that is gone. Without a pending state the button looked
   * untouched and invited repeat clicks, each starting another probe (D-184).
   */
  let joining = $state<string | null>(null);
  async function joinAgain(h: HistoryEntry) {
    if (joining) return;
    if (servers.rowsTick >= 0 && servers.rows.has(h.id)) {
      servers.select(h.id);
      servers.joiningId = h.id;
      return;
    }
    joining = h.id;
    try {
      // The entry's id is `ip:queryPort`, the address Steam knew it by. The game port
      // made the host guess the query port, and the guess misses 14 % of populated
      // servers; Friends stopped guessing in D-245, this page in D-256. A typed port is
      // tried as the query port first, so the id needs no conversion.
      let row = await servers.directConnect(h.id, false);
      // The query port may belong to another server by now (hosts hand ports on): the
      // game port it reports has to be the one this entry joined. And a server that
      // moved its query port but kept its game port is found from the game port, which
      // the host probes with the reply's game port checked (D-265).
      if (row && row.gamePort !== h.gamePort) row = null;
      if (!row) row = await servers.directConnect(`${h.ip}:${h.gamePort}`, false);
      // Not over a dialog opened while the probe ran (D-265), and not reported as
      // silent when it did answer: that dialog is simply left alone (D-281).
      if (!row) servers.error = `${h.name} did not answer on ${h.ip}:${h.gamePort}; it may be offline or have moved.`;
      else if (servers.joiningId === null) {
        servers.selectedId = row.id;
        servers.joiningId = row.id;
      }
    } finally {
      joining = null;
    }
  }

  const when = (unix: number) => new Date(unix * 1000).toLocaleString();

  // "Clear list" (D-130): the only way history rows are deleted, and only after an
  // inline confirmation, like the junction cleanup in Diagnostics (D-093).
  let confirmClear = $state(false);
  let clearing = $state(false);
  async function clearAll() {
    clearing = true;
    await servers.clearHistory();
    clearing = false;
    confirmClear = false;
  }
</script>

<section class="recent">
  <header class="row">
    <h1>Recent</h1>
    {#if servers.history.length}<span class="muted">{servers.history.length} join{servers.history.length === 1 ? "" : "s"}</span>{/if}
    {#if servers.error}<span class="error">{servers.error}</span>{/if}
    {#if servers.history.length}
      <span class="spacer"></span>
      {#if confirmClear}
        <span class="muted">Remove all {servers.history.length} join{servers.history.length === 1 ? "" : "s"} from the list?</span>
        <button class="btn danger" onclick={clearAll} disabled={clearing}>{clearing ? "Clearing…" : "Clear"}</button>
        <button class="btn ghost" onclick={() => (confirmClear = false)} disabled={clearing}>Keep</button>
      {:else}
        <button class="btn ghost" onclick={() => (confirmClear = true)} title="Remove every entry from this list; favourites and the server list are untouched">Clear list</button>
      {/if}
    {/if}
  </header>
  {#if servers.history.length === 0}
    <!-- Centred empty state, like LAN and Favourites (D-143). -->
    <div class="empty">
      <p>No recent servers.</p>
      <p class="muted">Every server you join is listed here with its address and mod count, so you can rejoin it in one click. Clear the list at any time.</p>
      <button class="btn" onclick={() => (servers.navigate = "servers")}>Browse servers</button>
    </div>
  {:else}
    <div class="scroll">
      <table>
        <thead><tr><th>When</th><th>Server</th><th>Address</th><th class="num">Mods</th><th></th></tr></thead>
        <tbody>
          <!-- Index in the key: two joins to the same server in the same second are
               otherwise duplicate keys and Svelte throws (D-151). -->
          {#each servers.history as h, i (`${h.id}@${h.joinedAt}#${i}`)}
            {@const live = servers.rowsTick >= 0 ? servers.rows.get(h.id) : undefined}
            <tr>
              <td class="muted">{when(h.joinedAt)}</td>
              <td>{live?.name ?? h.name}{#if servers.favourites.has(h.id)} <span class="star" title="Favourite">★</span>{/if}</td>
              <td class="mono">{h.ip}:{h.gamePort}</td>
              <td class="num">{h.mods}</td>
              <td class="act"><button class="btn" onclick={() => joinAgain(h)} disabled={joining !== null}>{joining === h.id ? "Looking…" : "Join again"}</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .recent { display: flex; flex-direction: column; gap: 8px; min-height: 0; }
  .row { display: flex; align-items: center; gap: 12px; }
  h1 { margin: 0; }
  .scroll { flex: 1; min-height: 0; overflow: auto; }
  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  th, td { text-align: left; padding: 5px 8px; border-bottom: 1px solid var(--border); }
  th { position: sticky; top: 0; background: var(--bg); color: var(--fg-muted); font-weight: 500; }
  .num { text-align: right; }
  .act { text-align: right; }
  .mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  .star { color: var(--accent-ink); }
  .spacer { flex: 1; }
  .empty { margin: auto; text-align: center; max-width: 440px; display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .empty p { margin: 0; }
  .empty .btn { margin-top: 6px; }
  .error { color: var(--danger); }
</style>
