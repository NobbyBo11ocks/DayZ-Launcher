// Server browser state (Svelte 5 runes, docs/05 §5). One instance for the app.
// Rows live in a plain Map keyed by "ip:queryPort" with one version source (D-176);
// batches from the Steam thread mutate it in place and bump the version, and the
// visible list is derived from filters + sort.
// Every command through the logging wrapper: a failure is recorded with its
// command name before it is rethrown (D-158).
import { invokeLogged as invoke } from "../log";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { uiPrefs } from "./uiprefs.svelte";
import { describe, logInfo, logWarn } from "../log";
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

export type SortKey = "name" | "map" | "mods" | "players" | "ping" | "time" | "version";
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

/** How long incoming rows are pooled before one merge into `rows` (D-160). */
const ROW_FLUSH_MS = 350;

/**
 * A pass that never reports back leaves its spinner on screen for ever, because the
 * matching `done` event is the only thing that clears it. These are deliberately far
 * above the measured times — a full verification of 2 600 servers took 39 s and a mod
 * scan of 2 400 a few minutes — so a slow machine is never cut short (D-160).
 */
const VERIFY_DEADLINE_MS = 5 * 60_000;
const SCAN_DEADLINE_MS = 20 * 60_000;
const WATCHDOG_MS = 30_000;

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
/** One collator for the whole session: `localeCompare` builds one per call (D-152). */
const COLLATOR = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Private (RFC 1918), loopback and link-local IPv4: what Steam's LAN discovery returns (D-087). */
export function isLanIp(ip: string): boolean {
  // Called once per row while the LAN tab is open. `split(".").map(Number)` allocated
  // two arrays and a closure per address — 6.97 ms against 1.07 ms over 19 000 (D-176).
  const dot = ip.indexOf(".");
  if (dot < 1) return false;
  const a = +ip.slice(0, dot);
  if (a === 10 || a === 127) return true;
  if (a !== 172 && a !== 192 && a !== 169) return false;
  const dot2 = ip.indexOf(".", dot + 1);
  const b = +ip.slice(dot + 1, dot2 < 0 ? undefined : dot2);
  return (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254);
}

