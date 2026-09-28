// Measures the shape of real A2S_PLAYER lists, to size the rules that read them
// (docs/11, D-238, D-314): how many honest lists carry zero-length sessions (R12), a
// sub-second cluster next to an old session (R14), entries out of join order, more
// entries than slots (R13), and between two passes how many sessions carry over and
// how many old enough to have been on at the first were missing from it (R11).
//
// Targets come from the launcher's own cache: servers it verified at five players or
// more, sampled at random. One PLAYER query per server, one server at a time, and the
// second pass five minutes after the first — the gap D-037 asks between sweeps.
// Nothing is written.
//
// Usage (Windows, Node 24): node tools/player_shape_probe.js [servers=150]
import { DatabaseSync } from "node:sqlite";
import path from "node:path";
import { buildPlayer, parsePlayers, query } from "./lib/a2s.js";

const wanted = Number.parseInt(process.argv[2] ?? "150", 10);
const dbPath = path.join(process.env.LOCALAPPDATA ?? "", "com.dayzlauncher.desktop", "cache.db");
const db = new DatabaseSync(dbPath, { readOnly: true });
const targets = db
  .prepare(
    `SELECT id, ip, query_port AS port, max_players AS max FROM servers
     WHERE verdict = 'verified' AND verified_players >= 5 ORDER BY RANDOM() LIMIT ?`,
  )
  .all(wanted);
db.close();
console.log(`${targets.length} servers verified at five or more players, from ${dbPath}`);

/** One PLAYER list per target that answers: durations in the order the server sent them. */
async function pass() {
  const lists = new Map();
  for (const t of targets) {
    try {
      const reply = await query(t.ip, t.port, buildPlayer, 2500);
      lists.set(t.id, { at: Date.now() / 1000, durations: parsePlayers(reply.data).players.map((p) => p.duration) });
    } catch {
      /* no answer: nothing to measure */
    }
  }
  return lists;
}

function shape(d, max) {
  let ascents = 0;
  for (let i = 1; i < d.length; i++) if (d[i] > d[i - 1] + 1) ascents++;
  return {
    entries: d.length,
    zeroLength: d.filter((x) => x >= 0 && x < 0.001).length,
    subSecond: d.filter((x) => x < 1).length,
    oldest: Math.round(Math.max(0, ...d)),
    ascents,
    overMax: max > 0 && d.length > max,
  };
}

/** Durations as R11 reads them: finite, longest first. */
const longestFirst = (v) => v.filter(Number.isFinite).sort((a, b) => b - a);

/**
 * `align` in verify.rs: `now` lined up with `prev` advanced by `shift`, both longest first and
 * matched in order within 3 s. Of the sessions in `now` at least `minAge` long: how many there were
 * and how many matched; `offsets` collects `now − prev` of each match.
 */
function align(prev, now, shift, minAge = -Infinity, offsets = null) {
  let old = 0;
  let matched = 0;
  let j = 0;
  for (const d of now) {
    if (d < minAge) break;
    old++;
    const want = d - shift;
    while (j < prev.length && prev[j] > want + 3) j++;
    if (j < prev.length && prev[j] >= want - 3) {
      offsets?.push(d - prev[j]);
      matched++;
      j++;
    }
  }
  return [old, matched];
}

/** `most_carried`: the whole-second shift within ±120 s of `gap` that carries the most, nearest first. */
function mostCarried(p, n, gap) {
  let best = [0, gap];
  for (let i = 0; i <= 240; i++) {
    const shift = gap + Math.floor((i + 1) / 2) * (i % 2 === 1 ? 1 : -1);
    const matched = align(p, n, shift)[1];
    if (matched > best[0]) best = [matched, shift];
  }
  return best;
}

/** R11's count (`carried`): the most sessions of `prev` one shift carries into `now`. */
const carriedOver = (prev, now, gap) => mostCarried(longestFirst(prev), longestFirst(now), gap)[0];

/**
 * R11's invariant (`unexplained`, D-314): null when some shift within ±120 s of `gap` — every half
 * second, then the one most sessions agree on refined to the median offset of its matches — leaves
 * at most one session of `now` old enough to have been on at `prev` without a partner in it, or
 * under a tenth of them; otherwise the fewest missing and how many were that old.
 */
