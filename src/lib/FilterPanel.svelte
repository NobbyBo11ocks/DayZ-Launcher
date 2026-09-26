<script lang="ts">
  // The server filters as a column in the left rail, under the section list (docs/06
  // §4, D-249). The user's sketch put them there, leaving the Servers header with the
  // search, Direct connect and Refresh. App.svelte mounts this only while the Servers
  // page is open, the one page these filters apply to; all state lives in the store.
  //
  // Every filter is in view at once, nothing scrolls (D-259): each choice is one 26 px
  // row that carries its own label, and the column is cut to fit under the sections at
  // the default window height. The body still scrolls if a window is too short for it,
  // but the rail turns the sections into a row of icons before that happens (app.css).
  import { servers, type HiveFilter, type ModFilter, type Perspective, type StyleFilter } from "./state/servers.svelte";
  import { countryName } from "./types";
  import { mapLabel } from "./maps";

  /** Prefix for the ids that tie each group to its visible label. */
  const uid = $props.id();

  // 1 611 of 3 446 cached servers answer inside 60 ms and 742 are 120 ms or worse, so
  // these are the cuts that actually divide the list (D-211).
  const PING_PRESETS = [50, 80, 100, 150, 200];

  // Sliced once rather than twice per render: the each block and the guard below it
  // both need the same list.
  const shownMaps = $derived(servers.maps.slice(0, 40));
  const shownCountries = $derived(servers.countries.slice(0, 60));

  const f = $derived(servers.filters);
  const fmt = new Intl.NumberFormat();
  const toggle = (key: "notFull" | "notEmpty" | "hasQueue" | "noPassword" | "dayOnly" | "versionMine" | "hideUntrusted" | "friendsOnly") => {
    servers.filters[key] = !servers.filters[key];
    servers.saveFilters();
  };
  const setHive = (h: HiveFilter) => {
    servers.filters.hive = h;
    servers.saveFilters();
  };
  const setStyle = (v: StyleFilter) => {
    servers.filters.style = v;
    servers.saveFilters();
  };
  const setPerspective = (p: Perspective) => {
    servers.filters.perspective = p;
    servers.saveFilters();
  };
  const setMods = (m: ModFilter) => {
    servers.filters.mods = m;
    servers.saveFilters();
  };

  // Mod filter (D-080): the catalogue can hold thousands of names, so a text box
  // narrows the select; the chosen mod always stays listed.
  let modQuery = $state("");
  const modOptions = $derived.by(() => {
    const q = modQuery.trim().toLowerCase();
    const list = servers.modOptions.filter((m) => !q || m.name.toLowerCase().includes(q)).slice(0, 300);
    if (f.mod && !list.some((m) => m.id === f.mod)) {
      const cur = servers.modCatalog.get(f.mod);
      list.unshift({ id: f.mod, name: cur?.name ?? String(f.mod), servers: cur?.servers ?? 0 });
    }
    return list;
  });
</script>

