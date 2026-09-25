// Live A2S probe for DayZ: INFO + RULES + PLAYER with challenge and split-packet handling.
// Usage: node tools/a2s_probe.js <ip> <queryPort>
import { buildInfo, buildPlayer, buildRules, parseInfo, parseKeywords, parsePlayers, parseRules, query } from "./lib/a2s.js";

const host = process.argv[2] ?? "51.81.8.81";
const port = Number.parseInt(process.argv[3] ?? "27017", 10);

const info = await query(host, port, buildInfo);
const parsedInfo = parseInfo(info.data);
// RULES fails on the servers you would probe — the ~170 that never answer it (D-244) —
// and used to abort the whole probe before INFO was printed (D-246). Built here, like
// `players`: the D-246 version left the output reading a `rules` that only existed
// inside the try, so every probe died with a ReferenceError (D-261).
let rules;
try {
  const r = await query(host, port, buildRules);
  rules = { rttMs: +r.rtt.toFixed(1), packets: r.packets, split: r.split, splitSize: r.splitSize, compressed: r.compressed, ...parseRules(r.data) };
} catch (e) {
  rules = `err: ${e.message}`;
}
let players;
try {
  const pl = await query(host, port, buildPlayer);
  players = { rttMs: +pl.rtt.toFixed(1), ...parsePlayers(pl.data) };
} catch (e) {
  players = `err: ${e.message}`;
}

console.log(
  JSON.stringify(
    {
      target: `${host}:${port}`,
      info: { rttMs: +info.rtt.toFixed(1), packets: info.packets, ...parsedInfo, tags: parseKeywords(parsedInfo.keywords) },
      rules,
      players,
    },
    null,
    2,
  ),
);
