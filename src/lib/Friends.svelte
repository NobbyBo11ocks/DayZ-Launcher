<script lang="ts">
  // Friends tab (D-092): Steam friends with presence, who is in DayZ and on which
  // server, and a Join that goes through the usual join dialog. Polled every 30 s
  // while the tab is open; Steam answers from its local cache.
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { invokeLogged as invoke } from "./log";
  import { untrack } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { avatarDataUrl } from "./avatar";
  import { servers } from "./state/servers.svelte";
  import { trustedPlayers, type FriendInfo, type FriendState, type ServerRow } from "./types";

  // Avatars (D-115): 32 px, requested once per friend when the row renders; Steam
  // answers from its cache, so a miss is retried once a few seconds later.
  /** Steam's small avatar, the size `Cmd::FriendAvatar` asks for. */
  const AVATAR_PX = 32;
  const avatars = new SvelteMap<string, string>();
  /** Friends whose avatar has been asked for. A plain set: `avatarFor` runs in the
   *  template, and the old counter lived beside a read of the reactive map, so every
   *  avatar that arrived re-ran each row still missing one and asked again while the
   *  first request was in flight; the 4 s timer only reset that counter, so the retry
   *  D-115 describes never happened as such (D-256). */
  const asked = new Set<string>();
  const retries = new Set<ReturnType<typeof setTimeout>>();
  $effect(() => () => {
    for (const t of retries) clearTimeout(t);
  });
  function requestAvatar(id: string, retry: boolean) {
    // Raw bytes, not a JSON array of numbers: the same 4 096-byte picture crossed
    // IPC as 14 657 bytes of decimal text before (D-181).
    void invoke<ArrayBuffer>("friend_avatar", { steamId: id })
      .then((buf) => {
        const url = buf.byteLength === AVATAR_PX * AVATAR_PX * 4 ? avatarDataUrl({ width: AVATAR_PX, height: AVATAR_PX, rgba: new Uint8ClampedArray(buf) }) : null;
        if (url) avatars.set(id, url);
        else if (retry) {
          // Steam answers from its cache and may not have it yet: once more, 4 s on —
          // unless the session has been released since, which the worker would not
          // re-open for an avatar; then it is asked again when the session is back.
          const t = setTimeout(() => {
            retries.delete(t);
            if (servers.steam?.idle) asked.delete(id);
            else requestAvatar(id, false);
          }, 4000);
          retries.add(t);
        }
      })
      .catch(() => {});
  }
  function avatarFor(f: FriendInfo): string | null {
    const have = avatars.get(f.steamId);
    if (have) return have;
    // Not while the session is released: "Show offline" asked for every avatar not yet
    // fetched, and the first one re-opened the session, putting "Playing DayZ" back on
    // the user's Steam profile for a whole idle period (D-165, D-275). Reading the flag
    // here re-runs this once it clears, which is when they are asked for.
    if (steamIdle) return null;
    if (!asked.has(f.steamId)) {
      asked.add(f.steamId);
      requestAvatar(f.steamId, true);
    }
    return null;
  }

  const LABEL: Record<FriendState, string> = {
    offline: "Offline",
    online: "Online",
    invisible: "Invisible",
    busy: "Busy",
    away: "Away",
    snooze: "Snooze",
    looking_to_trade: "Looking to trade",
    looking_to_play: "Looking to play",
  };
  const REFRESH_MS = 30_000;

  let friends = $state<FriendInfo[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let loadedAt = $state("");
  let joining = $state<string | null>(null);
  let showOffline = $state(false);

  const steamOk = $derived(servers.steam?.initialized === true);

  async function load() {
    if (loading) return;
    loading = true;
    try {
      friends = await invoke<FriendInfo[]>("friends_list");
      error = null;
      loadedAt = new Date().toLocaleTimeString();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // Re-runs only when Steam becomes available or goes away; `load` reads state, so
  // the first call is untracked to keep this effect from depending on it.
  /** Only the flag, so an equal boolean stops the propagation: `servers.steam` is
   *  a fresh object on every status event and rebuilt the 30 s poll each time (D-197). */
  const steamIdle = $derived(servers.steam?.idle === true);

  $effect(() => {
    if (!steamOk) return;
    // Same rule as the poll below (D-159): opening this page must not re-open an
    // idle-released session either, or simply looking at it restarts Steam's
    // playtime clock. Refresh does it deliberately when the user asks (D-160).
    if (!steamIdle) untrack(() => void load());
    // Skip the poll while the Steam session is idle-released (D-159): asking for
    // friends re-opens it, which puts the user back to "Playing DayZ" in Steam and
    // restarts playtime — exactly what the idle release (D-077) exists to stop.
    const t = setInterval(() => {
      if (!servers.steam?.idle) void load();
    }, REFRESH_MS);
    return () => clearInterval(t);
  });

  const visible = $derived(showOffline ? friends : friends.filter((f) => f.inDayz || f.state !== "offline"));
  const inDayz = $derived(friends.filter((f) => f.inDayz).length);
  const online = $derived(friends.filter((f) => f.state !== "offline").length);

  /** The friend's server when it is already in our list (by ip:queryPort). */
  const serverOf = (f: FriendInfo) => (f.server && servers.rowsTick >= 0 ? (servers.rows.get(`${f.server.ip}:${f.server.queryPort}`) ?? null) : null);

  /**
   * The listed row for a friend known only by a game address (rich presence carries no
   * query port): looked up by ip and game port, so a server already in the list is
   * not probed for, and the probe's port guess — which misses 14 % of populated
   * servers (D-245) — is not needed (D-265). Only on Join: it scans every row.
   */
  function byGameAddress(ip: string, gamePort: number): ServerRow | null {
    for (const r of servers.rows.values()) if (r.ip === ip && r.gamePort === gamePort) return r;
    return null;
  }

  async function join(f: FriendInfo) {
    if (!f.server) return;
    if (joining) return;
    const known = serverOf(f) ?? byGameAddress(f.server.ip, f.server.gamePort);
    if (known) {
      servers.select(known.id);
      servers.joiningId = known.id;
      return;
    }
    joining = f.steamId;
    error = null;
    // Steam hands over the query port with the game address; a direct connect on the
    // game port alone has to guess it, and the guess misses 14 % of populated servers
    // (733 of 5 249 in the 2026-09-24 cache, D-245). The typed port is tried as the
    // query port first, so the address form needs no host change.
    const port = f.server.queryPort > 0 ? f.server.queryPort : f.server.gamePort;
    const row = await servers.directConnect(`${f.server.ip}:${port}`);
    joining = null;
    // A probe that answers late must not replace a dialog opened meanwhile, which could
    // be mid-download or mid-launch (D-265).
    if (row && servers.joiningId === null) servers.joiningId = row.id;
    // The store records the reason, but this page shows its own error line, so a
    // friend on an unreachable server looked like a button that does nothing (D-160).
    else error = servers.error ?? `${f.name}'s server did not answer; it may block queries or be behind a firewall.`;
  }
</script>

<div class="friends">
  <div class="top">
    <div class="bar">
      <button class="btn" onclick={() => void load()} disabled={loading || !steamOk}>{loading ? "Refreshing…" : "Refresh"}</button>
      <label class="check"><input type="checkbox" bind:checked={showOffline} /> Show offline</label>
      <span class="muted">
        {friends.length} friend{friends.length === 1 ? "" : "s"} · {online} online · {inDayz} in DayZ
        {#if loadedAt}· updated {loadedAt}{/if}
      </span>
      {#if servers.steam?.idle && !friends.length}<span class="muted">Steam session released while idle — press Refresh to fetch the list.</span>{/if}
      {#if error}<span class="error" role="alert">{error}</span>{/if}
    </div>
  </div>
  {#if !steamOk}
    <div class="empty">
      <p>Steam is not running.</p>
      <p class="muted">The friends list comes from Steam; it fills in by itself once Steam is up.</p>
    </div>
  {:else if visible.length === 0}
    <div class="empty">
      <p>{friends.length ? "Nobody online right now." : "No friends found."}</p>
      <p class="muted">Friends in DayZ appear at the top with the server they are on, as soon as Steam knows it. Join takes you through the usual mod check and launch.</p>
    </div>
  {:else}
    <div class="scroll">
      <table>
        <thead><tr><th>Name</th><th>Status</th><th>Server</th><th></th></tr></thead>
        <tbody>
          {#each visible as f (f.steamId)}
            {@const row = serverOf(f)}
            <tr class:dim={f.state === "offline" && !f.inDayz}>
              <!-- The flex row sits inside the cell, not on it: a <td> made a flex box is no
                   longer a table cell, so it stayed its own height and its bottom border
                   ended short of the row's on every row a Join button made taller (D-250). -->
              <td>
                <span class="who">
                  {#if avatarFor(f)}
                    <img class="avatar" src={avatarFor(f)} alt="" width="24" height="24" />
                  {:else}
                    <span class="avatar placeholder" aria-hidden="true">{f.name.slice(0, 1).toUpperCase()}</span>
                  {/if}
                  <span class="dot {f.inDayz ? 'dayz' : f.state}" aria-hidden="true"></span>{f.name}
                </span>
              </td>
              <td class={f.inDayz ? "accent" : "muted"}>{f.inDayz ? "In DayZ" : LABEL[f.state]}</td>
              <td>
                {#if row}
                  {row.name} <span class="muted">· {row.map} · {trustedPlayers(row)}/{row.maxPlayers}</span>
                {:else if f.server}
                  <span class="mono">{f.server.ip}:{f.server.gamePort}</span>
                {:else if f.inDayz}
                  <span class="muted" title="Steam learns the server when the game authenticates with it; this list asks again every 30 seconds">server not reported by Steam yet (main menu, or still loading)</span>
                {:else}
                  <span class="muted">–</span>
                {/if}
              </td>
              <td class="act">
                {#if f.server}
                  <button class="btn" onclick={() => join(f)} disabled={joining !== null} title="Join the same server">{joining === f.steamId ? "…" : "Join"}</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .friends { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .top { padding: 8px 16px; border-bottom: 1px solid var(--border); }
  .bar { display: flex; align-items: center; gap: 12px; font-size: 12px; }
  .check { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; }
  .check input { accent-color: var(--accent-ink); }
  .empty { margin: auto; text-align: center; max-width: 480px; }
  .empty p { margin: 4px 0; }
  .scroll { flex: 1; min-height: 0; overflow: auto; padding: 0 16px; }
  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  th, td { text-align: left; padding: 4px 8px; border-bottom: 1px solid var(--border); vertical-align: middle; }
  th { position: sticky; top: 0; background: var(--bg); color: var(--fg-muted); font-weight: 500; }
  .act { text-align: right; }
  .act .btn { padding: 4px 10px; font-size: 12px; }
  .mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  .accent { color: var(--accent-ink); }
  .dim td { color: var(--fg-muted); }
  .who { display: flex; align-items: center; gap: 8px; }
  .avatar { width: 24px; height: 24px; border-radius: 50%; flex: none; }
  .avatar.placeholder { display: inline-grid; place-items: center; background: var(--bg-row); border: 1px solid var(--border); color: var(--fg-muted); font-size: 11px; font-weight: 600; }
  .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: 0; background: var(--fg-muted); opacity: 0.5; vertical-align: 1px; flex: none; }
  .dot.online, .dot.looking_to_play, .dot.looking_to_trade { background: var(--ok); opacity: 1; }
  .dot.away, .dot.snooze, .dot.busy { background: var(--warn); opacity: 1; }
  .dot.dayz { background: var(--accent); opacity: 1; }
  .error { color: var(--danger); }
</style>
