// Live A2S probe for DayZ: INFO + RULES + PLAYER with challenge and split-packet handling.
// Usage: node tools/a2s_probe.js <ip> <queryPort>
import { buildInfo, buildPlayer, buildRules, parseInfo, parseKeywords, parsePlayers, parseRules, query } from "./lib/a2s.js";

const host = process.argv[2] ?? "51.81.8.81";
const port = Number.parseInt(process.argv[3] ?? "27017", 10);

const info = await query(host, port, buildInfo);
const parsedInfo = parseInfo(info.data);
const rules = await query(host, port, buildRules);
const parsedRules = parseRules(rules.data);
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
      rules: { rttMs: +rules.rtt.toFixed(1), packets: rules.packets, split: rules.split, splitSize: rules.splitSize, compressed: rules.compressed, ...parsedRules },
      players,
    },
    null,
    2,
  ),
);