class ServersStore {
  /**
   * Every cached server by `ip:queryPort`. A plain `Map` with one version source
   * rather than a `SvelteMap`: see D-176 — every consumer reads the whole map, so
   * per-key sources were pure overhead. Anything reactive that reads it must read
   * `this.#rowsVersion` first; `rowsChanged()` is the only writer.
   */
  rows = new Map<string, ServerRow>();
  #rowsVersion = $state(0);
  /**
   * Position of each row in collator order by name. Sorting the visible list by name
   * costs 38.6 ms over 19 000 rows because `Intl.Collator.compare` is expensive and
   * runs n log n times; the names only change when rows are added, removed or
   * renamed, which verification never does. One collator sort here turns every later
   * comparison into integer subtraction (~3.6 ms), and the tie-break on id is baked
   * into the ranks (D-181).
   */
  #nameRank = new Map<string, number>();
  #namesDirty = true;
  #rankByName(): Map<string, number> {
    if (!this.#namesDirty) return this.#nameRank;
    const sorted = [...this.rows.values()].sort((a, b) => COLLATOR.compare(a.name, b.name) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    const rank = new Map<string, number>();
    for (let i = 0; i < sorted.length; i++) rank.set(sorted[i]!.id, i);
    this.#nameRank = rank;
    this.#namesDirty = false;
    return rank;
  }
  /** Call after any batch of writes to `rows`. */
  rowsChanged() {
    this.#rowsVersion++;
  }
  /** Reads the version so a derived or template re-runs when the map changes. */
  get rowsTick(): number {
    return this.#rowsVersion;
  }
  /**
   * Rows waiting to be merged into `rows`. Steam delivers a batch every 100 ms and
   * every write invalidates the whole derived chain — filter, sort, maps, countries,
   * counts — which measures ~13 ms at 19 000 rows, so applying each batch on arrival
   * spent ~130 ms of every second on the main thread for the length of a refresh and
   * dropped frames while scrolling (D-160). Folding them into one flush costs at most
   * `ROW_FLUSH_MS` of freshness on a list that already takes ~40 s to arrive.
   */
  #inbox: ServerRow[] = [];
  #flushTimer: ReturnType<typeof setTimeout> | undefined;
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
  #verifyingSince = 0;
  #scanningSince = 0;
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

  #pending = new Set<string>();
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
    void this.#rowsVersion;
    const counts = new Map<string, number>();
    for (const r of this.rows.values()) counts.set(r.map, (counts.get(r.map) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  });

  untrustedCount = $derived.by(() => {
    void this.#rowsVersion;
    let n = 0;
    for (const r of this.rows.values()) if (isUntrusted(r)) n++;
    return n;
  });

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
    void this.#rowsVersion;
    let n = 0;
    for (const r of this.rows.values()) if (!isUntrusted(r) && trustedPlayers(r) > 0) n++;
    return n;
  });

  /** Distinct countries with counts, most common first (rows without a country are skipped). */
  countries = $derived.by(() => {
    void this.#rowsVersion;
    const counts = new Map<string, number>();
    for (const r of this.rows.values()) if (r.country) counts.set(r.country, (counts.get(r.country) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  });

  list = $derived.by(() => {
    void this.#rowsVersion;
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
    // Mod counts once per sort rather than once per comparison (D-152): the map is
    // reactive, so a lookup inside the comparator was ~n log n signal reads.
    const modCounts =
      key === "mods" ? new Map(out.map((r) => [r.id, this.modsByServer.get(r.id)?.length ?? -1])) : null;
    const nameRank = key === "name" ? this.#rankByName() : null;
    const cmp = (a: ServerRow, b: ServerRow): number => {
      switch (key) {
        case "name":
          // Precomputed ranks (D-181); the id tie-break is already in them.
          return (nameRank?.get(a.id) ?? 0) - (nameRank?.get(b.id) ?? 0);
        case "map":
          return COLLATOR.compare(a.map, b.map) || trustedPlayers(b) - trustedPlayers(a) || byId(a, b);
        case "mods": {
          // Unscanned servers sort below every scanned one, in both directions.
          const ma = modCounts?.get(a.id) ?? -1;
          const mb = modCounts?.get(b.id) ?? -1;
          return ma - mb || trustedPlayers(b) - trustedPlayers(a) || byId(a, b);
        }
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

  selected = $derived.by(() => {
    void this.#rowsVersion;
    return this.selectedId ? (this.rows.get(this.selectedId) ?? null) : null;
  });

  /** Catalogue entries, most widely used first. */
  modOptions = $derived(
    [...this.modCatalog.entries()]
      .map(([id, e]) => ({ id, name: e.name, servers: e.servers }))
      .sort((a, b) => b.servers - a.servers || a.name.localeCompare(b.name)),
  );

  /** Populated modded servers whose mod list has not been scanned yet. */
  unscannedModded = $derived.by(() => {
    void this.#rowsVersion;
    let n = 0;
    for (const r of this.rows.values()) {
      if (r.tags.modded && trustedPlayers(r) > 0 && !this.modsByServer.has(r.id)) n++;
    }
    return n;
  });

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
    void this.#rowsVersion;
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
    void this.#rowsVersion;
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
      this.#namesDirty = true;
      this.rowsChanged();
      this.steam = await invoke<SteamStatus>("steam_status");
      this.localVersion = await invoke<string | null>("local_game_version");
      await this.loadFavourites();
      void this.loadModsIndex();
    } catch (e) {
      this.error = String(e);
    }
    this.#unlisten.push(
      await listen<ServerRow[]>("servers:batch", (ev) => {
        for (const r of ev.payload) this.#inbox.push(r);
        if (this.#flushTimer === undefined) {
          this.#flushTimer = setTimeout(() => this.flushRows(), ROW_FLUSH_MS);
        }
      }),
      await listen<RefreshDone>("servers:done", (ev) => {
        // A rejected refresh never reached Steam (D-160). It is sent so the UI stops
        // waiting, not as a result: keeping it would replace a real summary with
        // "0 of 0 shown · 0 from Steam in 0 s" and read as a success.
        this.flushRows();
        if (ev.payload.rejected) {
          this.verifying = false;
          this.#verifyingSince = 0;
          this.error = "Steam did not answer the refresh, so the list was not updated.";
          return;
        }
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
        this.#verifyingSince = 0;
      }),
      await listen<ModScanSummary>("servers:mods-start", (ev) => {
        this.modScan = ev.payload;
        this.modScanning = ev.payload.total > 0;
        this.#scanningSince = Date.now();
      }),
      await listen<[ServerMods[], [number, string][]]>("servers:mods", (ev) => this.applyMods(ev.payload[0], ev.payload[1])),
      await listen<ModScanSummary>("servers:mods-done", (ev) => {
        this.modScan = ev.payload;
        this.modScanning = false;
        this.#scanningSince = 0;
      }),
      await listen<FavouriteAlert>("favourite:alert", (ev) => {
        logInfo("alert", `${ev.payload.name}: ${ev.payload.kind === "slot" ? "a slot freed up" : "back online"} (${ev.payload.players}/${ev.payload.maxPlayers})`);
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
    setInterval(() => this.#watchdog(), WATCHDOG_MS);
  }

  /**
   * Clears a pass whose `done` event never arrived. Without this the status line
   * read "verifying player counts…" or "scanning mod lists…" until the app was
   * restarted, and the user had no way to tell a slow pass from a dead one (D-160).
   */
  #watchdog() {
    const now = Date.now();
    if (this.verifying && this.#verifyingSince && now - this.#verifyingSince > VERIFY_DEADLINE_MS) {
      this.verifying = false;
      this.#verifyingSince = 0;
      this.error = "Player-count verification stopped answering. Refresh to try again.";
    }
    if (this.modScanning && this.#scanningSince && now - this.#scanningSince > SCAN_DEADLINE_MS) {
      this.modScanning = false;
      this.#scanningSince = 0;
      logWarn("mods", "no mods-done inside the deadline; the flag was cleared");
    }
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
      // One pass over the rows for the whole list, not one per friend whose query
      // port Steam did not report (D-160).
      let byGamePort: Map<string, string> | null = null;
      const needsLookup = list.some((f) => f.server && !(f.server.queryPort > 0 && this.rows.has(`${f.server.ip}:${f.server.queryPort}`)));
      if (needsLookup) {
        byGamePort = new Map();
        for (const r of this.rows.values()) byGamePort.set(`${r.ip}:${r.gamePort}`, r.id);
      }
      for (const f of list) {
        if (!f.server) continue;
        const direct = f.server.queryPort > 0 ? `${f.server.ip}:${f.server.queryPort}` : null;
        const id = direct && this.rows.has(direct) ? direct : (byGamePort?.get(`${f.server.ip}:${f.server.gamePort}`) ?? null);
        if (id) on.set(id, [...(on.get(id) ?? []), f.name]);
      }
      for (const key of [...this.friendsOn.keys()]) if (!on.has(key)) this.friendsOn.delete(key);
      for (const [key, names] of on) this.friendsOn.set(key, names);
    } catch (e) {
      // Keep the last value on screen, but a friends list that keeps failing is the
      // first visible sign that the Steam session has gone (D-160).
      logWarn("friends", `poll failed: ${describe(e)}`);
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
      this.#verifyingSince = Date.now();
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
        this.#verifyingSince = Date.now();
      }
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Merges everything the last batches delivered. Idempotent and cheap when empty. */
  flushRows() {
    if (this.#flushTimer !== undefined) {
      clearTimeout(this.#flushTimer);
      this.#flushTimer = undefined;
    }
    if (this.#inbox.length === 0) return;
    const batch = this.#inbox;
    this.#inbox = [];
    for (const r of batch) {
      const prev = this.rows.get(r.id);
      // A new row, or one that renamed itself, invalidates the name ranks; a changed
      // player count does not (D-181).
      if (!prev || prev.name !== r.name) this.#namesDirty = true;
      // Keep verification results the Steam batch does not carry.
      this.rows.set(r.id, prev ? { ...r, verifiedPlayers: prev.verifiedPlayers, verifiedAt: prev.verifiedAt, verdict: prev.verdict } : r);
    }
    this.rowsChanged();
  }

  applyVerifications(list: Verification[]) {
    // A verification can land between a batch and its flush; without this the row
    // it refers to would not be in `rows` yet and the result would be dropped.
    this.flushRows();
    for (const v of list) {
      const r = this.rows.get(v.id);
      if (!r) continue;
      this.#pending.delete(v.id);
      this.rows.set(v.id, {
        ...r,
        players: v.reported,
        maxPlayers: v.maxPlayers,
        pingMs: v.pingMs ?? r.pingMs,
        tags: v.tags ?? r.tags,
        // Keep the last real count when this check could not produce one (D-160),
        // matching what the cache now stores.
        verifiedPlayers: v.verified ?? r.verifiedPlayers,
        verifiedAt: v.verified == null ? r.verifiedAt : v.verifiedAt,
        verdict: v.verdict,
      });
    }
    this.rowsChanged();
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
      this.#namesDirty = true;
      this.rowsChanged();
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
    else this.sort = { key, dir: key === "players" || key === "mods" ? -1 : 1 };
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
