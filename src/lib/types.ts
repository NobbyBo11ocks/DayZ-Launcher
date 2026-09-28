// IPC payload shapes. Mirrors the `#[derive(Serialize)]` structs in src-tauri/src
// (camelCase via serde). Keep the two in sync; the Rust side is the source of truth.
// Note: u64 fields (steamId, workshopId) arrive as JSON numbers; Steam IDs exceed
// 2^53 and lose precision, so they are never used for identity in the UI.

export type SteamInfo = {
  path: string | null;
  exe: string | null;
  /** Registry hive the path came from: "HKCU", "HKLM" or "none". */
  source: string;
  running: boolean;
  /** Live steam.exe PID from process enumeration (0 when not running). */
  pid: number;
  /** ActiveProcess\pid from the registry; known to go stale. */
  registryPid: number;
  activeUser: number;
};

export type LibraryInfo = {
  path: string;
  label: string;
  appCount: number;
  hasDayz: boolean;
};

export type GameInfo = {
  library: string;
  folder: string;
  exe: string;
  /** ProductVersion of DayZ_x64.exe, e.g. "1.29.0.163709". */
  exeVersion: string | null;
  /** Same version in the A2S form servers announce, e.g. "1.29.163709". */
  gameVersion: string | null;
  buildId: string;
  lastUpdated: number;
  sizeOnDisk: number;
  stateFlags: number;
  hasBattleyeExe: boolean;
  hasOfficialLauncher: boolean;
};

export type WorkshopItemInfo = {
  id: number;
  size: number;
  timeUpdated: number;
  folder: string | null;
  metaName: string | null;
  modName: string | null;
  needsUpdate: boolean;
};

export type WorkshopInfo = {
  acfPath: string;
  needsUpdate: boolean;
  needsDownload: boolean;
  lastBuildId: string;
  sizeOnDisk: number;
  items: WorkshopItemInfo[];
};

export type JunctionInfo = {
  name: string;
  target: string | null;
  workshopId: number | null;
  targetExists: boolean;
  /** Target certainly gone and a Workshop item: what "Clean" removes (D-276). */
  removable: boolean;
};

/**
 * Absent keys, not nulls: the host leaves out everything it has nothing to say
 * about, and the two fields nothing renders (`external`, `unknown`) never cross
 * IPC at all (D-164).
 */
export type DayzTags = {
  battleye: boolean;
  firstPersonOnly: boolean;
  privateHive: boolean;
  modded: boolean;
  dlc: boolean;
  allowedFilePatching: boolean;
  queue?: number | null;
  timeMultiplier?: number | null;
  nightMultiplier?: number | null;
  /** Minutes since midnight, in-game clock. */
  timeMinutes?: number | null;
};

export type Verdict = "verified" | "inflated" | "unverifiable" | "synthetic" | "offline";

export type ServerRow = {
  /** ip:queryPort */
  id: string;
  ip: string;
  gamePort: number;
  queryPort: number;
  name: string;
  map: string;
  description: string;
  /** Reported by the server; often inflated (D-038). */
  players: number;
  maxPlayers: number;
  password: boolean;
  serverVersion: number;
  version: string;
  pingMs: number;
  tags: DayzTags;
  /** Steam's INFO bot count. Equal to `players` on every spoofed row measured; see `isUntrusted` (D-233). */
  bots?: number;
  verifiedPlayers?: number | null;
  /** Steam's master reported zero authenticated players (row from a `noplayers` partition). */
  steamEmpty?: boolean | null;
  verifiedAt?: number | null;
  verdict?: Verdict | null;
  /**
   * Front-end only, set by the store: this row's name (case and spacing folded) belongs
   * to a server verified with at least five players on another address, and this copy's
   * own latest check has not verified it. 2 771 rows in the live cache, 0 collisions
   * among 1 077 honest populated names (D-233).
   */
  clone?: boolean;
  /** ISO 3166-1 alpha-2 from the embedded GeoIP table, absent when unknown (D-073). */
  country?: string | null;
};

/** Rule R0 (docs/11): Steam says empty, A2S_INFO claims players. */
export const isInflated = (r: ServerRow): boolean => r.steamEmpty === true && r.players > 0;

/**
 * Any rule fired: hidden by the default "Hide inflated" filter.
 * R4 (refuses PLAYER) only counts when Steam has not vouched for the server:
 * a small minority of servers drop A2S_PLAYER at the host firewall — D-050 measured 6 of 2 858, and D-047 retracts the earlier 24 % reading as burst loss. Steam's vouch keeps an "unverifiable" row trusted only when a head-count was taken before (R6, D-233).
 */
