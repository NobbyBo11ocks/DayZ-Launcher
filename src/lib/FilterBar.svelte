<script lang="ts">
  // Filter controls (docs/06 §4, D-108). All state lives in the servers store. Two
  // parts share this component: `primary` (search, perspective, map, country, mods;
  // 28 px controls) for the first row of the Servers page and `chips` (quick toggles,
  // ping, trust, specific mod, reset; 26 px) for the second.
  import { servers, type ModFilter, type Perspective } from "./state/servers.svelte";
  import { countryName } from "./types";

  let { part = "primary" }: { part?: "primary" | "chips" } = $props();

  let searchEl = $state<HTMLInputElement | null>(null);
  export function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  const f = $derived(servers.filters);
  const fmt = new Intl.NumberFormat();
  const toggle = (key: "notFull" | "notEmpty" | "hasQueue" | "noPassword" | "battleyeOnly" | "dayOnly" | "versionMine" | "hideUntrusted") => {
    servers.filters[key] = !servers.filters[key];
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
      <input
        class="search"
        type="search"
        placeholder="Search name, map or IP"
        bind:value={servers.filters.search}
        bind:this={searchEl}
        aria-label="Search servers (press / to focus)"
        spellcheck="false"
        onkeydown={(e) => {
          if (e.key === "Escape") {
            servers.filters.search = "";
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

    <select class="select" bind:value={servers.filters.map} onchange={() => servers.saveFilters()} aria-label="Map">
      <option value="">All maps</option>
      {#each servers.maps.slice(0, 40) as [map, n] (map)}
        <option value={map}>{map} ({n})</option>
      {/each}
    </select>

    <select class="select" bind:value={servers.filters.country} onchange={() => servers.saveFilters()} aria-label="Country">
      <option value="">All countries</option>
      {#each servers.countries.slice(0, 60) as [cc, n] (cc)}
        <option value={cc}>{countryName(cc)} ({n})</option>
      {/each}
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
    <button class="chip" class:on={f.battleyeOnly} aria-pressed={f.battleyeOnly} onclick={() => toggle("battleyeOnly")}>BattlEye</button>
    <button class="chip" class:on={f.dayOnly} aria-pressed={f.dayOnly} onclick={() => toggle("dayOnly")}>Daytime</button>
    <button class="chip" class:on={f.versionMine} aria-pressed={f.versionMine} onclick={() => toggle("versionMine")} disabled={!servers.localVersion} title={servers.localVersion ? `Only ${servers.localVersion}` : "DayZ not found"}>
      My version
    </button>

    <label class="ping" title="0 = no limit">
      Ping ≤
      <input type="number" min="0" max="999" step="10" bind:value={servers.filters.maxPing} onchange={() => servers.saveFilters()} aria-label="Maximum ping" />
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
  .search { width: 100%; box-sizing: border-box; height: 28px; padding: 0 28px 0 28px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); font-size: 12.5px; }
  .search:focus-visible { outline: 2px solid var(--accent); }
  .searchwrap kbd { position: absolute; right: 7px; padding: 0 5px; border: 1px solid var(--border); border-radius: 4px; font-size: 10.5px; line-height: 15px; color: var(--fg-muted); background: var(--bg-elev); pointer-events: none; }

  .seg { display: inline-flex; box-sizing: border-box; height: 28px; padding: 2px; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border); flex: none; }
  .segbtn { all: unset; cursor: pointer; display: inline-flex; align-items: center; padding: 0 9px; border-radius: 6px; color: var(--fg-muted); font-size: 12px; font-weight: 500; white-space: nowrap; }
  .segbtn:hover { color: var(--fg); }
  .segbtn.on { background: var(--accent); color: #111; font-weight: 600; }
  .segbtn:focus-visible { outline: 2px solid var(--accent); }

  .select { box-sizing: border-box; height: 28px; padding: 0 6px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); font-size: 12.5px; max-width: 150px; }
  .select:focus-visible { outline: 2px solid var(--accent); }
  .select.small { height: 26px; font-size: 11.5px; max-width: 150px; }
  .select.on { border-color: color-mix(in srgb, var(--accent) 65%, var(--border)); }

  .chip { all: unset; cursor: pointer; box-sizing: border-box; height: 26px; padding: 0 9px; display: inline-flex; align-items: center; gap: 6px; border-radius: 7px; border: 1px solid var(--border); background: var(--bg-row); color: var(--fg-muted); font-size: 11.5px; line-height: 1; white-space: nowrap; transition: border-color 120ms, color 120ms, background-color 120ms; }
  .chip:hover { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 25%, var(--border)); }
  .chip.on { background: color-mix(in srgb, var(--accent) 20%, var(--bg-row)); color: var(--fg); border-color: color-mix(in srgb, var(--accent) 65%, var(--border)); }
  .chip.on::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .chip:disabled { opacity: 0.45; cursor: default; }
  .chip:focus-visible { outline: 2px solid var(--accent); }
  .chip .num { color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .chip.reset { border-style: dashed; background: transparent; }

  .modf { display: inline-flex; gap: 4px; }
  .modsearch { box-sizing: border-box; width: 96px; height: 26px; padding: 0 8px; border-radius: 7px; border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); font-size: 11.5px; }
  .modsearch:focus-visible { outline: 2px solid var(--accent); }

  .ping { display: inline-flex; align-items: center; gap: 6px; box-sizing: border-box; height: 26px; padding: 0 0 0 9px; border: 1px solid var(--border); border-radius: 7px; background: var(--bg-row); color: var(--fg-muted); font-size: 11.5px; white-space: nowrap; }
  .ping input { width: 50px; height: 24px; padding: 0 6px; border: 0; border-left: 1px solid var(--border); border-radius: 0 7px 7px 0; background: transparent; color: var(--fg); font-size: 11.5px; }
  .ping input:focus-visible { outline: 2px solid var(--accent); }
</style>
