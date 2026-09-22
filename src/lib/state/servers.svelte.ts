// Server browser state (Svelte 5 runes, docs/05 §5). One instance for the app.
// Rows live in a SvelteMap keyed by "ip:queryPort"; batches from the Steam thread
// mutate it in place, and the visible list is derived from filters + sort.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { uiPrefs } from "./uiprefs.svelte";
import {
  isUntrusted,
  trustedPlayers,
  type CachedServers,
  type Favourite,
  type FavouriteAlert,
  type FriendInfo,
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
  /** Only servers a Steam friend is playing on (D-128). */
  friendsOnly: boolean;
};

const FILTERS_KEY = "dayz-launcher.filters.v1";

/**
 * Windows toast for a favourite alert when the launcher is not the focused window
 * (D-086); the in-app toast and taskbar flash cover the focused case. Permission is
 * asked for on the first alert only.
 */
async function notifyIfUnfocused(a: FavouriteAlert) {
  try {
    if (await getCurrentWindow().isFocused()) return;
    if (!(await isPermissionGranted()) && (await requestPermission()) !== "granted") return;
    sendNotification({
      title: a.kind === "slot" ? "Free slot" : "Back online",
      body: `${a.name}: ${a.players}/${a.maxPlayers}`,
    });
  } catch {
    /* notifications unavailable; the in-app toast remains */
  }
}

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
  friendsOnly: false,
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
/** Title-bar friend count poll (D-103); Steam answers from its local cache. */
const FRIENDS_POLL_MS = 60_000;

