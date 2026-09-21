<script lang="ts">
  // Details pane (docs/06 §3): live INFO/RULES/PLAYER for the selected server,
  // mods with installed state, verification verdict with its reason.
  import { invoke } from "@tauri-apps/api/core";
  import Flag from "./Flag.svelte";
  import Sparkline from "./Sparkline.svelte";
  import { servers } from "./state/servers.svelte";
  import { clock, countryName, isInflated, trustedPlayers, type Diagnostics, type PopulationSample, type ServerDetails, type ServerRow } from "./types";

  let { row, localVersion }: { row: ServerRow | null; localVersion: string | null } = $props();

  // The effects below key on these primitives, never on `row` itself: every
  // verification (including the one `server_details` publishes) replaces the row
  // object in the store, and an effect that read `row` re-ran the query on each
  // replacement, looping forever and blanking the pane each time (D-065).
  const id = $derived(row?.id ?? null);
  const verifiedAt = $derived(row?.verifiedAt ?? null);

  let samples = $state<PopulationSample[]>([]);
  let samplesFor: string | null = null;

  // Population samples for the selected server; refetched (without blanking) when a
  // new verification lands, cleared only when the selection changes.
  $effect(() => {
    const cur = id;
    void verifiedAt;
    if (!cur) {
      samples = [];
      samplesFor = null;
      return;
    }
    if (samplesFor !== cur) {
      samples = [];
      samplesFor = cur;
    }
    let cancelled = false;
    invoke<PopulationSample[]>("population_history", { id: cur, hours: 72 })
      .then((s) => {
        if (!cancelled) samples = s;
      })
      .catch(() => {
        /* keep what we have */
      });
    return () => {
      cancelled = true;
    };
  });

  let details = $state<ServerDetails | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let installed = $state<Set<number> | null>(null);
  let copied = $state(false);

  // Installed Workshop items, once per session (diagnostics is a few ms).
  $effect(() => {
    if (installed) return;
    invoke<Diagnostics>("diagnostics")
      .then((d) => (installed = new Set(d.workshop?.items.map((i) => i.id) ?? [])))
      .catch(() => (installed = new Set()));
  });

  // Live INFO/RULES/PLAYER, once per selection.
  $effect(() => {
    const cur = id;
    details = null;
    error = null;
    if (!cur) {
      loading = false;
      return;
    }
    loading = true;
    let cancelled = false;
    invoke<ServerDetails>("server_details", { id: cur })
      .then((d) => {
        if (!cancelled) details = d;
      })
      .catch((e) => {
        if (!cancelled) error = String(e);
      })
      .finally(() => {
        if (!cancelled) loading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  const mods = $derived(details?.rules?.dayz?.mods ?? []);
  const missing = $derived(installed ? mods.filter((m) => !installed!.has(m.workshopId)).length : 0);
  const verdictLabel: Record<string, string> = {
    verified: "Verified head-count",
    inflated: "Inflated player count",
    unverifiable: "Refuses player queries",
    synthetic: "Fabricated player list",
    offline: "Not answering",
  };
  const fmtDur = (s: number) => (s >= 3600 ? `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} m` : `${Math.floor(s / 60)} m`);

  async function copyAddress() {
    if (!row) return;
    try {
      await navigator.clipboard.writeText(`${row.ip}:${row.gamePort}`);
      copied = true;
      setTimeout(() => (copied = false), 1200);
    } catch {
      /* clipboard blocked */
    }
  }
</script>

<aside class="details" aria-label="Server details">
  {#if !row}
    <p class="muted empty">Select a server to see its mods, players and rules.</p>
  {:else}
    {@const v = details?.verification}
    {@const verdict = v?.verdict ?? row.verdict}
    <header class="head">
      <h2 title={row.name}>{row.name}</h2>
      <div class="sub muted">
        {#if row.country}<Flag code={row.country} /> {countryName(row.country)} · {/if}{row.map} · {row.ip}:{row.gamePort}
        <button class="link" onclick={copyAddress}>{copied ? "copied" : "copy"}</button>
      </div>
      <div class="actions">
        <button class="join" onclick={() => (servers.joiningId = row.id)} title="Check mods, download what is missing, and start DayZ">Join</button>
        <button class="star" class:on={servers.favourites.has(row.id)} onclick={() => servers.toggleFavourite(row.id)} aria-pressed={servers.favourites.has(row.id)} title="Favourite (F)">
          {servers.favourites.has(row.id) ? "★" : "☆"}
        </button>
        {#if localVersion && row.version !== localVersion}
          <span class="badge warn" title="Server version differs from your DayZ_x64.exe">v{row.version} ≠ {localVersion}</span>
        {:else}
          <span class="badge">v{row.version}</span>
        {/if}
      </div>
    </header>

    {@const vouched = row.steamEmpty === false && verdict === "unverifiable"}
    <section class="trust" class:bad={verdict && verdict !== "verified" && !vouched} class:good={verdict === "verified" || vouched}>
      {#if isInflated(row) && !v}
        <strong>Inflated player count</strong>
        <span class="muted">Steam reports 0 authenticated players; the server claims {row.players}.</span>
      {:else if v && vouched}
        <strong>Steam-confirmed players</strong>
        <span class="muted">The server does not answer player queries, but Steam's authenticated session count shows real players. Showing the server's own number.</span>
      {:else if v}
        <strong>{verdictLabel[v.verdict]}</strong>
        <span class="muted">{v.reason}</span>
      {:else}
        <strong class="muted">{loading ? "Querying server…" : "Not verified yet"}</strong>
      {/if}
    </section>

    <dl class="grid">
      <dt>Players</dt>
      <dd>{trustedPlayers(row)}/{row.maxPlayers}{#if row.tags.queue} <span class="muted">+{row.tags.queue} in queue</span>{/if}{#if row.verifiedPlayers != null && row.players !== row.verifiedPlayers} <span class="muted">(server claims {row.players})</span>{/if}</dd>
      <dt>Ping</dt>
      <dd>{details?.infoRttMs ?? row.pingMs} ms</dd>
      <dt>Perspective</dt>
      <dd>{row.tags.firstPersonOnly ? "First person only" : "First and third person"}</dd>
      <dt>Time</dt>
      <dd>{clock(row.tags.timeMinutes)}{#if row.tags.timeMultiplier != null} <span class="muted">day ×{row.tags.timeMultiplier}{#if row.tags.nightMultiplier != null}, night ×{row.tags.nightMultiplier}{/if}</span>{/if}</dd>
      <dt>Hive</dt>
      <dd>{row.tags.privateHive ? "Private" : "Public"}{#if row.tags.shard} <span class="muted">shard {row.tags.shard}</span>{/if}</dd>
      <dt>Protection</dt>
      <dd>{row.tags.battleye ? "BattlEye" : "No BattlEye"}{row.password ? " · password" : ""}{row.tags.allowedFilePatching ? " · file patching allowed" : ""}</dd>
      <dt>Query port</dt>
      <dd>{row.queryPort}</dd>
    </dl>

    {#if details?.info?.game || row.description}
      <p class="desc">{details?.info?.game || row.description}</p>
    {/if}

    <section>
      <h3>Population, last 72 h</h3>
      <Sparkline {samples} maxPlayers={row.maxPlayers} />
    </section>

    <section>
      <h3>Mods {#if details?.rules?.dayz}({mods.length}){#if installed && missing}<span class="warn"> · {missing} missing</span>{/if}{/if}</h3>
      {#if loading && !details}
        <p class="muted">Loading…</p>
      {:else if details?.rulesError}
        <p class="muted">Rules unavailable: {details.rulesError}</p>
      {:else if details?.rules?.dayz && mods.length === 0}
        <p class="muted">Vanilla (no mods reported).</p>
      {:else if mods.length}
        <ul class="mods">
          {#each mods as m (m.workshopId)}
            <li class:missing={installed && !installed.has(m.workshopId)}>
              <span class="tick" aria-hidden="true">{installed ? (installed.has(m.workshopId) ? "✓" : "○") : "·"}</span>
              <span class="mname" title={m.name}>{m.name}</span>
              <a class="mid muted" href="https://steamcommunity.com/sharedfiles/filedetails/?id={m.workshopId}" target="_blank" rel="noreferrer">{m.workshopId}</a>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    {#if details?.players && details.players.players.length}
      <section>
        <h3>Connected ({details.players.players.length})</h3>
        <p class="muted">Sessions: {details.players.players.map((p) => fmtDur(p.durationSecs)).join(", ")}</p>
      </section>
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
  {/if}
</aside>

<style>
  .details { display: flex; flex-direction: column; gap: 12px; padding: 14px 16px; overflow: auto; min-height: 0; height: 100%; background: var(--bg-elev); border-left: 1px solid var(--border); }
  .empty { margin: auto; text-align: center; }
  .head h2 { font-size: 15px; font-weight: 600; margin: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sub { font-size: 12px; margin-top: 2px; }
  .link { all: unset; cursor: pointer; color: var(--accent); margin-left: 6px; font-size: 11px; }
  .actions { display: flex; align-items: center; gap: 10px; margin-top: 10px; }
  .join { all: unset; cursor: pointer; padding: 7px 18px; border-radius: var(--radius); background: var(--accent); color: #111; font-weight: 600; }
  .join:hover { filter: brightness(1.08); }
  .join:focus-visible { outline: 2px solid var(--fg); }
  .star { all: unset; cursor: pointer; font-size: 18px; color: var(--fg-muted); padding: 0 4px; }
  .star.on, .star:hover { color: var(--accent); }
  .star:focus-visible { outline: 2px solid var(--accent); border-radius: 4px; }
  .badge { font-size: 11px; padding: 2px 6px; border-radius: 4px; border: 1px solid var(--border); color: var(--fg-muted); }
  .badge.warn { color: var(--warn); border-color: var(--warn); }
  .trust { padding: 8px 10px; border-radius: var(--radius); border: 1px solid var(--border); display: flex; flex-direction: column; gap: 2px; font-size: 12px; }
  .trust.good { border-color: color-mix(in srgb, var(--ok) 50%, var(--border)); }
  .trust.bad { border-color: color-mix(in srgb, var(--warn) 60%, var(--border)); }
  .grid { display: grid; grid-template-columns: max-content 1fr; gap: 4px 12px; margin: 0; font-size: 12.5px; }
  .grid dt { color: var(--fg-muted); }
  .grid dd { margin: 0; }
  .desc { font-size: 12px; color: var(--fg-muted); margin: 0; white-space: pre-wrap; }
  h3 { font-size: 12px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; margin: 0 0 6px; }
  .mods { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; font-size: 12px; }
  .mods li { display: grid; grid-template-columns: 14px 1fr auto; gap: 6px; align-items: center; }
  .mods li.missing { color: var(--warn); }
  .tick { color: var(--ok); }
  .missing .tick { color: var(--warn); }
  .mname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mid { font-size: 11px; text-decoration: none; }
  .mid:hover { text-decoration: underline; }
  .warn { color: var(--warn); }
  .error { color: var(--danger); }
</style>
