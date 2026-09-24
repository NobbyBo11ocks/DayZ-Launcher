<script lang="ts">
  // Filter controls (docs/06 §4, D-108). All state lives in the servers store. Two
  // parts share this component: `primary` (search, perspective, map, country, mods;
  // 28 px controls) for the first row of the Servers page and `chips` (quick toggles,
  // ping, trust, specific mod, reset; 26 px) for the second.
  import { untrack } from "svelte";
  import { servers, type HiveFilter, type ModFilter, type Perspective, type StyleFilter } from "./state/servers.svelte";
  import { countryName } from "./types";
  import { mapLabel } from "./maps";

  let { part = "primary" }: { part?: "primary" | "chips" } = $props();

  let searchEl = $state<HTMLInputElement | null>(null);
  export function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  // Debounced search (D-152). `typed` follows the keyboard; the store follows `typed`
  // after a pause, or at once when the field is cleared.
  const SEARCH_DELAY_MS = 180;
  let typed = $state(servers.filters.search);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  function setSearch(value: string, now = false) {
    typed = value;
    clearTimeout(searchTimer);
    if (now || value === "") {
      servers.filters.search = value;
      servers.saveFilters();
      return;
    }
    searchTimer = setTimeout(() => {
      servers.filters.search = value;
      servers.saveFilters();
    }, SEARCH_DELAY_MS);
  }
  // A reset elsewhere (the Reset chip) must show up in the field AND cancel a pending
  // write: without the cancel, a timer armed moments earlier wrote the old term back
  // into the freshly reset filters, leaving the list filtered by an invisible term
  // (D-159).
  //
  // Only the store is a dependency. Reading `typed` inside made it one too, so every
  // keystroke re-ran this while the store’s search was still empty (it is written on
  // a 180 ms delay), which cancelled that write and blanked the field: focus worked,
  // characters never appeared. Reproduced live on 0.1.39 (D-230).
  $effect(() => {
    const stored = servers.filters.search;
    untrack(() => {
      if (stored === "" && typed !== "") {
        clearTimeout(searchTimer);
        typed = "";
      }
    });
  });
  $effect(() => () => clearTimeout(searchTimer));

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