export const isUntrusted = (r: ServerRow): boolean =>
  isInflated(r) ||
  r.verdict === "inflated" ||
  r.verdict === "synthetic" ||
  r.verdict === "offline" ||
  // No DayZ server has ever verified above 116 here, and 2 323 cached rows claim more
  // than 127, every one of them a Steam-says-empty fake: the line rests on that
  // measurement, not on an engine limit nobody has sourced (docs/11 records an honest
  // server advertising 225 slots, D-268). Insurance for the day one of them also holds
  // a Steam session (D-233).
  r.players > 127 ||
  // The bots byte, as a prior on rows nobody has counted: the INFO patchers write it
  // equal to their fabricated count. Honest servers with AI declare bots != players, and
  // the three honest rows where the two happen to agree all have a head-count that
  // exempts them here (D-233).
  (r.verifiedPlayers == null && (r.bots ?? 0) > 0 && r.bots === r.players) ||
  r.clone === true ||
  // Steam vouching used to override "unverifiable" outright, which trusted the
  // server's own INFO claim for any operator holding one Steam session and dropping
  // PLAYER: three fingerprinted farm boxes sat on screen at 96, 67 and 44 that way.
  // The vouch now only keeps a server visible when a real head-count was taken
  // before; a claim nobody has ever counted is untrusted whatever Steam says (D-233).
  (r.verdict === "unverifiable" && !(r.steamEmpty === false && r.verifiedPlayers != null));

/**
 * True when the number on screen is the server's own claim rather than a head-count
 * we made ourselves (D-288). Steam vouching for a server keeps it visible, but the
 * count is still unchecked and must not look verified: hiding behind a green tick is
 * exactly the gap a server that answers INFO and firewalls PLAYER relies on.
 */
export const isUnchecked = (r: ServerRow): boolean =>
  r.verifiedPlayers == null &&
  // A fabricated list is no head-count either (D-237): the cell shows the server's own
  // claim under the warning, not a count nobody took.
  (r.verdict === "unverifiable" || r.verdict === "offline" || r.verdict === "synthetic" || r.verdict == null);

/**
 * Population a player may rely on (mirrors `judge` in browser/verify.rs). This is
 * what the list sorts by and what the title-bar count adds up, so it must never be
 * a number a check has contradicted. A row no check has reached yet counts its own
 * claim until one does; the grid marks it "?" (`isUnchecked`).
 */
export const trustedPlayers = (r: ServerRow): number => {
  // A fresh Steam batch saying empty, with INFO agreeing at 0, beats a head-count
  // from hours ago: 87 live rows painted a 20-hour-old 3–5 over a server everyone
  // else could see was empty (D-233).
  if (r.steamEmpty === true && r.players === 0) return 0;
  if (r.verifiedPlayers != null) return r.verifiedPlayers;
  if (isInflated(r)) return 0;
  // A server that refuses PLAYER has a claim, not a count. The cell still shows the
  // claim, dimmed and marked "?"; the sort must not reward it (D-233). Nor may a list
  // judged fabricated, whose length is the claim again (D-237).
  if (r.verdict === "unverifiable" || r.verdict === "synthetic") return 0;
  return r.players;
};

/**
 * The queue a server advertises, believed only when the server is actually full.
 * `lqs<N>` is a free-text keyword, and a 7-player server was showing "+172" with
 * it — 486 never-verified rows advertise one (D-233).
 */
export const queueOf = (r: ServerRow): number =>
  r.tags.queue && r.tags.queue > 0 && trustedPlayers(r) >= r.maxPlayers - 2 ? r.tags.queue : 0;

export type SteamStatus = {
  initialized: boolean;
  error: string | null;
  appId: number;
  steamId: number | null;
  persona: string | null;
  refreshing: boolean;
  lastRefreshSecsAgo: number | null;
  /** Steamworks was released after inactivity (Q16); the next command that needs a
   *  session re-opens it, while the cache reads (Workshop flags, avatars) do not (D-275). */
  idle: boolean;
  /** Steam runs but nobody is signed in to it (row 14, F12). */
  signedOut?: boolean;
};

export type PartitionResult = {
  filters: Record<string, string>;
  total: number;
  responded: number;
  failed: number;
  /** Rows flagged by rule R0. */
  inflated: number;
  elapsedMs: number;
  response: string;
  /** Steam truncated this partition at its 10 000-server cap. */
  capped: boolean;
};