function unexplained(prev, now, gap) {
  const [p, n] = [longestFirst(prev), longestFirst(now)];
  let fewest = null;
  const explains = (s) => {
    const [old, matched] = align(p, n, s, s + 3);
    const missing = old - matched;
    if (missing < 2 || missing * 10 < old) return true;
    if (!fewest || missing < fewest[0]) fewest = [missing, old];
    return false;
  };
  for (let i = 0; i <= 480; i++) {
    if (explains(gap + Math.floor((i + 1) / 2) * 0.5 * (i % 2 === 1 ? 1 : -1))) return null;
  }
  const [agreed, shift] = mostCarried(p, n, gap);
  if (agreed > 0) {
    const offsets = [];
    align(p, n, shift, -Infinity, offsets);
    offsets.sort((x, y) => x - y);
    if (explains(offsets[Math.floor(offsets.length / 2)])) return null;
  }
  return fewest;
}

const first = await pass();
console.log(`first pass: ${first.size} lists; waiting five minutes (D-037)`);
await new Promise((r) => setTimeout(r, 300_000));
const second = await pass();

const found = { lists: 0, zeroLength1: 0, zeroLength2: 0, subSecondCluster: 0, ascending: 0, overMax: 0, pairs: 0, missing: 0 };
const carriedShare = [];
for (const t of targets) {
  for (const lists of [first, second]) {
    const l = lists.get(t.id);
    if (!l || l.durations.length < 5) continue;
    const s = shape(l.durations, t.max);
    found.lists++;
    if (s.zeroLength >= 1) found.zeroLength1++;
    if (s.zeroLength >= 2) {
      found.zeroLength2++;
      console.log("zero-length sessions (R12 would fire)", t.id, JSON.stringify(s));
    }
    if (s.subSecond >= 3 && s.oldest >= 600) {
      found.subSecondCluster++;
      console.log("sub-second cluster beside an old session (R14 would fire)", t.id, JSON.stringify(s));
    }
    if (s.ascents > 0) {
      found.ascending++;
      console.log("not in join order", t.id, JSON.stringify(s));
    }
    if (s.overMax) {
      found.overMax++;
      console.log("more entries than slots (R13 would fire)", t.id, JSON.stringify(s));
    }
  }
  const a = first.get(t.id);
  const b = second.get(t.id);
  if (a && b && a.durations.length >= 5) {
    const gap = b.at - a.at;
    // R11's invariant, with its exemptions: a restart and a list that halved.
    if (b.durations.length >= 5 && !b.durations.every((d) => d < gap) && b.durations.length * 2 >= a.durations.length) {
      found.pairs++;
      const u = unexplained(a.durations, b.durations, gap);
      if (u) {
        found.missing++;
        console.log(`${u[0]} of ${u[1]} sessions older than the gap missing from the first pass (an R11 strike)`, t.id, JSON.stringify({ before: a.durations.length, after: b.durations.length, gap: Math.round(gap) }));
      }
    }
    const share = carriedOver(a.durations, b.durations, gap) / a.durations.length;
    carriedShare.push(share);
    // A restart between the passes is exempt from R11 and says so here: every session
    // in the second list is younger than the gap. Anything else this low is what R11
    // is for, and is worth looking at by hand (D-243).
    if (share <= 0.2) {
      const restarted = b.durations.every((d) => d < gap);
      console.log(
        `few sessions carried over (${Math.round(share * 100)} %)`,
        t.id,
        restarted ? "— restarted between the passes" : "— NOT a restart",
        JSON.stringify({ before: a.durations.length, after: b.durations.length, gap: Math.round(gap) }),
      );
    }
  }
}
carriedShare.sort((x, y) => x - y);
const pct = (q) => (carriedShare.length ? carriedShare[Math.floor(q * (carriedShare.length - 1))].toFixed(2) : "-");
console.log(JSON.stringify(found));
console.log(`carried over across ~5 min: ${carriedShare.length} servers, min ${pct(0)}, p5 ${pct(0.05)}, median ${pct(0.5)}`);
