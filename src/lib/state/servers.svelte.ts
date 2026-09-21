// Server browser state (Svelte 5 runes, docs/05 §5). One instance for the app.
// Rows live in a SvelteMap keyed by "ip:queryPort"; batches from the Steam thread
// mutate it in place, and the visible list is derived from filters + sort.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { uiPrefs } from "./uiprefs.svelte";
import {
  isUntrusted,
  trustedPlayers,
  type CachedServers,
  type Favourite,
  type HistoryEntry,
  type ImportResult,
  type ModScanSummary,
  type ModsIndex,
  type RefreshDone,
  type ServerMods,
  type ServerRow,
  type SteamStatus,
  type Verification,
  type VerifySummary,
} from "../types";

export type SortKey = "name" | "map" | "players" | "ping" | "time" | "version";
export type Perspective = "any" | "1pp" | "3pp";
export type ModFilter = "any" | "modded" | "vanilla";

export type Filters = {
  search: string;
  perspective: Perspective;
  map: string;
  /** ISO country code, "" = any (D-073). */
  country: string;
  /** Workshop id the server must run, 0 = any (D-080). */
  mod: number;
  notFull: boolean;
  notEmpty: boolean;
  hasQueue: boolean;
  noPassword: boolean;
  battleyeOnly: boolean;
  mods: ModFilter;
  dayOnly: boolean;
  /** 0 = no limit. */
  maxPing: number;
  versionMine: boolean;
  hideUntrusted: boolean;
};

const FILTERS_KEY = "dayz-launcher.filters.v1";

export const defaultFilters = (): Filters => ({
  search: "",
  perspective: "any",
  map: "",
  country: "",
  mod: 0,
  notFull: false,
  notEmpty: false,
  hasQueue: false,
  noPassword: false,
  battleyeOnly: false,
  mods: "any",
  dayOnly: false,
  maxPing: 0,
  versionMine: false,
  hideUntrusted: true,
});

/** localStorage cache for an instant start; the settings file wins once read (D-070). */
function loadFilters(): Filters {
  try {
    const raw = localStorage.getItem(FILTERS_KEY);
    if (raw) return { ...defaultFilters(), ...(JSON.parse(raw) as Partial<Filters>), search: "" };
  } catch {
    /* storage unavailable */
  }
  return defaultFilters();
}

/** Re-verify a visible row when its verification is older than this. */
const STALE_SECS = 120;

class ServersStore {
  rows = new SvelteMap<string, ServerRow>();
  steam = $state<SteamStatus | null>(null);
  done = $state<RefreshDone | null>(null);
  verifySummary = $state<VerifySummary | null>(null);
  filters = $state<Filters>(loadFilters());
  sort = $state<{ key: SortKey; dir: 1 | -1 }>({ key: "players", dir: -1 });
  selectedId = $state<string | null>(null);
  localVersion = $state<string | null>(null);
  error = $state<string | null>(null);
  fromCache = $state(0);
  lastRefresh = $state<number | null>(null);
  verifying = $state(false);
  favourites = new SvelteSet<string>();
  history = $state<HistoryEntry[]>([]);
  /** Server the join dialog is open for. */
  joiningId = $state<string | null>(null);
  /** Mod ids per scanned server and the mod catalogue with server counts (D-080). */
  modsByServer = new SvelteMap<string, number[]>();
  modCatalog = new SvelteMap<number, { name: string; servers: number }>();
  modScan = $state<ModScanSummary | null>(null);
  modScanning = $state(false);
  /** Set by a view that wants the app to switch section (Mods → Servers with a mod filter). */
  navigate = $state<string | null>(null);

  #pending = new SvelteSet<string>();
  #unlisten: UnlistenFn[] = [];
  #started = false;
  #autoRefreshed = false;

  constructor() {
    void uiPrefs.ready.then((u) => {
      if (u.filters) this.filters = { ...defaultFilters(), ...(u.filters as Partial<Filters>), search: this.filters.search };
    });
  }