/** The start-up list in columns (D-284): the field names once, each row an array in
 *  `keys` order with its tags an array in `tagKeys` order. */
export type CachedServers = {
  keys: string[];
  tagKeys: string[];
  rows: unknown[][];
  lastRefresh: number | null;
};

/** Strings that repeat across rows, one instance each: 71 397 rows carry 6 958
 *  addresses, 199 maps, 24 versions, five verdicts and about 200 countries, and JSON
 *  gave every row its own copies (D-297). Bounded by what the session has seen. */
const sharedStrings = new Map<string, string>();
function share<T extends string | null | undefined>(s: T): T {
  if (typeof s !== "string") return s;
  const k = sharedStrings.get(s);
  if (k !== undefined) return k as T;
  sharedStrings.set(s, s);
  return s;
}

/** The tags in one shape, every field present (D-297). */
function tagsOf(t: DayzTags): DayzTags {
  return {
    battleye: t.battleye,
    firstPersonOnly: t.firstPersonOnly,
    privateHive: t.privateHive,
    modded: t.modded,
    dlc: t.dlc,
    allowedFilePatching: t.allowedFilePatching,
    queue: t.queue,
    timeMultiplier: t.timeMultiplier,
    nightMultiplier: t.nightMultiplier,
    timeMinutes: t.timeMinutes,
  };
}

const sameTags = (a: DayzTags, b: DayzTags): boolean =>
  a.battleye === b.battleye &&
  a.firstPersonOnly === b.firstPersonOnly &&
  a.privateHive === b.privateHive &&
  a.modded === b.modded &&
  a.dlc === b.dlc &&
  a.allowedFilePatching === b.allowedFilePatching &&
  a.queue === b.queue &&
  a.timeMultiplier === b.timeMultiplier &&
  a.nightMultiplier === b.nightMultiplier &&
  a.timeMinutes === b.timeMinutes;

/**
 * A row in the one shape every row in the store has: all fields present, in one order,
 * the strings of the row it replaces reused when equal, and the repeating ones shared.
 * The start-up decode added fields one by one and left the absent ones out, and
 * batches kept the shape JSON gave them, so rows came in several shapes — the ones with
 * all twenty fields in dictionary mode — and no two rows shared a string: 692 B a row in
 * Node, about 407 in the renderer (row 13, D-297). An absent field is `undefined`,
 * which every reader already treats as absent.
 */
export function compactRow(r: ServerRow, prev?: ServerRow): ServerRow {
  return {
    id: prev !== undefined ? prev.id : r.id,
    ip: prev !== undefined && prev.ip === r.ip ? prev.ip : share(r.ip),
    gamePort: r.gamePort,
    queryPort: r.queryPort,
    name: prev !== undefined && prev.name === r.name ? prev.name : r.name,
    map: share(r.map),
    description: prev !== undefined && prev.description === r.description ? prev.description : r.description,
    players: r.players,
    maxPlayers: r.maxPlayers,
    password: r.password,
    serverVersion: r.serverVersion,
    version: share(r.version),
    pingMs: r.pingMs,
    tags: prev !== undefined && sameTags(prev.tags, r.tags) ? prev.tags : tagsOf(r.tags),
    bots: r.bots,
    verifiedPlayers: r.verifiedPlayers,
    steamEmpty: r.steamEmpty,
    verifiedAt: r.verifiedAt,
    verdict: share(r.verdict),
    clone: r.clone,
    country: share(r.country),
  };
}

/** The rows of `servers_cached`, each the same object every other event sends: a
 *  `null` is a value that form leaves out (D-284). Built straight into the store's one
 *  shape, with the names and descriptions mirror farms repeat shared across the list
 *  (D-297). */
