// Server browser state (Svelte 5 runes, docs/05 §5). One instance for the app.
// Rows live in a plain Map keyed by "ip:queryPort" with one version source (D-176);
// batches from the Steam thread mutate it in place and bump the version, and the
// visible list is derived from filters + sort.
// Every command through the logging wrapper: a failure is recorded with its
// command name before it is rethrown (D-158).
import { invokeLogged as invoke } from "../log";
import { mapHaystack, mapLabel } from "../maps";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { uiPrefs } from "./uiprefs.svelte";
import { describe, logWarn } from "../log";
import { type CachedServers, type Favourite, type FriendInfo, type HistoryEntry, type ImportResult, isUntrusted, type ModScanSummary, type ModsIndex, queueOf, type RefreshDone, type ServerMods, type ServerRow, type SteamStatus, trustedPlayers, type Verification, type VerifySummary } from "../types";

export type SortKey = "name" | "map" | "mods" | "players" | "ping" | "time" | "version";
export type Perspective = "any" | "1pp" | "3pp";
export type ModFilter = "any" | "modded" | "vanilla";
/** Official = Bohemia's public hive, your character follows you between them.
 *  Community = a private shard, your character lives on that one box (D-195). */
export type HiveFilter = "any" | "official" | "community";
/** A claim the server makes about itself, not a ruleset anyone verified (D-211). */
export type StyleFilter = "any" | "pve" | "pvp" | "rp";

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
  mods: ModFilter;
  hive: HiveFilter;
  /** What the server says it is, in its own name or description (D-211). */
  style: StyleFilter;
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
/** The worker's `MIN_REFRESH_INTERVAL` (sdk.rs): an automatic refresh inside it is declined. */
const AUTO_REFRESH_GAP_SECS = 60;
const VERIFY_STALLED = "Player-count verification stopped answering. Refresh to try again.";
const SCAN_DEADLINE_MS = 20 * 60_000;
const WATCHDOG_MS = 30_000;



/**
 * What a server calls itself, lower-cased once. Both the search and the playstyle
 * filter read it, so the per-row `toLowerCase()` the search used to do on every pass
 * over 13 000 rows is now done once per rename instead (D-188, D-211).
 */
type Hay = { n: string; d: string | null; m: string; text: string; style: number; map: string };

/** Bit 1 = says PVE, 2 = says PVP, 4 = says RP. A server may say several. */
const STYLE_PVE = 1;
const STYLE_PVP = 2;
const STYLE_RP = 4;
// Word boundaries matter: "rp" as a plain substring matches corp, sharp and airport,
// and "pve"/"pvp" inside "PVE/PVP" still get their boundaries from the slash.
const RE_PVE = /\bpve\b/;
const RE_PVP = /\bpvp\b/;
const RE_RP = /\b(?:rp|roleplay|role-play)\b/;

/** Exact-name identity for the clone rule: case and whitespace folded, nothing else. */
const cloneKey = (name: string): string => name.trim().toLowerCase().replace(/\s+/g, " ");

function styleOf(text: string): number {
  return (RE_PVE.test(text) ? STYLE_PVE : 0) | (RE_PVP.test(text) ? STYLE_PVP : 0) | (RE_RP.test(text) ? STYLE_RP : 0);
}

const STYLE_BIT: Record<Exclude<StyleFilter, "any">, number> = { pve: STYLE_PVE, pvp: STYLE_PVP, rp: STYLE_RP };

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
  mods: "any",
  hive: "any",
  style: "any",
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
    if (raw) {
      const f = { ...defaultFilters(), ...(JSON.parse(raw) as Partial<Filters>), search: "" };
      // Saved before D-195 the map could be any capitalisation the server used, and
      // the filter compares against the lower-cased id now.
      f.map = f.map.toLowerCase();
      return f;
    }
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

/**
 * Collator order over the *distinct* values of a field, as a lookup.
 *
 * A comparator that calls `COLLATOR.compare` does so n log n times; when the field
 * has few distinct values — 149 maps across 13 000 servers — sorting those once and
 * comparing integers afterwards is several times cheaper, and the order is identical
 * because equal strings get equal ranks and the comparator falls through to its own
 * tie-breaks (D-193, after D-181).
 */
function rankDistinct<T>(rows: readonly T[], of: (row: T) => string): Map<string, number> {
  const distinct = new Set<string>();
  for (const r of rows) distinct.add(of(r));
  const sorted = [...distinct].sort(COLLATOR.compare);
  const rank = new Map<string, number>();
  for (let i = 0; i < sorted.length; i++) rank.set(sorted[i]!, i);
  return rank;
}

/** No ping has been measured: rows from the DZSA list, or an unreachable imported
 *  favourite, carry 0. A LAN server can genuinely answer in under a millisecond, so it
 *  keeps its 0 (D-242). */