  /** Distinct maps with counts, most common first. */
  maps = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const r of this.rows.values()) counts.set(r.map, (counts.get(r.map) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  });

  untrustedCount = $derived([...this.rows.values()].filter(isUntrusted).length);

  /** Distinct countries with counts, most common first (rows without a country are skipped). */
  countries = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const r of this.rows.values()) if (r.country) counts.set(r.country, (counts.get(r.country) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  });

  list = $derived.by(() => {
    const f = this.filters;
    const q = f.search.trim().toLowerCase();
    const out: ServerRow[] = [];
    for (const r of this.rows.values()) {
      if (f.hideUntrusted && isUntrusted(r)) continue;
      if (q && !(r.name.toLowerCase().includes(q) || r.map.toLowerCase().includes(q) || r.ip.startsWith(q))) continue;
      if (f.perspective === "1pp" && !r.tags.firstPersonOnly) continue;
      if (f.perspective === "3pp" && r.tags.firstPersonOnly) continue;
      if (f.map && r.map !== f.map) continue;
      if (f.country && r.country !== f.country) continue;
      if (f.mod && !this.modsByServer.get(r.id)?.includes(f.mod)) continue;
      const pop = trustedPlayers(r);
      if (f.notFull && pop >= r.maxPlayers) continue;
      if (f.notEmpty && pop <= 0) continue;
      if (f.hasQueue && !(r.tags.queue && r.tags.queue > 0)) continue;
      if (f.noPassword && r.password) continue;
      if (f.battleyeOnly && !r.tags.battleye) continue;
      if (f.mods === "modded" && !r.tags.modded) continue;
      if (f.mods === "vanilla" && r.tags.modded) continue;
      if (f.dayOnly && !(r.tags.timeMinutes != null && r.tags.timeMinutes >= 6 * 60 && r.tags.timeMinutes < 20 * 60)) continue;
      if (f.maxPing > 0 && r.pingMs > f.maxPing) continue;
      if (f.versionMine && this.localVersion && r.version !== this.localVersion) continue;
      out.push(r);
    }
    const { key, dir } = this.sort;
    // Ping is quantised to 20 ms steps and ties break on the id so that the
    // jitter from re-verification never reorders rows (D-060: reorders exposed
    // fresh rows to verification in a loop that grew CPU and memory).
    const pingBucket = (r: ServerRow) => Math.round(r.pingMs / 20);
    const byId = (a: ServerRow, b: ServerRow) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
    const cmp = (a: ServerRow, b: ServerRow): number => {
      switch (key) {
        case "name":
          return a.name.localeCompare(b.name) || byId(a, b);
        case "map":
          return a.map.localeCompare(b.map) || trustedPlayers(b) - trustedPlayers(a) || byId(a, b);
        case "players":
          return trustedPlayers(a) - trustedPlayers(b) || pingBucket(b) - pingBucket(a) || byId(a, b);
        case "ping":
          return pingBucket(a) - pingBucket(b) || trustedPlayers(b) - trustedPlayers(a) || byId(a, b);
        case "time":
          return (a.tags.timeMinutes ?? -1) - (b.tags.timeMinutes ?? -1) || byId(a, b);
        case "version":
          return a.serverVersion - b.serverVersion || byId(a, b);
      }
    };
    out.sort((a, b) => dir * cmp(a, b));
    return out;
  });

  selected = $derived(this.selectedId ? (this.rows.get(this.selectedId) ?? null) : null);

  /** Catalogue entries, most widely used first. */
  modOptions = $derived(
    [...this.modCatalog.entries()]
      .map(([id, e]) => ({ id, name: e.name, servers: e.servers }))
      .sort((a, b) => b.servers - a.servers || a.name.localeCompare(b.name)),
  );

  /** Populated modded servers whose mod list has not been scanned yet. */
  unscannedModded = $derived([...this.rows.values()].filter((r) => r.tags.modded && trustedPlayers(r) > 0 && !this.modsByServer.has(r.id)).length);

  private async loadModsIndex() {
    try {
      const idx = await invoke<ModsIndex>("mods_index");
      this.modCatalog.clear();
      for (const c of idx.catalog) this.modCatalog.set(c.id, { name: c.name, servers: c.servers });
      this.modsByServer.clear();
      for (const s of idx.index) this.modsByServer.set(s.id, s.mods);
    } catch {
      /* the scan will fill it in */
    }
  }

  /** A batch of freshly scanned servers plus the names of the mods they mention. */
  applyMods(list: ServerMods[], names: [number, string][]) {
    for (const [id, name] of names) if (!this.modCatalog.has(id)) this.modCatalog.set(id, { name, servers: 0 });
    const delta = new Map<number, number>();
    for (const s of list) {
      for (const m of this.modsByServer.get(s.id) ?? []) delta.set(m, (delta.get(m) ?? 0) - 1);
      for (const m of s.mods) delta.set(m, (delta.get(m) ?? 0) + 1);
      this.modsByServer.set(s.id, s.mods);
    }
    for (const [m, d] of delta) {
      const e = this.modCatalog.get(m);
      if (e && d !== 0) this.modCatalog.set(m, { name: e.name, servers: Math.max(0, e.servers + d) });
    }
  }

  async scanMods(force: boolean) {
    try {
      await invoke("mods_scan", { force });
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Favourite rows, search-filtered and sorted like the main list; trust filters do not apply. */
  favouriteRows = $derived.by(() => {
    const q = this.filters.search.trim().toLowerCase();
    const out: ServerRow[] = [];
    for (const id of this.favourites) {
      const r = this.rows.get(id);
      if (!r) continue;
      if (q && !(r.name.toLowerCase().includes(q) || r.map.toLowerCase().includes(q) || r.ip.startsWith(q))) continue;
      out.push(r);
    }
    out.sort((a, b) => trustedPlayers(b) - trustedPlayers(a) || a.pingMs - b.pingMs);
    return out;
  });

  async start() {
    if (this.#started) return;
    this.#started = true;
    try {
      const cached = await invoke<CachedServers>("servers_cached");
      this.fromCache = cached.rows.length;
      this.lastRefresh = cached.lastRefresh;
      for (const r of cached.rows) this.rows.set(r.id, r);
      this.steam = await invoke<SteamStatus>("steam_status");
      this.localVersion = await invoke<string | null>("local_game_version");
      await this.loadFavourites();
      void this.loadModsIndex();
    } catch (e) {
      this.error = String(e);
    }
    this.#unlisten.push(
      await listen<ServerRow[]>("servers:batch", (ev) => {
        for (const r of ev.payload) {
          const prev = this.rows.get(r.id);
          // Keep verification results the Steam batch does not carry.
          this.rows.set(r.id, prev ? { ...r, verifiedPlayers: prev.verifiedPlayers, verifiedAt: prev.verifiedAt, verdict: prev.verdict } : r);
        }
      }),
      await listen<RefreshDone>("servers:done", (ev) => {
        this.done = ev.payload;
        this.lastRefresh = Math.floor(Date.now() / 1000);
      }),
      await listen<SteamStatus>("steam:status", (ev) => {
        this.steam = ev.payload;
        this.maybeAutoRefresh();
      }),
      await listen<Verification[]>("servers:verified", (ev) => this.applyVerifications(ev.payload)),
      await listen<VerifySummary>("servers:verify-done", (ev) => {
        this.verifySummary = ev.payload;
        this.verifying = false;
      }),
      await listen<ModScanSummary>("servers:mods-start", (ev) => {
        this.modScan = ev.payload;
        this.modScanning = ev.payload.total > 0;
      }),
      await listen<[ServerMods[], [number, string][]]>("servers:mods", (ev) => this.applyMods(ev.payload[0], ev.payload[1])),
      await listen<ModScanSummary>("servers:mods-done", (ev) => {
        this.modScan = ev.payload;
        this.modScanning = false;
      }),
    );
    this.maybeAutoRefresh();
  }

  stop() {
    this.#unlisten.forEach((u) => u());
    this.#unlisten = [];
    this.#started = false;
  }

  private maybeAutoRefresh() {
    if (this.steam?.initialized && !this.steam.refreshing && !this.#autoRefreshed) {
      this.#autoRefreshed = true;
      void this.refresh(false, false);
    }
  }

  async refresh(force: boolean, full: boolean) {
    this.error = null;
    try {
      const started = await invoke<boolean>("servers_refresh", { force, full });
      if (started) {
        this.done = null;
        this.verifySummary = null;
        this.verifying = true;
      }
    } catch (e) {
      this.error = String(e);
    }
  }

  applyVerifications(list: Verification[]) {
    for (const v of list) {
      const r = this.rows.get(v.id);
      if (!r) continue;
      this.#pending.delete(v.id);
      this.rows.set(v.id, {
        ...r,
        players: v.reported,
        maxPlayers: v.maxPlayers,
        pingMs: v.pingMs ?? r.pingMs,
        keywords: v.keywords ?? r.keywords,
        tags: v.tags ?? r.tags,
        verifiedPlayers: v.verified,
        verifiedAt: v.verifiedAt,
        verdict: v.verdict,
      });
    }
  }

  /** Called by the table with the ids currently on screen (debounced there). */
  async verifyVisible(ids: string[]) {
    const now = Math.floor(Date.now() / 1000);
    const stale = ids.filter((id) => {
      const r = this.rows.get(id);
      if (!r || this.#pending.has(id)) return false;
      if (r.steamEmpty === true && r.players > 0) return false; // R0 already decided
      return r.verifiedAt == null || now - r.verifiedAt > STALE_SECS;
    });
    if (stale.length === 0) return;
    for (const id of stale) this.#pending.add(id);
    try {
      await invoke<VerifySummary>("servers_verify", { ids: stale.slice(0, 120) });
    } catch (e) {
      this.error = String(e);
    } finally {
      for (const id of stale) this.#pending.delete(id);
    }
  }

  async loadFavourites() {
    const list = await invoke<Favourite[]>("favourites_list");
    this.favourites.clear();
    for (const f of list) this.favourites.add(f.id);
  }

  async toggleFavourite(id: string) {
    const on = !this.favourites.has(id);
    if (on) this.favourites.add(id);
    else this.favourites.delete(id);
    try {
      await invoke("favourite_set", { id, on });
    } catch (e) {
      this.error = String(e);
      if (on) this.favourites.delete(id);
      else this.favourites.add(id);
    }
  }

  async loadHistory() {
    try {
      this.history = await invoke<HistoryEntry[]>("history_list", { limit: 100 });
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Adds a server by address, selects it, and returns it. */
  async directConnect(address: string): Promise<ServerRow | null> {
    this.error = null;
    try {
      const row = await invoke<ServerRow>("direct_connect", { address });
      this.rows.set(row.id, row);
      this.selectedId = row.id;
      return row;
    } catch (e) {
      this.error = String(e);
      return null;
    }
  }

  async importOfficial(): Promise<ImportResult | null> {
    this.error = null;
    try {
      const r = await invoke<ImportResult>("import_official_favourites");
      await this.loadFavourites();
      return r;
    } catch (e) {
      this.error = String(e);
      return null;
    }
  }

  setSort(key: SortKey) {
    if (this.sort.key === key) this.sort = { key, dir: this.sort.dir === 1 ? -1 : 1 };
    else this.sort = { key, dir: key === "players" ? -1 : 1 };
  }

  select(id: string | null) {
    this.selectedId = id;
  }

  /** Moves the selection within the current list; returns the new id. */
  step(delta: number): string | null {
    const list = this.list;
    if (list.length === 0) return null;
    const idx = this.selectedId ? list.findIndex((r) => r.id === this.selectedId) : -1;
    const next = Math.min(list.length - 1, Math.max(0, idx + delta));
    this.selectedId = list[next]?.id ?? null;
    return this.selectedId;
  }

  saveFilters() {
    const { search: _s, ...rest } = this.filters;
    uiPrefs.patch({ filters: rest });
    try {
      localStorage.setItem(FILTERS_KEY, JSON.stringify(rest));
    } catch {
      /* ignore */
    }
  }

  resetFilters() {
    this.filters = defaultFilters();
    this.saveFilters();
  }
}

export const servers = new ServersStore();