export function decodeCachedRows(c: CachedServers): ServerRow[] {
  const { keys, tagKeys, rows } = c;
  const col = (k: string) => keys.indexOf(k);
  const tcol = (k: string) => tagKeys.indexOf(k);
  const get = (a: unknown[], i: number | undefined): any => (i === undefined || i < 0 ? undefined : (a[i] ?? undefined));
  const [iId, iIp, iGame, iQuery, iName, iMap, iDesc, iPlayers, iMax, iPass, iSv, iVer, iPing, iTags, iBots, iVp, iSe, iVa, iVerdict, iClone, iCountry] = [
    "id", "ip", "gamePort", "queryPort", "name", "map", "description", "players", "maxPlayers", "password", "serverVersion", "version", "pingMs", "tags",
    "bots", "verifiedPlayers", "steamEmpty", "verifiedAt", "verdict", "clone", "country",
  ].map(col);
  const [tBe, tFp, tPh, tMod, tDlc, tFile, tQueue, tTime, tNight, tMin] = [
    "battleye", "firstPersonOnly", "privateHive", "modded", "dlc", "allowedFilePatching", "queue", "timeMultiplier", "nightMultiplier", "timeMinutes",
  ].map(tcol);
  // For this list only: names and descriptions repeat within it, not across sessions.
  const local = new Map<string, string>();
  const once = (s: string | undefined): string | undefined => {
    if (s === undefined) return s;
    const k = local.get(s);
    if (k !== undefined) return k;
    local.set(s, s);
    return s;
  };
  const out = new Array<ServerRow>(rows.length);
  for (let i = 0; i < rows.length; i++) {
    const a = rows[i]!;
    const tv = (get(a, iTags) ?? []) as unknown[];
    out[i] = {
      id: get(a, iId),
      ip: share(get(a, iIp)),
      gamePort: get(a, iGame),
      queryPort: get(a, iQuery),
      name: once(get(a, iName))!,
      map: share(get(a, iMap)),
      description: once(get(a, iDesc))!,
      players: get(a, iPlayers),
      maxPlayers: get(a, iMax),
      password: get(a, iPass),
      serverVersion: get(a, iSv),
      version: share(get(a, iVer)),
      pingMs: get(a, iPing),
      tags: {
        battleye: get(tv, tBe),
        firstPersonOnly: get(tv, tFp),
        privateHive: get(tv, tPh),
        modded: get(tv, tMod),
        dlc: get(tv, tDlc),
        allowedFilePatching: get(tv, tFile),
        queue: get(tv, tQueue),
        timeMultiplier: get(tv, tTime),
        nightMultiplier: get(tv, tNight),
        timeMinutes: get(tv, tMin),
      },
      bots: get(a, iBots),
      verifiedPlayers: get(a, iVp),
      steamEmpty: get(a, iSe),
      verifiedAt: get(a, iVa),
      verdict: share(get(a, iVerdict)),
      clone: get(a, iClone),
      country: share(get(a, iCountry)),
    };
  }
  return out;
}

export type RefreshDone = {
  /** "steam" for the master server, "lan" for LAN discovery (D-087), "dzsa" for the fallback list (D-089). */
  source: "steam" | "lan" | "dzsa";
  total: number;
  responded: number;
  failed: number;
  inflated: number;
  elapsedMs: number;
  partitions: PartitionResult[];
  capped: boolean;
  /** Later partitions were skipped after Steam's master stopped answering (throttling). */
  stoppedEarly: boolean;
  /** Steam finished the `hasplayers` answer, uncapped: the only kind of refresh that may
   *  withdraw a vouch from the servers it did not list. The host decides (D-236). */
  complete?: boolean;
  /** The request never reached Steam: an answer so the UI stops waiting, not a result (D-161). */
  rejected?: boolean;
  /** Why: "busy" (a refresh is already running and will report for itself — not an
   *  error), "no-session" (Steam is not there) or "no-answer" (Steam listed nothing,
   *  D-236). One flag for all of them put a permanent failure on screen for a
   *  double-click on Refresh (D-208). */
  reason?: "busy" | "no-session" | "no-answer" | null;
};

export type Verification = {
  id: string;
  verdict: Verdict;
  reported: number;
  verified: number | null;
  maxPlayers: number;
  pingMs: number | null;
  /** PLAYER's round trip; taken as the ping when none was ever measured (D-247). */
  playerRttMs?: number | null;
  /** The raw `keywords` string is `skip_serializing` on the Rust side since D-188 —
   *  the host writes it to the cache, the browser reads the parsed `tags`. */
  tags: DayzTags | null;
  verifiedAt: number;
  reason: string;
  /** What the check's INFO said about the server itself, sent only when it differs from
   *  the row (row 23). Empty fields and a missing game port say nothing. */
  facts?: InfoFacts;
};

/** The server's own description of itself in A2S_INFO, as far as a row keeps it. */
export type InfoFacts = { name: string; map: string; version: string; serverVersion: number; gamePort: number | null; password: boolean; bots: number };

