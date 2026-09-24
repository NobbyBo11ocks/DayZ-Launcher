<script lang="ts">
  // Details pane (docs/06 §3): live INFO/RULES/PLAYER for the selected server,
  // mods with installed state, verification verdict with its reason. Layout rules
  // (D-081): nothing may overflow the 360 px column, long lists collapse behind a
  // toggle, and the player sessions are summarised instead of listed.
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158/D-160).
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { invokeLogged as invoke } from "./log";
  import Flag from "./Flag.svelte";
  import Sparkline from "./Sparkline.svelte";
  import { external } from "./external";
  import { mapLabel } from "./maps";
  import { servers } from "./state/servers.svelte";
  import { clock, countryName, type Diagnostics, isInflated, type PopulationSample, queueOf, type ServerDetails, type ServerRow, trustedPlayers } from "./types";

  let { row, localVersion }: { row: ServerRow | null; localVersion: string | null } = $props();

  // The effects below key on these primitives, never on `row` itself: every
  // verification (including the one `server_details` publishes) replaces the row
  // object in the store, and an effect that read `row` re-ran the query on each
  // replacement, looping forever and blanking the pane each time (D-065).
  const id = $derived(row?.id ?? null);
  const verifiedAt = $derived(row?.verifiedAt ?? null);

  /** How long a selection has to hold still before the pane queries the server.
   *  Long enough that arrow-keying through rows costs nothing, short enough that a
   *  deliberate click still feels immediate. */
  const DETAILS_SETTLE_MS = 220;

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
  let allMods = $state(false);
  let fullDesc = $state(false);

  // Installed Workshop items (diagnostics is a few ms). Read once, then again after
  // any Workshop download finishes: it was read once per session, so mods installed
  // during the session kept their "missing" mark until a restart (D-160).
  $effect(() => {
    if (installed) return;
    invoke<Diagnostics>("diagnostics")
      .then((d) => (installed = new Set(d.workshop?.items.map((i) => i.id) ?? [])))
      .catch(() => (installed = new Set()));
  });
  $effect(() => {
    // The unlisten handle arrives after an await, so cleanup has to wait for the
    // promise rather than read a variable that may still be undefined (D-222).
    const pending = listen("mods:done", () => (installed = null));
    return () => void pending.then((f) => f());
  });

  // Live INFO/RULES/PLAYER, once per selection.
  $effect(() => {
    const cur = id;
    details = null;
    error = null;
    allMods = false;
    fullDesc = false;
    if (!cur) {
      loading = false;
      return;
    }
    loading = true;
    let cancelled = false;
    // Held back until the selection settles. Each call is three paced A2S queries out
    // of the same budget a verification pass is using, plus a `servers:verified` emit
    // whose derived chain costs ~6.4 ms however few rows it carries — so holding
    // ArrowDown down the list was spending ~64 ms of main thread and ~50 datagrams a
    // second on servers the user was scrolling straight past (D-193).
    const timer = setTimeout(() => {
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
    }, DETAILS_SETTLE_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  });

  const MODS_COLLAPSED = 10;
  const mods = $derived(details?.rules?.dayz?.mods ?? []);

  // 644 hosts in the cached list run two or more servers, and 1 689 rows — 49 % of
  // the list — have at least one sibling; 478 of those groups span more than one map
  // and the largest runs nine across six. Finding a server you like and not being able
  // to get on it, while the launcher silently knows about its eight neighbours, is a
  // gap worth closing. Titled by address rather than by owner, because sharing a host
  // is not the same as sharing a community — some of these groups are one community,
  // others are one host's unrelated customers (D-212).
  const SIBLINGS_SHOWN = 6;
  const siblings = $derived.by(() => {
    void servers.rowsTick;
    const ip = row?.ip;
    const self = row?.id;
    if (!ip) return [];
    const out: ServerRow[] = [];
    for (const r of servers.rows.values()) if (r.ip === ip && r.id !== self) out.push(r);
    out.sort((a, b) => trustedPlayers(b) - trustedPlayers(a) || a.pingMs - b.pingMs);
    return out;
  });
  let allSiblings = $state(false);
  const shownSiblings = $derived(allSiblings ? siblings : siblings.slice(0, SIBLINGS_SHOWN));
  // A fresh selection collapses the list again.
  $effect(() => {
    void id;
    allSiblings = false;
  });
  const shownMods = $derived(allMods ? mods : mods.slice(0, MODS_COLLAPSED));
  const missing = $derived(installed ? mods.filter((m) => !installed!.has(m.workshopId)).length : 0);
  const description = $derived((details?.info?.game || row?.description || "").trim());
  const descLong = $derived(description.length > 220 || description.split("\n").length > 3);

  // Session lengths summarised (D-081): count, median, longest, arrivals in the last
  // 10 minutes, and a five-bucket distribution instead of one value per player.
  const sessions = $derived.by(() => {
    const secs = (details?.players?.players ?? []).map((p) => p.durationSecs).sort((a, b) => a - b);
    if (!secs.length) return null;
    const buckets = [
      { label: "< 15 m", max: 15 * 60, n: 0 },
      { label: "15–60 m", max: 60 * 60, n: 0 },
      { label: "1–3 h", max: 3 * 3600, n: 0 },
      { label: "3–6 h", max: 6 * 3600, n: 0 },
      { label: "6 h +", max: Infinity, n: 0 },
    ];
    for (const s of secs) buckets.find((b) => s < b.max)!.n++;
    return {
      count: secs.length,
      median: secs[Math.floor(secs.length / 2)] ?? 0,
      longest: secs[secs.length - 1] ?? 0,
      recent: secs.filter((s) => s < 600).length,
      buckets,
      peak: Math.max(...buckets.map((b) => b.n)),
    };
  });

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
    {@const vouched = row.steamEmpty === false && verdict === "unverifiable"}
    {@const counted = row.verifiedPlayers != null}
    {@const versionOk = !localVersion || row.version === localVersion}

    <header class="head">
      <h2 title={row.name}>{row.name}</h2>
      <div class="meta">
        {#if row.country}<span class="chip"><Flag code={row.country} /> {countryName(row.country)}</span>{/if}
        <span class="chip" title={row.map}>{mapLabel(row.map)}</span>
        <span class="chip" class:bad={!versionOk} title={versionOk ? "Server version" : `Server version differs from your DayZ_x64.exe (${localVersion})`}>v{row.version}{#if !versionOk} ≠ mine{/if}</span>
        {#if row.password}<span class="chip">🔒 password</span>{/if}
      </div>
      <div class="addr">
        <code>{row.ip}:{row.gamePort}</code>
        <button class="link" onclick={copyAddress}>{copied ? "copied" : "copy"}</button>
        <span class="muted">· query {row.queryPort}</span>
      </div>
      <div class="actions">
        <button class="join" onclick={() => (servers.joiningId = row.id)} title="Check mods, download what is missing, and start DayZ">Join</button>
        <button class="star" aria-label={servers.favourites.has(row.id) ? "Remove from favourites" : "Add to favourites"} class:on={servers.favourites.has(row.id)} onclick={() => servers.toggleFavourite(row.id)} aria-pressed={servers.favourites.has(row.id)} title="Favourite (F)">
          {servers.favourites.has(row.id) ? "★" : "☆"}
        </button>
      </div>
    </header>

    <section class="trust" class:bad={verdict && verdict !== "verified" && !(vouched && counted)} class:good={verdict === "verified"}>
      {#if isInflated(row) && !v}
        <strong>Inflated player count</strong>
        <span class="muted">Steam reports 0 authenticated players; the server claims {row.players}.</span>
      {:else if v && vouched && counted}
        <strong>Last counted {row.verifiedPlayers}</strong>
        <span class="muted">The server has stopped answering player queries; Steam still sees players on it. Showing the last head-count, not the server's claim.</span>
      {:else if v && vouched}
        <strong>Player count unconfirmed</strong>
        <span class="muted">The server does not answer player queries. Steam sees at least one session, which does not confirm the {row.players} it claims (D-233).</span>
      {:else if v}
        <strong>{verdictLabel[v.verdict]}</strong>
        <span class="muted">{v.reason}</span>
      {:else}
        <strong class="muted">{loading ? "Querying server…" : "Not verified yet"}</strong>
      {/if}
    </section>

    <dl class="facts">
      <dt>Players</dt>
      <dd>
        <strong>{trustedPlayers(row)}</strong> / {row.maxPlayers}
        {#if queueOf(row)} <span class="muted">· {queueOf(row)} in queue</span>{/if}
        {#if row.verifiedPlayers != null && row.players !== row.verifiedPlayers} <span class="muted">· server claims {row.players}</span>{/if}
      </dd>
      <dt>Ping</dt>
      <dd>{details?.infoRttMs ?? row.pingMs} ms</dd>
      <dt>View</dt>
      <dd>{row.tags.firstPersonOnly ? "First person only" : "First and third person"}</dd>
      <dt>Time</dt>
      <dd>
        {clock(row.tags.timeMinutes)}
        {#if row.tags.timeMultiplier != null}<span class="muted"> · day ×{row.tags.timeMultiplier}{#if row.tags.nightMultiplier != null}, night ×{row.tags.nightMultiplier}{/if}</span>{/if}
      </dd>
      <dt>Hive</dt>
      <dd>
        {row.tags.privateHive ? "Community" : "Official"}
        <span class="muted"> · {row.tags.privateHive ? "your character lives on this server" : "your character follows you across official servers"}</span>
      </dd>
      {#if !row.tags.battleye || row.tags.allowedFilePatching}
        <dt>Anti-cheat</dt>
        <dd class="bad">
          {row.tags.battleye ? "BattlEye" : "No BattlEye"}{#if row.tags.allowedFilePatching}<span class="muted"> · file patching allowed</span>{/if}
        </dd>
      {/if}
    </dl>

    {#if description}
      <section>
        <h3>Description</h3>
        <p class="desc" class:clamped={descLong && !fullDesc}>{description}</p>
        {#if descLong}<button class="link" onclick={() => (fullDesc = !fullDesc)}>{fullDesc ? "less" : "more"}</button>{/if}
      </section>
    {/if}

    {#if siblings.length}
      <section>
        <h3>Other servers at this address <span class="count">{siblings.length}</span></h3>
        <ul class="sibs">
          {#each shownSiblings as sv (sv.id)}
            <li>
              <button class="sib" onclick={() => (servers.selectedId = sv.id)} title={sv.name}>
                <span class="sname">{sv.name}</span>
                <span class="smeta">{mapLabel(sv.map)} · {trustedPlayers(sv)}/{sv.maxPlayers}</span>
              </button>
            </li>
          {/each}
        </ul>
        {#if siblings.length > SIBLINGS_SHOWN}
          <button class="link" onclick={() => (allSiblings = !allSiblings)}>{allSiblings ? "Show fewer" : `Show all ${siblings.length}`}</button>
        {/if}
      </section>
    {/if}

    <section>
      <h3>Population, last 72 h</h3>
      <Sparkline {samples} maxPlayers={row.maxPlayers} />
    </section>

    <section>
      <h3>
        Mods{#if details?.rules?.dayz} <span class="count">{mods.length}</span>{/if}
        {#if installed && missing}<span class="warn"> · {missing} missing</span>{/if}
      </h3>
      {#if loading && !details}
        <p class="muted">Loading…</p>
      {:else if details?.rulesError}
        <p class="muted">Rules unavailable: {details.rulesError}</p>
      {:else if details?.rules?.dayz && mods.length === 0}
        <p class="muted">Vanilla, no mods required.</p>
      {:else if mods.length}
        <ul class="mods">
          {#each shownMods as m, i (`${m.workshopId}#${i}`)}
            <li class:missing={installed && !installed.has(m.workshopId)}>
              <span class="tick" aria-hidden="true">{installed ? (installed.has(m.workshopId) ? "✓" : "○") : "·"}</span>
              <span class="mname" title={m.name}>{m.name}</span>
              <a class="mid" href="https://steamcommunity.com/sharedfiles/filedetails/?id={m.workshopId}" onclick={external} title="Open in the Steam Workshop">{m.workshopId}</a>
            </li>
          {/each}
        </ul>
        {#if mods.length > MODS_COLLAPSED}
          <button class="link" onclick={() => (allMods = !allMods)}>{allMods ? "Show fewer" : `Show all ${mods.length}`}</button>
        {/if}
      {/if}
    </section>

    {#if sessions}
      <section>
        <h3>Connected <span class="count">{sessions.count}</span></h3>
        <p class="sum">
          Median session <strong>{fmtDur(sessions.median)}</strong> · longest <strong>{fmtDur(sessions.longest)}</strong>
          {#if sessions.recent} · <strong>{sessions.recent}</strong> joined in the last 10 min{/if}
        </p>
        <ul class="hist" aria-label="Session lengths">
          {#each sessions.buckets as b (b.label)}
            <li>
              <span class="hl">{b.label}</span>
              <span class="hb"><span class="hf" style="width: {sessions.peak ? (100 * b.n) / sessions.peak : 0}%"></span></span>
              <span class="hn">{b.n}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
  {/if}
</aside>

<style>
  .details { display: flex; flex-direction: column; gap: 14px; padding: 14px 16px 18px; overflow-y: auto; overflow-x: hidden; min-height: 0; height: 100%; min-width: 0; background: var(--bg-elev); border-left: 1px solid var(--border); font-size: 12.5px; overflow-wrap: anywhere; }
  .empty { margin: auto; text-align: center; }

  .head { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .head h2 { font-size: 15px; font-weight: 600; line-height: 1.3; margin: 0; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .meta { display: flex; flex-wrap: wrap; gap: 4px; }
  .chip { display: inline-flex; align-items: center; gap: 5px; font-size: 11.5px; padding: 2px 7px; border-radius: 999px; border: 1px solid var(--border-control); background: var(--bg-elev); color: var(--fg-muted); max-width: 100%; }
  .chip.bad { color: var(--warn); border-color: var(--warn); }
  .addr { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; font-size: 12px; color: var(--fg-muted); }
  .addr code { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; color: var(--fg); }
  .link { all: unset; cursor: pointer; color: var(--accent-ink); font-size: 11.5px; }
  .link:hover { text-decoration: underline; }
  .link:focus-visible { outline: 2px solid var(--accent-ink); border-radius: 3px; }
  .actions { display: flex; align-items: center; gap: 8px; margin-top: 2px; }
  .join { all: unset; cursor: pointer; flex: 1; text-align: center; padding: 8px 18px; border-radius: var(--radius); background: var(--accent); color: var(--accent-fg); font-weight: 600; }
  .join:hover { filter: brightness(1.08); }
  .join:focus-visible { outline: 2px solid var(--accent-ink); }
  .star { all: unset; cursor: pointer; flex: none; width: 34px; height: 34px; display: inline-flex; align-items: center; justify-content: center; border-radius: var(--radius); border: 1px solid var(--border); font-size: 17px; color: var(--fg-muted); }
  .star.on, .star:hover { color: var(--accent-ink); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); }
  .star:focus-visible { outline: 2px solid var(--accent-ink); }

  .trust { padding: 8px 10px; border-radius: var(--radius); border: 1px solid var(--border); display: flex; flex-direction: column; gap: 2px; font-size: 12px; }
  .trust.good { border-color: color-mix(in srgb, var(--ok) 50%, var(--border)); }
  .trust.bad { border-color: color-mix(in srgb, var(--warn) 60%, var(--border)); }

  .facts { display: grid; grid-template-columns: 76px minmax(0, 1fr); gap: 5px 10px; margin: 0; }
  .facts dt { color: var(--fg-muted); }
  /* `.bad` existed only as `.chip.bad` and `.trust.bad`, so the anti-cheat warning
     D-195 added rendered in plain `--fg` — the same silent miss as D-186's
     `var(--muted)`. `--warn` measures 6.85:1 dark and 4.87:1 light here (D-197). */
  .facts dd.bad { color: var(--warn); }
  .facts dd { margin: 0; min-width: 0; }

  section { display: flex; flex-direction: column; gap: 6px; padding-top: 12px; border-top: 1px solid var(--border); min-width: 0; }
  h3 { font-size: 11.5px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.05em; margin: 0; display: flex; align-items: center; gap: 6px; }
  .count { font-weight: 500; color: var(--fg); padding: 0 6px; border-radius: 999px; background: var(--bg-row); border: 1px solid var(--border); text-transform: none; letter-spacing: 0; }

  .desc { margin: 0; color: var(--fg-muted); white-space: pre-line; line-height: 1.4; }
  .desc.clamped { display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }

  .sibs { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 3px; }
  .sib { all: unset; cursor: pointer; box-sizing: border-box; width: 100%; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; align-items: baseline; padding: 2px 4px; border-radius: var(--radius); }
  .sib:hover { background: var(--bg-row); }
  .sib:focus-visible { outline: 2px solid var(--accent-ink); }
  .sname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .smeta { color: var(--fg-muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .mods { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 3px; }
  .mods li { display: grid; grid-template-columns: 14px minmax(0, 1fr) auto; gap: 6px; align-items: center; }
  .mods li.missing { color: var(--warn); }
  .tick { color: var(--ok); }
  .missing .tick { color: var(--warn); }
  .mname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mid { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11px; color: var(--fg-muted); text-decoration: none; }
  .mid:hover { text-decoration: underline; }

  .sum { margin: 0; color: var(--fg-muted); line-height: 1.5; }
  .sum strong { color: var(--fg); font-weight: 600; }
  .hist { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 3px; font-size: 11.5px; }
  .hist li { display: grid; grid-template-columns: 52px minmax(0, 1fr) 28px; gap: 8px; align-items: center; }
  .hl { color: var(--fg-muted); white-space: nowrap; }
  .hb { height: 6px; border-radius: 3px; background: var(--bg-row); overflow: hidden; }
  .hf { display: block; height: 100%; background: color-mix(in srgb, var(--accent) 70%, transparent); border-radius: 3px; }
  .hn { text-align: right; color: var(--fg-muted); }

  .warn { color: var(--warn); }
  .error { color: var(--danger); margin: 0; }
</style>
