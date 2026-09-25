// Measures how many live servers answer A2S_RULES (Q10): the official launcher logs
// "Server failed to respond to rules request" for some hosts, and the browser needs to
// know whether that is common enough to need a fallback.
//
// Samples populated servers from the launcher's own cache, queries INFO then RULES with
// the same pacing the app uses, and prints the failure rate by reason.
//
// Usage: node tools/rules_failure_rate.js [sampleSize] [concurrency]
// One NAT flow per address: keep the sample small and wait between runs (D-037).
import { createRequire } from "node:module";
import { buildInfo, buildRules, parseInfo, parseRules, query } from "./lib/a2s.js";

const require = createRequire(import.meta.url);
const SAMPLE = Number.parseInt(process.argv[2] ?? "150", 10);
const CONCURRENCY = Number.parseInt(process.argv[3] ?? "12", 10);
const DB = `${process.env.LOCALAPPDATA}\\com.dayzlauncher.desktop\\cache.db`;

// The cache is SQLite; read it with the same bundled engine the app uses, through
// node:sqlite (Node 24) so the tool needs no dependency of its own.
const { DatabaseSync } = require("node:sqlite");
const db = new DatabaseSync(DB, { readOnly: true });

// Populated, not flagged as inflated, seen recently: the rows a user actually browses.
const rows = db
  .prepare(
    `SELECT ip, query_port, players, name FROM servers
     WHERE players > 0 AND ip NOT LIKE '10.%' AND ip NOT LIKE '192.168.%'
     ORDER BY RANDOM() LIMIT ?`,
  )
  .all(SAMPLE);
db.close();

console.log(`sampling ${rows.length} populated servers from the cache, ${CONCURRENCY} at a time`);

const out = { infoOk: 0, infoFail: 0, rulesOk: 0, rulesTimeout: 0, rulesError: 0, rttInfo: [], rttRules: [], mods: [] };
const failures = [];
// Widths seen in the mod id-length byte (Q9): the parser accepts 1–8; live servers
// send 4, and four of 3 213 entries once sent 1 (D-140).
const idWidths = new Map();

async function probe(r) {
  const port = Number(r.query_port);
  try {
    const info = await query(r.ip, port, buildInfo);
    parseInfo(info.data);
    out.infoOk++;
    out.rttInfo.push(info.rtt);
  } catch {
    out.infoFail++;
    return; // offline or firewalled: not a RULES failure
  }
  try {
    const rules = await query(r.ip, port, buildRules);
    const parsed = parseRules(rules.data);
    out.rulesOk++;
    out.rttRules.push(rules.rtt);
    out.mods.push(parsed.mods?.length ?? 0);
    for (const m of parsed.mods ?? []) {
      const w = (m.idLenByte ?? 0) & 0x0f;
      idWidths.set(w, (idWidths.get(w) ?? 0) + 1);
    }
  } catch (e) {
    const msg = String(e.message ?? e);
    if (/timeout|timed out/i.test(msg)) out.rulesTimeout++;
    else out.rulesError++;
    failures.push(`${r.ip}:${port} ${msg.slice(0, 60)} (${r.name ?? ""})`.trim());
  }
}

const queue = [...rows];
await Promise.all(
  Array.from({ length: CONCURRENCY }, async () => {
    for (let next = queue.pop(); next; next = queue.pop()) await probe(next);
  }),
);

const pct = (n, d) => (d ? ((n / d) * 100).toFixed(1) : "0.0");
const median = (a) => (a.length ? [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)].toFixed(0) : "-");
const answered = out.rulesOk + out.rulesTimeout + out.rulesError;

console.log(`
INFO   answered ${out.infoOk}/${rows.length}, no reply ${out.infoFail} (offline or filtered)
RULES  answered ${out.rulesOk}/${answered} = ${pct(out.rulesOk, answered)}% of servers that answered INFO
       timeout ${out.rulesTimeout} (${pct(out.rulesTimeout, answered)}%), other error ${out.rulesError}
RTT    INFO median ${median(out.rttInfo)} ms, RULES median ${median(out.rttRules)} ms
MODS   median ${median(out.mods)} per server, max ${out.mods.length ? Math.max(...out.mods) : 0}
IDLEN  mod id widths seen: ${[...idWidths.entries()].sort((a, b) => a[0] - b[0]).map(([w, n]) => `${w} bytes ×${n}`).join(", ") || "none"}`);

if (failures.length) {
  console.log("\nfailures:");
  for (const f of failures.slice(0, 15)) console.log("  " + f);
  if (failures.length > 15) console.log(`  … ${failures.length - 15} more`);
}