<section class="panel" aria-labelledby="{uid}-title">
  <div class="head">
    <h2 id="{uid}-title">Filters</h2>
    {#if servers.activeFilterCount > 0}
      <button class="reset" onclick={() => servers.resetFilters()} title="Back to the default filters">Reset · {servers.activeFilterCount}</button>
    {/if}
  </div>

  <div class="body">
    <div class="row" role="group" aria-labelledby="{uid}-perspective">
      <span class="tag" id="{uid}-perspective">Perspective</span>
      <div class="seg">
        {#each [["any", "Any"], ["1pp", "1PP"], ["3pp", "3PP"]] as [v, label] (v)}
          <button class="segbtn" class:on={f.perspective === v} aria-pressed={f.perspective === v} onclick={() => setPerspective(v as Perspective)}>{label}</button>
        {/each}
      </div>
    </div>

    <!-- What the server says it is, read out of its own name and description. Nobody
         verifies a ruleset, so the tooltips say "says" — and a server that claims both
         shows under both. 41.6 % of the list says PVE (D-211). -->
    <div class="row" role="group" aria-labelledby="{uid}-style">
      <span class="tag" id="{uid}-style">Playstyle</span>
      <div class="seg">
        {#each [["any", "Any", "Every playstyle"], ["pve", "PVE", "The name or description says PVE"], ["pvp", "PVP", "The name or description says PVP"], ["rp", "RP", "The name or description says RP or roleplay"]] as [v, label, hint] (v)}
          <button class="segbtn" class:on={f.style === v} aria-pressed={f.style === v} title={hint} onclick={() => setStyle(v as StyleFilter)}>{label}</button>
        {/each}
      </div>
    </div>

    <!-- Real checkboxes now that the toggles have a column of their own: a ticked row
         is ticked, where a chip only changed colour. Two to a row for the short ones;
         the one with a count gets the full width so the number lines up at the right
         (Hide inflated, the other, went down with the foot). The block needs no
         heading: every row names itself. -->
    <div class="checks" role="group" aria-label="Status">
      <label class="check"><input type="checkbox" checked={f.notEmpty} onchange={() => toggle("notEmpty")} />Not empty</label>
      <label class="check"><input type="checkbox" checked={f.notFull} onchange={() => toggle("notFull")} />Not full</label>
      <label class="check"><input type="checkbox" checked={f.hasQueue} onchange={() => toggle("hasQueue")} />Has queue</label>
      <label class="check"><input type="checkbox" checked={f.noPassword} onchange={() => toggle("noPassword")} />No password</label>
      <label class="check"><input type="checkbox" checked={f.dayOnly} onchange={() => toggle("dayOnly")} />Daytime</label>
      <label class="check" class:off={!servers.localVersion} title={servers.localVersion ? `Only ${servers.localVersion}` : "DayZ not found"}>
        <input type="checkbox" checked={f.versionMine} disabled={!servers.localVersion} onchange={() => toggle("versionMine")} />My version
      </label>
      <label class="check wide" title="Only servers a Steam friend is playing on right now">
        <input type="checkbox" checked={f.friendsOnly} onchange={() => toggle("friendsOnly")} />Friends playing <span class="num">{servers.friendsOn.size}</span>
      </label>
    </div>

    <!-- The foot: from Hide inflated down to the mod search, at the bottom of the rail
         with the free height above it, on the user's call (D-270). A window with no
         height to spare leaves it where it was, under the checkboxes. -->
    <div class="foot">
      <label class="check" title="Hide servers whose player count cannot be trusted: inflated, fabricated, not answering, or a copy of another server's name">
        <input type="checkbox" checked={f.hideUntrusted} onchange={() => toggle("hideUntrusted")} />Hide inflated <span class="num">{fmt.format(servers.untrustedCount)}</span>
      </label>

      <!-- Official is Bohemia's public hive, where your character follows you between
           servers; Community is a private shard, where it does not. The in-game browser
           makes this a top-level tab, and the tag is already on every row (D-195). -->
      <div class="row" role="group" aria-labelledby="{uid}-hive">
        <span class="tag" id="{uid}-hive">Hive</span>
        <div class="seg">
          {#each [["any", "Any", "Both hives"], ["official", "Official", "Bohemia's public hive: your character follows you between these"], ["community", "Community", "Private shards: your character lives on that one server"]] as [v, label, hint] (v)}
            <button class="segbtn" class:on={f.hive === v} aria-pressed={f.hive === v} title={hint} onclick={() => setHive(v as HiveFilter)}>{label}</button>
          {/each}
        </div>
      </div>

      <!-- The label wraps the select, so it names it and a click on it opens it. -->
      <label class="row" class:on={f.map !== ""}>
        <span class="tag">Map</span>
        <select bind:value={servers.filters.map} onchange={() => servers.saveFilters()}>
          <option value="">All maps</option>
          {#each shownMaps as [id, label, n] (id)}
            <option value={id}>{label} ({n})</option>
          {/each}
          <!-- A map filtered to and then ranked out of the top 40, or restored from a
               saved filter, still has to name itself (D-222). -->
          {#if f.map && !shownMaps.some(([id]) => id === f.map)}
            <option value={f.map}>{mapLabel(f.map)}</option>
          {/if}
        </select>
      </label>
      <label class="row" class:on={f.country !== ""}>
        <span class="tag">Country</span>
        <select bind:value={servers.filters.country} onchange={() => servers.saveFilters()}>
          <option value="">All countries</option>
          {#each shownCountries as [cc, n] (cc)}
            <option value={cc}>{countryName(cc)} ({n})</option>
          {/each}
          {#if f.country && !shownCountries.some(([cc]) => cc === f.country)}
            <option value={f.country}>{countryName(f.country)}</option>
          {/if}
        </select>
      </label>

      <!-- Presets rather than a number field: this was the one filter you had to click
           into and type a number for, and an emptied number input binds as null (D-211).
           A value saved before this, or set some other way, keeps its own option. -->
      <label class="row" class:on={f.maxPing > 0} title="Hide servers slower than this">
        <span class="tag">Max ping</span>
        <select bind:value={servers.filters.maxPing} onchange={() => servers.saveFilters()}>
          <option value={0}>Any</option>
          {#each PING_PRESETS as ms (ms)}
            <option value={ms}>{ms} ms</option>
          {/each}
          {#if f.maxPing > 0 && !PING_PRESETS.includes(f.maxPing)}
            <option value={f.maxPing}>{f.maxPing} ms</option>
          {/if}
        </select>
      </label>

      <div class="row" role="group" aria-labelledby="{uid}-mods">
        <span class="tag" id="{uid}-mods">Mods</span>
        <div class="seg">
          {#each [["any", "Any"], ["modded", "Modded"], ["vanilla", "Vanilla"]] as [v, label] (v)}
            <button class="segbtn" class:on={f.mods === v} aria-pressed={f.mods === v} onclick={() => setMods(v as ModFilter)}>{label}</button>
          {/each}
        </div>
      </div>
      {#if servers.modCatalog.size > 0 || f.mod}
        <div class="modf" role="group" aria-label="Running a specific mod">
          <input class="modsearch" type="search" placeholder="Find a mod" bind:value={modQuery} aria-label="Search the mod list" spellcheck="false" />
          <select class="modselect" class:on={f.mod !== 0} bind:value={servers.filters.mod} onchange={() => servers.saveFilters()} aria-label="Servers running this mod" title="Servers whose mod list includes this Workshop item">
            <option value={0}>Any mod</option>
            {#each modOptions as m (m.id)}
              <option value={m.id}>{m.name} ({m.servers})</option>
            {/each}
          </select>
        </div>
      {/if}
      <!-- A mod filter only matches servers whose mod list has been read. The scan runs
           after every refresh; this asks for it now (D-160). It sat on the status line
           under the search until that went, and it matters only with a mod chosen (D-250).
           One line, so it costs the column 21 px rather than a wrapped paragraph. -->
      {#if f.mod && (servers.modScanning || servers.unscannedModded > 0)}
        <p class="hint">
          {#if servers.modScanning}
            Reading mod lists…
          {:else}
            {fmt.format(servers.unscannedModded)} not scanned yet ·
            <button class="link" onclick={() => void servers.scanMods()} title="Read the mod list of every populated modded server that has not been scanned">Scan now</button>
          {/if}
        </p>
      {/if}
    </div>
  </div>
</section>

<style>
  /* Takes what the rail has left under the sections, so the update item stays at the
     foot (D-216). */
  .panel { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; margin-top: 6px; padding-top: 8px; border-top: 1px solid var(--border); }
  /* A fixed height, so Reset appearing with the first filter does not push every row
     down. */
  .head { flex: none; display: flex; align-items: center; justify-content: space-between; gap: 8px; height: 24px; padding-bottom: 4px; }
  h2 { margin: 0; font-size: 10.5px; font-weight: 600; color: var(--accent-ink); text-transform: uppercase; letter-spacing: 0.07em; }
  .reset { all: unset; cursor: pointer; box-sizing: border-box; height: 20px; padding: 0 8px; display: inline-flex; align-items: center; border-radius: var(--radius); border: 1px dashed var(--border-control); color: var(--fg-muted); font-size: 11px; white-space: nowrap; }
  .reset:hover { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 25%, var(--border)); }
  .reset:focus-visible { outline: 2px solid var(--accent-ink); }

  /* The body runs to the rail's edges, and its reserved 10 px scrollbar gutter plus
     2 px stand in for the rail's 12 px right padding (app.css): the rows keep the
     sections' width whether or not the fallback scrollbar is showing. The 2 px around
     the rows keeps their focus rings clear of the clip. The bottom 6 px fade out: with
     everything in view they are only padding, and on a window too short for the rows
     the cut-off one fades instead of ending in a hard line. */
  .body { flex: 1 1 auto; min-height: 0; overflow-y: auto; scrollbar-gutter: stable; margin: 0 -12px; padding: 2px 2px 6px 12px; display: flex; flex-direction: column; gap: 5px; mask-image: linear-gradient(to bottom, #000 calc(100% - 6px), transparent); }

  /* One box per choice, its label inside at the left and the control filling the rest:
     a 26 px row where a label above the control took 48. */
  .row { flex: none; display: flex; align-items: center; box-sizing: border-box; height: 26px; min-width: 0; padding-left: 8px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); }
  .row.on { border-color: color-mix(in srgb, var(--accent) 65%, var(--border)); }
  .tag { flex: none; padding-right: 8px; font-size: 10.5px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.06em; white-space: nowrap; }

  .seg { flex: 1 1 auto; min-width: 0; display: flex; align-self: stretch; padding: 2px; }
  /* Equal widths where the labels allow it; a longer label keeps its own width. */
  .segbtn { all: unset; cursor: pointer; flex: 1 1 0; display: inline-flex; align-items: center; justify-content: center; padding: 0 4px; border-radius: 5px; color: var(--fg-muted); font-size: 12px; font-weight: 500; white-space: nowrap; }
  .segbtn:hover { color: var(--fg); }
  .segbtn.on { background: var(--accent); color: var(--accent-fg); font-weight: 600; }
  .segbtn:focus-visible { outline: 2px solid var(--accent-ink); }
  /* On the accent fill the accent ring is invisible (1.00:1); the `.btn` rule's
     foreground ring with a gap is what D-186 chose for the same case (D-248). */
  .segbtn.on:focus-visible { outline: 2px solid var(--fg); outline-offset: 2px; }

  /* The select is drawn by its row: no border or surface of its own, the row's colour
     when something is chosen. */
  /* The row's own surface rather than `transparent`: Chromium draws the open list from
     the select's background, and a transparent one fell back to the system's white
     under the theme's light text (D-261). The same colour, so nothing changes shut. */
  .row select { flex: 1 1 auto; min-width: 0; align-self: stretch; padding: 0 2px; border: 0; border-radius: 0 var(--radius) var(--radius) 0; background: var(--bg-row); color: var(--fg); font-size: 12.5px; }
  .row select:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: -2px; }

  .checks { display: grid; grid-template-columns: 1fr 1fr; column-gap: 8px; margin: 5px 0; }
  /* `margin-top: auto` takes whatever height the body has spare, so the foot sits on
     the rail's floor; with none spare it is 0 and the body scrolls as before (D-270). */
  .foot { flex: none; margin-top: auto; display: flex; flex-direction: column; gap: 5px; }
  .check { display: flex; align-items: center; gap: 8px; min-width: 0; min-height: 19px; color: var(--fg-muted); font-size: 12.5px; white-space: nowrap; cursor: pointer; }
  .check.wide { grid-column: 1 / -1; }
  .check:not(.off):hover, .check:has(:checked) { color: var(--fg); }
  .check.off { opacity: 0.45; cursor: default; }
  .check .num { margin-left: auto; color: var(--fg-muted); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  /* Drawn rather than native, in the segments' language: the control border and
     surface, the accent fill when ticked, and the tick in the accent's own text colour
     (`--accent-fg`, which is what makes it readable on every accent in both themes). */
  .check input { appearance: none; flex: none; display: grid; place-content: center; width: 14px; height: 14px; margin: 0; border: 1px solid var(--border-control); border-radius: 4px; background: var(--bg-row); cursor: pointer; transition: background-color 120ms, border-color 120ms; }
  .check input::before { content: ""; width: 8px; height: 8px; background: var(--accent-fg); clip-path: polygon(14% 44%, 0 65%, 50% 100%, 100% 16%, 80% 0, 43% 62%); transform: scale(0); transition: transform 120ms; }
  .check:hover input:not(:disabled) { border-color: var(--accent-ink); }
  .check input:checked { background: var(--accent); border-color: var(--accent); }
  .check input:checked::before { transform: scale(1); }
  .check input:disabled { cursor: default; }
  .check input:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 2px; }
  /* Same case as the ticked segment: an accent ring on the accent fill is invisible (D-248). */
  .check input:checked:focus-visible { outline-color: var(--fg); }
  /* High contrast drops backgrounds, which would take the drawn tick with them. */
  @media (forced-colors: active) {
    .check input { appearance: auto; }
    .check input::before { display: none; }
  }

  /* The search and the pick side by side: a chosen mod's name is cut in the closed
     select, and whole in its list. */
  .modf { display: flex; gap: 5px; }
  .modsearch, .modselect { box-sizing: border-box; flex: 1 1 0; min-width: 0; height: 26px; padding: 0 6px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-size: 12.5px; }
  .modselect { padding: 0 2px; }
  .modsearch:focus-visible, .modselect:focus-visible { outline: 2px solid var(--accent-ink); }
  .modselect.on { border-color: color-mix(in srgb, var(--accent) 65%, var(--border)); }

  .hint { margin: 0; font-size: 11px; line-height: 16px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .link { all: unset; cursor: pointer; color: var(--accent-ink); text-decoration: underline; }
  .link:hover { filter: brightness(1.15); }
  .link:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 2px; }
</style>
