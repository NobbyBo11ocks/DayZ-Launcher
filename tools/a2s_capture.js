// Captures raw A2S datagrams from a live DayZ server as Rust test fixtures.
// Usage: node tools/a2s_capture.js <ip> <queryPort> <name> [outDir]
// Writes <outDir>/<name>.info.bin, <name>.rules.<n>.bin, <name>.player.bin and
// <name>.expected.json (decoded with the reference parser in tools/lib/a2s.js).
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { buildInfo, buildPlayer, buildRules, parseInfo, parseKeywords, parsePlayers, parseRules, query } from "./lib/a2s.js";

const [host, portArg, name, outArg] = process.argv.slice(2);
if (!host || !portArg || !name) {
  console.error("usage: node tools/a2s_capture.js <ip> <queryPort> <name> [outDir]");
  process.exit(2);
}
const port = Number.parseInt(portArg, 10);
const outDir = outArg ?? path.join("src-tauri", "tests", "fixtures", "a2s");
await mkdir(outDir, { recursive: true });

const expected = { target: `${host}:${port}`, capturedAt: new Date().toISOString() };

const info = await query(host, port, buildInfo);
await writeFile(path.join(outDir, `${name}.info.bin`), info.datagrams[0]);
expected.info = parseInfo(info.data);
expected.info.tags = parseKeywords(expected.info.keywords);

const rules = await query(host, port, buildRules);
for (let i = 0; i < rules.datagrams.length; i++) await writeFile(path.join(outDir, `${name}.rules.${i}.bin`), rules.datagrams[i]);
expected.rulesDatagrams = rules.datagrams.length;
expected.rulesSplit = { split: rules.split, total: rules.total ?? 1, splitSize: rules.splitSize ?? null, compressed: rules.compressed ?? false };
try {
  expected.rules = parseRules(rules.data);
} catch (e) {
  expected.rulesParseError = e.message;
}

try {
  const pl = await query(host, port, buildPlayer);
  await writeFile(path.join(outDir, `${name}.player.bin`), pl.datagrams[0]);
  expected.players = parsePlayers(pl.data);
} catch (e) {
  expected.playersError = e.message;
}

await writeFile(path.join(outDir, `${name}.expected.json`), JSON.stringify(expected, null, 2) + "\n");
console.log(
  `captured ${name}: info ${info.datagrams[0].length} B, rules ${rules.datagrams.length} datagram(s) ${rules.datagrams.reduce((a, d) => a + d.length, 0)} B` +
    (expected.rules ? `, ${expected.rules.mods?.length ?? 0} mods` : `, RULES PARSE ERROR: ${expected.rulesParseError}`) +
    (expected.players ? `, ${expected.players.count} players` : ""),
);