export type VerifySummary = {
  /** No pass ran: one was already in flight. An all-zero summary would read as
   *  "0 verified · 0 fake · 0 offline", which is a result nobody produced (D-208). */
  skipped?: boolean;
  total: number;
  verified: number;
  inflated: number;
  unverifiable: number;
  synthetic: number;
  offline: number;
  elapsedMs: number;
};

export type A2sInfo = {
  protocol: number;
  name: string;
  map: string;
  folder: string;
  game: string;
  appId: number;
  players: number;
  maxPlayers: number;
  bots: number;
  serverType: string;
  environment: string;
  password: boolean;
  vac: boolean;
  version: string;
  edf: number;
  gamePort: number | null;
  steamId: number | null;
  spectatorPort: number | null;
  spectatorName: string | null;
  keywords: string | null;
  gameId: number | null;
  tags: DayzTags;
};

export type A2sMod = { hash: number; workshopId: number; idLen: number; name: string };
export type A2sDlc = { bit: number; name: string; hash: number };
export type DayzRules = {
  protocolVersion: number;
  overflow: number;
  dlcFlags: number;
  dlc: A2sDlc[];
  mods: A2sMod[];
  signatures: string[];
  description: string | null;
  trailing: number;
};
export type A2sRules = {
  ruleCount: number;
  fragmentCount: number;
  plain: Record<string, string>;
  dayz: DayzRules | null;
};
export type A2sPlayer = { index: number; name: string; score: number; durationSecs: number };
export type A2sPlayers = { count: number; players: A2sPlayer[] };

export type ServerDetails = {
  id: string;
  info: A2sInfo | null;
  infoRttMs: number | null;
  rules: A2sRules | null;
  rulesError: string | null;
  players: A2sPlayers | null;
  verification: Verification;
};

export type Diagnostics = {
  steam: SteamInfo;
  libraries: LibraryInfo[];
  dayz: GameInfo | null;
  workshop: WorkshopInfo | null;
  junctions: JunctionInfo[];
  warnings: string[];
  timingMs: number;
};

/** Country name for an ISO 3166-1 alpha-2 code in the UI language, or the code itself. */
const displayNames = typeof Intl !== "undefined" && "DisplayNames" in Intl ? new Intl.DisplayNames(undefined, { type: "region" }) : null;
export function countryName(code: string | null | undefined): string {
  if (!code) return "";
  try {
    return displayNames?.of(code.toUpperCase()) ?? code;
  } catch {
    return code;
  }
}

/** UI preferences stored in settings.json (D-070); `filters` is the browser's saved filter object. */
export type UiPrefs = {
  theme: string;
  accent: string;
  onboarded: boolean;
  filters: Record<string, unknown> | null;
  lastUpdateCheckMs: number;
  /** Unix seconds of the newest news post the user has seen (D-099). */
  newsSeen: number;
  /** Show the News page, and fetch the feed behind it, at all (D-174). */
  news: boolean;
  /** The version whose "What's new" notes were last shown; empty before 0.1.78 (D-301). */
  lastSeenVersion: string;
  /** The page the launcher opens on (row 16); absent before 0.1.83. */
  openOn?: StartPage;
};

/** The pages the launcher can open on (row 16); `OPEN_ON` in settings.rs. */
export type StartPage = "news" | "servers" | "favourites";

/** One DayZ news post from Steam's feed (D-099). */
export type NewsItem = {
  gid: string;
  title: string;
  url: string;
  author: string;
  feed: string;
  official: boolean;
  date: number;
  summary: string;
  update: boolean;
  /** First picture of the post (Steam's image CDN), if any (D-100). */
  image?: string;
  /** YouTube id of the first embedded video, if any (D-100). */
  video?: string;
};
export type NewsCached = { items: NewsItem[] };


/** A saved set of launch options (D-088). */
export type LaunchProfile = {
  name: string;
  profileName: string;
  extraArgs: string;
  skipIntro: boolean;
  noSplash: boolean;
  noPause: boolean;
};

export type Settings = {
  profileName: string;
  extraArgs: string;
  skipIntro: boolean;
  noSplash: boolean;
  noPause: boolean;
  /** Saved presets; the flat fields above are the current launch options. */
  launchProfiles: LaunchProfile[];
  /** Minutes of inactivity after which the Steam session is released; 0 = never (D-077). */
  steamIdleMinutes: number;
  /** Record what the launcher does to logs/launcher.log and the Logs page (D-169). */
  logging: boolean;
  /** Log areas switched off; entries are dropped at the source (D-172). */
  logMuted: string[];
  /** Read-only for `settings_set`; written through `ui_prefs_set`. */
  ui: UiPrefs;
  /** From `settings_get` only: the file exists but cannot be read, so these are the
   *  defaults (row 14, H4). */
  unreadable?: boolean;
  /** From `settings_get` only: the file was damaged and kept aside; the defaults. */
  reset?: boolean;
  /** From `settings_get` only: the name of the copy kept of the damaged file. */
  keptAs?: string;
};

