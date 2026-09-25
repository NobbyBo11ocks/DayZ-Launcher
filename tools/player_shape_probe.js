// Measures the shape of real A2S_PLAYER lists, to size the rules that read them
// (docs/11, D-238): how many honest lists carry zero-length sessions (R12), a
// sub-second cluster next to an old session, entries out of join order, more entries
// than slots, and how many sessions carry over between two passes (R11).
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
    zeroLength: d.filter((x) => x > 0 && x < 0.001).length,
    subSecond: d.filter((x) => x < 1).length,
    oldest: Math.round(Math.max(0, ...d)),
    ascents,
    overMax: max > 0 && d.length > max,
  };
}

/** R11's test: the most of `prev` that one shift within ±120 s of `gap` carries into `now`, to 3 s. */
function carriedOver(prev, now, gap) {
  let best = 0;
  for (const p of prev.slice(0, 3)) {
    for (const n of now) {
      const shift = n - p;
      if (Math.abs(shift - gap) > 120) continue;
      const pool = [...now];
      let matched = 0;
      for (const d of prev) {
        const i = pool.findIndex((x) => Math.abs(x - (d + shift)) <= 3);
        if (i >= 0) {
          pool.splice(i, 1);
          matched++;
        }
      }
      best = Math.max(best, matched);
    }
  }
  return best;
}

const first = await pass();
console.log(`first pass: ${first.size} lists; waiting five minutes (D-037)`);
await new Promise((r) => setTimeout(r, 300_000));
const second = await pass();

const found = { lists: 0, zeroLength1: 0, zeroLength2: 0, subSecondCluster: 0, ascending: 0, overMax: 0 };
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
      console.log("sub-second cluster beside an old session", t.id, JSON.stringify(s));
    }
    if (s.ascents > 0) {
      found.ascending++;
      console.log("not in join order", t.id, JSON.stringify(s));
    }
    if (s.overMax) {
      found.overMax++;
      console.log("more entries than slots", t.id, JSON.stringify(s));
    }
  }
  const a = first.get(t.id);
  const b = second.get(t.id);
  if (a && b && a.durations.length >= 5) {
    carriedShare.push(carriedOver(a.durations, b.durations, b.at - a.at) / a.durations.length);
  }
}
carriedShare.sort((x, y) => x - y);
const pct = (q) => (carriedShare.length ? carriedShare[Math.floor(q * (carriedShare.length - 1))].toFixed(2) : "-");
console.log(JSON.stringify(found));
console.log(`carried over across ~5 min: ${carriedShare.length} servers, min ${pct(0)}, p5 ${pct(0.05)}, median ${pct(0.5)}`);
