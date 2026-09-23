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
};

/**
 * Absent keys, not nulls: the host leaves out everything it has nothing to say
 * about, and the two fields nothing renders (`external`, `unknown`) never cross
 * IPC at all (D-160).
 */
export type DayzTags = {
  battleye: boolean;
  firstPersonOnly: boolean;
  privateHive: boolean;
  modded: boolean;
  dlc: boolean;
  allowedFilePatching: boolean;
  shard?: string | null;
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
  verifiedPlayers?: number | null;
  /** Steam's master reported zero authenticated players (row from a `noplayers` partition). */
  steamEmpty?: boolean | null;
  verifiedAt?: number | null;
  verdict?: Verdict | null;
  /** ISO 3166-1 alpha-2 from the embedded GeoIP table, absent when unknown (D-073). */
  country?: string | null;
};

/** Rule R0 (docs/11): Steam says empty, A2S_INFO claims players. */
export const isInflated = (r: ServerRow): boolean => r.steamEmpty === true && r.players > 0;

/**
 * Any rule fired: hidden by the default "Hide inflated" filter.
 * R4 (refuses PLAYER) only counts when Steam has not vouched for the server:
 * a small minority of servers drop A2S_PLAYER at the host firewall — D-050 measured 6 of 2 858, and D-047 retracts the earlier 24 % reading as burst loss, so `steamEmpty === false` overrides an "unverifiable" verdict.
 */
export const isUntrusted = (r: ServerRow): boolean =>
  isInflated(r) ||
  r.verdict === "inflated" ||
  r.verdict === "synthetic" ||
  r.verdict === "offline" ||
  (r.verdict === "unverifiable" && r.steamEmpty !== false);

/**
 * True when the number on screen is the server's own claim rather than a head-count
 * we made ourselves (D-160). Steam vouching for a server keeps it visible, but the
 * count is still unchecked and must not look verified: hiding behind a green tick is
 * exactly the gap a server that answers INFO and firewalls PLAYER relies on.
 */
export const isUnchecked = (r: ServerRow): boolean =>
  r.verifiedPlayers == null && (r.verdict === "unverifiable" || r.verdict === "offline" || r.verdict == null);

/** Population a player may rely on (mirrors `judge` in browser/verify.rs). */
export const trustedPlayers = (r: ServerRow): number => (r.verifiedPlayers ?? (isInflated(r) ? 0 : r.players));

export type SteamStatus = {
  initialized: boolean;
  error: string | null;
  appId: number;
  steamId: number | null;
  persona: string | null;
  refreshing: boolean;
  lastRefreshSecsAgo: number | null;
  /** Steamworks was released after inactivity; any Steam command re-initialises it (Q16). */
  idle: boolean;
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

export type CachedServers = {
  rows: ServerRow[];
  lastRefresh: number | null;
};

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
  /** The request never reached Steam: an answer so the UI stops waiting, not a result (D-160). */
  rejected?: boolean;
  /** Why: "busy" (a refresh is already running and will report for itself — not an
   *  error) or "no-session" (Steam is not there). One flag for both put a permanent
   *  failure on screen for a double-click on Refresh (D-208). */
  reason?: "busy" | "no-session" | null;
};

export type Verification = {
  id: string;
  verdict: Verdict;
  reported: number;
  verified: number | null;
  maxPlayers: number;
  pingMs: number | null;
  /** The raw `keywords` string is `skip_serializing` on the Rust side since D-188 —
   *  the host writes it to the cache, the browser reads the parsed `tags`. */
  tags: DayzTags | null;
  verifiedAt: number;
  reason: string;
};

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
};

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
export type NewsCached = { items: NewsItem[]; fetchedAt: number | null };

/** A Steam avatar as raw RGBA (D-100, D-115); drawn on a canvas. */
export type Avatar = { width: number; height: number; rgba: number[] };

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
export type ModsIndex = { catalog: ModCatalogEntry[]; index: ServerMods[] };
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
export type SyncDone = { job: number; ok: boolean; error: string | null; items: ItemProgress[]; elapsedMs: number };
export type Launched = { pid: number; exe: string; commandLine: string };
export type LaunchExited = { pid: number; code: number | null };

export type Favourite = { id: string; addedAt: number };
/** One past join, for the Recent page (D-076). */
export type HistoryEntry = { id: string; joinedAt: number; name: string; ip: string; gamePort: number; mods: number };
export type PopulationSample = { ts: number; players: number; queue: number };
export type ImportResult = { total: number; imported: number; already: number; unreachable: number; path: string };

export const fmtBytes = (n: number): string =>
  n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} kB`;

export const clock = (minutes: number | null | undefined): string =>
  minutes == null ? "–" : `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
