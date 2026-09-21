// Empirical fake-population probe: compares A2S_INFO `players` with the A2S_PLAYER
// head-count and inspects per-player durations on a sample of DayZ servers.
// Usage: node tools/spoof_probe.js <addr-list-file> [sampleSize=80] [extra ip:port ...]
// The list file has one ip:queryPort per line (e.g. the DZSA export). Prints one line
// per server and a summary of the detection heuristics (docs/11-fake-population-detection.md).
import { readFile } from "node:fs/promises";
import { buildInfo, buildPlayer, parseInfo, parsePlayers, query } from "./lib/a2s.js";

const [file, sizeArg, ...extra] = process.argv.slice(2);
if (!file) {
  console.error("usage: node tools/spoof_probe.js <addr-list-file> [sampleSize] [extra ip:port ...]");
  process.exit(2);
}
const size = Number.parseInt(sizeArg ?? "80", 10);
const all = (await readFile(file, "utf8")).split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
// Deterministic pseudo-random sample so runs are comparable.
let seed = 42;
const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
const sample = [...extra];
const pool = all.slice();
while (sample.length < size + extra.length && pool.length) sample.push(pool.splice(Math.floor(rnd() * pool.length), 1)[0]);

async function probe(addr) {
  const [host, portStr] = addr.split(":");
  const port = Number.parseInt(portStr, 10);
  const out = { addr };
  try {
    const i = await query(host, port, buildInfo, 2500);
    out.info = parseInfo(i.data);
    out.rtt = +i.rtt.toFixed(0);
  } catch (e) {
    out.error = "info: " + e.message;
    return out;
  }
  try {
    const p = await query(host, port, buildPlayer, 2500);
    out.players = parsePlayers(p.data);
  } catch (e) {
    out.playersError = e.message;
  }
  return out;
}

function stats(durations) {
  if (!durations.length) return null;
  const s = durations.slice().sort((a, b) => a - b);
  const median = s[Math.floor(s.length / 2)];
  const distinct = new Set(s.map((d) => Math.round(d))).size;
  const zeros = s.filter((d) => d < 1).length;
  return { min: s[0].toFixed(0), median: median.toFixed(0), max: s[s.length - 1].toFixed(0), distinct, zeros };
}

const results = [];
const CONCURRENCY = 16;
let idx = 0;
await Promise.all(
  Array.from({ length: CONCURRENCY }, async () => {
    while (idx < sample.length) {
      const a = sample[idx++];
      results.push(await probe(a));
    }
  }),
);

let answered = 0;
let mismatch = 0;
let inflated = 0;
let big = 0;
let playerFail = 0;
const buckets = { match: 0, "info>player": 0, "info<player": 0 };
for (const r of results) {
  if (!r.info) {
    console.log(`${r.addr.padEnd(22)} ${r.error}`);
    continue;
  }
  answered++;
  const infoN = r.info.players;
  const pN = r.players ? r.players.players.length : null;
  const durations = r.players ? r.players.players.map((p) => p.duration) : [];
  const st = stats(durations);
  let verdict = "ok";
  if (pN == null) {
    playerFail++;
    verdict = "no-player-reply";
  } else if (infoN === pN) buckets.match++;
  else if (infoN > pN) {
    buckets["info>player"]++;
    mismatch++;
    if (infoN - pN >= 5) {
      inflated++;
      verdict = "INFLATED";
    } else verdict = "drift";
  } else {
    buckets["info<player"]++;
    mismatch++;
    verdict = "drift-";
  }
  if (infoN >= 60) big++;
  const durTxt = st ? `dur min/med/max ${st.min}/${st.median}/${st.max}s distinct ${st.distinct} zeros ${st.zeros}` : "dur -";
  console.log(
    `${r.addr.padEnd(22)} info ${String(infoN).padStart(3)}/${String(r.info.maxPlayers).padEnd(3)} player ${String(pN ?? "?").padStart(3)}  ${verdict.padEnd(15)} ${durTxt}  ${r.rtt}ms  ${r.info.name.slice(0, 38)}`,
  );
}
console.log("\nsummary:", JSON.stringify({ sampled: sample.length, answered, playerReplyFailed: playerFail, ...buckets, inflatedBy5plus: inflated, infoAtLeast60: big }));
