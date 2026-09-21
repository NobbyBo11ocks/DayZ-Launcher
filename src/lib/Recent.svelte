<script lang="ts">
  // Recent joins from the launch history table.
  import { servers } from "./state/servers.svelte";
  import type { HistoryEntry } from "./types";

  $effect(() => {
    void servers.loadHistory();
  });

  async function joinAgain(h: HistoryEntry) {
    if (servers.rows.has(h.id)) {
      servers.select(h.id);
      servers.joiningId = h.id;
      return;
    }
    const row = await servers.directConnect(`${h.ip}:${h.gamePort}`);
    if (row) servers.joiningId = row.id;
  }

  const when = (unix: number) => new Date(unix * 1000).toLocaleString();
</script>

<section class="recent">
  <h1>Recent</h1>
  {#if servers.history.length === 0}
    <p class="muted">Servers you join appear here.</p>
  {:else}
    <table>
      <thead><tr><th>When</th><th>Server</th><th>Address</th><th class="num">Mods</th><th></th></tr></thead>
      <tbody>
        {#each servers.history as h (h.id + h.joinedAt)}
          {@const live = servers.rows.get(h.id)}
          <tr>
            <td class="muted">{when(h.joinedAt)}</td>
            <td>{live?.name ?? h.name}{#if servers.favourites.has(h.id)} <span class="star" title="Favourite">★</span>{/if}</td>
            <td class="mono">{h.ip}:{h.gamePort}</td>
            <td class="num">{h.mods}</td>
            <td><button class="btn" onclick={() => joinAgain(h)}>Join again</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  {#if servers.error}<p class="error">{servers.error}</p>{/if}
</section>

<style>
  .recent { display: flex; flex-direction: column; gap: 10px; }
  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid var(--border); }
  th { color: var(--fg-muted); font-weight: 500; }
  .num { text-align: right; }
  .mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  .star { color: var(--accent); }
  .btn { all: unset; cursor: pointer; padding: 4px 10px; border-radius: var(--radius); background: var(--accent); color: #111; font-weight: 600; font-size: 12px; }
  .btn:focus-visible { outline: 2px solid var(--fg); }
  .error { color: var(--danger); }
</style>