export const pingUnmeasured = (r: ServerRow): boolean => r.pingMs <= 0 && !isLanIp(r.ip);

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
   * Bumped only when the row *set* changes, never when a verification rewrites one.
   *
   * A verification writes `verifiedPlayers`, `verdict`, `players`, `pingMs` and
   * `tags` - it can never change a row’s map or its country. Both deriveds keyed on
   * `#rowsVersion`, so a 30 s pass re-counted 13 000 rows on every one of its ~80
   * flushes for an answer that could not have moved: 1.04 ms a flush, ~85 ms a pass
   * (D-223). `untrustedCount` stays on the main tick - verdicts do change it.
   */
  #rowSetVersion = $state(0);
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
    if (this.#dirtyTimer !== undefined) {
      clearTimeout(this.#dirtyTimer);
      this.#dirtyTimer = undefined;
    }
    this.#recomputeClones();
    this.#rowsVersion++;
    this.#rowSetVersion++;
  }
  #dirtyTimer: ReturnType<typeof setTimeout> | undefined;
  /**
   * Same pooling as `#inbox`, for writes that are already in `rows`. The host used to
   * verify in chunks of 500 and so emitted about six events a pass; it streams now
   * (D-193), which is ~28, and each one bumping the version would run the whole
   * derived chain 28 times instead of six. The rows are already merged — only the
   * recompute is deferred, by at most `ROW_FLUSH_MS`.
   */
  #markDirty() {
    if (this.#dirtyTimer !== undefined) return;
    this.#dirtyTimer = setTimeout(() => {
      this.#dirtyTimer = undefined;
      this.#recomputeClones();
      this.#rowsVersion++;
      if (this.#setDirty) {
        this.#setDirty = false;
        this.#rowSetVersion++;
      }
    }, ROW_FLUSH_MS);
  }
  /** A deferred recompute must also bump the row-set version: rows were removed. */
  #setDirty = false;

  /**
   * Marks rows whose exact name belongs to a server that verified with five or more
   * players on a different address, when this copy itself has not verified.
   *
   * Mirror farms clone the names of real communities: "MIDNIGHT PLUS 3PP | n2" exists
   * 408 times in the live cache, "HATE | STALKER RP" 245 times. Among 1 077 honest
   * populated names, none runs on two addresses, and the generic names that do collide
   * ("DayZ Server", "test server") never reach five verified players, so the reference
   * set excludes them. The flag clears itself the moment the copy verifies (D-233).
   * Two passes over the map, once per flush. `cloneKey` — trim, lower-case, a regex —
   * on every row twice was 60 % of the whole flush chain once the farms tripled the
   * row count: 50.7 ms at 71 000 rows, 17 ms at 24 000 (the "~2 ms" this comment
   * used to claim was never measured). The key is cached per id against the name,
   * like `#hayFor`: 17 ms at 71 000, 7 ms at 40 000 (D-246).
   */
  #cloneKeys = new Map<string, { n: string; k: string }>();
  #cloneKeyOf(r: ServerRow): string {
    const e = this.#cloneKeys.get(r.id);
    if (e !== undefined && e.n === r.name) return e.k;
    const k = cloneKey(r.name);
    this.#cloneKeys.set(r.id, { n: r.name, k });
    return k;
  }
  #recomputeClones() {
    const owners = new Map<string, string>();
    for (const r of this.rows.values()) {
      if (r.verdict === "verified" && (r.verifiedPlayers ?? 0) >= 5) owners.set(this.#cloneKeyOf(r), r.ip);
    }
    for (const r of this.rows.values()) {
      const owner = owners.size ? owners.get(this.#cloneKeyOf(r)) : undefined;
      const clone = owner !== undefined && owner !== r.ip && r.verdict !== "verified";
      // A new object when the flag flips, never a write into the old one: the grid's
      // keyed rows and the details pane compare by identity, so an in-place flag kept
      // the trusted look until something else replaced the row (D-236).
      if (clone !== (r.clone === true)) this.rows.set(r.id, { ...r, clone });
    }
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

  /** Changes when the filter set changes and never when data arrives. The table keys
   *  its visibility effect on it: a search term with the list scrolled to the top
   *  leaves both viewport bounds at 0:25, so the effect never re-ran and the rows the
   *  search had just brought on screen waited up to 60 s to be verified (D-209).
   *  Keeping data out of the key is the whole point of D-060 and D-160. */
  filterKey = $derived(JSON.stringify(this.filters));

  /** Lower-cased name + description per row, with the playstyle it claims and its map.
   *  Rebuilt for a row only when one of those changes, so the filter loop allocates
   *  nothing (D-211). The map was left out of that check: a server that changed map
   *  kept its old one here, so the map filter hid it under the new map, which the
   *  dropdown counted it under (D-256). */
  #hay = new Map<string, Hay>();

  #hayFor(r: ServerRow): Hay {
    let e = this.#hay.get(r.id);
    if (e === undefined || e.n !== r.name || e.d !== r.description || e.m !== r.map) {
      const text = r.description ? (r.name + " | " + r.description).toLowerCase() : r.name.toLowerCase();
      e = { n: r.name, d: r.description, m: r.map, text, style: styleOf(text), map: r.map.toLowerCase() };
      this.#hay.set(r.id, e);
    }
    return e;
  }
  sort = $state<{ key: SortKey; dir: 1 | -1 }>({ key: "players", dir: -1 });

  /** Whether the list holds any server Steam reported as empty. The automatic refresh
   *  asks for `hasplayers` only (D-046), so until someone presses Refresh the list
   *  structurally cannot contain the freshly wiped server they are searching for — and
   *  the empty state told them to widen filters that could never produce it (D-209). */
  hasEmptyServers = $state(false);
  selectedId = $state<string | null>(null);
  localVersion = $state<string | null>(null);
  error = $state<string | null>(null);
  lastRefresh = $state<number | null>(null);
  verifying = $state(false);
  #verifyingSince = 0;
  #scanningSince = 0;
  favourites = new SvelteSet<string>();
  /** Servers joined before, newest first (D-076). */
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

  /** The user changed a filter before the settings file was read (D-197). */
  #filtersTouched = false;

  constructor() {
    void uiPrefs.ready.then((u) => {
      if (!u.filters || this.#filtersTouched) return;
      const f = { ...defaultFilters(), ...(u.filters as Partial<Filters>), search: this.filters.search };
      // Same migration as `loadFilters`: before D-195 this held whatever capitalisation
      // the server used, and the predicate compares against the lower-cased id.
      f.map = f.map.toLowerCase();
      this.filters = f;
    });
  }

  /**
   * Distinct maps with counts, most common first, as `[id, label, count]`.
   *
   * Keyed by the lower-cased id, because servers disagree about capitalisation —
   * `chernarusplus` and `ChernarusPlus`, `deerisle` and `DeerIsle` — and the dropdown
   * used to list each spelling as its own map (D-195).
   */
  maps = $derived.by(() => {
    void this.#rowSetVersion;
    return [...this.#mapCounts.entries()]
      .filter(([, n]) => n > 0)
      .map(([id, n]) => [id, mapLabel(id), n] as [string, string, number])
      .sort((a, b) => b[2] - a[2]);
  });

  /**
   * Map and country counts kept as rows come and go, instead of two passes over
   * every row on every batch flush: 12.7 ms per flush at 71 000 rows for two
   * dropdowns that show the top 40 and 60 (D-223 keyed them off the verification
   * tick; the batch tick still drove them 1 389 times in one refresh). Per batch of
   * 358 rows the deltas cost 0.17 ms including the re-sort of ~150 entries (D-246).
   */
  #mapCounts = new Map<string, number>();
  #countryCounts = new Map<string, number>();
  #count(r: ServerRow, d: 1 | -1) {
    const m = r.map.toLowerCase();
    this.#mapCounts.set(m, (this.#mapCounts.get(m) ?? 0) + d);
    if (r.country) this.#countryCounts.set(r.country, (this.#countryCounts.get(r.country) ?? 0) + d);
  }
  /** `rows.set` for a row that may be new or may have changed map: keeps the counts. */
  #put(r: ServerRow) {
    const prev = this.rows.get(r.id);
    if (prev) this.#count(prev, -1);
    this.#count(r, 1);
    this.rows.set(r.id, r);
  }

  /**
   * The two head-line counts in one pass: as two derived passes they cost 6.0 ms per
   * flush at 71 000 rows; fused, 3.7 ms (D-246).
   */
  #trustCounts = $derived.by(() => {
    void this.#rowsVersion;
    let untrusted = 0;
    let populated = 0;
    for (const r of this.rows.values()) {
      if (isUntrusted(r)) untrusted++;
      else if (trustedPlayers(r) > 0) populated++;
    }
    return { untrusted, populated };
  });
  untrustedCount = $derived(this.#trustCounts.untrusted);

  /** Filters away from their defaults, all rows of the bar (D-108). */
  activeFilterCount = $derived.by(() => {
    const f = this.filters;
    return (
      this.moreFilterCount +
      (f.perspective !== "any" ? 1 : 0) +
      (f.mods !== "any" ? 1 : 0) +
      // Missed when the hive filter was added (D-195): the Reset chip only renders
      // while this is above zero, so filtering to Official alone hid the way back.
      (f.hive !== "any" ? 1 : 0) +
      (f.style !== "any" ? 1 : 0) +
      (f.map ? 1 : 0) +
      (f.country ? 1 : 0)
    );
  });

  /** Filters away from their defaults in the fold-away row only (D-108). */
  moreFilterCount = $derived.by(() => {
    const f = this.filters;
    return (
      [f.notFull, f.notEmpty, f.hasQueue, f.noPassword, f.dayOnly, f.versionMine].filter(Boolean).length +
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
  populatedCount = $derived(this.#trustCounts.populated);

  /** Distinct countries with counts, most common first (rows without a country are skipped). */
  countries = $derived.by(() => {
    void this.#rowSetVersion;
    return [...this.#countryCounts.entries()].filter(([, n]) => n > 0).sort((a, b) => b[1] - a[1]);
  });

  list = $derived.by(() => {
    void this.#rowsVersion;
    const f = this.filters;
    // Read every filter once, not once per row: `filters` is a `$state` proxy, and a
    // proxy get is ~24x a plain read. Seventeen of them across 19 000 rows, ten times
    // a second during a refresh, was the single most expensive thing in the app (D-188).
    const q = f.search.trim().toLowerCase();
    const hideUntrusted = f.hideUntrusted;
    const perspective = f.perspective;
    const map = f.map;
    const country = f.country;
    const mod = f.mod;
    const notFull = f.notFull;
    const notEmpty = f.notEmpty;
    const hasQueue = f.hasQueue;
    const noPassword = f.noPassword;
    const hive = f.hive;
    const mods = f.mods;
    const styleBit = f.style === "any" ? 0 : STYLE_BIT[f.style];
    const dayOnly = f.dayOnly;
    const maxPing = f.maxPing;
    const versionMine = f.versionMine;
    const friendsOnly = f.friendsOnly;
    const localVersion = this.localVersion;
    const modsByServer = this.modsByServer;
    const friendsOn = this.friendsOn;
    const out: ServerRow[] = [];
    for (const r of this.rows.values()) {
      if (hideUntrusted && isUntrusted(r)) continue;
      // Only when something reads it: with no search term this was a Map.get plus
      // three string compares per row for a value nobody looked at - 0.475 ms per
      // pass at 13 000 rows (D-223).
      const hay = q ? this.#hayFor(r) : null;
      if (q && !(hay!.text.includes(q) || mapHaystack(r.map).includes(q) || r.ip.startsWith(q))) continue;
      if (perspective === "1pp" && !r.tags.firstPersonOnly) continue;
      if (perspective === "3pp" && r.tags.firstPersonOnly) continue;
      // The dropdown's value is the lower-cased id, because servers disagree about
      // capitalisation and `ChernarusPlus` is the same map as `chernarusplus` (D-195).
      // The lower-cased id lives in the cached entry rather than being rebuilt per
      // row per pass - 0.164 ms at 13 000 rows whenever the dropdown is set (D-223).
      if (map && this.#hayFor(r).map !== map) continue;
      if (country && r.country !== country) continue;
      if (mod && !modsByServer.get(r.id)?.includes(mod)) continue;
      const pop = trustedPlayers(r);
      if (notFull && pop >= r.maxPlayers) continue;
      if (notEmpty && pop <= 0) continue;
      if (hasQueue && queueOf(r) === 0) continue;
      if (noPassword && r.password) continue;
      // 13 375 of 13 380 servers run BattlEye, so the chip that used to sit here
      // filtered out five of them; the hive is the distinction that changes what a
      // join means (D-195).
      if (hive !== "any" && r.tags.privateHive !== (hive === "community")) continue;
      if (styleBit && !(this.#hayFor(r).style & styleBit)) continue;
      if (mods === "modded" && !r.tags.modded) continue;
      if (mods === "vanilla" && r.tags.modded) continue;
      if (dayOnly && !(r.tags.timeMinutes != null && r.tags.timeMinutes >= 6 * 60 && r.tags.timeMinutes < 20 * 60)) continue;
      // An unmeasured ping cannot be said to meet a limit; as 0 it passed every one.
      if (maxPing > 0 && (r.pingMs > maxPing || pingUnmeasured(r))) continue;
      if (versionMine && localVersion && r.version !== localVersion) continue;
      if (friendsOnly && !friendsOn.has(r.id)) continue;
      out.push(r);
    }
    this.#sortInPlace(out);
    return out;
  });

  /** Sorts rows in place by the chosen column. Shared so that the sort arrows the
   *  Favourites and LAN tabs draw actually move their rows: both passed `servers.sort`
   *  and `setSort` into the table while keeping a hard-coded busiest-first order, so
   *  clicking a header moved the arrow, reordered nothing, and silently changed the
   *  Servers page behind your back (D-209). */
  #sortInPlace(out: ServerRow[]) {
    const { key, dir } = this.sort;
    const n = out.length;
    if (n < 2) return;
    // Ping is quantised to 20 ms steps and ties break on the id so that the
    // jitter from re-verification never reorders rows (D-060: reorders exposed
    // fresh rows to verification in a loop that grew CPU and memory).
    const pingBucket = (r: ServerRow) => Math.round(r.pingMs / 20);
    // Mod counts once per sort rather than once per comparison (D-152): the map is
    // reactive, so a lookup inside the comparator was ~n log n signal reads.
    const modCounts =
      key === "mods" ? new Map(out.map((r) => [r.id, this.modsByServer.get(r.id)?.length ?? -1])) : null;
    const nameRank = key === "name" ? this.#rankByName() : null;
    // Same argument as D-181’s name ranks, and cheaper still because there are only
    // ~100 distinct map names across the whole list: one collator pass over the
    // distinct values turns every comparison into integer subtraction (D-193).
    const mapRank = key === "map" ? rankDistinct(out, (r) => mapLabel(r.map).toLowerCase()) : null;

    // Decorate, sort indices, permute. D-181 and D-193 precomputed the ranks but left
    // the *lookup* inside the comparator, so every one of ~346 000 comparisons at
    // 12 710 rows did two hashed string lookups. Reading a typed array by integer
    // index instead: map 11.86 -> 6.06 ms, mods 10.17 -> 4.65, name 7.28 -> 2.81,
    // players 6.93 -> 4.84, with byte-identical order in both directions. This runs on
    // every 350 ms flush for the whole length of a refresh (D-223).
    //
    // `k2` is negated where the old comparator sorted that level descending, so both
    // levels compare ascending and `dir` still flips the whole thing, tie-break
    // included, exactly as `dir * cmp(a, b)` did.
    const k1 = new Float64Array(n);
    const k2 = new Float64Array(n);
    for (let i = 0; i < n; i++) {
      const r = out[i]!;
      switch (key) {
        case "name":
          // The id tie-break is already inside the rank (D-181).
          k1[i] = nameRank!.get(r.id) ?? 0;
          break;
        case "map":
          k1[i] = mapRank!.get(mapLabel(r.map).toLowerCase()) ?? 0;
          k2[i] = -trustedPlayers(r);
          break;
        case "mods":
          // Unscanned servers sort below every scanned one, in both directions.
          k1[i] = modCounts!.get(r.id) ?? -1;
          k2[i] = -trustedPlayers(r);
          break;
        case "players":
          k1[i] = trustedPlayers(r);
          // An unmeasured ping (D-242) is the worst tie-breaker, not the best: at 0 it
          // sorted a DZSA row above a measured 40 ms one among equal counts (D-245).
          k2[i] = pingUnmeasured(r) ? -1e6 : -pingBucket(r);
          break;
        case "ping":
          k1[i] = pingUnmeasured(r) ? -1 : pingBucket(r);
          k2[i] = -trustedPlayers(r);
          break;
        case "time":
          k1[i] = r.tags.timeMinutes ?? -1;
          break;
        case "version":
          k1[i] = r.serverVersion;
          break;
      }
    }
    const idx = new Uint32Array(n);
    for (let i = 0; i < n; i++) idx[i] = i;
    // Unscanned servers (-1) go below every scanned one whichever way the column
    // sorts, as D-146 says, and so do unmeasured pings (D-242); multiplied by `dir` like the rest, an ascending sort put
    // every one of them first (D-240).
    const unscannedLast = key === "mods" || key === "ping";
    idx.sort((x, y) => {
      if (unscannedLast) {
        const u = +(k1[x]! < 0) - +(k1[y]! < 0);
        if (u !== 0) return u;
      }
      const d = k1[x]! - k1[y]! || k2[x]! - k2[y]!;
      if (d !== 0) return dir * d;
      const a = out[x]!.id;
      const b = out[y]!.id;
      return dir * (a < b ? -1 : a > b ? 1 : 0);
    });
    const permuted = new Array<ServerRow>(n);
    for (let i = 0; i < n; i++) permuted[i] = out[idx[i]!]!;
    for (let i = 0; i < n; i++) out[i] = permuted[i]!;
  }

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
      // Only when something reads it: with no search term this was a Map.get plus
      // three string compares per row for a value nobody looked at - 0.475 ms per
      // pass at 13 000 rows (D-223).
      const hay = q ? this.#hayFor(r) : null;
      if (q && !(hay!.text.includes(q) || mapHaystack(r.map).includes(q) || r.ip.startsWith(q))) continue;
      out.push(r);
    }
    this.#sortInPlace(out);
    return out;
  });

  /** LAN rows before the search, so the tab can tell "none found" from "none match" (D-240). */
  lanTotal = $derived.by(() => {
    void this.#rowSetVersion;
    let n = 0;
    for (const r of this.rows.values()) if (isLanIp(r.ip)) n++;
    return n;
  });

  /** Rows with a local-network address (LAN tab, D-087), search-filtered, busiest first. */
  lanRows = $derived.by(() => {
    void this.#rowsVersion;
    const q = this.filters.search.trim().toLowerCase();
    const out: ServerRow[] = [];
    for (const r of this.rows.values()) {
      if (!isLanIp(r.ip)) continue;
      // Only when something reads it: with no search term this was a Map.get plus
      // three string compares per row for a value nobody looked at - 0.475 ms per
      // pass at 13 000 rows (D-223).
      const hay = q ? this.#hayFor(r) : null;
      if (q && !(hay!.text.includes(q) || mapHaystack(r.map).includes(q) || r.ip.startsWith(q))) continue;
      out.push(r);
    }
    this.#sortInPlace(out);
    return out;
  });

  async start() {
    if (this.#started) return;
    this.#started = true;
    try {
      const cached = await invoke<CachedServers>("servers_cached");
      this.lastRefresh = cached.lastRefresh;
      for (const r of cached.rows) {
        this.#put(r);
        if (r.steamEmpty === true) this.hasEmptyServers = true;
      }
      this.#namesDirty = true;
      this.rowsChanged();
    } catch (e) {
      // The list is the one thing a refresh rebuilds by itself, so say so rather
      // than leaving a bare backend string on screen.
      this.error = `The cached server list could not be read (${describe(e)}). Refresh to fetch a new one.`;
    }
    // Favourites are the user's own data and live in their own tables: a failure to
    // read the server list must not take them off the screen with it. Nor may a failure
    // to read *them* take the session: this sits before every `listen()` below, and an
    // unguarded rejection here used to skip all of them plus the status snapshot, the
    // first refresh and the watchdog — one unlucky SQLITE_BUSY from a backup and the
    // grid read "Starting up…" until the app was restarted (D-194).
    try {
      await this.loadFavourites();
    } catch (e) {
      logWarn("cache", `favourites unavailable at start: ${describe(e)}`);
    }
    void this.loadModsIndex();
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
          // "Busy" is not a failure: a refresh is already running and will report for
          // itself. Treating the two the same put a permanent red "Steam did not answer
          // the refresh" on screen for a double-click on Refresh, and switched off the
          // verifying indicator for the pass that was genuinely running (D-208).
          if (ev.payload.reason !== "busy") {
            this.verifying = false;
            this.#verifyingSince = 0;
            this.error = "Steam did not answer the refresh, so the list was not updated.";
          }
          return;
        }
        this.done = ev.payload;
        // The verification pass starts now, not when Refresh was pressed. A full refresh
        // measured 473 s (D-046), so a clock started at the button press ran out while
        // Steam was still listing, and a red "verification stopped answering" appeared
        // next to a pass that then finished normally (D-236).
        if (this.verifying) this.#verifyingSince = Date.now();
        // A LAN scan or a DZSA import is not a Steam refresh. It claimed the Steam card's
        // "last refresh" time, which the host keeps for Steam alone (D-220), and a DZSA
        // import finishing during a Steam refresh threw away that refresh's record of
        // the servers it had seen, so the vouches below were never withdrawn (D-256).
        if (ev.payload.source !== "steam") return;
        this.lastRefresh = Math.floor(Date.now() / 1000);
        // Steam may have updated DayZ while the list was coming in.
        void this.refreshLocalVersion();
        // Mirror of the host's `unvouch_unseen` (D-233): the in-memory rows must agree
        // with the cache, or the vouch would linger on screen until the next start.
        const seen = this.#seenThisRefresh;
        this.#seenThisRefresh = null;
        // The host decides what "complete" means from the partition answers, so the
        // two cannot disagree about a refresh Steam timed out or throttled (D-236).
        if (seen && ev.payload.complete === true) {
          let n = 0;
          for (const r of this.rows.values()) {
            if (r.steamEmpty === false && !seen.has(r.id)) {
              // A new object, not a write into the old one: the grid's keyed rows and
              // the details pane compare by identity, so an in-place change kept the
              // vouched look until a verification happened to replace the row (D-236).
              this.rows.set(r.id, { ...r, steamEmpty: null });
              n++;
            }
          }
          if (n > 0) this.rowsChanged();
        }
      }),
      await listen<SteamStatus>("steam:status", (ev) => {
        this.steam = ev.payload;
        this.maybeAutoRefresh();
        if (this.friendsInDayz == null) void this.pollFriends();
      }),
      await listen<string[]>("servers:pruned", (ev) => this.dropRows(ev.payload)),
      await listen<Verification[]>("servers:verified", (ev) => this.applyVerifications(ev.payload)),
      await listen<VerifySummary>("servers:verify-done", (ev) => {
        this.verifySummary = ev.payload;
        this.verifying = false;
        this.#verifyingSince = 0;
        // A late answer proves the watchdog wrong; its message must not sit beside it.
        if (this.error === VERIFY_STALLED) this.error = null;
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
    );
    // Only now that `steam:status` is being listened for. The worker emits the
    // "session open" status exactly once, so reading the snapshot before registering
    // meant an event landing in between was lost for the whole session: Refresh stayed
    // disabled, nothing refreshed automatically, and the DZSA fallback downloaded
    // ~24 MB instead (D-190).
    try {
      this.steam = await invoke<SteamStatus>("steam_status");
      this.localVersion = await invoke<string | null>("local_game_version");
    } catch (e) {
      logWarn("steam", `status unavailable at start: ${describe(e)}`);
    }
    // And again whenever the user comes back to the window, at most twice a minute.
    window.addEventListener("focus", () => {
      if (Date.now() - this.#versionReadAt > 30_000) void this.refreshLocalVersion();
    });
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
    if (this.verifying && this.#verifyingSince && !this.steam?.refreshing && now - this.#verifyingSince > VERIFY_DEADLINE_MS) {
      this.verifying = false;
      this.#verifyingSince = 0;
      this.error = VERIFY_STALLED;
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
    // An idle release is deliberate and keeps the last values: the session comes back
    // by itself. Steam actually going is different, and nothing used to clear these —
    // so the title bar went on saying "2 friends in DayZ" for the rest of the session
    // and rows kept pills naming people who had left, while the Friends page
    // correctly said Steam was not running (D-222).
    if (!s?.initialized) {
      this.friendsInDayz = null;
      this.friendsOn.clear();
      return;
    }
    if (s.idle) return;
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
  /** Ids delivered by the refresh in flight, or null when none is (D-233). */
  #seenThisRefresh: Set<string> | null = null;
  dzsaLoading = $state(false);

  private maybeAutoRefresh() {
    // Not while the session is released: a status event is the only thing that calls
    // this, and after a declined start-up refresh the next one was the idle release
    // itself — so the retry re-opened the session the release had just closed and put
    // the user back to "Playing DayZ" for another quarter of an hour (D-236).
    if (this.steam?.initialized && !this.steam.idle && !this.steam.refreshing && !this.#autoRefreshed) {
      // Armed by `refresh` itself, from what the worker actually answered. Set here,
      // it was spent even when the worker declined — and it declines for 60 s after
      // the last completed refresh, seeded across restarts from the cache. So a
      // restart inside that minute (routine after the updater relaunches) sat on the
      // cached list with stale counts, said nothing, and left the user to press a
      // Refresh that has been the multi-minute full pass since D-141 (D-222).
      void this.refresh(false, false);
    } else if (this.steam && !this.steam.initialized && this.steam.error && !this.#dzsaTried && !this.#autoRefreshed) {
      // Steam failed to initialise: fall back to the DZSA list once (D-089), unless a
      // recent cached list already covers the session.
      this.#dzsaTried = true;
      // An hour, not ten minutes: the launcher often starts seconds before Steam does
      // (the worker retries every 10 s, D-125), and this branch imported 13 096 rows
      // without a Steam id — 7 236 of them never counted and listed by nobody since,
      // 46 % of the visible list once the empties were loaded (D-247). A cache an
      // hour old shows counts an hour old until Steam is back; the manual "load the
      // DZSA list" stays for a Steam that is really down.
      const fresh = this.lastRefresh != null && Date.now() / 1000 - this.lastRefresh < 3600;
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
    // Armed before the call, not after: the host emits `servers:verify-done` from
    // inside the command when the list has nothing to verify (D-162), so setting it
    // afterwards re-armed a flag that had already been cleared and the watchdog then
    // invented a failure five minutes later (D-190).
    this.verifying = true;
    this.#verifyingSince = Date.now();
    try {
      await invoke<number>("servers_dzsa");
      void this.loadModsIndex();
    } catch (e) {
      this.error = String(e);
      this.verifying = false;
      this.#verifyingSince = 0;
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
      this.#autoRefreshed = started;
      // Declined by the worker's 60 s throttle (a restart right after a refresh, the
      // updater's relaunch): nothing else would ask again until the next status
      // event, which in a quiet session is the idle release a quarter of an hour
      // later — or never, with the release switched off (D-236).
      if (!started && !force && !full) this.#retryAutoRefresh();
      // Every id the batches deliver from here on; the rows Steam does not return
      // this time lose their vouch when the refresh completes (D-233).
      if (started) this.#seenThisRefresh = new Set();
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

  #versionReadAt = Date.now();
  /**
   * The installed DayZ build, read again on window focus and after a Steam refresh.
   * Read only at start, a DayZ update that Steam applied mid-session left the old build
   * here, and "My version" then hid every server that had updated while listing the
   * ones the game could no longer join (D-256).
   */
  async refreshLocalVersion() {
    this.#versionReadAt = Date.now();
    try {
      this.localVersion = await invoke<string | null>("local_game_version");
    } catch (e) {
      logWarn("steam", `local game version unavailable: ${describe(e)}`);
    }
  }

  #autoRetry: ReturnType<typeof setTimeout> | undefined;
  #retryAutoRefresh() {
    if (this.#autoRetry !== undefined) return;
    const ago = this.steam?.lastRefreshSecsAgo ?? 0;
    const waitMs = Math.max(5, AUTO_REFRESH_GAP_SECS - ago + 2) * 1000;
    this.#autoRetry = setTimeout(() => {
      this.#autoRetry = undefined;
      this.maybeAutoRefresh();
    }, waitMs);
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
      // A row without Steam's empty flag is a DZSA row: the host keeps its measured
      // values over the list's placeholders, and so does this.
      this.#put(prev ? this.#merge(prev, r, r.steamEmpty == null) : r);
      this.#seenThisRefresh?.add(r.id);
      if (r.steamEmpty === true && !this.hasEmptyServers) this.hasEmptyServers = true;
    }
    this.rowsChanged();
  }

  /**
   * What an incoming row keeps from the one it replaces: the host's upsert rules
   * (`upsert_sql` in cache.rs), so the list shows what the cache holds. Verification
   * results never travel with a listing or a probe. A row without Steam's empty flag
   * keeps the flag, and while that says empty, the player count R0 reads; a DZSA row
   * (`keepMeasured`) also keeps every measurement it has only a placeholder for. With
   * the verdict alone kept, a DZSA import blanked the ping, description, bots and flag
   * of every row it touched: ping "—", search and playstyle misses, R0 and R9 off,
   * until a restart (D-242, D-245, D-256).
   */
  #merge(prev: ServerRow, r: ServerRow, keepMeasured: boolean): ServerRow {
    const m: ServerRow = { ...r, verifiedPlayers: prev.verifiedPlayers, verifiedAt: prev.verifiedAt, verdict: prev.verdict };
    if (r.steamEmpty == null) {
      m.steamEmpty = prev.steamEmpty;
      if (prev.steamEmpty === true) m.players = prev.players;
    }
    if (keepMeasured) {
      if (r.pingMs === 0) m.pingMs = prev.pingMs;
      if (!r.bots) m.bots = prev.bots;
      if (r.description === "") m.description = prev.description;
    }
    return m;
  }

  /**
   * Drops the rows the cache pruned after a completed refresh (30 days unseen, never a
   * favourite). The map is otherwise only ever added to, so a long-lived session kept
   * in the WebView every server it had been sent since start-up (Q24, D-235).
   */
  dropRows(ids: string[]) {
    // A batch still in the inbox for one of these ids would put it straight back.
    this.flushRows();
    let n = 0;
    const delta = new Map<number, number>();
    for (const id of ids) {
      const gone = this.rows.get(id);
      if (!gone) continue;
      this.rows.delete(id);
      this.#count(gone, -1);
      n++;
      this.#hay.delete(id);
      this.#cloneKeys.delete(id);
      this.#pending.delete(id);
      const mods = this.modsByServer.get(id);
      if (mods) {
        for (const m of mods) delta.set(m, (delta.get(m) ?? 0) - 1);
        this.modsByServer.delete(id);
      }
    }
    if (n === 0) return;
    for (const [m, d] of delta) {
      const e = this.modCatalog.get(m);
      if (e) this.modCatalog.set(m, { name: e.name, servers: Math.max(0, e.servers + d) });
    }
    for (const id of ids) this.friendsOn.delete(id);
    if (this.selectedId && !this.rows.has(this.selectedId)) this.selectedId = null;
    // A join dialog open on a pruned row would fail its next step with "unknown
    // server"; it closes instead (D-248).
    if (this.joiningId && !this.rows.has(this.joiningId)) this.joiningId = null;
    // It only ever went from false to true, so once the prune took the last row Steam
    // listed as empty, the empty state kept saying "widen the filters" instead of
    // offering the Refresh that brings them back (D-210, D-236).
    if (this.hasEmptyServers) {
      let any = false;
      for (const r of this.rows.values()) {
        if (r.steamEmpty === true) {
          any = true;
          break;
        }
      }
      this.hasEmptyServers = any;
    }
    this.#namesDirty = true;
    // Deferred like `#markDirty`: the host sends the pruned ids in chunks of 8 000,
    // and one derived chain per chunk was 242–290 ms for 31 000 ids; pooled, one
    // chain at the size that remains (D-246).
    this.#setDirty = true;
    this.#markDirty();
  }

  applyVerifications(list: Verification[]) {
    // A verification can land between a batch and its flush; without this the row
    // it refers to would not be in `rows` yet and the result would be dropped.
    this.flushRows();
    for (const v of list) {
      const r = this.rows.get(v.id);
      if (!r) continue;
      this.#pending.delete(v.id);
      // Mirrors `apply_verifications` (cache.rs), rule for rule (D-236).
      const infoAnswered = v.pingMs != null;
      const synthetic = v.verdict === "synthetic";
      this.rows.set(v.id, {
        ...r,
        // Only a fresh INFO moves the claim: the fallback is the count the check
        // started from, which a later Steam batch may already have replaced.
        players: infoAnswered ? v.reported : r.players,
        maxPlayers: infoAnswered ? v.maxPlayers : r.maxPlayers,
        // A never-measured ping takes PLAYER's round trip (D-247); `infoAnswered`
        // above keeps reading `pingMs` alone, as the cache does.
        pingMs: v.pingMs ?? (r.pingMs === 0 && !isLanIp(r.ip) && v.playerRttMs != null ? v.playerRttMs : r.pingMs),
        tags: v.tags ?? r.tags,
        // Keep the last real count when this check could not produce one (D-160) —
        // but a list judged synthetic is not a count, and must not become the "last
        // head-count" R6 trusts.
        verifiedPlayers: synthetic ? null : (v.verified ?? r.verifiedPlayers),
        verifiedAt: synthetic ? null : v.verified == null ? r.verifiedAt : v.verifiedAt,
        verdict: v.verdict,
        // Our own count of real players beats Steam's "empty" from a listing that
        // also said 0; a farm's listing claimed its number and keeps R0.
        steamEmpty:
          r.steamEmpty === true && r.players === 0 && infoAnswered && v.verdict === "verified" && (v.verified ?? 0) > 0 ? null : r.steamEmpty,
      });
    }
    this.#markDirty();
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
    for (const f of list) {
      this.favourites.add(f.id);
    }
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
      // Merged like a listing: a probe carries no verdict, and the known one and its
      // count vanished until the re-check landed (D-256).
      const prev = this.rows.get(row.id);
      this.#put(prev ? this.#merge(prev, row, false) : row);
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

  saveFilters() {
    // The cached list paints before the settings file has been read, and that read
    // retries for up to two seconds (D-112): anything changed in that window used to
    // be overwritten wholesale when the file arrived, while the queued patch still
    // wrote the user's value to disk, so the two disagreed until the next save (D-197).
    this.#filtersTouched = true;
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
