// The details pane's arithmetic (docs/06 §3), kept out of its markup so it can be tested:
// the claim beside a head-count, the other servers at the address, the session summary
// and the mods' marks and counts (row 26, approved).
import { type A2sPlayer, isUntrusted, playersShown, type ServerRow, trustedPlayers, type Verdict } from "./types";

/** The server's claim, said beside a real head-count when the two differ; null beside
 *  anything else. It sat beside a claim and beside the 0 Steam's "empty" puts there,
 *  where it said nothing true (row 26, approved). */
export function claimBeside(r: ServerRow): number | null {
  if (r.verifiedPlayers == null || playersShown(r) !== r.verifiedPlayers) return null;
  return r.players !== r.verifiedPlayers ? r.players : null;
}

/**
 * The other servers at an address as the pane lists them (D-212): never the pane's own
 * server listed again under an old query port — one game port at an address is one
 * server — and the trusted ones first by their count, the untrusted after them (row 26,
 * approved).
 */
export function siblingsOf(self: ServerRow, rows: Iterable<ServerRow>): ServerRow[] {
  const out: { r: ServerRow; untrusted: boolean; n: number }[] = [];
  for (const r of rows) {
    if (r.id === self.id || r.ip !== self.ip || (self.gamePort > 0 && r.gamePort === self.gamePort)) continue;
    out.push({ r, untrusted: isUntrusted(r), n: trustedPlayers(r) });
  }
  out.sort((a, b) => Number(a.untrusted) - Number(b.untrusted) || b.n - a.n || a.r.pingMs - b.r.pingMs);
  return out.map((x) => x.r);
}

export type SessionSummary = {
  count: number;
  median: number;
  longest: number;
  /** Joined in the last 10 minutes. */
  recent: number;
  buckets: { label: string; n: number }[];
  peak: number;
};

const BUCKETS: readonly { label: string; max: number }[] = [
  { label: "< 15 min", max: 15 * 60 },
  { label: "15–60 min", max: 60 * 60 },
  { label: "1–3 h", max: 3 * 3600 },
  { label: "3–6 h", max: 6 * 3600 },
  { label: "6 h +", max: Infinity },
];

/** An entry no clock produced, as R12 reads one (`judge` in browser/verify.rs). */
const zeroLength = (s: number) => Number.isFinite(s) && s >= 0 && s < 0.001;

/**
 * Session lengths summarised (D-081), over real sessions only: none for a list judged
 * fake, and without the zero-length entries R12 leaves out of the count when there are two
 * or more of them, so the section agrees with the trust box (row 26, approved).
 */
export function sessionSummary(players: readonly A2sPlayer[] | null | undefined, verdict: Verdict | null | undefined): SessionSummary | null {
  if (!players?.length || verdict === "synthetic") return null;
  let secs = players.map((p) => p.durationSecs);
  if (secs.filter(zeroLength).length >= 2) secs = secs.filter((s) => !zeroLength(s));
  if (!secs.length) return null;
  secs.sort((a, b) => a - b);
  const buckets = BUCKETS.map((b) => ({ label: b.label, n: 0 }));
  for (const s of secs) {
    const bucket = buckets[BUCKETS.findIndex((b) => s < b.max)];
    if (bucket) bucket.n++;
  }
  return {
    count: secs.length,
    median: secs[Math.floor(secs.length / 2)] ?? 0,
    longest: secs[secs.length - 1] ?? 0,
    recent: secs.filter((s) => s < 600).length,
    buckets,
    peak: Math.max(...buckets.map((b) => b.n)),
  };
}

/** A mod as the pane lists it, from the server's RULES reply or the scan's list. */
export type PaneMod = { workshopId: number; name: string };

/**
 * The server's mods, each once (row 26, approved): a Workshop id the server lists twice
 * is one mod, as the list's Mods column counts it; mods with no Workshop id (0,
 * server-side) are told apart by name.
 */
export function distinctMods(mods: readonly PaneMod[]): PaneMod[] {
  const seen = new Set<string>();
  return mods.filter((m) => {
    const key = m.workshopId > 0 ? String(m.workshopId) : `0:${m.name}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export type ModMark = "installed" | "update" | "missing" | "server-side" | "unknown";

/**
 * One mod's mark: server-side (no Workshop id: nothing to download or link to), missing
 * (what the join downloads, D-265), installed with an update waiting, installed, or
 * unknown while the installed set is (D-281; row 26, approved).
 */
export function modMark(m: PaneMod, installed: ReadonlySet<number> | null, stale: ReadonlySet<number>): ModMark {
  if (m.workshopId <= 0) return "server-side";
  if (!installed) return "unknown";
  if (!installed.has(m.workshopId)) return "missing";
  return stale.has(m.workshopId) ? "update" : "installed";
}

/** The Mods heading's "· N missing" and "· N to update", each Workshop id once (D-281). */
export function modCounts(mods: readonly PaneMod[], installed: ReadonlySet<number> | null, stale: ReadonlySet<number>): { missing: number; toUpdate: number } {
  const missing = new Set<number>();
  const toUpdate = new Set<number>();
  for (const m of mods) {
    const mark = modMark(m, installed, stale);
    if (mark === "missing") missing.add(m.workshopId);
    else if (mark === "update") toUpdate.add(m.workshopId);
  }
  return { missing: missing.size, toUpdate: toUpdate.size };
}
