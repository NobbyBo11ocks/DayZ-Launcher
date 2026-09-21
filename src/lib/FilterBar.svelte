<script lang="ts">
  // Filter chips and search (docs/06 §4). All state lives in the servers store.
  import { servers, type ModFilter, type Perspective } from "./state/servers.svelte";

  let searchEl = $state<HTMLInputElement | null>(null);
  export function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  const f = $derived(servers.filters);
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
  const activeCount = $derived(
    [f.notFull, f.notEmpty, f.hasQueue, f.noPassword, f.battleyeOnly, f.dayOnly, f.versionMine].filter(Boolean).length +
      (f.perspective !== "any" ? 1 : 0) +
      (f.mods !== "any" ? 1 : 0) +
      (f.map ? 1 : 0) +
      (f.maxPing > 0 ? 1 : 0),
  );
</script>

<div class="filters" role="toolbar" aria-label="Filters">
  <input
    class="search"
    type="search"
    placeholder="Search name, map or IP…  ( / )"
    bind:value={servers.filters.search}
    bind:this={searchEl}
    aria-label="Search servers"
    onkeydown={(e) => {
      if (e.key === "Escape") {
        servers.filters.search = "";
        (e.target as HTMLInputElement).blur();
      }
    }}
  />

  <div class="group" role="group" aria-label="Perspective">
    {#each [["any", "Any"], ["1pp", "1PP"], ["3pp", "3PP"]] as [v, label] (v)}
      <button class="chip" class:on={f.perspective === v} onclick={() => setPerspective(v as Perspective)}>{label}</button>
    {/each}
  </div>

  <select class="select" bind:value={servers.filters.map} onchange={() => servers.saveFilters()} aria-label="Map">
    <option value="">All maps</option>
    {#each servers.maps.slice(0, 40) as [map, n] (map)}
      <option value={map}>{map} ({n})</option>
    {/each}
  </select>

  <div class="group" role="group" aria-label="Mods">
    {#each [["any", "Mods: any"], ["modded", "Modded"], ["vanilla", "Vanilla"]] as [v, label] (v)}
      <button class="chip" class:on={f.mods === v} onclick={() => setMods(v as ModFilter)}>{label}</button>
    {/each}
  </div>

  <button class="chip" class:on={f.notEmpty} onclick={() => toggle("notEmpty")}>Not empty</button>
  <button class="chip" class:on={f.notFull} onclick={() => toggle("notFull")}>Not full</button>
  <button class="chip" class:on={f.hasQueue} onclick={() => toggle("hasQueue")}>Has queue</button>
  <button class="chip" class:on={f.noPassword} onclick={() => toggle("noPassword")}>No password</button>
  <button class="chip" class:on={f.battleyeOnly} onclick={() => toggle("battleyeOnly")}>BattlEye</button>
  <button class="chip" class:on={f.dayOnly} onclick={() => toggle("dayOnly")}>Daytime</button>
  <button class="chip" class:on={f.versionMine} onclick={() => toggle("versionMine")} disabled={!servers.localVersion} title={servers.localVersion ? `Only ${servers.localVersion}` : "DayZ not found"}>
    My version
  </button>

  <label class="ping">
    Ping ≤
    <input type="number" min="0" max="999" step="10" bind:value={servers.filters.maxPing} onchange={() => servers.saveFilters()} aria-label="Maximum ping" />
  </label>

  <button class="chip trust" class:on={f.hideUntrusted} onclick={() => toggle("hideUntrusted")} title="Hide servers whose player counts are inflated or unverifiable (docs/11)">
    Hide inflated ({servers.untrustedCount})
  </button>

  {#if activeCount > 0}
    <button class="chip reset" onclick={() => servers.resetFilters()}>Reset ({activeCount})</button>
  {/if}
</div>

<style>
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding: 8px 0; }
  .search { flex: 0 0 260px; padding: 6px 10px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); }
  .search:focus-visible { outline: 2px solid var(--accent); }
  .group { display: inline-flex; border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .group .chip { border: 0; border-radius: 0; }
  .group .chip + .chip { border-left: 1px solid var(--border); }
  .chip { all: unset; cursor: pointer; padding: 5px 10px; border-radius: var(--radius); border: 1px solid var(--border); color: var(--fg-muted); font-size: 12px; line-height: 1; white-space: nowrap; }
  .chip:hover { color: var(--fg); }
  .chip.on { background: color-mix(in srgb, var(--accent) 22%, var(--bg-row)); color: var(--fg); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); }
  .chip:disabled { opacity: 0.45; cursor: default; }
  .chip:focus-visible { outline: 2px solid var(--accent); }
  .chip.trust.on { border-color: var(--warn); }
  .chip.reset { border-style: dashed; }
  .select { padding: 5px 8px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); max-width: 200px; }
  .ping { display: inline-flex; align-items: center; gap: 4px; color: var(--fg-muted); font-size: 12px; }
  .ping input { width: 60px; padding: 4px 6px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); }
</style>