export type ModPlanItem = {
  id: number;
  name: string;
  title: string | null;
  size: number | null;
  installed: boolean;
  needsUpdate: boolean;
  folder: string | null;
};

/** Mod lists collected from A2S_RULES across servers (D-080). */
export type ModCatalogEntry = { id: number; name: string; servers: number };
export type ServerMods = { id: string; mods: number[] };
export type ModsIndex = { catalog: ModCatalogEntry[]; index: ServerMods[]; unreadable: string[] };
export type ModScanSummary = { total: number; scanned: number; failed: number; elapsedMs: number };

/** Dangling `!Workshop` junctions removed on request from the Mods page (D-093, D-170). */
export type JunctionCleanup = { removed: string[]; failed: { name: string; error: string }[] };

/** One Workshop item unsubscribed through Steam (D-075). */
export type UnsubscribeResult = { id: number; ok: boolean; error: string | null };

/** Where a friend is playing, as Steam reports it (D-092). */
export type FriendServer = { ip: string; gamePort: number; queryPort: number };
export type FriendState = "offline" | "online" | "invisible" | "busy" | "away" | "snooze" | "looking_to_trade" | "looking_to_play";
export type FriendInfo = {
  /** SteamID64 as text (precision). */
  steamId: string;
  name: string;
  state: FriendState;
  inDayz: boolean;
  server?: FriendServer;
};

/** INFO-only snapshot for the wait-for-slot option (D-074). */
export type ServerSlots = {
  players: number;
  maxPlayers: number;
  queue: number | null;
  pingMs: number;
};

export type JoinPlan = {
  id: string;
  name: string;
  ip: string;
  gamePort: number;
  passwordRequired: boolean;
  serverVersion: string;
  localVersion: string | null;
  versionMismatch: boolean;
  steamRunning: boolean;
  gameFound: boolean;
  battleyePresent: boolean;
  rulesOk: boolean;
  mods: ModPlanItem[];
  missing: number;
  updates: number;
  downloadBytes: number;
  profileName: string;
  warnings: string[];
};

export type ItemProgress = {
  id: number;
  state: "subscribing" | "subscribed" | "pending" | "downloading" | "needs_update" | "installed" | "failed";
  downloaded: number;
  total: number;
  folder: string | null;
  error: string | null;
};

export type SyncProgress = { job: number; items: ItemProgress[]; installed: number; total: number; elapsedMs: number };
/** `failedId`: the item `error` is about, when it is about one; `superseded`: a newer
 *  download took over, which is not a failure (D-277). */
export type SyncDone = { job: number; ok: boolean; error: string | null; failedId: number | null; superseded: boolean; items: ItemProgress[]; elapsedMs: number };
/** `historyError`: why the join could not be added to Recent (row 14, F15). */
export type Launched = { pid: number; exe: string; commandLine: string; historyError?: string };
export type LaunchExited = { pid: number; code: number | null };

export type Favourite = { id: string };
/** One past join, for the Recent page (D-054). */
export type HistoryEntry = { id: string; joinedAt: number; name: string; ip: string; gamePort: number; mods: number };
export type PopulationSample = { ts: number; players: number; queue: number };
/** `missing`: the official launcher has no favourites file on this PC (row 14, F7). */
/** `skipped`: entries whose address is not one the launcher can ask (row 23, approved). */
export type ImportResult = { imported: number; already: number; unreachable: number; skipped: number; missing?: boolean };
/** Shown when the official launcher has no favourites file (row 14, F7, approved). */
export const NOTHING_TO_IMPORT = "Nothing to import: the official DayZ launcher has no saved favourites on this PC.";

/** What start-up did with the cache, and whether its writes fail now (D-303). */
export type CacheStatus = { inMemory: boolean; movedTo?: string; writeFailure?: string };

/** 0 is "0 kB": it said "1 kB" for an empty Workshop and before a download began (D-256). */
export const fmtBytes = (n: number): string =>
  n <= 0 ? "0 kB" : n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} kB`;

export const clock = (minutes: number | null | undefined): string =>
  minutes == null ? "–" : `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