/** Private (RFC 1918), loopback and link-local IPv4: what Steam's LAN discovery returns (D-087). */
export function isLanIp(ip: string): boolean {
  const p = ip.split(".").map(Number);
  if (p.length !== 4 || p.some((n) => !Number.isInteger(n))) return false;
  const a = p[0] ?? 0;
  const b = p[1] ?? 0;
  return a === 10 || a === 127 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254);
}

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
  /** Favourites the backend watches for a free slot or a return online (D-083). */
  favouriteAlerts = new SvelteSet<string>();
  /** Alerts not yet dismissed, newest last (at most five). */
  alerts = $state<FavouriteAlert[]>([]);
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
  /** Friends in DayZ right now, for the title bar (D-103); polled while the Steam session is active. */
  friendsInDayz = $state<number | null>(null);
  /** Friend names by the id of the server they play on (D-128); from the same poll. */
  friendsOn = new SvelteMap<string, string[]>();

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

  /** Filters away from their defaults, all rows of the bar (D-108). */
  activeFilterCount = $derived.by(() => {
    const f = this.filters;
    return (
      this.moreFilterCount + (f.perspective !== "any" ? 1 : 0) + (f.mods !== "any" ? 1 : 0) + (f.map ? 1 : 0) + (f.country ? 1 : 0)
    );
  });

  /** Filters away from their defaults in the fold-away row only (D-108). */
  moreFilterCount = $derived.by(() => {
    const f = this.filters;
    return (
      [f.notFull, f.notEmpty, f.hasQueue, f.noPassword, f.battleyeOnly, f.dayOnly, f.versionMine].filter(Boolean).length +
      (f.mod ? 1 : 0) +
      (f.maxPing > 0 ? 1 : 0) +
      (f.hideUntrusted ? 0 : 1) +
      (f.friendsOnly ? 1 : 0)
    );
  });

  /**
   * Servers with a trusted head-count above zero (title bar, D-105). Cached rows make
   * it instant at start; it then follows the refresh and verification batches live.
   */
  populatedCount = $derived.by(() => {
    let n = 0;
    for (const r of this.rows.values()) if (!isUntrusted(r) && trustedPlayers(r) > 0) n++;
    return n;
  });

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
      if (f.friendsOnly && !this.friendsOn.has(r.id)) continue;
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

  /** Rows with a local-network address (LAN tab, D-087), search-filtered, busiest first. */
  lanRows = $derived.by(() => {
    const q = this.filters.search.trim().toLowerCase();
    const out: ServerRow[] = [];
    for (const r of this.rows.values()) {
      if (!isLanIp(r.ip)) continue;
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
        if (this.friendsInDayz == null) void this.pollFriends();
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
      await listen<FavouriteAlert>("favourite:alert", (ev) => {
        // One entry per server; the newest replaces an older one for the same server.
        this.alerts = [...this.alerts.filter((a) => a.id !== ev.payload.id), ev.payload].slice(-5);
        void getCurrentWindow()
          .requestUserAttention(UserAttentionType.Informational)
          .catch(() => {});
        void notifyIfUnfocused(ev.payload);
      }),
    );
    this.maybeAutoRefresh();
    void this.pollFriends();
    setInterval(() => void this.pollFriends(), FRIENDS_POLL_MS);
  }

  /**
   * Friends-in-DayZ count for the title bar (D-103). Skipped while the Steam session
   * is released so the poll never wakes it; the backend does not count the read as
   * activity, so the idle release still happens.
   */
  async pollFriends() {
    const s = this.steam;
    if (!s?.initialized || s.idle) return;
    try {
      const list = await invoke<FriendInfo[]>("friends_list");
      this.friendsInDayz = list.filter((f) => f.inDayz).length;
      // Servers with friends (D-128): by ip:queryPort when Steam reports the query
      // port, else by ip + game port against the known rows.
      const on = new Map<string, string[]>();
      for (const f of list) {
        if (!f.server) continue;
        let id: string | null = f.server.queryPort > 0 ? `${f.server.ip}:${f.server.queryPort}` : null;
        if (!id || !this.rows.has(id)) {
          id = null;
          for (const r of this.rows.values()) {
            if (r.ip === f.server.ip && r.gamePort === f.server.gamePort) {
              id = r.id;
              break;
            }
          }
        }
        if (id) on.set(id, [...(on.get(id) ?? []), f.name]);
      }
      for (const key of [...this.friendsOn.keys()]) if (!on.has(key)) this.friendsOn.delete(key);
      for (const [key, names] of on) this.friendsOn.set(key, names);
    } catch {
      /* Steam busy or gone: keep the last value */
    }
  }

  #dzsaTried = false;
  dzsaLoading = $state(false);

  private maybeAutoRefresh() {
    if (this.steam?.initialized && !this.steam.refreshing && !this.#autoRefreshed) {
      this.#autoRefreshed = true;
      void this.refresh(false, false);
    } else if (this.steam && !this.steam.initialized && this.steam.error && !this.#dzsaTried && !this.#autoRefreshed) {
      // Steam failed to initialise: fall back to the DZSA list once (D-089), unless a
      // recent cached list already covers the session.
      this.#dzsaTried = true;
      const fresh = this.lastRefresh != null && Date.now() / 1000 - this.lastRefresh < 600;
      if (!fresh) void this.loadDzsa();
    }
  }

  /** DZSA Launcher's public list: the fallback when Steam is unavailable (D-089). */
  async loadDzsa() {
    if (this.dzsaLoading) return;
    this.error = null;
    this.dzsaLoading = true;
    this.done = null;
    this.verifySummary = null;
    try {
      await invoke<number>("servers_dzsa");
      this.verifying = true;
      void this.loadModsIndex();
    } catch (e) {
      this.error = String(e);
    } finally {
      this.dzsaLoading = false;
    }
  }

  /**
   * Steam's LAN discovery (D-087): servers on the local network, merged into the list.
   * LAN rows carry no Steam head-count, so no verification pass follows; visible rows
   * are checked on demand like any other.
   */
  async refreshLan(): Promise<boolean> {
    this.error = null;
    try {
      return await invoke<boolean>("servers_refresh", { partitions: [{ lan: "1" }], force: true });
    } catch (e) {
      this.error = String(e);
      return false;
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
    this.favouriteAlerts.clear();
    for (const f of list) {
      this.favourites.add(f.id);
      if (f.alert) this.favouriteAlerts.add(f.id);
    }
  }

  /** Watch or stop watching a favourite (D-083). */
  async toggleAlert(id: string) {
    const on = !this.favouriteAlerts.has(id);
    if (on) this.favouriteAlerts.add(id);
    else this.favouriteAlerts.delete(id);
    try {
      await invoke("favourite_alert_set", { id, on });
    } catch (e) {
      this.error = String(e);
      if (on) this.favouriteAlerts.delete(id);
      else this.favouriteAlerts.add(id);
    }
  }

  dismissAlert(at: number) {
    this.alerts = this.alerts.filter((a) => a.at !== at);
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

  /** Empties the join history after the user's confirmation on the Recent view (D-130). */
  async clearHistory() {
    try {
      await invoke("history_clear");
      this.history = [];
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
