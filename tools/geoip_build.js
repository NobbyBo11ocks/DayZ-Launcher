// Builds the IPv4 → country table the app embeds (src-tauri/resources/geoip-v4.bin)
// from DB-IP's free "IP to Country Lite" CSV (CC BY 4.0, attribution required; docs/08).
// Usage: node tools/geoip_build.js [dbip-country-lite-YYYY-MM.csv[.gz]]
//   Without an argument the current month's file is downloaded from download.db-ip.com.
//
// Format (little-endian), read by src-tauri/src/geoip.rs:
//   "DZGEO1"            6 bytes magic
//   u16 countries       count N of ISO 3166-1 alpha-2 codes
//   N × 2 bytes         the codes, index = position ("ZZ" = unknown)
//   u32 ranges          count M
//   M × u32             range start addresses, ascending
//   M × u8              country index per range (a range ends where the next starts)
// Adjacent ranges with the same country are merged; the gap check relies on DB-IP's
// rows being contiguous, which the tool verifies.
import { gunzipSync } from "node:zlib";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const out = path.join(root, "src-tauri", "resources", "geoip-v4.bin");
const meta = path.join(root, "src-tauri", "resources", "geoip-v4.json");

const src = process.argv[2];
let csv;
let label;
if (src) {
  const raw = readFileSync(src);
  csv = (src.endsWith(".gz") ? gunzipSync(raw) : raw).toString("latin1");
  label = path.basename(src);
} else {
  const d = new Date();
  label = `dbip-country-lite-${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}.csv.gz`;
  const url = `https://download.db-ip.com/free/${label}`;
  const res = await fetch(url, { headers: { "user-agent": "dayz-launcher-tools" } });
  if (!res.ok) {
    console.error(`${url} → HTTP ${res.status}`);
    process.exitCode = 2;
  } else {
    csv = gunzipSync(Buffer.from(await res.arrayBuffer())).toString("latin1");
  }
}

if (csv) {
  const ip = (s) => {
    const p = s.split(".");
    return ((+p[0] << 24) | (+p[1] << 16) | (+p[2] << 8) | +p[3]) >>> 0;
  };
  const codes = [];
  const index = new Map();
  const starts = [];
  const idx = [];
  let rows = 0;
  let expectNext = 0;
  let gaps = 0;
  for (const line of csv.split("\n")) {
    if (!/^\d+\.\d+\.\d+\.\d+,/.test(line)) continue; // IPv4 rows only (IPv6 rows also start with a digit)
    const [a, b, cc] = line.trim().split(",");
    const s = ip(a);
    const e = ip(b);
    rows++;
    if (s !== expectNext) gaps++;
    expectNext = (e + 1) >>> 0;
    let i = index.get(cc);
    if (i === undefined) {
      i = codes.length;
      codes.push(cc);
      index.set(cc, i);
    }
    if (idx.length && idx[idx.length - 1] === i) continue; // merge with the previous range
    starts.push(s);
    idx.push(i);
  }
  if (gaps) console.error(`warning: ${gaps} non-contiguous rows; the table treats a gap as part of the previous range`);
  if (codes.length > 255) throw new Error("more than 255 countries; widen the index");

  const buf = Buffer.alloc(6 + 2 + codes.length * 2 + 4 + starts.length * 5);
  let o = buf.write("DZGEO1", 0, "ascii");
  o = buf.writeUInt16LE(codes.length, o);
  for (const c of codes) o += buf.write(c.padEnd(2, "?").slice(0, 2), o, "ascii");
  o = buf.writeUInt32LE(starts.length, o);
  for (const s of starts) o = buf.writeUInt32LE(s, o);
  for (const i of idx) o = buf.writeUInt8(i, o);
  writeFileSync(out, buf);
  writeFileSync(
    meta,
    JSON.stringify({ source: label, licence: "CC BY 4.0, attribution: IP Geolocation by DB-IP (https://db-ip.com)", rows, ranges: starts.length, countries: codes.length, built: new Date().toISOString().slice(0, 10) }, null, 2) + "\n",
  );
  console.log(`${label}: ${rows} IPv4 rows → ${starts.length} ranges, ${codes.length} countries, ${buf.length} bytes → ${path.relative(root, out)}`);
}