{#if part === "primary"}
  <div class="filters" role="toolbar" aria-label="Filters">
    <label class="searchwrap">
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" /></svg>
      <!-- Typed value is local and the store is written on a short delay (D-152): every
           write re-filters and re-sorts up to 20 000 rows, so binding straight to the
           store made each keystroke pay for a full pass. -->
      <input
        class="search"
        type="search"
        placeholder="Search name, map or IP"
        value={typed}
        bind:this={searchEl}
        aria-label="Search servers (press / to focus)"
        spellcheck="false"
        oninput={(e) => setSearch((e.currentTarget as HTMLInputElement).value)}
        onkeydown={(e) => {
          if (e.key === "Escape") {
            setSearch("", true);
            (e.target as HTMLInputElement).blur();
          }
        }}
      />
      <kbd aria-hidden="true">/</kbd>
    </label>

    <div class="seg" role="group" aria-label="Perspective">
      {#each [["any", "Any"], ["1pp", "1PP"], ["3pp", "3PP"]] as [v, label] (v)}
        <button class="segbtn" class:on={f.perspective === v} aria-pressed={f.perspective === v} onclick={() => setPerspective(v as Perspective)}>{label}</button>
      {/each}
    </div>

    <!-- Official is Bohemia's public hive, where your character follows you between
         servers; Community is a private shard, where it does not. The in-game browser
         makes this a top-level tab, and the tag is already on every row (D-195). -->
    <!-- What the server says it is, read out of its own name and description. Nobody
         verifies a ruleset, so the labels and the tooltips say "says" — and a server
         that claims both shows under both. 41.6 % of the list says PVE (D-211). -->
    <div class="seg" role="group" aria-label="Playstyle">
      {#each [["any", "Any", "Every playstyle"], ["pve", "PVE", "The name or description says PVE"], ["pvp", "PVP", "The name or description says PVP"], ["rp", "RP", "The name or description says RP or roleplay"]] as [v, label, hint] (v)}
        <button class="segbtn" class:on={f.style === v} aria-pressed={f.style === v} title={hint} onclick={() => setStyle(v as StyleFilter)}>{label}</button>
      {/each}
    </div>

    <div class="seg" role="group" aria-label="Hive">
      {#each [["any", "Any", "Both hives"], ["official", "Official", "Bohemia's public hive: your character follows you between these"], ["community", "Community", "Private shards: your character lives on that one server"]] as [v, label, hint] (v)}
        <button class="segbtn" class:on={f.hive === v} aria-pressed={f.hive === v} title={hint} onclick={() => setHive(v as HiveFilter)}>{label}</button>
      {/each}
    </div>

    <select class="select" bind:value={servers.filters.map} onchange={() => servers.saveFilters()} aria-label="Map">
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

    <select class="select" bind:value={servers.filters.country} onchange={() => servers.saveFilters()} aria-label="Country">
      <option value="">All countries</option>
      {#each shownCountries as [cc, n] (cc)}
        <option value={cc}>{countryName(cc)} ({n})</option>
      {/each}
      {#if f.country && !shownCountries.some(([cc]) => cc === f.country)}
        <option value={f.country}>{countryName(f.country)}</option>
      {/if}
    </select>

    <div class="seg" role="group" aria-label="Mods">
      {#each [["any", "Any mods"], ["modded", "Modded"], ["vanilla", "Vanilla"]] as [v, label] (v)}
        <button class="segbtn" class:on={f.mods === v} aria-pressed={f.mods === v} onclick={() => setMods(v as ModFilter)}>{label}</button>
      {/each}
    </div>
  </div>
{:else}
  <div class="filters chips" role="toolbar" aria-label="Quick filters">
    <button class="chip" class:on={f.notEmpty} aria-pressed={f.notEmpty} onclick={() => toggle("notEmpty")}>Not empty</button>
    <button class="chip" class:on={f.notFull} aria-pressed={f.notFull} onclick={() => toggle("notFull")}>Not full</button>
    <button class="chip" class:on={f.hasQueue} aria-pressed={f.hasQueue} onclick={() => toggle("hasQueue")}>Has queue</button>
    <button class="chip" class:on={f.noPassword} aria-pressed={f.noPassword} onclick={() => toggle("noPassword")}>No password</button>
    <button class="chip" class:on={f.dayOnly} aria-pressed={f.dayOnly} onclick={() => toggle("dayOnly")}>Daytime</button>
    <button class="chip" class:on={f.versionMine} aria-pressed={f.versionMine} onclick={() => toggle("versionMine")} disabled={!servers.localVersion} title={servers.localVersion ? `Only ${servers.localVersion}` : "DayZ not found"}>
      My version
    </button>
    <button class="chip" class:on={f.friendsOnly} aria-pressed={f.friendsOnly} onclick={() => toggle("friendsOnly")} title="Only servers a Steam friend is playing on right now">
      Friends <span class="num">{servers.friendsOn.size}</span>
    </button>

    <!-- Presets rather than a number field: this was the one filter you had to click
         into and type a number for, and an emptied number input binds as null (D-211).
         A value saved before this, or set some other way, keeps its own option. -->
    <label class="ping" title="Hide servers slower than this">
      Ping ≤
      <select class="select small" class:on={f.maxPing > 0} bind:value={servers.filters.maxPing} onchange={() => servers.saveFilters()} aria-label="Maximum ping">
        <option value={0}>Any</option>
        {#each PING_PRESETS as ms (ms)}
          <option value={ms}>{ms} ms</option>
        {/each}
        {#if f.maxPing > 0 && !PING_PRESETS.includes(f.maxPing)}
          <option value={f.maxPing}>{f.maxPing} ms</option>
        {/if}
      </select>
    </label>

    <button class="chip" class:on={f.hideUntrusted} aria-pressed={f.hideUntrusted} onclick={() => toggle("hideUntrusted")} title="Hide servers whose player counts are inflated or unverifiable (docs/11)">
      Hide inflated <span class="num">{fmt.format(servers.untrustedCount)}</span>
    </button>

    {#if servers.modCatalog.size > 0 || f.mod}
      <span class="modf" role="group" aria-label="Running a specific mod">
        <input class="modsearch" type="search" placeholder="Find mod" bind:value={modQuery} aria-label="Search the mod list" spellcheck="false" />
        <select class="select small" class:on={f.mod !== 0} bind:value={servers.filters.mod} onchange={() => servers.saveFilters()} aria-label="Servers running this mod" title="Servers whose mod list includes this Workshop item">
          <option value={0}>Any mod</option>
          {#each modOptions as m (m.id)}
            <option value={m.id}>{m.name} ({m.servers})</option>
          {/each}
        </select>
      </span>
    {/if}

    {#if servers.activeFilterCount > 0}
      <button class="chip reset" onclick={() => servers.resetFilters()} title="Back to the default filters">Reset · {servers.activeFilterCount}</button>
    {/if}
  </div>
{/if}

<style>
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; }
  .filters.chips { gap: 6px; }

  .searchwrap { position: relative; display: inline-flex; align-items: center; flex: 1 1 180px; min-width: 150px; max-width: 300px; }
  .searchwrap .icon { position: absolute; left: 9px; width: 14px; height: 14px; fill: none; stroke: var(--fg-muted); stroke-width: 1.5; stroke-linecap: round; pointer-events: none; }
  .search { width: 100%; box-sizing: border-box; height: 28px; padding: 0 28px 0 28px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-size: 12.5px; }
  .search:focus-visible { outline: 2px solid var(--accent-ink); }
  .searchwrap kbd { position: absolute; right: 7px; padding: 0 5px; border: 1px solid var(--border); border-radius: 4px; font-size: 10.5px; line-height: 15px; color: var(--fg-muted); background: var(--bg-elev); pointer-events: none; }

  .seg { display: inline-flex; box-sizing: border-box; height: 28px; padding: 2px; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border-control); flex: none; }
  .segbtn { all: unset; cursor: pointer; display: inline-flex; align-items: center; padding: 0 9px; border-radius: 6px; color: var(--fg-muted); font-size: 12px; font-weight: 500; white-space: nowrap; }
  .segbtn:hover { color: var(--fg); }
  .segbtn.on { background: var(--accent); color: var(--accent-fg); font-weight: 600; }
  .segbtn:focus-visible { outline: 2px solid var(--accent-ink); }

  .select { box-sizing: border-box; height: 28px; padding: 0 6px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-size: 12.5px; max-width: 150px; }
  .select:focus-visible { outline: 2px solid var(--accent-ink); }
  .select.small { height: 26px; font-size: 11.5px; max-width: 150px; }
  .select.on { border-color: color-mix(in srgb, var(--accent) 65%, var(--border)); }

  .chip { all: unset; cursor: pointer; box-sizing: border-box; height: 26px; padding: 0 9px; display: inline-flex; align-items: center; gap: 6px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg-muted); font-size: 11.5px; line-height: 1; white-space: nowrap; transition: border-color 120ms, color 120ms, background-color 120ms; }
  .chip:hover { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 25%, var(--border)); }
  .chip.on { background: color-mix(in srgb, var(--accent) 20%, var(--bg-row)); color: var(--fg); border-color: color-mix(in srgb, var(--accent) 65%, var(--border)); }
  .chip.on::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .chip:disabled { opacity: 0.45; cursor: default; }
  .chip:focus-visible { outline: 2px solid var(--accent-ink); }
  /* On the accent fill the muted number measured 3.22:1 (lime, dark) and 3.82
     (rose, light); Mods.svelte already does exactly this for its own (D-197). */
  .chip.on .num { color: var(--fg); }
  .chip .num { color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .chip.reset { border-style: dashed; background: transparent; }

  .modf { display: inline-flex; gap: 4px; }
  .modsearch { box-sizing: border-box; width: 96px; height: 26px; padding: 0 8px; border-radius: var(--radius); border: 1px solid var(--border-control); background: var(--bg-row); color: var(--fg); font-size: 11.5px; }
  .modsearch:focus-visible { outline: 2px solid var(--accent-ink); }

  .ping { display: inline-flex; align-items: center; gap: 6px; box-sizing: border-box; height: 26px; padding: 0 0 0 9px; border: 1px solid var(--border-control); border-radius: var(--radius); background: var(--bg-row); color: var(--fg-muted); font-size: 11.5px; white-space: nowrap; }
  .ping :global(select) { height: 24px; border: 0; border-left: 1px solid var(--border-control); border-radius: 0 7px 7px 0; }
</style>
